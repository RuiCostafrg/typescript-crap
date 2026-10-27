#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use serde_json::json;
use std::{fs, path::Path};
fn collect(root: &Path, format: &str, fixture: &str, extra: &str) -> std::process::Output {
    fs::write(
        root.join("runner.cjs"),
        format!(
            "const fs=require('node:fs'); fs.writeFileSync('raw', {}); {extra}",
            serde_json::to_string(fixture).unwrap()
        ),
    )
    .unwrap();
    common::node_script("import-coverage.mjs")
        .args([
            "--root",
            root.to_str().unwrap(),
            "--format",
            format,
            "--input",
            root.join("raw").to_str().unwrap(),
            "--output",
            root.join("native.json").to_str().unwrap(),
            "--",
            "node",
            "runner.cjs",
        ])
        .output()
        .unwrap()
}
fn fixture() -> tempfile::TempDir {
    common::project(&[("a.ts", "export function f(){\nreturn 1;\n}\n")])
}

#[test]
fn independent_lcov_test_records_combine_hits_without_changing_source_identity() {
    for names in [("test_a", "test_b"), ("same", "same"), ("", "")] {
        for reversed in [false, true] {
            let dir = common::project(&[(
                "a.ts",
                "export function f(a:boolean,b:boolean){\nif(a && b)return 1;\nreturn 0;\n}\n",
            )]);
            let mut records = [
                format!("TN:{}\nSF:a.ts\nDA:2,2\nDA:3,1\nend_of_record\n", names.0),
                format!("TN:{}\nSF:a.ts\nDA:2,0\nDA:3,0\nend_of_record\n", names.1),
            ];
            if reversed {
                records.reverse();
            }
            let out = collect(dir.path(), "lcov", &records.concat(), "");
            assert!(out.status.success(), "{out:?}");
            let native: serde_json::Value =
                serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
            assert_eq!(native["files"]["a.ts"]["lines"], json!({"2":2,"3":1}));
            let report = ts_crap::scan(&ts_crap::ScanOptions {
                root: dir.path().into(),
                coverage: Some(dir.path().join("native.json")),
                require_coverage: Some(true),
                ..Default::default()
            })
            .unwrap();
            assert!(!report.failed);
            assert_eq!(report.functions[0].coverage, 1.0);
            assert_eq!(report.functions[0].crap, 3.0);
        }
    }
}

#[test]
fn real_lcov_aggregate_retains_positive_test_hits() {
    // Generated with LCOV v1.16 (dfeb7505ef372f806ddb280b52a90f41cd8169cd):
    // cd tests/fixtures/lcov && lcov -a covered.info -a uncovered.info -o aggregate.info
    let dir = common::project(&[(
        "a.ts",
        "export function f(a:boolean,b:boolean){\nif(a && b)return 1;\nreturn 0;\n}\n",
    )]);
    let out = collect(
        dir.path(),
        "lcov",
        include_str!("fixtures/lcov/aggregate.info"),
        "",
    );
    assert!(out.status.success(), "{out:?}");
    let report = ts_crap::scan(&ts_crap::ScanOptions {
        root: dir.path().into(),
        coverage: Some(dir.path().join("native.json")),
        require_coverage: Some(true),
        ..Default::default()
    })
    .unwrap();
    assert!(!report.failed);
    assert_eq!(report.functions[0].coverage, 1.0);
    assert_eq!(report.functions[0].crap, 3.0);
}

#[test]
fn lcov_repeated_records_add_counts_but_reject_ambiguous_lines_and_overflow() {
    for (raw, expected) in [
        (
            "TN:same\nSF:a.ts\nDA:2,2\nend_of_record\nTN:same\nSF:./a.ts\nDA:2,3\nend_of_record\n",
            Some(5),
        ),
        (
            "SF:a.ts\nDA:2,0\nend_of_record\nSF:a.ts\nDA:2,0\nend_of_record\n",
            Some(0),
        ),
        ("SF:a.ts\nDA:2,0\nDA:2,1\nend_of_record\n", None),
        ("SF:a.ts\nDA:2,1\nDA:2,0\nend_of_record\n", None),
        (
            "TN:a\nSF:a.ts\nDA:2,9007199254740991\nend_of_record\nTN:b\nSF:a.ts\nDA:2,1\nend_of_record\n",
            None,
        ),
        (
            "TN:a\nSF:a.ts\nDA:2,9007199254740991\nend_of_record\nTN:a\nSF:a.ts\nDA:2,1\nend_of_record\n",
            None,
        ),
        ("SF:a.ts\nTN:interrupted\nDA:2,1\nend_of_record\n", None),
    ] {
        let dir = fixture();
        let out = collect(dir.path(), "lcov", raw, "");
        if let Some(count) = expected {
            assert!(out.status.success(), "{out:?}");
            let native: serde_json::Value =
                serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
            assert_eq!(native["files"]["a.ts"]["lines"]["2"], count);
        } else {
            assert_eq!(out.status.code(), Some(2), "{out:?}");
            assert!(!dir.path().join("native.json").exists());
            assert_no_staging(dir.path());
        }
    }
}

