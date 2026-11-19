//! Public integration tests for the opt-in Oximut mutation evidence
//! (`ScanExtensions::mutation_report`), driven by the recorded fixtures in
//! `tests/fixtures/oximut` with `<ROOT>` substitution. See
//! `docs/MUTATION_EVIDENCE.md` and `tests/fixtures/oximut/README.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(dead_code)]
mod common;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use ts_crap::{
    EvidenceKind, Format, MutatorScope, Report, ScanExtensions, ScanOptions, baseline::Baseline,
    render, scan, scan_with,
};

const SUBJECT: &str = "src/subject.ts";

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/oximut")
            .join(name),
    )
    .unwrap()
}

/// Oximut records canonical absolute paths, so `<ROOT>` becomes the canonical root.
fn substituted(report: &str, root: &Path) -> String {
    let encoded = serde_json::to_string(&fs::canonicalize(root).unwrap()).unwrap();
    report.replace("<ROOT>", &encoded[1..encoded.len() - 1])
}

fn write_report(project: impl AsRef<Path>, name: &str, body: &str) -> PathBuf {
    let path = project.as_ref().join(name);
    fs::write(&path, body).unwrap();
    path
}

/// The recorded subject at its recorded location, plus the canonical root.
fn subject_project() -> (tempfile::TempDir, PathBuf) {
    let project = common::project(&[(SUBJECT, &fixture("subject.ts"))]);
    let root = fs::canonicalize(project.path()).unwrap();
    (project, root)
}

fn options(root: &Path) -> ScanOptions {
    ScanOptions {
        root: root.into(),
        ..Default::default()
    }
}

fn with_mutation_report(report: &Path) -> ScanExtensions {
    let mut extensions = ScanExtensions::default();
    extensions.mutation_report = Some(report.to_path_buf());
    extensions
}

fn scanned(root: &Path, report: &Path) -> Report {
    scan_with(&options(root), &with_mutation_report(report)).unwrap()
}

fn json_report(report: &Report) -> Value {
    serde_json::from_str(&render(report, Format::Json).unwrap()).unwrap()
}

fn index_of(report: &Report, name: &str) -> usize {
    report
        .functions
        .iter()
        .position(|f| f.name == name)
        .unwrap_or_else(|| panic!("function {name} not in {:?}", report.functions))
}

/// Mirrors the mutant-id derivation observed in Oximut 1.0.0-rc.1 and checked
/// by `Mutation::for_source`: root-relative path, whole source, then the edit.
fn mutant_id(relative: &str, source: &str, edit: &Edit) -> String {
    let mut hash = Sha256::new();
    for bytes in [
        relative.as_bytes(),
        source.as_bytes(),
        edit.start.to_string().as_bytes(),
        edit.end.to_string().as_bytes(),
        edit.operator.as_bytes(),
        edit.replacement.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hex::encode(hash.finalize())
}

struct Edit {
    start: usize,
    end: usize,
    operator: String,
    original: String,
    replacement: String,
    outcome: String,
}

/// `span` 0 means the whole needle; otherwise the edit starts at the needle
/// and covers `span` bytes.
fn edit(
    source: &str,
    needle: &str,
    span: usize,
    operator: &str,
    replacement: &str,
    outcome: &str,
) -> Edit {
    let start = source.find(needle).expect("needle in source");
    let end = start + if span == 0 { needle.len() } else { span };
    Edit {
        start,
        end,
        operator: operator.to_owned(),
        original: source[start..end].to_owned(),
        replacement: replacement.to_owned(),
        outcome: outcome.to_owned(),
    }
}

/// A minimal valid report the loader accepts; the recorded fixtures cover the
/// long form, this covers the cases they do not.
fn position(source: &str, offset: usize) -> Value {
    let (mut line, mut column, mut previous_cr) = (1, 1, false);
    for ch in source[..offset].chars() {
        if ch == '\n' && previous_cr {
            previous_cr = false;
            continue;
        }
        if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            line += 1;
            column = 1;
        } else {
            column += ch.len_utf16();
        }
        previous_cr = ch == '\r';
    }
    json!({"offset":offset,"line":line,"column":column})
}

fn synthetic_report(root: &Path, relative: &str, source: &str, edits: &[Edit]) -> Value {
    let mut summary = serde_json::Map::new();
    summary.insert("total".into(), json!(edits.len()));
    for outcome in [
        "killed",
        "survived",
        "noCoverage",
        "invalid",
        "timeout",
        "infrastructureError",
        "resourceExhausted",
        "skipped",
    ] {
        summary.insert(
            outcome.into(),
            json!(
                edits
                    .iter()
                    .filter(|edit| edit.outcome.replace('-', "").eq_ignore_ascii_case(outcome))
                    .count()
            ),
        );
    }
    let scored = edits
        .iter()
        .filter(|e| matches!(e.outcome.as_str(), "killed" | "survived" | "no-coverage"))
        .count();
    let killed = edits.iter().filter(|e| e.outcome == "killed").count();
    summary.insert("scoreDenominator".into(), json!(scored));
    summary.insert(
        "score".into(),
        if scored == 0 {
            Value::Null
        } else {
            json!(100.0 * killed as f64 / scored as f64)
        },
    );
    summary.insert("complete".into(), json!(scored == edits.len()));
    json!({
        "schemaVersion": 1,
        "runId": "synthetic", "cache": {}, "resources": {},
        "baseline": {"protocol": 1, "outcome": "passed", "tests": {"passed":1,"failed":0,"skipped":0}, "errors":[]},
        "mutants": edits.iter().map(|edit| json!({
            "id": mutant_id(relative, source, edit),
            "file": root.join(relative).to_string_lossy(),
            "operator": edit.operator,
            "original": edit.original,
            "replacement": edit.replacement,
            "location": {"start": position(source, edit.start), "end": position(source, edit.end)},
            "tests": {"passed":1,"failed":0,"skipped":0}, "errors": [],
            "outcome": edit.outcome,
        })).collect::<Vec<_>>(),
        "diagnostics": ["mutator families=all"],
        "summary": summary,
    })
}

/// Writes `source` at `relative` inside the project plus a report for it.
/// The project outlives the returned root, so keep its binding alive.
fn synthetic_project(
    files: &[(&str, &str)],
    relative: &str,
    source: &str,
    edits: &[Edit],
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let project = common::project(files);
    let root = fs::canonicalize(project.path()).unwrap();
    let report = write_report(
        &project,
        "synthetic.json",
        &serde_json::to_string(&synthetic_report(&root, relative, source, edits)).unwrap(),
    );
    (project, root, report)
}

fn cover(root: &Path, relative: &str, lines: &[usize]) -> PathBuf {
    let source = fs::read_to_string(root.join(relative)).unwrap();
    let mut hit = serde_json::Map::new();
    for line in lines {
        hit.insert(line.to_string(), json!(1));
    }
    write_report(
        root,
        "coverage.json",
        &serde_json::to_string(&json!({"version": 1, "files": {relative: {
            "sha256": hex::encode(Sha256::digest(source.as_bytes())),
            "lines": hit,
        }}}))
        .unwrap(),
    )
}

fn all_lines(root: &Path, relative: &str) -> PathBuf {
    let count = fs::read_to_string(root.join(relative))
        .unwrap()
        .lines()
        .count();
    cover(root, relative, &(1..=count).collect::<Vec<_>>())
}

