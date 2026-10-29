//! Deterministic behavior checks for workloads protected by performance CI.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use sha2::{Digest, Sha256};
use std::{fmt::Write, fs};
use ts_crap::{ScanOptions, scan};

#[test]
/// Check every long-line function against an independent full-prefix UTF-16 reference.
fn long_unicode_tsx_line_preserves_every_function_and_utf16_column() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = String::from("\u{feff}const marker = '😀'; ");
    let mut starts = Vec::new();
    for index in 0..400 {
        write!(source, "const f{index}=").unwrap();
        starts.push(source.len());
        source.push_str("(flag:boolean)=>flag?<span>😀</span>:<span/>; ");
    }
    fs::write(dir.path().join("unicode.tsx"), &source).unwrap();
    let report = scan(&ScanOptions {
        root: dir.path().into(),
        threshold: Some(5.0),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(report.functions.len(), starts.len());
    assert!(report.failed);
    for (index, (function, offset)) in report.functions.iter().zip(starts).enumerate() {
        assert_eq!(function.name, format!("f{index}"));
        assert_eq!(function.line, 1);
        assert_eq!(function.column, offset + 1);
        // Full-prefix reference is independent of the production UTF-16 cursor.
        assert_eq!(
            function.column_utf16,
            source[..offset].encode_utf16().count() + 1
        );
        assert_eq!(
            (function.complexity, function.coverage, function.crap),
            (2, 0.0, 6.0)
        );
        assert!(function.failed);
    }
}

#[test]
/// Localize missing coverage among many functions and reject evidence after a source edit.
fn many_covered_functions_keep_one_missing_statement_local_and_reject_staleness() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = serde_json::Map::new();
    for file in 0..10 {
        let mut source = String::new();
        for index in 0..100 {
            writeln!(
                source,
                "function f{file}_{index}(flag:boolean){{\nif(flag)return 1;\nreturn 0;\n}}"
            )
            .unwrap();
        }
        let path = format!("source-{file:02}.ts");
        fs::write(dir.path().join(&path), &source).unwrap();
        let mut lines: serde_json::Map<_, _> = (1..=400)
            .map(|line| (line.to_string(), serde_json::json!(1)))
            .collect();
        if file == 9 {
            lines.remove("398"); // Only the final function loses its if statement.
        }
        files.insert(
            path,
            serde_json::json!({
                "sha256":hex::encode(Sha256::digest(source.as_bytes())), "lines":lines
            }),
        );
    }
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        serde_json::to_vec(&serde_json::json!({"version":1,"files":files})).unwrap(),
    )
    .unwrap();
    let options = ScanOptions {
        root: dir.path().into(),
        coverage: Some(coverage),
        threshold: Some(2.0),
        require_coverage: Some(true),
        ..Default::default()
    };
    let report = scan(&options).unwrap();
    assert_eq!(report.functions.len(), 1000);
    assert_eq!(report.functions.iter().filter(|f| f.failed).count(), 1);
    assert_eq!(report.functions_missing_coverage, 1);
    assert!(report.completeness_failed);
    for function in &report.functions {
        if function.name == "f9_99" {
            assert_eq!((function.coverage, function.crap), (0.5, 2.5));
        } else {
            assert_eq!((function.coverage, function.crap), (1.0, 2.0));
            assert!(!function.failed);
        }
    }
    let changed_path = dir.path().join("source-09.ts");
    let original = fs::read_to_string(&changed_path).unwrap();
    let changed = original.replacen("return 0;", "return 2;", 1);
    assert_ne!(original, changed);
    assert_eq!(original.lines().count(), changed.lines().count());
    fs::write(changed_path, changed).unwrap();
    assert_eq!(
        scan(&options).unwrap_err().to_string(),
        "coverage SHA256 mismatch for source-09.ts: source changed since coverage was collected"
    );
}
