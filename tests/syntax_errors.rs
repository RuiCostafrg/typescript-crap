#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(dead_code)]
mod common;

use serde_json::Value;
use std::{fs, path::Path, process::Command};
use ts_crap::{OnSyntaxError, ScanExtensions, ScanOptions, scan, scan_with};

const VALID: &str = "export function ok() {\n  return 1;\n}\n";
// The first error follows a two-byte, one-unit character, so byte and UTF-16 columns differ.
const BROKEN: &str = "const é = (;\nexport function later() {\n  return 2;\n}\n";

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn report_mode() -> ScanExtensions {
    let mut extensions = ScanExtensions::default();
    extensions.on_syntax_error = OnSyntaxError::Report;
    extensions
}

#[test]
fn default_mode_still_aborts_the_whole_scan_with_the_same_message() {
    let project = common::project(&[("a.ts", VALID), ("b.ts", BROKEN)]);
    let root = project.path();
    assert_eq!(OnSyntaxError::default(), OnSyntaxError::Fail);
    assert_eq!(
        ScanExtensions::default().on_syntax_error,
        OnSyntaxError::Fail
    );
    for args in [&[][..], &["--on-syntax-error", "fail"][..]] {
        let output = run(root, args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: invalid TypeScript syntax in b.ts:1:10 (UTF-8 byte column): const é = (;\n"
        );
    }
    let options = ScanOptions {
        root: root.into(),
        ..Default::default()
    };
    let error = scan(&options).unwrap_err().to_string();
    assert_eq!(
        error,
        "invalid TypeScript syntax in b.ts:1:10 (UTF-8 byte column): const é = (;"
    );
    assert_eq!(
        scan_with(&options, &ScanExtensions::default())
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(
        run(root, &["--on-syntax-error", "skip"]).status.code(),
        Some(2)
    );
}

#[test]
fn report_mode_scores_the_rest_and_fails_completeness_under_any_policy() {
    let project = common::project(&[("a.ts", VALID), ("b.ts", BROKEN)]);
    let root = project.path();
    let report = scan_with(
        &ScanOptions {
            root: root.into(),
            threshold: Some(1000.0),
            require_coverage: Some(false),
            ..Default::default()
        },
        &report_mode(),
    )
    .unwrap();
    assert_eq!(report.files_scanned, 2);
    assert_eq!(report.files_missing_coverage, 2);
    assert_eq!(report.functions.len(), 1);
    assert_eq!(report.functions[0].path, "a.ts");
    assert!(report.completeness_failed && report.failed);
    // The pure library capture API may snapshot scored functions, but applying
    // that snapshot must never suppress a syntax completeness failure.
    let baseline = ts_crap::baseline::Baseline::capture(&report);
    let mut report = report;
    baseline.apply(&mut report, &[]).unwrap();
    assert!(report.completeness_failed && report.failed);
    let [error] = &report.syntax_errors[..] else {
        panic!("{:?}", report.syntax_errors)
    };
    assert_eq!(
        (
            error.path.as_str(),
            error.line,
            error.column,
            error.column_utf16
        ),
        ("b.ts", 1, 10, 9)
    );
    assert!(
        error
            .message
            .starts_with("invalid TypeScript syntax in b.ts:1:10")
    );

    let output = run(
        root,
        &[
            "--on-syntax-error",
            "report",
            "--format",
            "json",
            "--threshold",
            "1000",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["completeness_failed"], true);
    assert_eq!(json["syntax_errors"][0]["path"], "b.ts");
    assert_eq!(json["syntax_errors"][0]["column_utf16"], 9);
}

#[test]
fn every_format_lists_unparsable_files() {
    let project = common::project(&[("a.ts", VALID), ("dir é/b.ts", BROKEN)]);
    let root = project.path();
    let format = |name: &str| {
        let output = run(root, &["--on-syntax-error", "report", "--format", name]);
        assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
        stdout(&output)
    };
    let text = format("text");
    assert!(
        text.contains("Syntax errors: 1 files not scored (incomplete)\n"),
        "{text}"
    );
    assert!(
        text.contains("  invalid TypeScript syntax in dir é/b.ts:1:10"),
        "{text}"
    );
    let xml = format("xml");
    assert!(xml.contains("<syntax-error path=\"dir é/b.ts\" line=\"1\" column=\"10\" message=\"invalid TypeScript syntax"), "{xml}");
    let html = format("html");
    assert!(html.contains("<h2 class=\"fail\">Syntax errors: 1 files not scored</h2><ul><li><code>invalid TypeScript syntax in dir é/b.ts:1:10"), "{html}");
    let sarif: Value = serde_json::from_str(&format("sarif")).unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    let located: Vec<_> = results
        .iter()
        .filter(|r| r["locations"].is_array())
        .collect();
    assert_eq!(located.len(), 1, "{results:?}");
    assert_eq!(located[0]["ruleId"], "TSCRAP002");
    let location = &located[0]["locations"][0]["physicalLocation"];
    assert_eq!(location["artifactLocation"]["uri"], "dir%20%C3%A9/b.ts");
    assert_eq!(location["region"]["startLine"], 1);
    assert_eq!(location["region"]["startColumn"], 9);
    assert!(
        located[0]["message"]["text"]
            .as_str()
            .unwrap()
            .starts_with("Not scored: invalid TypeScript syntax")
    );
    assert!(
        results
            .iter()
            .any(|r| r["ruleId"] == "TSCRAP002" && r["locations"].is_null()),
        "aggregate completeness result"
    );
}

#[test]
fn clean_scans_render_identically_in_both_modes() {
    let project = common::project(&[("a.ts", VALID)]);
    let root = project.path();
    for format in ["text", "json", "xml", "html", "sarif"] {
        let default = run(root, &["--format", format]);
        let report = run(root, &["--format", format, "--on-syntax-error", "report"]);
        assert_eq!(default.status.code(), Some(0));
        assert_eq!(default.stdout, report.stdout, "{format}");
        assert!(!stdout(&default).contains("syntax"), "{format}");
    }
}

#[test]
fn baselines_are_refused_while_files_are_unparsable() {
    let project = common::project(&[("a.ts", VALID), ("b.ts", BROKEN)]);
    let root = project.path();
    let baseline = root.join("baseline.json");
    let output = run(
        root,
        &[
            "--on-syntax-error",
            "report",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot write a baseline while 1 selected files have syntax errors")
    );
    assert!(!baseline.exists());
    fs::remove_file(root.join("b.ts")).unwrap();
    let output = run(
        root,
        &[
            "--on-syntax-error",
            "report",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(baseline.exists());
}

#[test]
fn report_mode_still_propagates_other_errors() {
    let project = common::project(&[("a.ts", VALID), ("b.ts", BROKEN)]);
    let root = project.path();
    let error = scan_with(
        &ScanOptions {
            root: root.into(),
            coverage: Some(root.join("absent-coverage.json")),
            ..Default::default()
        },
        &report_mode(),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("reading coverage"),
        "{error:#}"
    );
}

#[test]
fn report_mode_rejects_invalid_coverage_for_unparsable_sources() {
    use sha2::{Digest, Sha256};
    let project = common::project(&[("b.ts", BROKEN)]);
    let root = project.path();
    let coverage = root.join("coverage.json");
    let hash = hex::encode(Sha256::digest(BROKEN.as_bytes()));
    for (sha256, lines, diagnostic) in [
        (
            "0".repeat(64),
            serde_json::json!({"1": 1}),
            "coverage SHA256 mismatch for b.ts",
        ),
        (
            hash.clone(),
            serde_json::json!({"999": 1}),
            "coverage line 999 is outside source b.ts",
        ),
    ] {
        fs::write(
            &coverage,
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "files": {"b.ts": {"sha256": sha256, "lines": lines}}
            }))
            .unwrap(),
        )
        .unwrap();
        let output = run(
            root,
            &[
                "--on-syntax-error",
                "report",
                "--coverage",
                coverage.to_str().unwrap(),
            ],
        );
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{output:?}"
        );
    }
    fs::write(
        &coverage,
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "files": {"b.ts": {"sha256": hash, "lines": {"1": 1}}}
        }))
        .unwrap(),
    )
    .unwrap();
    let output = run(
        root,
        &[
            "--on-syntax-error",
            "report",
            "--coverage",
            coverage.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn saved_reports_omit_source_snippets_and_count_coverage_for_unparsable_files() {
    use sha2::{Digest, Sha256};
    let source = "const privateValue = (; // source-snippet-must-stay-private\n";
    let project = common::project(&[("b.ts", source)]);
    let root = project.path();
    let coverage = root.join("coverage.json");
    fs::write(
        &coverage,
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "files": {"b.ts": {
                "sha256": hex::encode(Sha256::digest(source.as_bytes())), "lines": {"1": 1}
            }}
        }))
        .unwrap(),
    )
    .unwrap();
    for format in ["text", "json", "xml", "html", "sarif"] {
        let output = run(root, &["--on-syntax-error", "report", "--format", format]);
        assert_eq!(output.status.code(), Some(1));
        let text = stdout(&output);
        assert!(text.contains("invalid TypeScript syntax"), "{text}");
        assert!(!text.contains("source-snippet-must-stay-private"), "{text}");
        assert!(!text.contains("privateValue"), "{text}");
    }
    for (args, missing) in [
        (vec!["--on-syntax-error", "report", "--format", "json"], 1),
        (
            vec![
                "--on-syntax-error",
                "report",
                "--format",
                "json",
                "--coverage",
                coverage.to_str().unwrap(),
            ],
            0,
        ),
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(1));
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["files_missing_coverage"], missing);
        assert_eq!(json["completeness_failed"], true);
    }
}