/// The recorded full run with its one survivor recorded as killed instead.
fn all_killed_report(root: &Path) -> Value {
    let mut report =
        serde_json::from_str::<Value>(&substituted(&fixture("full.json"), root)).unwrap();
    for mutant in report["mutants"].as_array_mut().unwrap() {
        mutant["outcome"] = json!("killed");
    }
    let summary = report["summary"].as_object_mut().unwrap();
    summary.insert("killed".into(), json!(3));
    summary.insert("survived".into(), json!(0));
    summary.insert("score".into(), json!(100.0));
    report
}

#[test]
fn surviving_mutant_fails_at_100_percent_line_coverage_and_low_complexity() {
    let (project, root) = subject_project();
    let report_path = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let mut opts = options(&root);
    opts.coverage = Some(all_lines(&root, SUBJECT));
    let report = scan_with(&opts, &with_mutation_report(&report_path)).unwrap();
    assert!(report.failed, "{report:?}");
    let mutation = report.mutation.as_ref().unwrap();
    assert!(mutation.run_complete);
    assert_eq!(mutation.mutators, MutatorScope::All);
    assert_eq!(mutation.mutants_in_functions, 3);
    assert_eq!(mutation.mutants_outside_functions, 0);
    assert_eq!(mutation.mutants_outside_selection, 0);
    assert_eq!(mutation.files_with_mutants, 1);
    assert_eq!(mutation.files_without_mutants, 0);
    for (function, evidence) in report.functions.iter().zip(&mutation.functions) {
        assert_eq!(&evidence.path, &function.path);
        assert_eq!((evidence.mutants, evidence.no_coverage), (1, 0));
        assert_eq!(evidence.evidence, EvidenceKind::Scored);
    }
    // `uncalled` sits at 100% line coverage with CC 1 (CRAP 1.0), yet its only
    // mutant survived: the separate mutation gate fails it independently.
    let uncalled = index_of(&report, "uncalled");
    let uncalled = &mutation.functions[uncalled];
    assert_eq!(
        (uncalled.killed, uncalled.survived, uncalled.no_coverage),
        (0, 1, 0)
    );
    assert!(report.functions[index_of(&report, "uncalled")].failed);
    assert_eq!(uncalled.kill_ratio, Some(0.0));
    assert_eq!(uncalled.effective_coverage, Some(0.0));
    assert_eq!(uncalled.effective_crap, Some(2.0));
    // `enabled` and `belowLimit` killed their mutants: estimates confirm, the
    // gate stays quiet, and nothing about the CRAP scores changes.
    for name in ["enabled", "belowLimit"] {
        let index = index_of(&report, name);
        let evidence = &mutation.functions[index];
        assert_eq!(
            (evidence.killed, evidence.survived, evidence.no_coverage),
            (1, 0, 0)
        );
        assert_eq!(evidence.kill_ratio, Some(1.0));
        assert_eq!(evidence.effective_coverage, Some(1.0));
        assert_eq!(evidence.effective_crap, Some(1.0));
        assert!(!report.functions[index].failed);
    }
    assert_eq!(
        report
            .mutation
            .as_ref()
            .unwrap()
            .functions_failed_by_mutation_only,
        1
    );
    // Mutation evidence never rescores: identical functions to a flagless scan.
    let mut plain = json_report(&scan(&opts).unwrap())["functions"].clone();
    let mut mutated = json_report(&report)["functions"].clone();
    for functions in [&mut plain, &mut mutated] {
        for function in functions.as_array_mut().unwrap() {
            function.as_object_mut().unwrap().remove("failed");
        }
    }
    assert_eq!(mutated, plain);
}

#[test]
fn all_killed_mutants_leave_the_gate_and_scores_unchanged() {
    let (project, root) = subject_project();
    let report_path = write_report(
        &project,
        "killed.json",
        &serde_json::to_string(&all_killed_report(&root)).unwrap(),
    );
    let mut opts = options(&root);
    opts.coverage = Some(all_lines(&root, SUBJECT));
    let report = scan_with(&opts, &with_mutation_report(&report_path)).unwrap();
    assert!(!report.failed, "{report:?}");
    let mutation = report.mutation.as_ref().unwrap();
    assert_eq!(mutation.functions_failed_by_mutation_only, 0);
    for evidence in &mutation.functions {
        assert_eq!(evidence.kill_ratio, Some(1.0));
        assert_eq!(evidence.effective_coverage, Some(1.0));
        assert_eq!(evidence.effective_crap, Some(1.0));
        assert!(!evidence.failed);
    }
    assert_eq!(report.functions.iter().filter(|f| f.failed).count(), 0);
    // The function list is identical to a scan without the flag.
    let mut plain = json_report(&scan(&opts).unwrap())["functions"].clone();
    let mut mutated = json_report(&report)["functions"].clone();
    for functions in [&mut plain, &mut mutated] {
        for function in functions.as_array_mut().unwrap() {
            function.as_object_mut().unwrap().remove("failed");
        }
    }
    assert_eq!(mutated, plain);
    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains(
            "Mutation gate: 0 functions pass on CRAP and fail the mutation gate; \
             0 functions have no mutants"
        ),
        "{text}"
    );
}

#[test]
fn default_scan_json_is_unchanged_without_the_flag() {
    let (project, root) = subject_project();
    // A report sits in the tree; without the opt-in it must be ignored entirely.
    write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let default_extensions = ScanExtensions::default();
    assert!(default_extensions.mutation_report.is_none());
    let plain = scan(&options(&root)).unwrap();
    let defaulted = scan_with(&options(&root), &default_extensions).unwrap();
    let plain_json = render(&plain, Format::Json).unwrap();
    assert_eq!(plain_json, render(&defaulted, Format::Json).unwrap());
    assert!(
        serde_json::from_str::<Value>(&plain_json)
            .unwrap()
            .get("mutation")
            .is_none()
    );
}

#[test]
fn cli_flag_fails_the_gate_and_exit_codes_follow_both_gates() {
    let (project, root) = subject_project();
    let survivor = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let killed = write_report(
        &project,
        "killed.json",
        &serde_json::to_string(&all_killed_report(&root)).unwrap(),
    );
    let run = |report: Option<&Path>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ts-crap"));
        command.arg("--root").arg(&root).arg("--format").arg("json");
        if let Some(report) = report {
            command.arg("--mutation-report").arg(report);
        }
        command.output().unwrap()
    };
    let plain = run(None);
    assert_eq!(plain.status.code(), Some(0), "{plain:?}");
    assert!(
        serde_json::from_slice::<Value>(&plain.stdout)
            .unwrap()
            .get("mutation")
            .is_none()
    );
    let flagged = run(Some(&survivor));
    assert_eq!(flagged.status.code(), Some(1), "{flagged:?}");
    let json: Value = serde_json::from_slice(&flagged.stdout).unwrap();
    assert_eq!(json["mutation"]["tool"], "oximut");
    assert_eq!(json["mutation"]["report_schema_version"], 1);
    assert_eq!(json["mutation"]["functions_failed_by_mutation_only"], 1);
    let clean = run(Some(&killed));
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
}

