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
    options.fail_on_empty = Some(true);
    assert!(scan(&options).unwrap().failed);
    options.exclude.clear();
    options.fail_on_empty = Some(false);
    options.require_coverage = Some(true);
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
    options.threshold = Some(0.0);
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
        require_coverage: Some(true),
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
    options.require_coverage = Some(false);
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
    options.require_coverage = Some(true);
    let report = scan(&options).unwrap();
    assert!(report.functions.is_empty());
    assert_eq!(report.files_missing_coverage, 1);
    assert_eq!(report.functions_missing_coverage, 0);
    assert!(report.completeness_failed && report.failed);
    options.require_coverage = Some(false);
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

#[cfg(unix)]
#[test]
fn unrepresentable_source_paths_cannot_borrow_coverage_or_selection_identity() {
    let dir = project(BAD);
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/a.ts"), BAD).unwrap();
    for name in ["src\\a.ts", "a:b.ts"] {
        fs::write(dir.path().join(name), BAD).unwrap();
        for include in [vec![], vec!["src/**".into()]] {
            let mut options = opts(&dir);
            options.include = include;
            let error = scan(&options).unwrap_err().to_string();
            assert!(error.contains("cannot be represented"), "{error}");
        }
        fs::remove_file(dir.path().join(name)).unwrap();
    }
    fs::write(dir.path().join("src/日本語.ts"), BAD).unwrap();
    assert_eq!(scan(&opts(&dir)).unwrap().files_scanned, 3);
}

#[test]
fn inferred_renames_are_new_but_unchanged_moves_and_anonymous_shifts_match() {
    let body = "(a:boolean,b:boolean)=>{if(a && b)return 1;return 0;}";
    for template in [
        "const OLD = BODY;",
        "let OLD; OLD = BODY;",
        "const obj = {OLD: BODY};",
        "class C { OLD = BODY; }",
    ] {
        let old = template.replace("OLD", "oldName").replace("BODY", body);
        let dir = project(&old);
        let options = opts(&dir);
        let baseline = Baseline::capture(&scan(&options).unwrap());
        fs::rename(dir.path().join("a.ts"), dir.path().join("moved.ts")).unwrap();
        let mut moved = scan(&options).unwrap();
        baseline.apply(&mut moved, &[]).unwrap();
        assert!(!moved.failed, "{template}");
        fs::write(
            dir.path().join("moved.ts"),
            old.replace("oldName", "newName"),
        )
        .unwrap();
        let mut renamed = scan(&options).unwrap();
        baseline.apply(&mut renamed, &[]).unwrap();
        assert!(renamed.failed, "{template}");
    }
    let source = format!("consume({body});");
    let dir = project(&source);
    let baseline = Baseline::capture(&scan(&opts(&dir)).unwrap());
    fs::remove_file(dir.path().join("a.ts")).unwrap();
    fs::write(
        dir.path().join("moved.ts"),
        format!("\n// trivia\n{source}"),
    )
    .unwrap();
    let mut shifted = scan(&opts(&dir)).unwrap();
    baseline.apply(&mut shifted, &[]).unwrap();
    assert!(!shifted.failed);
}