#[test]
fn lcov_tests_add_after_conservative_source_map_overlap() {
    for reversed in [false, true] {
        let dir = fixture();
        let source = fs::read_to_string(dir.path().join("a.ts")).unwrap();
        let map = json!({"version":3,"sources":["a.ts"],"sourcesContent":[source],"names":[],"mappings":"AACA;AAAA"});
        fs::write(dir.path().join("generated.js.map"), map.to_string()).unwrap();
        fs::write(
            dir.path().join("generated.js"),
            "x\ny\n//# sourceMappingURL=generated.js.map",
        )
        .unwrap();
        let mut records = [
            "TN:partial\nSF:generated.js\nDA:1,3\nDA:2,0\nend_of_record\n",
            "TN:covered\nSF:generated.js\nDA:1,2\nDA:2,1\nend_of_record\n",
        ];
        if reversed {
            records.reverse();
        }
        let out = collect(dir.path(), "lcov", &records.concat(), "");
        assert!(out.status.success(), "{out:?}");
        let native: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
        // Each test first takes min(3,0) or min(2,1); only then do tests add.
        assert_eq!(native["files"]["a.ts"]["lines"]["2"], 1);
    }
}

#[test]
fn generated_sources_in_one_lcov_test_share_conservative_overlap() {
    for independent in [false, true] {
        let dir = fixture();
        let source = fs::read_to_string(dir.path().join("a.ts")).unwrap();
        for name in ["first", "second"] {
            fs::write(
                dir.path().join(format!("{name}.js")),
                format!("x\n//# sourceMappingURL={name}.js.map"),
            )
            .unwrap();
            fs::write(dir.path().join(format!("{name}.js.map")), json!({"version":3,"sources":["a.ts"],"sourcesContent":[source],"names":[],"mappings":"AACA"}).to_string()).unwrap();
        }
        let raw = format!(
            "TN:one\nSF:first.js\nDA:1,3\nend_of_record\nTN:{}\nSF:second.js\nDA:1,0\nend_of_record\n",
            if independent { "two" } else { "one" }
        );
        let out = collect(dir.path(), "lcov", &raw, "");
        assert!(out.status.success(), "{out:?}");
        let native: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
        assert_eq!(
            native["files"]["a.ts"]["lines"]["2"],
            if independent { 3 } else { 0 }
        );
    }
}
#[test]
fn istanbul_and_lcov_native_fixtures_roundtrip_and_detect_staleness() {
    for (format, raw) in [
        ("istanbul", json!({"a.ts":{"path":"a.ts","statementMap":{"0":{"start":{"line":2,"column":0}}},"s":{"0":1}}}).to_string()),
        ("lcov", "TN:\nSF:a.ts\nDA:2,1\nend_of_record\n".into())
    ] {
        let dir = fixture(); let out = collect(dir.path(),format,&raw, "");
        assert!(out.status.success(), "{}",String::from_utf8_lossy(&out.stderr));
        let mut options = ts_crap::ScanOptions { root: dir.path().into(), coverage:Some(dir.path().join("native.json")), require_coverage: Some(true), ..Default::default() };
        let report = ts_crap::scan(&options).unwrap(); assert!(!report.failed); assert_eq!(report.functions[0].coverage,1.0);
        fs::write(dir.path().join("a.ts"), "export function changed(){return 2;}").unwrap();
        assert!(ts_crap::scan(&options).unwrap_err().to_string().contains("SHA256 mismatch"));
        options.coverage = None; assert!(ts_crap::scan(&options).unwrap().failed);
    }
}
#[test]
fn source_maps_remap_generated_statements_and_reject_missing_or_stale_sources() {
    for kind in ["valid", "stale", "missing", "unmapped"] {
        let dir = fixture();
        let source = fs::read_to_string(dir.path().join("a.ts")).unwrap();
        let mut map = json!({"version":3,"sources":["a.ts"],"sourcesContent":[source],"names":[],"mappings":"AACA"});
        if kind == "stale" {
            map["sourcesContent"][0] = json!("old source");
        }
        if kind == "missing" {
            map["sources"][0] = json!("absent.ts");
        }
        if kind == "unmapped" {
            map["mappings"] = json!("A");
        }
        let raw = json!({"generated.js":{"path":"generated.js","inputSourceMap":map,"statementMap":{"0":{"start":{"line":1,"column":0}}},"s":{"0":3}}}).to_string();
        let out = collect(dir.path(), "istanbul", &raw, "");
        assert_eq!(
            out.status.success(),
            kind == "valid",
            "{kind}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if kind == "valid" {
            let native: serde_json::Value =
                serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
            assert_eq!(native["files"]["a.ts"]["lines"]["2"], 3);
        } else {
            assert!(!dir.path().join("native.json").exists());
        }
    }
}
#[test]
fn importer_rejects_mutation_failing_commands_and_preexisting_reports() {
    for extra in [
        "fs.appendFileSync('a.ts','// changed');",
        "process.exitCode=1;",
    ] {
        let dir = fixture();
        let out = collect(
            dir.path(),
            "lcov",
            "SF:a.ts\nDA:2,1\nend_of_record\n",
            extra,
        );
        assert!(!out.status.success());
        assert!(!dir.path().join("native.json").exists());
    }
    let dir = fixture();
    fs::write(dir.path().join("raw"), "old").unwrap();
    let out = collect(dir.path(), "lcov", "SF:a.ts\nDA:2,1\nend_of_record\n", "");
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(dir.path().join("raw")).unwrap(), "old");
}