#[test]
fn stale_edits_are_rejected_inside_and_outside_mutant_spans() {
    for (name, edited, expected) in [
        (
            // Appends after every span, but the ids bind the whole source.
            "outside-span",
            None,
            "mutation report does not match src/subject.ts",
        ),
        (
            // Replaces the recorded original of the first mutant in place.
            "inside-span",
            Some("True"),
            "mutation report is stale for src/subject.ts",
        ),
    ] {
        let (project, root) = subject_project();
        let report = write_report(
            &project,
            "full.json",
            &substituted(&fixture("full.json"), &root),
        );
        let source = match edited {
            Some(replacement) => fixture("subject.ts").replacen("true", replacement, 1),
            None => format!(
                "{} // edited after the mutation run\n",
                fixture("subject.ts")
            ),
        };
        fs::write(root.join(SUBJECT), source).unwrap();
        let error = format!(
            "{:#}",
            scan_with(&options(&root), &with_mutation_report(&report)).unwrap_err()
        );
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn invalid_reports_are_rejected_with_specific_diagnostics() {
    let (project, root) = subject_project();
    let full = substituted(&fixture("full.json"), &root);
    let zero_summary = json!({
        "total": 0, "killed": 0, "survived": 0, "noCoverage": 0, "invalid": 0,
        "timeout": 0, "infrastructureError": 0, "resourceExhausted": 0,
        "skipped": 0, "complete": true,
    });
    let cases = [
        (
            "malformed",
            "const report = {".to_owned(),
            "invalid Oximut report JSON",
        ),
        (
            "missing-version",
            serde_json::to_string(&json!({
                "baseline": {"outcome": "passed"},
                "mutants": [],
                "diagnostics": [],
                "summary": zero_summary,
            }))
            .unwrap(),
            "schemaVersion is missing",
        ),
        (
            "future-version",
            full.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2"),
            "unsupported Oximut report schemaVersion 2",
        ),
        (
            "failed-baseline",
            full.replace("\"outcome\": \"passed\"", "\"outcome\": \"failed\""),
            "Oximut baseline outcome is \"failed\"",
        ),
        (
            "infrastructure-error",
            fixture("infrastructure-error.json").replace("<ROOT>", &root.to_string_lossy()),
            "baseline outcome is \"infrastructure-error\"",
        ),
        (
            "summary-total",
            full.replace("\"total\": 3,", "\"total\": 4,"),
            "summary does not match its mutant entries",
        ),
        (
            "summary-counts",
            full.replace("\"killed\": 2,", "\"killed\": 3,"),
            "summary does not match its mutant entries",
        ),
    ];
    for (name, body, expected) in cases {
        let report = write_report(&project, &format!("bad-{name}.json"), &body);
        let error = format!(
            "{:#}",
            scan_with(&options(&root), &with_mutation_report(&report)).unwrap_err()
        );
        assert!(error.contains(expected), "{name}: {error}");
    }

    // A mutant recorded under another root maps to no source identity here.
    let other = common::project(&[]);
    let report = write_report(
        &project,
        "bad-outside-root.json",
        &fixture("full.json").replace(
            "<ROOT>",
            &fs::canonicalize(other.path()).unwrap().to_string_lossy(),
        ),
    );
    let error = format!(
        "{:#}",
        scan_with(&options(&root), &with_mutation_report(&report)).unwrap_err()
    );
    assert!(error.contains("is not inside the project root"), "{error}");

    // Duplicate ids break the one-mutant-one-evidence invariant.
    let mut duplicated = serde_json::from_str::<Value>(&full).unwrap();
    let first = duplicated["mutants"][0].clone();
    duplicated["mutants"].as_array_mut().unwrap().push(first);
    let summary = duplicated["summary"].as_object_mut().unwrap();
    summary.insert("total".into(), json!(4));
    summary.insert("killed".into(), json!(3));
    let report = write_report(
        &project,
        "bad-duplicate-id.json",
        &serde_json::to_string(&duplicated).unwrap(),
    );
    let error = format!(
        "{:#}",
        scan_with(&options(&root), &with_mutation_report(&report)).unwrap_err()
    );
    assert!(
        error.contains("duplicate mutant id 641c30d54b8d"),
        "{error}"
    );

    // Serde rejects a duplicate of any consumed field.
    let report = write_report(
        &project,
        "bad-duplicate-key.json",
        r#"{"schemaVersion":1,"schemaVersion":1,"baseline":{"outcome":"passed"},"mutants":[],"diagnostics":[],"summary":{"total":0,"killed":0,"survived":0,"noCoverage":0,"invalid":0,"timeout":0,"infrastructureError":0,"resourceExhausted":0,"skipped":0,"complete":true}}"#,
    );
    let error = format!(
        "{:#}",
        scan_with(&options(&root), &with_mutation_report(&report)).unwrap_err()
    );
    assert!(error.contains("duplicate field `schemaVersion`"), "{error}");
}

#[test]
fn empty_report_leaves_every_function_unflagged_and_unclaimed() {
    let source = fixture("subject.ts");
    let (_project, root, report_path) =
        synthetic_project(&[(SUBJECT, &source)], SUBJECT, &source, &[]);
    let report = scanned(&root, &report_path);
    assert!(!report.failed, "{report:?}");
    let mutation = report.mutation.as_ref().unwrap();
    assert_eq!(mutation.mutants_in_functions, 0);
    assert_eq!(mutation.mutants_outside_functions, 0);
    assert_eq!(mutation.mutants_outside_selection, 0);
    assert_eq!(mutation.files_with_mutants, 0);
    assert_eq!(mutation.files_without_mutants, 1);
    assert_eq!(mutation.functions_failed_by_mutation_only, 0);
    for (function, evidence) in report.functions.iter().zip(&mutation.functions) {
        assert!(!function.failed);
        assert_eq!(evidence.evidence, EvidenceKind::None);
        assert_eq!(evidence.mutants, 0);
        assert_eq!(evidence.kill_ratio, None);
        assert_eq!(evidence.effective_coverage, None);
        assert_eq!(evidence.effective_crap, None);
    }
    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains(
            "Mutation evidence (oximut): 0 mutants in functions, 0 outside functions, \
             0 in unselected files; 1 selected files have no mutants. Mutators: all families"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "Mutation gate: 0 functions pass on CRAP and fail the mutation gate; \
             3 functions have no mutants"
        ),
        "{text}"
    );
}

#[test]
fn nested_same_line_functions_own_their_own_mutants() {
    let source = "export const LIMIT: number = 1;\nexport function outer(value: number): number { const inner = (x: number): number => x + 1; return value + inner(value) + LIMIT; }\n";
    let edits = [
        // Module-level mutant: owned by no function, fails no gate.
        edit(source, "= 1;", 0, "numeric-literal", "= 2;", "no-coverage"),
        // Inside the arrow body: owned by `inner`, not by `outer`.
        edit(source, "x + 1", 0, "arithmetic", "x + 2", "killed"),
        // After `inner` ends, still inside `outer`: owned by `outer`.
        edit(source, "+ LIMIT", 1, "arithmetic", "-", "survived"),
    ];
    let (_project, root, report_path) =
        synthetic_project(&[("nested.ts", source)], "nested.ts", source, &edits);
    let report = scanned(&root, &report_path);
    assert!(report.failed, "{report:?}");
    assert_eq!(
        (
            report.functions[0].name.as_str(),
            report.functions[1].name.as_str()
        ),
        ("outer", "outer::inner")
    );
    let mutation = report.mutation.as_ref().unwrap();
    let outer = &mutation.functions[0];
    let inner = &mutation.functions[1];
    assert_eq!((outer.mutants, outer.killed, outer.survived), (1, 0, 1));
    assert!(outer.failed, "{outer:?}");
    assert_eq!(outer.evidence, EvidenceKind::Scored);
    assert_eq!((inner.mutants, inner.killed, inner.survived), (1, 1, 0));
    assert!(!inner.failed, "{inner:?}");
    assert_eq!(inner.evidence, EvidenceKind::Scored);
    // Both functions pass the CRAP gate (CC 1, uncovered: CRAP 2.0), so the
    // survivor fails by mutation only; the module-level mutant fails nothing.
    assert_eq!(mutation.mutants_in_functions, 2);
    assert_eq!(mutation.mutants_outside_functions, 1);
    assert_eq!(mutation.mutants_outside_selection, 0);
    assert_eq!(
        report
            .mutation
            .as_ref()
            .unwrap()
            .functions_failed_by_mutation_only,
        1
    );
    assert!(mutation.functions.iter().all(|f| f.no_coverage == 0));
}

#[test]
fn subset_report_is_flagged_as_partial_evidence() {
    let (project, root) = subject_project();
    let report = write_report(
        &project,
        "subset.json",
        &substituted(&fixture("subset.json"), &root),
    );
    let report = scanned(&root, &report);
    let mutation = report.mutation.as_ref().unwrap();
    assert_eq!(mutation.mutators, MutatorScope::Subset);
    assert_eq!(mutation.mutator_families, ["boolean-literal"]);
    assert!(report.failed);
    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains(
            "Mutators: subset boolean-literal (partial evidence: other families were not run)"
        ),
        "{text}"
    );
    let xml = render(&report, Format::Xml).unwrap();
    assert!(
        xml.contains("mutators=\"subset\" mutator-families=\"boolean-literal\""),
        "{xml}"
    );
    assert_eq!(json_report(&report)["mutation"]["mutators"], "subset");

    // An unreadable diagnostic is unknown, still partial evidence.
    let unnamed = substituted(&fixture("full.json"), &root)
        .replace("\"mutator families=all\"", "\"execution=fresh\"");
    let report = scanned(&root, &write_report(&project, "unnamed.json", &unnamed));
    let mutation = report.mutation.as_ref().unwrap();
    assert_eq!(mutation.mutators, MutatorScope::Unknown);
    assert!(mutation.mutator_families.is_empty());
    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains(
            "Mutators: unknown (partial evidence: the report does not name its families)"
        ),
        "{text}"
    );
}