#[test]
fn streaming_formats_match_wrappers_and_propagate_writer_errors() {
    use std::io::{self, Write};
    struct FailAfter(usize);
    impl Write for FailAfter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::other("injected writer failure"));
            }
            let count = self.0.min(bytes.len());
            self.0 -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = project(BAD);
    let mut report = scan(&opts(&dir)).unwrap();
    report.completeness_failed = true;
    report.functions[0].name = "name<&\"'\t\n\r😀\u{fffe}\u{ffff}\u{1}".into();
    for format in [
        Format::Text,
        Format::Html,
        Format::Json,
        Format::Xml,
        Format::Sarif,
    ] {
        let reference = render(&report, format).unwrap();
        let mut bytes = Vec::new();
        ts_crap::render_to(&report, format, &mut bytes).unwrap();
        assert_eq!(bytes, reference.as_bytes());
        for limit in [0, reference.len() / 2, reference.len() - 1] {
            assert!(ts_crap::render_to(&report, format, FailAfter(limit)).is_err());
        }
    }
    let xml = render(&report, Format::Xml).unwrap();
    // Parse XML bytes so the declaration controls decoding on every platform.
    // Force ASCII text I/O to catch accidental dependence on Python's locale.
    let mut child = Command::new("python3").args(["-c", "import sys,xml.etree.ElementTree as E; r=E.fromstring(sys.stdin.buffer.read()); assert r[0].attrib['name']==\"name<&\\\"'\\t\\n\\r😀\\ufffd\\ufffd\\ufffd\""])
        .env("PYTHONIOENCODING", "ascii")
        .stdin(std::process::Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(xml.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let snapshot = Baseline::capture(&report);
    assert!(snapshot.render_to(FailAfter(1)).is_err());
}

#[test]
fn sarif_stream_preserves_order_fingerprints_locations_and_completeness() {
    let dir = project(BAD);
    let mut report = scan(&opts(&dir)).unwrap();
    let mut second = scan(&opts(&dir)).unwrap().functions.remove(0);
    second.name = "second".into();
    second.path = "z #/é.ts".into();
    second.column_utf16 = 7;
    second.line = 4;
    second.end_line = 9;
    report.functions.insert(0, second);
    report.completeness_failed = true;
    let sarif: serde_json::Value =
        serde_json::from_str(&render(&report, Format::Sarif).unwrap()).unwrap();
    let results = &sarif["runs"][0]["results"];
    assert_eq!(results.as_array().unwrap().len(), 3);
    for (index, function) in report.functions.iter().enumerate() {
        assert_eq!(results[index]["ruleId"], "TSCRAP001");
        assert_eq!(
            results[index]["partialFingerprints"]["function/v1"],
            function.identity
        );
        assert_eq!(
            results[index]["message"]["text"],
            format!(
                "{} has CRAP {:.3} (threshold {})",
                function.name, function.crap, report.threshold
            )
        );
        assert_eq!(
            results[index]["locations"][0]["physicalLocation"]["region"],
            serde_json::json!({"startLine":function.line,"startColumn":function.column_utf16,"endLine":function.end_line})
        );
    }
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"],
        serde_json::json!({"uri":"z%20%23/%C3%A9.ts","uriBaseId":"%SRCROOT%"})
    );
    assert_eq!(
        results[2],
        serde_json::json!({"ruleId":"TSCRAP002","level":"error","message":{"text":format!("Incomplete selection: {} files and {} functions missing coverage; 2 functions selected", report.files_missing_coverage, report.functions_missing_coverage)}})
    );
}

#[test]
fn baseline_gate_passes_exact_threshold_for_new_and_previously_covered_functions() {
    use sha2::{Digest, Sha256};
    let source = "export function f(){\n return 1;\n}\n";
    let dir = project(source);
    let mut options = opts(&dir);
    options.threshold = Some(2.0);
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        serde_json::json!({"version":1,"files":{"a.ts":{
            "sha256":hex::encode(Sha256::digest(source.as_bytes())),"lines":{"2":1}
        }}})
        .to_string(),
    )
    .unwrap();
    options.coverage = Some(coverage);
    let covered = scan(&options).unwrap();
    assert_eq!(covered.functions[0].crap, 1.0);
    let matched = Baseline::capture(&covered);
    fs::write(dir.path().join("a.ts"), "export type Empty = string;\n").unwrap();
    options.coverage = None;
    let empty = Baseline::capture(&scan(&options).unwrap());
    fs::write(dir.path().join("a.ts"), source).unwrap();
    for baseline in [matched, empty] {
        let mut current = scan(&options).unwrap();
        assert_eq!(current.functions[0].crap, current.threshold);
        baseline.apply(&mut current, &[]).unwrap();
        assert!(!current.failed);
        assert!(!current.functions[0].failed);
    }
}