#[test]
fn publication_failures_publish_no_partial_report_or_staging_files() {
    for (failure, code) in [("write", "ENOSPC"), ("sync", "EIO"), ("link", "ENOTSUP")] {
        let dir = fixture();
        fs::write(
            dir.path().join("runner.cjs"),
            "const fs=require('node:fs'); fs.writeFileSync('raw', 'SF:a.ts\\nDA:2,1\\nend_of_record\\n');",
        )
        .unwrap();
        let preload = dir.path().join("fault.cjs");
        let fault = r#"
            const fs = require('node:fs/promises');
            const failure = '__FAILURE__';
            const fail = () => { throw Object.assign(new Error('injected ' + failure + ' failure'), { code: '__CODE__' }); };
            const original = fs.open;
            fs.open = async (...args) => {
              const handle = await original(...args);
              const write = handle.writeFile.bind(handle);
              if (failure === 'write') handle.writeFile = async () => { await write('{"version":1'); fail(); };
              if (failure === 'sync') handle.sync = async () => fail();
              return handle;
            };
            if (failure === 'link') fs.link = async () => fail();
            require('node:module').syncBuiltinESMExports();
        "#;
        fs::write(
            &preload,
            fault
                .replace("__FAILURE__", failure)
                .replace("__CODE__", code),
        )
        .unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/import-coverage.mjs");
        let output = dir.path().join("native.json");
        let result = std::process::Command::new("node")
            .arg("--require")
            .arg(&preload)
            .arg(script)
            .args([
                "--root",
                dir.path().to_str().unwrap(),
                "--format",
                "lcov",
                "--input",
                dir.path().join("raw").to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
                "--",
                "node",
                "runner.cjs",
            ])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{failure}: {result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains(&format!("injected {failure} failure"))
        );
        assert!(!output.exists());
        assert_no_staging(dir.path());
    }
}

fn assert_no_staging(root: &Path) {
    let leftovers: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".ts-crap-coverage-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn importer_never_replaces_a_destination_created_during_the_child_command() {
    for directory in [false, true] {
        let dir = fixture();
        let extra = if directory {
            "fs.mkdirSync('native.json'); fs.writeFileSync('native.json/sentinel', 'concurrent');"
        } else {
            "fs.writeFileSync('native.json', 'concurrent');"
        };
        let result = collect(
            dir.path(),
            "lcov",
            "SF:a.ts\nDA:2,1\nend_of_record\n",
            extra,
        );
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        let output = dir.path().join("native.json");
        let sentinel = if directory {
            output.join("sentinel")
        } else {
            output
        };
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "concurrent");
        assert_no_staging(dir.path());
    }
}