#[test]
fn inconclusive_mutants_count_neither_for_nor_against() {
    let (project, root) = subject_project();
    let timed_out = substituted(&fixture("full.json"), &root)
        .replace("\"outcome\": \"survived\"", "\"outcome\": \"timeout\"")
        .replace("\"survived\": 1,", "\"survived\": 0,")
        .replace("\"timeout\": 0,", "\"timeout\": 1,");
    let mut timed_out: Value = serde_json::from_str(&timed_out).unwrap();
    timed_out["summary"]["complete"] = json!(false);
    timed_out["summary"]["scoreDenominator"] = json!(2);
    timed_out["summary"]["score"] = json!(100.0);
    let timed_out = serde_json::to_string_pretty(&timed_out).unwrap();
    let report = scanned(&root, &write_report(&project, "timed-out.json", &timed_out));
    assert!(!report.failed, "{report:?}");
    let uncalled = &report.mutation.as_ref().unwrap().functions[index_of(&report, "uncalled")];
    assert_eq!(uncalled.evidence, EvidenceKind::Inconclusive);
    assert_eq!(
        (uncalled.mutants, uncalled.killed, uncalled.survived),
        (1, 0, 0)
    );
    assert_eq!(uncalled.kill_ratio, None);
    assert_eq!(uncalled.effective_coverage, None);
    assert_eq!(uncalled.effective_crap, None);
    assert!(!uncalled.failed);
    // Inconclusive functions are not listed as unkilled, and nothing fails.
    let text = render(&report, Format::Text).unwrap();
    assert!(
        !text.contains(&mutation_row(&report, "uncalled", "PASS")),
        "{text}"
    );
    assert!(
        text.contains(
            "Mutation gate: 0 functions pass on CRAP and fail the mutation gate; \
             0 functions have no mutants"
        ),
        "{text}"
    );

    // An incomplete run is labelled in the report.
    let incomplete = timed_out.replace("\"complete\": true", "\"complete\": false");
    let report = scanned(
        &root,
        &write_report(&project, "incomplete.json", &incomplete),
    );
    assert!(!report.mutation.as_ref().unwrap().run_complete);
    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains("incomplete run: inconclusive mutants count neither way"),
        "{text}"
    );
}

/// The mutation table row a function renders as, built from its own fields.
fn mutation_row(report: &Report, name: &str, status: &str) -> String {
    let index = index_of(report, name);
    let function = &report.functions[index];
    let mutation = &report.mutation.as_ref().unwrap().functions[index];
    let inconclusive =
        mutation.mutants - mutation.killed - mutation.survived - mutation.no_coverage;
    let percent = mutation
        .effective_coverage
        .map_or("n/a".to_owned(), |value| format!("{:.1}%", value * 100.0));
    let crap = mutation
        .effective_crap
        .map_or("n/a".to_owned(), |value| format!("{value:.3}"));
    format!(
        "{}:{}:{}  {}  {}  {}  {}  {}  {}  {}  {}",
        function.path,
        function.line,
        function.column,
        function.name,
        mutation.killed,
        mutation.survived,
        mutation.no_coverage,
        inconclusive,
        percent,
        crap,
        status
    )
}

#[test]
fn baselines_hold_crap_scores_only_and_never_suppress_the_mutation_gate() {
    let (project, root) = subject_project();
    let survivor = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    // A snapshot taken before any mutation evidence exists records only CRAP.
    let baseline = Baseline::capture(&scan(&options(&root)).unwrap());
    let baseline = Baseline::load(baseline.render().unwrap().as_bytes()).unwrap();
    let mut gated = scanned(&root, &survivor);
    assert!(gated.failed);
    baseline.apply(&mut gated, &[]).unwrap();
    let uncalled = index_of(&gated, "uncalled");
    assert!(gated.functions[uncalled].failed, "{gated:?}");
    assert!(gated.failed);

    // The same snapshot leaves an all-killed run passing.
    let killed = write_report(
        &project,
        "killed.json",
        &serde_json::to_string(&all_killed_report(&root)).unwrap(),
    );
    let mut clean = scanned(&root, &killed);
    assert!(!clean.failed);
    baseline.apply(&mut clean, &[]).unwrap();
    assert!(!clean.functions[uncalled].failed, "{clean:?}");
    assert!(!clean.failed);

    // A baseline may excuse an unchanged CRAP violation; the mutation gate
    // must still fail even when CRAP itself exceeds the threshold.
    let source = "export function complex(x: number) { if (x > 0) return true; if (x > 1) return false; if (x > 2) return false; return false; }\n";
    let edit = edit(source, "true", 0, "boolean-literal", "false", "survived");
    let (_project, root, input) =
        synthetic_project(&[("complex.ts", source)], "complex.ts", source, &[edit]);
    let mut plain = scan(&options(&root)).unwrap();
    assert!(plain.functions[0].crap > plain.threshold);
    let baseline = Baseline::capture(&plain);
    baseline.apply(&mut plain, &[]).unwrap();
    assert!(!plain.failed);
    let mut gated = scanned(&root, &input);
    baseline.apply(&mut gated, &[]).unwrap();
    assert!(gated.failed);
    assert!(gated.functions[0].failed);
}

