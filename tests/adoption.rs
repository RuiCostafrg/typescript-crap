#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{fs, process::Command};
use ts_crap::{Format, ScanOptions, baseline::Baseline, render, scan};
fn project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), source).unwrap();
    dir
}
fn opts(dir: &tempfile::TempDir) -> ScanOptions {
    ScanOptions {
        root: dir.path().into(),
        ..Default::default()
    }
}
const BAD: &str = "export function f(a:boolean,b:boolean){if(a)return 1;if(b)return 2;return 0;}";
#[test]
fn regression_baseline_transitions_moves_ambiguity_and_validation() {
    let dir = project(BAD);
    let options = opts(&dir);
    let original = scan(&options).unwrap();
    let baseline =
        Baseline::load(Baseline::capture(&original).render().unwrap().as_bytes()).unwrap();
    let mut unchanged = scan(&options).unwrap();
    baseline.apply(&mut unchanged, &[]).unwrap();
    assert!(!unchanged.failed);
    assert!(!unchanged.functions[0].failed);
    fs::write(dir.path().join("a.ts"), format!("\n// shift\n{BAD}")).unwrap();
    let mut shifted = scan(&options).unwrap();
    baseline.apply(&mut shifted, &[]).unwrap();
    assert!(!shifted.failed);
    fs::rename(dir.path().join("a.ts"), dir.path().join("moved.ts")).unwrap();
    let mut moved = scan(&options).unwrap();
    baseline.apply(&mut moved, &[]).unwrap();
    assert!(!moved.failed);
    fs::write(
        dir.path().join("moved.ts"),
        BAD.replace("return 0;", "if(a && b)return 3;return 0;"),
    )
    .unwrap();
    let mut worse = scan(&options).unwrap();
    baseline
        .apply(&mut worse, &["a.ts=moved.ts".into()])
        .unwrap();
    assert!(worse.failed);
    fs::write(
        dir.path().join("moved.ts"),
        BAD.replace("return 0;", "return 5;"),
    )
    .unwrap();
    let mut changed_move = scan(&options).unwrap();
    baseline.apply(&mut changed_move, &[]).unwrap();
    assert!(changed_move.failed);
    let mut explicit = scan(&options).unwrap();
    baseline
        .apply(&mut explicit, &["a.ts=moved.ts".into()])
        .unwrap();
    assert!(!explicit.failed);
    fs::write(dir.path().join("duplicate.ts"), BAD).unwrap();
    fs::write(dir.path().join("moved.ts"), BAD).unwrap();
    let mut ambiguous = scan(&options).unwrap();
    baseline.apply(&mut ambiguous, &[]).unwrap();
    assert!(ambiguous.functions.iter().all(|f| f.failed));
    let mut absolute = scan(&options).unwrap();
    absolute.threshold = 11.0;
    assert!(baseline.apply(&mut absolute, &[]).is_err());
    assert!(Baseline::load(br#"{"version":2,"threshold":10,"functions":[]}"#).is_err());
    assert!(baseline.apply(&mut absolute, &["bad".into()]).is_err());
}
#[test]
fn selection_config_precedence_and_completeness() {
    let dir = project("export function f(){return 1;}");
    fs::write(dir.path().join("b.ts"), "export function b(){return 2;}").unwrap();
    fs::write(
        dir.path().join("ts-crap.json"),
        r#"{"version":1,"include":["a.ts"],"exclude":[]}"#,
    )
    .unwrap();
    let mut options = opts(&dir);
    assert_eq!(scan(&options).unwrap().files_scanned, 1);
    options.include = vec!["b.ts".into()];
    assert_eq!(scan(&options).unwrap().functions[0].name, "b");
    options.exclude = vec!["b.ts".into()];
    assert!(!scan(&options).unwrap().failed);
    options.fail_on_empty = true;
    assert!(scan(&options).unwrap().failed);
    options.exclude.clear();
    options.fail_on_empty = false;
    options.require_coverage = true;
    let report = scan(&options).unwrap();
    assert!(report.failed && report.completeness_failed);
    assert!(!report.functions[0].failed);
    assert_eq!(
        (
            report.files_missing_coverage,
            report.functions_missing_coverage
        ),
        (1, 1)
    );
    options.include = vec!["../*.ts".into()];
    assert!(scan(&options).is_err());
    fs::write(dir.path().join("ts-crap.json"), r#"{"version":2}"#).unwrap();
    assert!(scan(&opts(&dir)).is_err());
}
#[test]
fn terminal_safety_and_sarif_uri_escaping() {
    assert_eq!(
        ts_crap::escape_terminal("a\n\r\t\x1b\u{85}\u{202e}z"),
        "a\\u{a}\\u{d}\\u{9}\\u{1b}\\u{85}\\u{202e}z"
    );
    let dir = project(BAD);
    let mut report = scan(&opts(&dir)).unwrap();
    report.functions[0].path = "dir/a %?#\nü.ts".into();
    report.functions[0].name = "bad\x1b[31m\nname".into();
    let text = render(&report, Format::Text).unwrap();
    assert!(!text.contains('\x1b'));
    assert!(text.contains("\\u{a}"));
    let json: serde_json::Value =
        serde_json::from_str(&render(&report, Format::Json).unwrap()).unwrap();
    assert_eq!(json["functions"][0]["name"], report.functions[0].name);
    let sarif: serde_json::Value =
        serde_json::from_str(&render(&report, Format::Sarif).unwrap()).unwrap();
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "TSCRAP001");
    assert_eq!(
        result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "dir/a%20%25%3F%23%0A%C3%BC.ts"
    );
    assert!(report.failed);
}
#[test]
fn cli_baseline_gate_and_completeness_exit_codes() {
    let dir = project(BAD);
    let baseline = dir.path().join("baseline.json");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert_eq!(
        run(&["--write-baseline", baseline.to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(&["--baseline", baseline.to_str().unwrap()])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run(&[
            "--baseline",
            baseline.to_str().unwrap(),
            "--require-coverage"
        ])
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        run(&[
            "--baseline",
            baseline.to_str().unwrap(),
            "--output",
            baseline.to_str().unwrap()
        ])
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn bare_relative_report_destination_uses_current_directory() {
    let dir = project("function f(){return 1;}");
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .current_dir(dir.path())
        .args(["--output", "report.json", "--format", "json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(json["functions"][0]["name"], "f");
}

#[test]
fn sarif_uses_utf16_columns_after_unicode_source() {
    let dir = project("const ü='😀'; const f=()=>1;");
    let mut options = opts(&dir);
    options.threshold = 0.0;
    let report = scan(&options).unwrap();
    let f = &report.functions[0];
    assert_eq!(f.column, "const ü='😀'; const f=".len() + 1);
    assert_eq!(
        f.column_utf16,
        "const ü='😀'; const f=".encode_utf16().count() + 1
    );
    let sarif: serde_json::Value =
        serde_json::from_str(&render(&report, Format::Sarif).unwrap()).unwrap();
    assert_eq!(sarif["runs"][0]["columnKind"], "utf16CodeUnits");
    assert_eq!(
        sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startColumn"],
        f.column_utf16
    );
}

#[test]
fn fingerprints_ignore_internal_trivia_but_preserve_literals_and_names() {
    let dir = project("function f(){return 1;}");
    let first = scan(&opts(&dir)).unwrap();
    let identity = &first.functions[0].identity;
    fs::write(
        dir.path().join("a.ts"),
        "function f( ) { /* note */ return  1 ; }",
    )
    .unwrap();
    assert_eq!(&scan(&opts(&dir)).unwrap().functions[0].identity, identity);
    fs::write(dir.path().join("a.ts"), "function f(){return 2;}").unwrap();
    assert_ne!(&scan(&opts(&dir)).unwrap().functions[0].identity, identity);
    fs::write(dir.path().join("a.ts"), "function renamed(){return 1;}").unwrap();
    assert_ne!(&scan(&opts(&dir)).unwrap().functions[0].identity, identity);
}

#[test]
fn baseline_new_worse_improved_and_invalid_rename_transitions() {
    let dir = project(BAD);
    let options = opts(&dir);
    let baseline = Baseline::capture(&scan(&options).unwrap());
    fs::write(dir.path().join("a.ts"), BAD.replace("if(a)", "if(a && b)")).unwrap();
    let mut worse = scan(&options).unwrap();
    baseline.apply(&mut worse, &[]).unwrap();
    assert!(worse.failed);
    fs::write(dir.path().join("a.ts"), "function f(){return 1;}").unwrap();
    let mut improved = scan(&options).unwrap();
    baseline.apply(&mut improved, &[]).unwrap();
    assert!(!improved.failed);
    fs::write(
        dir.path().join("new.ts"),
        BAD.replace("function f(", "function newFunction("),
    )
    .unwrap();
    let mut new = scan(&options).unwrap();
    baseline.apply(&mut new, &[]).unwrap();
    assert!(new.failed);
    for renames in [
        vec!["bad".into()],
        vec!["=a".into()],
        vec!["a=".into()],
        vec!["a=b".into(), "a=c".into()],
        vec!["a=c".into(), "b=c".into()],
    ] {
        assert!(baseline.apply(&mut new, &renames).is_err());
    }
    let snapshot = baseline.render().unwrap();
    for invalid in [
        snapshot.replace("\"version\": 1", "\"version\": 4"),
        snapshot.replace("a.ts", "../a.ts"),
        snapshot.replace("\"crap\": 12.0", "\"crap\": -1.0"),
    ] {
        assert!(Baseline::load(invalid.as_bytes()).is_err());
    }
}

#[test]
fn completeness_distinguishes_zero_hits_from_absent_lines_and_files() {
    use sha2::{Digest, Sha256};
    let dir = project("function f(){\nreturn 1;\n}\n");
    let options = opts(&dir);
    let hash = hex::encode(Sha256::digest(fs::read(dir.path().join("a.ts")).unwrap()));
    let mut options = ScanOptions {
        coverage: Some(dir.path().join("cov.json")),
        require_coverage: true,
        ..options
    };
    for (lines, expected_missing) in [
        (serde_json::json!({"2":0}), 0),
        (serde_json::json!({"1":1}), 1),
    ] {
        fs::write(
            dir.path().join("cov.json"),
            serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":lines}}})
                .to_string(),
        )
        .unwrap();
        let report = scan(&options).unwrap();
        assert_eq!(report.files_missing_coverage, 0);
        assert_eq!(report.functions_missing_coverage, expected_missing);
        assert_eq!(report.completeness_failed, expected_missing > 0);
    }
    fs::write(dir.path().join("types.ts"), "export type T=string;").unwrap();
    let report = scan(&options).unwrap();
    assert_eq!(report.files_missing_coverage, 1);
    assert!(report.failed);
    options.require_coverage = false;
    assert!(!scan(&options).unwrap().failed);
}

#[test]
fn malformed_baseline_and_selection_configuration_are_rejected() {
    let dir = project(BAD);
    let baseline = Baseline::capture(&scan(&opts(&dir)).unwrap());
    let good: serde_json::Value = serde_json::from_str(&baseline.render().unwrap()).unwrap();
    for path in [
        "", "/a.ts", "a\\b.ts", "C:a.ts", "a//b.ts", "./a.ts", "../a.ts",
    ] {
        let mut bad = good.clone();
        bad["functions"][0]["path"] = serde_json::json!(path);
        assert!(
            Baseline::load(bad.to_string().as_bytes()).is_err(),
            "{path:?}"
        );
    }
    for hash in ["short".to_string(), "g".repeat(64)] {
        let mut bad = good.clone();
        bad["functions"][0]["identity"] = serde_json::json!(hash);
        assert!(Baseline::load(bad.to_string().as_bytes()).is_err());
    }
    for pattern in ["", "/a.ts", "a\\b.ts", "../a.ts", "["] {
        let mut options = opts(&dir);
        options.include = vec![pattern.into()];
        assert!(scan(&options).is_err(), "{pattern:?}");
    }
    let mut options = opts(&dir);
    options.config = Some(dir.path().join("missing.json"));
    assert!(scan(&options).is_err());
    fs::create_dir(dir.path().join("ts-crap.json")).unwrap();
    assert!(scan(&opts(&dir)).is_err());
    fs::remove_dir(dir.path().join("ts-crap.json")).unwrap();
    let explicit = dir.path().join("selection.json");
    fs::write(&explicit, r#"{"version":1,"exclude":["a.ts"]}"#).unwrap();
    options.config = Some(explicit);
    assert_eq!(scan(&options).unwrap().files_scanned, 0);
    options.exclude = vec!["b.ts".into()];
    assert_eq!(scan(&options).unwrap().files_scanned, 1);
}

#[test]
fn cli_help_config_protection_and_completeness_sarif() {
    let dir = project("function f(){return 1;}");
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    for flag in ["--help", "--version"] {
        assert!(command(&[flag]).status.success());
    }
    let config = dir.path().join("ts-crap.json");
    fs::write(&config, r#"{"version":1}"#).unwrap();
    for flag in ["--output", "--write-baseline"] {
        let out = command(&[flag, config.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(fs::read_to_string(&config).unwrap(), r#"{"version":1}"#);
    }
    let baseline = dir.path().join("baseline.json");
    let report = dir.path().join("report.json");
    assert!(
        command(&[
            "--write-baseline",
            baseline.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
            "--format",
            "json"
        ])
        .status
        .success()
    );
    assert!(Baseline::load(&fs::read(baseline).unwrap()).is_ok());
    let out = command(&["--require-coverage", "--format", "sarif"]);
    assert_eq!(out.status.code(), Some(1));
    let sarif: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "TSCRAP002");
    let invalid = command(&["--format", "bad\x1b[31m\nformat"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(!invalid.stderr.contains(&27));
}

#[test]
fn single_star_globs_stop_at_directory_boundaries_and_double_star_recurses() {
    let dir = project("function root(){return 1;}");
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::write(dir.path().join("src/one.ts"), "function one(){return 1;}").unwrap();
    fs::write(
        dir.path().join("src/nested/two.ts"),
        "function two(){return 2;}",
    )
    .unwrap();
    let mut options = opts(&dir);
    options.include = vec!["src/*.ts".into()];
    let report = scan(&options).unwrap();
    assert_eq!(report.files_scanned, 1);
    assert_eq!(report.functions[0].name, "one");
    options.include = vec!["src/**/*.ts".into()];
    assert_eq!(scan(&options).unwrap().files_scanned, 2);
}

#[test]
fn nested_function_edits_change_only_their_owned_fingerprints() {
    let dir = project("function outer(){function inner(){return 1;} return inner();}");
    let before = scan(&opts(&dir)).unwrap();
    fs::write(
        dir.path().join("a.ts"),
        "function outer(){function inner(){return 2;} return inner();}",
    )
    .unwrap();
    let after = scan(&opts(&dir)).unwrap();
    assert_eq!(before.functions[0].identity, after.functions[0].identity);
    assert_ne!(before.functions[1].identity, after.functions[1].identity);
}

#[test]
fn coverage_gate_requires_evidence_for_type_only_files_without_functions() {
    let dir = project("export type T = string;");
    let mut options = opts(&dir);
    options.require_coverage = true;
    let report = scan(&options).unwrap();
    assert!(report.functions.is_empty());
    assert_eq!(report.files_missing_coverage, 1);
    assert_eq!(report.functions_missing_coverage, 0);
    assert!(report.completeness_failed && report.failed);
    options.require_coverage = false;
    assert!(!scan(&options).unwrap().failed);
}

#[test]
fn duplicate_qualified_names_do_not_borrow_an_unrelated_baseline_score() {
    let dir = project(BAD);
    let baseline = Baseline::capture(&scan(&opts(&dir)).unwrap());
    let first = BAD.replace("return 0;", "return 5;");
    let second = BAD.replace("return 0;", "return 6;");
    fs::write(dir.path().join("a.ts"), format!("{first}\n{second}")).unwrap();
    let mut ambiguous_current = scan(&opts(&dir)).unwrap();
    baseline.apply(&mut ambiguous_current, &[]).unwrap();
    assert_eq!(ambiguous_current.functions.len(), 2);
    assert!(ambiguous_current.functions.iter().all(|f| f.failed));

    let ambiguous_baseline = Baseline::capture(&scan(&opts(&dir)).unwrap());
    fs::write(
        dir.path().join("a.ts"),
        BAD.replace("return 0;", "return 7;"),
    )
    .unwrap();
    let mut unique_current = scan(&opts(&dir)).unwrap();
    ambiguous_baseline.apply(&mut unique_current, &[]).unwrap();
    assert!(unique_current.failed && unique_current.functions[0].failed);
}

#[cfg(target_os = "linux")]
#[test]
fn report_is_staged_on_the_destination_filesystem() {
    use std::os::unix::fs::MetadataExt;
    let shared_memory = std::path::Path::new("/dev/shm");
    if !shared_memory.is_dir() {
        eprintln!("No /dev/shm mount for cross-filesystem output test");
        return;
    }
    let dir = project("function f(){return 1;}");
    let destination = tempfile::tempdir_in(shared_memory).unwrap();
    if fs::metadata(dir.path()).unwrap().dev() == fs::metadata(destination.path()).unwrap().dev() {
        eprintln!("/dev/shm shares the project filesystem; cross-filesystem case unavailable");
        return;
    }
    let report = destination.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .current_dir(dir.path())
        .args(["--format", "json", "--output"])
        .arg(&report)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(json["functions"][0]["name"], "f");
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 1);
}