#[test]
fn lcov_external_and_inline_source_maps_preserve_zero_hit_segments() {
    for inline in [false, true] {
        let dir = fixture();
        let source = fs::read_to_string(dir.path().join("a.ts")).unwrap();
        let map=json!({"version":3,"sources":["a.ts"],"sourcesContent":[source],"names":[],"mappings":"AACA;AAAA"}).to_string();
        fs::write(dir.path().join("generated.js.map"), &map).unwrap();
        let directive = if inline {
            // Fixture's inline map is generated by the real Node runtime.
            format!(
                "fs.writeFileSync('generated.js','x\\ny\\n//# sourceMappingURL=data:application/json;base64,'+Buffer.from({}).toString('base64'));",
                serde_json::to_string(&map).unwrap()
            )
        } else {
            "fs.writeFileSync('generated.js','x\\ny\\n//# sourceMappingURL=generated.js.map');"
                .into()
        };
        let out = collect(
            dir.path(),
            "lcov",
            "SF:generated.js\nDA:1,3\nDA:2,0\nend_of_record\n",
            &directive,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let native: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("native.json")).unwrap()).unwrap();
        assert_eq!(native["files"]["a.ts"]["lines"]["2"], 0);
    }
}

#[test]
fn duplicate_istanbul_evidence_is_rejected_before_publication() {
    let good = r#"{"a.ts":{"path":"a.ts","statementMap":{"0":{"start":{"line":2,"column":0}}},"s":{"0":1}}}"#;
    for raw in [
        good.replace(r#""s":{"0":1}"#, r#""s":{"0":0,"0":1}"#),
        good.replace(r#""s":{"0":1}"#, r#""s":{"0":1,"\u0030":0}"#),
        good.replace(r#""path":"a.ts""#, r#""path":"ignored.ts","path":"a.ts""#),
        good.replace(
            r#""statementMap":{"0":{"start":{"line":2,"column":0}}}"#,
            r#""statementMap":{"0":{},"0":{"start":{"line":2,"column":0}}}"#,
        ),
        format!(
            "{{{},{} }}",
            &good[1..good.len() - 1],
            &good[1..good.len() - 1]
        ),
    ] {
        let dir = fixture();
        let out = collect(dir.path(), "istanbul", &raw, "");
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("Duplicate JSON key"));
        assert!(!dir.path().join("native.json").exists());
        assert_no_staging(dir.path());
    }
}

#[test]
fn default_argument_counters_supply_direct_evidence_and_reject_invalid_metadata() {
    for count in [0, 2] {
        let dir = common::project(&[(
            "a.ts",
            "export function f(value = 1) {\n  return value;\n}\n",
        )]);
        let raw = json!({"a.ts":{"path":"a.ts", "statementMap":{"0":{"start":{"line":2,"column":2}}},
            "s":{"0":2}, "branchMap":{"0":{"type":"default-arg","locations":[{"start":{"line":1,"column":26}}]}}, "b":{"0":[count]}}}).to_string();
        let out = collect(dir.path(), "istanbul", &raw, "");
        assert!(out.status.success(), "{out:?}");
        let options = ts_crap::ScanOptions {
            root: dir.path().into(),
            coverage: Some(dir.path().join("native.json")),
            require_coverage: Some(true),
            ..Default::default()
        };
        let report = ts_crap::scan(&options).unwrap();
        assert_eq!(report.functions_missing_coverage, 0);
        assert_eq!(
            report.functions[0].coverage,
            if count == 0 { 0.5 } else { 1.0 }
        );
    }
    for counters in [json!([]), json!([1, 2]), json!([-1]), json!(null)] {
        let dir = fixture();
        let raw = json!({"a.ts":{"path":"a.ts", "statementMap":{},"s":{},
            "branchMap":{"0":{"type":"default-arg","locations":[{"start":{"line":1,"column":0}}]}}, "b":{"0":counters}}}).to_string();
        let out = collect(dir.path(), "istanbul", &raw, "");
        assert!(!out.status.success(), "{out:?}");
        assert!(!dir.path().join("native.json").exists());
    }
}