#[test]
fn all_formats_agree_on_the_mutation_gate() {
    let (project, root) = subject_project();
    let report_path = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let report = scanned(&root, &report_path);
    assert_eq!(index_of(&report, "uncalled"), 2);

    let text = render(&report, Format::Text).unwrap();
    assert!(
        text.contains(
            "Mutation evidence (oximut): 3 mutants in functions, 0 outside functions, \
             0 in unselected files; 0 selected files have no mutants. Mutators: all families"
        ),
        "{text}"
    );
    assert!(
        text.contains(&mutation_row(&report, "uncalled", "FAIL")),
        "{text}"
    );
    let function = &report.functions[index_of(&report, "uncalled")];
    assert!(
        text.contains(&format!(
            "{}:{}:{}  {}  {}  {:.1}%  {:.3}  FAIL (mutation)",
            function.path,
            function.line,
            function.column,
            function.name,
            function.complexity,
            function.coverage * 100.0,
            function.crap
        )),
        "{text}"
    );
    assert!(
        text.contains(
            "Mutation gate: 1 functions pass on CRAP and fail the mutation gate; \
             0 functions have no mutants"
        ),
        "{text}"
    );

    let json = json_report(&report);
    assert_eq!(json["mutation"]["run_complete"], true);
    assert_eq!(json["mutation"]["mutants_in_functions"], 3);
    let uncalled = &json["mutation"]["functions"][2];
    assert_eq!(uncalled["evidence"], "scored");
    assert_eq!(uncalled["survived"], 1);
    assert_eq!(uncalled["kill_ratio"].as_f64().unwrap(), 0.0);
    assert_eq!(uncalled["effective_coverage"].as_f64().unwrap(), 0.0);
    assert_eq!(uncalled["effective_crap"].as_f64().unwrap(), 2.0);

    let xml = render(&report, Format::Xml).unwrap();
    assert!(
        xml.contains("<mutation-evidence tool=\"oximut\" run-complete=\"true\" mutators=\"all\""),
        "{xml}"
    );
    assert!(
        xml.contains("functions-failed-by-mutation-only=\"1\""),
        "{xml}"
    );
    assert!(
        xml.contains(
            "mutants=\"1\" killed=\"0\" survived=\"1\" no-coverage=\"0\" equivalent=\"0\" inconclusive=\"0\" effective-crap=\"2\""
        ),
        "{xml}"
    );

    let html = render(&report, Format::Html).unwrap();
    assert!(html.contains("<h2>Mutation evidence</h2>"), "{html}");
    assert!(
        html.contains("Functions with surviving or uncovered mutants"),
        "{html}"
    );
    assert!(html.contains(">uncalled</code></td>"), "{html}");

    let sarif: Value = serde_json::from_str(&render(&report, Format::Sarif).unwrap()).unwrap();
    let rules = &sarif["runs"][0]["tool"]["driver"]["rules"];
    assert_eq!(rules[2]["id"], "TSCRAP003");
    assert_eq!(
        rules[2]["shortDescription"]["text"],
        "Surviving or uncovered mutants"
    );
    let findings = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["ruleId"], "TSCRAP003");
    assert_eq!(
        findings[0]["message"]["text"],
        format!(
            "uncalled has 1 surviving and 0 uncovered mutants; mutation gate failed \
             (CRAP {:.3}, threshold 10)",
            report.functions[2].crap
        )
    );
    assert_eq!(
        findings[0]["partialFingerprints"]["function/v1"],
        report.functions[2].identity
    );
}

#[test]
fn producer_line_separators_validate_without_changing_nested_ownership() {
    for separator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
        let source = format!(
            "export function outer() {{ const s = `𝒳{separator}é`; const inner = () => true; return inner(); }}\n"
        );
        let edit = edit(&source, "true", 0, "boolean-literal", "false", "survived");
        let (_project, root, input) = synthetic_project(
            &[("separators.ts", &source)],
            "separators.ts",
            &source,
            &[edit],
        );
        let document: Value = serde_json::from_str(&fs::read_to_string(&input).unwrap()).unwrap();
        let start = &document["mutants"][0]["location"]["start"];
        assert_eq!(start["line"], 2);
        assert_eq!(
            start["column"],
            "é`; const inner = () => ".encode_utf16().count() + 1
        );
        let report = scanned(&root, &input);
        let evidence = report.mutation.as_ref().unwrap();
        assert_eq!(report.functions[1].name, "outer::inner");
        assert_eq!(evidence.functions[0].mutants, 0);
        assert_eq!(evidence.functions[1].survived, 1, "separator {separator:?}");
        assert!(report.failed);
        // The producer's redundant line/column data is still validated.
        let mut invalid = document;
        invalid["mutants"][0]["location"]["start"]["column"] = json!(1);
        fs::write(&input, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let error = scan_with(&options(&root), &with_mutation_report(&input))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Oximut location disagrees"), "{error}");
    }
}

#[test]
fn unselected_mutants_are_counted_without_affecting_selected_function_evidence() {
    let source = "export function f() { return true; }\n";
    let edit_a = edit(source, "true", 0, "boolean-literal", "false", "survived");
    let (_project, root, input) = synthetic_project(
        &[("a.ts", source), ("b.ts", source)],
        "a.ts",
        source,
        &[edit_a],
    );
    let mut document: Value = serde_json::from_str(&fs::read_to_string(&input).unwrap()).unwrap();
    let edit_b = edit(source, "true", 0, "boolean-literal", "false", "survived");
    let second = synthetic_report(&root, "b.ts", source, &[edit_b]);
    document["mutants"]
        .as_array_mut()
        .unwrap()
        .push(second["mutants"][0].clone());
    for key in ["total", "survived", "scoreDenominator"] {
        document["summary"][key] = json!(2);
    }
    fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut options = options(&root);
    options.paths = vec![PathBuf::from("a.ts")];
    let report = scan_with(&options, &with_mutation_report(&input)).unwrap();
    let evidence = report.mutation.unwrap();
    assert_eq!(report.functions.len(), 1);
    assert_eq!(evidence.mutants_in_functions, 1);
    assert_eq!(evidence.mutants_outside_selection, 1);
    assert_eq!(evidence.mutants_outside_functions, 0);
    assert_eq!(evidence.functions[0].survived, 1);
    assert_eq!(evidence.files_with_mutants, 1);
    assert_eq!(evidence.files_without_mutants, 0);
}

#[test]
fn a_function_failing_both_gates_is_a_plain_fail() {
    let source = "export function complex(x: number) { if (x > 0) return true; if (x > 1) return false; if (x > 2) return false; return false; }\n";
    let edit = edit(source, "true", 0, "boolean-literal", "false", "survived");
    let (_project, root, input) =
        synthetic_project(&[("complex.ts", source)], "complex.ts", source, &[edit]);
    let report = scanned(&root, &input);
    let f = &report.functions[0];
    assert!(f.crap > report.threshold && f.failed, "{report:?}");
    let evidence = report.mutation.as_ref().unwrap();
    assert!(evidence.functions[0].failed);
    assert_eq!(evidence.functions_failed_by_mutation_only, 0);
    let text = render(&report, Format::Text).unwrap();
    let row = format!(
        "{}:{}:{}  {}  {}  {:.1}%  {:.3}  FAIL\n",
        f.path,
        f.line,
        f.column,
        f.name,
        f.complexity,
        f.coverage * 100.0,
        f.crap
    );
    assert!(text.contains(&row), "{text}");
    assert!(!text.contains("FAIL (mutation)"), "{text}");
}

#[test]
fn mutation_tables_list_only_unkilled_functions_with_exact_inconclusive_counts() {
    let source = "export function killedOnly() { return 21; }\n\
                  export function mixed() { return 31 + 32 + 33 + 34 + 35; }\n\
                  export function uncovered() { return 41; }\n";
    let mutate =
        |needle: &str, outcome: &str| edit(source, needle, 0, "numeric-literal", "0", outcome);
    let edits = [
        mutate("21", "killed"),
        mutate("31", "killed"),
        mutate("32", "killed"),
        mutate("33", "survived"),
        mutate("34", "timeout"),
        mutate("35", "timeout"),
        mutate("41", "no-coverage"),
    ];
    let (_project, root, input) = synthetic_project(&[("m.ts", source)], "m.ts", source, &edits);
    let report = scanned(&root, &input);
    let evidence = report.mutation.as_ref().unwrap();
    let mixed = &evidence.functions[index_of(&report, "mixed")];
    assert_eq!(
        (
            mixed.mutants,
            mixed.killed,
            mixed.survived,
            mixed.no_coverage
        ),
        (5, 2, 1, 0)
    );

    let text = render(&report, Format::Text).unwrap();
    assert!(text.contains("  mixed  2  1  0  2  "), "{text}");
    assert!(text.contains("  uncovered  0  0  1  0  "), "{text}");
    assert!(!text.contains("  killedOnly  1  0  0  0  "), "{text}");

    let html = render(&report, Format::Html).unwrap();
    let cells = |name: &str, counts: &str| format!("<code>{name}</code></td>{counts}");
    assert!(
        html.contains(&cells("mixed", "<td>2</td><td>1</td><td>0</td><td>2</td>")),
        "{html}"
    );
    assert!(
        html.contains(&cells(
            "uncovered",
            "<td>0</td><td>0</td><td>1</td><td>0</td>"
        )),
        "{html}"
    );
    assert!(
        !html.contains(&cells(
            "killedOnly",
            "<td>1</td><td>0</td><td>0</td><td>0</td>"
        )),
        "{html}"
    );

    let xml = render(&report, Format::Xml).unwrap();
    assert!(
        xml.contains(
            "mutants=\"5\" killed=\"2\" survived=\"1\" no-coverage=\"0\" equivalent=\"0\" inconclusive=\"2\""
        ),
        "{xml}"
    );
}

#[test]
fn unparsable_files_count_their_mutants_in_the_selection_totals() {
    let broken = "export function broken( {\n";
    let good = "const top = 1;\nexport function f() { return true; }\n";
    let empty = "export function e() { return 0; }\n";
    let survived =
        |source: &str, needle: &str| edit(source, needle, 0, "operator", "x", "survived");
    let (_project, root, input) = synthetic_project(
        &[("bad.ts", broken), ("good.ts", good), ("empty.ts", empty)],
        "bad.ts",
        broken,
        &[
            survived(broken, "export"),
            survived(broken, "broken"),
            survived(broken, "function"),
        ],
    );
    let mut document: Value = serde_json::from_str(&fs::read_to_string(&input).unwrap()).unwrap();
    let extra = synthetic_report(
        &root,
        "good.ts",
        good,
        &[survived(good, "1"), survived(good, "true")],
    );
    for mutant in extra["mutants"].as_array().unwrap() {
        document["mutants"]
            .as_array_mut()
            .unwrap()
            .push(mutant.clone());
    }
    for key in ["total", "survived", "scoreDenominator"] {
        document["summary"][key] = json!(5);
    }
    fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut extensions = with_mutation_report(&input);
    extensions.on_syntax_error = ts_crap::OnSyntaxError::Report;
    let report = scan_with(&options(&root), &extensions).unwrap();
    assert_eq!(report.syntax_errors.len(), 1);
    let evidence = report.mutation.unwrap();
    assert_eq!(evidence.mutants_in_functions, 1);
    assert_eq!(evidence.mutants_outside_functions, 4);
    assert_eq!(evidence.mutants_outside_selection, 0);
    assert_eq!(evidence.files_with_mutants, 2);
    assert_eq!(evidence.files_without_mutants, 1);
}

/// Runs the CLI in `root` with the recorded survivor report; returns exit code and JSON.
fn cli(root: &Path, report: &Path) -> (Option<i32>, Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .args(["--format", "json", "--mutation-report"])
        .arg(report)
        .output()
        .unwrap();
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The recorded survivor in `uncalled` as a config entry; `identity` can be replaced.
fn survivor_entry(root: &Path, report: &Path, identity: Option<&str>) -> Value {
    let (_, json, _) = cli(root, report);
    let function = json["mutation"]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(json["functions"].as_array().unwrap())
        .find(|(_, f)| f["name"] == "uncalled")
        .unwrap()
        .0
        .clone();
    let mutant = serde_json::from_str::<Value>(&substituted(&fixture("full.json"), root)).unwrap()
        ["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["outcome"] == "survived")
        .unwrap()
        .clone();
    json!({
        "path": SUBJECT,
        "identity": identity.unwrap_or(function["identity"].as_str().unwrap()),
        "operator": mutant["operator"],
        "original": mutant["original"],
        "replacement": mutant["replacement"],
        "reason": "the replacement cannot change observable behaviour",
    })
}

fn write_config(root: &Path, equivalent: &[Value]) {
    fs::write(
        root.join("ts-crap.json"),
        serde_json::to_string(&json!({"version": 1, "mutation": {"equivalent": equivalent}}))
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn acknowledged_equivalent_survivor_passes_the_mutation_gate() {
    let (project, root) = subject_project();
    let report = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let entry = survivor_entry(&root, &report, None);
    // A second, distinct entry is not a duplicate; it matches nothing here.
    let other = survivor_entry(&root, &report, Some(&"0".repeat(64)));
    write_config(&root, &[entry, other]);
    let (code, json, stderr) = cli(&root, &report);
    assert_eq!(code, Some(0), "{stderr}");
    let mutation = &json["mutation"];
    let function = mutation["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["equivalent"] == 1)
        .unwrap();
    assert_eq!(function["survived"], 0);
    assert_eq!(function["failed"], false);
    assert_eq!(function["evidence"], "inconclusive");
    assert_eq!(function["mutants"], 1);
    assert_eq!(function["kill_ratio"], Value::Null);
    assert_eq!(mutation["equivalent_acknowledged"], 1);
    assert_eq!(
        mutation["unmatched_equivalents"].as_array().unwrap().len(),
        1
    );
    assert_eq!(mutation["functions_failed_by_mutation_only"], 0);
    for format in ["text", "html", "xml"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(&root)
            .args(["--format", format, "--mutation-report"])
            .arg(&report)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains("1 acknowledged by configuration; 1 configured entries matched")
                || text.contains("equivalent-acknowledged=\"1\" unmatched-equivalents=\"1\""),
            "{format}: {text}"
        );
    }
}

#[test]
fn equivalent_entry_for_another_function_identity_does_not_suppress() {
    let (project, root) = subject_project();
    let report = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let other = "0".repeat(64);
    let entry = survivor_entry(&root, &report, Some(&other));
    write_config(&root, std::slice::from_ref(&entry));
    let (code, json, _) = cli(&root, &report);
    assert_eq!(code, Some(1));
    assert_eq!(json["mutation"]["equivalent_acknowledged"], 0);
    assert_eq!(json["mutation"]["functions_failed_by_mutation_only"], 1);
    let mut unmatched = entry;
    unmatched.as_object_mut().unwrap().remove("reason");
    assert_eq!(
        json["mutation"]["unmatched_equivalents"],
        json!([unmatched])
    );
}

#[test]
fn equivalent_entries_are_ignored_without_a_mutation_report() {
    let (_project, root) = subject_project();
    write_config(
        &root,
        &[
            json!({"path": SUBJECT, "identity": "0".repeat(64), "operator": "o",
                 "original": "a", "replacement": "b", "reason": "r"}),
        ],
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(&root)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.get("mutation").is_none());
}

#[test]
fn invalid_equivalent_config_is_rejected_with_exit_code_2() {
    let (project, root) = subject_project();
    let report = write_report(
        &project,
        "full.json",
        &substituted(&fixture("full.json"), &root),
    );
    let good = survivor_entry(&root, &report, None);
    let with = |field: &str, value: Value| {
        let mut entry = good.clone();
        entry[field] = value;
        entry
    };
    for (entries, needle) in [
        (vec![with("reason", json!("  \t"))], "reason"),
        (vec![good.clone(), good.clone()], "duplicate"),
        (vec![with("identity", json!("abc"))], "identity"),
        (vec![with("path", json!("../x.ts"))], "path"),
    ] {
        write_config(&root, &entries);
        let (code, _, stderr) = cli(&root, &report);
        assert_eq!(code, Some(2), "{stderr}");
        assert!(stderr.contains(needle), "{needle}: {stderr}");
    }
    // Unknown and missing entry fields fail the strict schema too.
    for bad in [with("extra", json!(1)), {
        let mut entry = good.clone();
        entry.as_object_mut().unwrap().remove("original");
        entry
    }] {
        write_config(&root, &[bad]);
        assert_eq!(cli(&root, &report).0, Some(2));
    }
}

#[test]
fn a_no_coverage_mutant_cannot_be_acknowledged() {
    let source = "export function f(a: boolean): boolean {\n  return a;\n}\n";
    let survived = edit(source, "a;", 1, "boolean-literal", "!a", "no-coverage");
    let (_project, root, report) =
        synthetic_project(&[(SUBJECT, source)], SUBJECT, source, &[survived]);
    let (_, json, _) = cli(&root, &report);
    let identity = json["functions"][0]["identity"]
        .as_str()
        .unwrap()
        .to_owned();
    write_config(
        &root,
        &[
            json!({"path": SUBJECT, "identity": identity, "operator": "boolean-literal",
                 "original": "a", "replacement": "!a", "reason": "r"}),
        ],
    );
    let (code, json, _) = cli(&root, &report);
    assert_eq!(code, Some(1));
    let function = &json["mutation"]["functions"][0];
    assert_eq!(function["no_coverage"], 1);
    assert_eq!(function["equivalent"], 0);
    assert_eq!(function["failed"], true);
    assert_eq!(
        json["mutation"]["unmatched_equivalents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

const OTHER: &str = "src/other.ts";
const OTHER_SOURCE: &str = "export function other(a: number): number {\n  return a;\n}\n";

fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// The recorded subject plus a mutant-free `src/other.ts`.
fn two_file_project() -> (tempfile::TempDir, PathBuf) {
    let project = common::project(&[(SUBJECT, &fixture("subject.ts")), (OTHER, OTHER_SOURCE)]);
    let root = fs::canonicalize(project.path()).unwrap();
    (project, root)
}

/// A fixture with the `sources` and `mutators` fields newer Oximut emits.
fn with_sources(root: &Path, fixture_name: &str, sources: &[(&str, &str)]) -> Value {
    let mut report: Value =
        serde_json::from_str(&substituted(&fixture(fixture_name), root)).unwrap();
    let entries: Vec<Value> = sources
        .iter()
        .map(|(relative, source)| {
            let file = root.join(relative).to_string_lossy().into_owned();
            let mutants = report["mutants"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|mutant| mutant["file"] == file.as_str())
                .count();
            json!({"file": file, "sha256": sha256_hex(source), "mutants": mutants})
        })
        .collect();
    report["sources"] = Value::Array(entries);
    report["mutators"] = json!({"selection": "all", "families": ["arithmetic", "boolean-literal", "comparison-boundary"]});
    report
}

fn try_scan(root: &Path, project: &tempfile::TempDir, report: &Value) -> anyhow::Result<Report> {
    let path = write_report(project, "sources.json", &report.to_string());
    scan_with(&options(root), &with_mutation_report(&path))
}

#[test]
fn source_hashes_bind_every_listed_file_including_ones_without_mutants() {
    let (project, root) = two_file_project();
    let report = with_sources(
        &root,
        "full.json",
        &[(SUBJECT, &fixture("subject.ts")), (OTHER, OTHER_SOURCE)],
    );
    let scanned = try_scan(&root, &project, &report).unwrap();
    let mutation = scanned.mutation.as_ref().unwrap();
    assert_eq!(mutation.source_binding, "oximut-source-sha256");
    assert_eq!(mutation.files_without_mutants, 1);
    assert_eq!(mutation.files_not_in_report, Some(0));
    assert_eq!(mutation.mutators, MutatorScope::All);
    assert!(mutation.mutator_families.is_empty());
    let json = json_report(&scanned);
    assert_eq!(json["mutation"]["source_binding"], "oximut-source-sha256");
    assert_eq!(json["mutation"]["files_not_in_report"], 0);
    let xml = render(&scanned, Format::Xml).unwrap();
    assert!(
        xml.contains("source-binding=\"oximut-source-sha256\" files-not-in-report=\"0\"/>"),
        "{xml}"
    );
    assert!(xml.contains("files-with-mutants=\"1\""), "{xml}");

    // A file with no mutants is now bound: editing it makes the report stale.
    fs::write(
        root.join(OTHER),
        OTHER_SOURCE.replace("return a", "return a + 1"),
    )
    .unwrap();
    let error = try_scan(&root, &project, &report).unwrap_err().to_string();
    assert!(
        error.contains("mutation report is stale for src/other.ts")
            && error.contains("SHA256 mismatch"),
        "{error}"
    );
    fs::write(root.join(OTHER), OTHER_SOURCE).unwrap();

    // With hashes, mutant ids are opaque again: a different derivation passes.
    let mut opaque = report.clone();
    for mutant in opaque["mutants"].as_array_mut().unwrap() {
        let id = mutant["id"].as_str().unwrap();
        mutant["id"] = json!(sha256_hex(id));
    }
    try_scan(&root, &project, &opaque).unwrap();

    // Byte ranges are still checked against the hashed source.
    let mut shifted = report.clone();
    let start = &mut shifted["mutants"][0]["location"]["start"]["offset"];
    *start = json!(start.as_u64().unwrap() + 1);
    let error = try_scan(&root, &project, &shifted).unwrap_err().to_string();
    assert!(error.contains("expects"), "{error}");
}

#[test]
fn a_rc1_report_without_sources_keeps_the_mutant_id_binding() {
    let (project, root) = two_file_project();
    let report: Value = serde_json::from_str(&substituted(&fixture("full.json"), &root)).unwrap();
    let scanned = try_scan(&root, &project, &report).unwrap();
    let mutation = scanned.mutation.as_ref().unwrap();
    assert_eq!(mutation.source_binding, "oximut-1.0.0-rc.1-mutant-id");
    assert_eq!(mutation.files_not_in_report, None);
    assert!(json_report(&scanned)["mutation"]["files_not_in_report"].is_null());
}

#[test]
fn selected_files_missing_from_sources_are_counted_as_not_run() {
    let (project, root) = two_file_project();
    let report = with_sources(&root, "full.json", &[(SUBJECT, &fixture("subject.ts"))]);
    let scanned = try_scan(&root, &project, &report).unwrap();
    let mutation = scanned.mutation.as_ref().unwrap();
    assert_eq!(mutation.files_not_in_report, Some(1));
    let text = render(&scanned, Format::Text).unwrap();
    assert!(
        text.contains("1 selected files have no mutants (1 not run by Oximut)"),
        "{text}"
    );
}

#[test]
fn sources_must_agree_with_the_mutant_list() {
    let (project, root) = two_file_project();
    let subject = fixture("subject.ts");
    let good = with_sources(
        &root,
        "full.json",
        &[(SUBJECT, &subject), (OTHER, OTHER_SOURCE)],
    );
    let rejects = |report: &Value, needle: &str| {
        let error = format!("{:#}", try_scan(&root, &project, report).unwrap_err());
        assert!(error.contains(needle), "{needle:?} not in {error}");
    };

    let mut miscounted = good.clone();
    miscounted["sources"][1]["mutants"] = json!(2);
    rejects(&miscounted, "claims 2 mutants, the report has 0");

    let mut unlisted = good.clone();
    unlisted["sources"].as_array_mut().unwrap().remove(0);
    rejects(
        &unlisted,
        "has mutants for src/subject.ts but no sources entry",
    );

    let mut duplicate = good.clone();
    let first = duplicate["sources"][1].clone();
    duplicate["sources"].as_array_mut().unwrap().push(first);
    rejects(
        &duplicate,
        "duplicate Oximut sources entry for src/other.ts",
    );

    let mut bad_hash = good.clone();
    bad_hash["sources"][0]["sha256"] = json!("not-a-hash");
    rejects(&bad_hash, "invalid Oximut source SHA256 for src/subject.ts");

    let mut outside = good.clone();
    outside["sources"][1]["file"] = json!("/elsewhere/other.ts");
    rejects(&outside, "is not inside the project root");
}

#[test]
fn the_mutators_field_sets_the_scope_and_must_agree_with_diagnostics() {
    let (project, root) = subject_project();
    let subject = fixture("subject.ts");
    let mut subset = with_sources(&root, "subset.json", &[(SUBJECT, &subject)]);
    subset["mutators"] = json!({"selection": "subset", "families": ["boolean-literal"]});
    let scanned = try_scan(&root, &project, &subset).unwrap();
    let mutation = scanned.mutation.as_ref().unwrap();
    assert_eq!(mutation.mutators, MutatorScope::Subset);
    assert_eq!(mutation.mutator_families, ["boolean-literal"]);
    // A filtered run's survivors still fail: they are real evidence.
    assert!(scanned.failed);

    // Without the diagnostic, the field alone decides the scope.
    let mut field_only = subset.clone();
    field_only["diagnostics"] = json!([]);
    let scanned = try_scan(&root, &project, &field_only).unwrap();
    assert_eq!(
        scanned.mutation.as_ref().unwrap().mutators,
        MutatorScope::Subset
    );

    let mut contradicting = subset.clone();
    contradicting["mutators"] = json!({"selection": "all", "families": ["arithmetic", "boolean-literal", "comparison-boundary"]});
    let error = format!(
        "{:#}",
        try_scan(&root, &project, &contradicting).unwrap_err()
    );
    assert!(
        error.contains("disagrees with its mutator families diagnostic"),
        "{error}"
    );

    // Diagnostics that disagree with each other, or cannot be read, also
    // contradict the field rather than being skipped as unknown.
    for diagnostics in [
        json!(["mutator families=boolean-literal", "mutator families=all"]),
        json!([
            "mutator families=boolean-literal",
            "mutator families=not-a-family"
        ]),
    ] {
        let mut corrupt = subset.clone();
        corrupt["diagnostics"] = diagnostics;
        let error = format!("{:#}", try_scan(&root, &project, &corrupt).unwrap_err());
        assert!(
            error.contains("disagrees with its mutator families diagnostic"),
            "{error}"
        );
    }

    // An explicit null is malformed, not a quiet fall back to the rc1 binding.
    for field in ["sources", "mutators"] {
        let mut null = subset.clone();
        null[field] = Value::Null;
        assert!(try_scan(&root, &project, &null).is_err(), "{field}: null");
    }

    let mut empty = subset.clone();
    empty["mutators"] = json!({"selection": "subset", "families": []});
    let error = format!("{:#}", try_scan(&root, &project, &empty).unwrap_err());
    assert!(error.contains("subset names no families"), "{error}");
}

#[test]
fn an_acknowledged_survivor_leaves_the_inconclusive_count_and_the_table_row_exact() {
    let source = "export function mixed() { return 31 + 32 + 33 + 34 + 35; }\n";
    let mutate = |needle: &str, outcome: &str, replacement: &str| {
        edit(source, needle, 0, "numeric-literal", replacement, outcome)
    };
    let edits = [
        mutate("31", "killed", "0"),
        mutate("32", "killed", "0"),
        mutate("33", "survived", "0"),
        mutate("34", "survived", "1"),
        mutate("35", "timeout", "0"),
    ];
    let (_project, root, input) = synthetic_project(&[("m.ts", source)], "m.ts", source, &edits);
    let report = scanned(&root, &input);
    // Nothing configured: no equivalents line in any human format.
    for format in [Format::Text, Format::Html] {
        let output = render(&report, format).unwrap();
        assert!(!output.contains("Equivalent mutants"), "{output}");
    }
    let identity = report.functions[index_of(&report, "mixed")]
        .identity
        .clone();
    write_config(
        &root,
        &[
            json!({"path": "m.ts", "identity": identity, "operator": "numeric-literal",
                 "original": "34", "replacement": "1", "reason": "34 and 1 are both truthy here"}),
        ],
    );
    let report = scanned(&root, &input);
    let mixed = &report.mutation.as_ref().unwrap().functions[index_of(&report, "mixed")];
    assert_eq!(
        (
            mixed.mutants,
            mixed.killed,
            mixed.survived,
            mixed.equivalent
        ),
        (5, 2, 1, 1)
    );
    assert!(mixed.failed);
    let text = render(&report, Format::Text).unwrap();
    assert!(text.contains("  mixed  2  1  0  1  "), "{text}");
    assert!(
        text.contains("Equivalent mutants: 1 acknowledged by configuration; 0 configured entries matched no mutant"),
        "{text}"
    );
    // The XML row adds up: 2 killed + 1 survived + 1 equivalent + 1 inconclusive.
    let xml = render(&report, Format::Xml).unwrap();
    assert!(
        xml.contains("mutants=\"5\" killed=\"2\" survived=\"1\" no-coverage=\"0\" equivalent=\"1\" inconclusive=\"1\""),
        "{xml}"
    );
}

/// `ci-run.json` is an unedited report from a real, successful Oximut run in
/// Oximut's own CI (m1 fixture, all families), with only its project root
/// replaced by `<ROOT>`. It proves ts-crap reads what Oximut actually writes.
#[test]
fn a_real_oximut_ci_report_binds_by_source_hash_and_fails_the_survivor() {
    let (project, root) = subject_project();
    let report_path = write_report(
        &project,
        "ci-run.json",
        &substituted(&fixture("ci-run.json"), &root),
    );
    let mut opts = options(&root);
    opts.coverage = Some(all_lines(&root, SUBJECT));
    let report = scan_with(&opts, &with_mutation_report(&report_path)).unwrap();
    let mutation = report.mutation.as_ref().unwrap();
    assert!(mutation.run_complete);
    assert_eq!(mutation.source_binding, "oximut-source-sha256");
    assert_eq!(mutation.files_not_in_report, Some(0));
    assert_eq!(mutation.mutators, MutatorScope::All);
    assert_eq!(mutation.mutants_in_functions, 3);
    let uncalled = &mutation.functions[index_of(&report, "uncalled")];
    assert_eq!((uncalled.killed, uncalled.survived), (0, 1));
    assert!(report.functions[index_of(&report, "uncalled")].failed);
    assert!(report.failed);

    // Any edit to the scanned source makes the real report stale.
    fs::write(root.join(SUBJECT), format!("{}\n", fixture("subject.ts"))).unwrap();
    // Scan without coverage: its own hash check would otherwise fail first.
    let error = scan_with(&options(&root), &with_mutation_report(&report_path))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("mutation report is stale for src/subject.ts")
            && error.contains("Oximut source SHA256 mismatch"),
        "{error}"
    );
}
