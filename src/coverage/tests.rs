#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::Coverage;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn load(value: serde_json::Value) -> anyhow::Result<Coverage> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), serde_json::to_vec(&value).unwrap()).unwrap();
    Coverage::load(file.path())
}

fn load_raw(contents: &str) -> anyhow::Result<Coverage> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), contents).unwrap();
    Coverage::load(file.path())
}

fn contains_message(error: &anyhow::Error, needle: &str) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains(needle))
}

#[test]
fn duplicate_line_and_file_entries_are_rejected_in_both_orders() {
    let hash = "0".repeat(64);
    let file = |lines: &str| format!(r#"{{"sha256":"{hash}","lines":{lines}}}"#);
    for lines in [
        r#"{"1":0,"1":1}"#,
        r#"{"1":1,"1":0}"#,
        r#"{"1":0,"1":0}"#,
        r#"{"1":0,"\u0031":1}"#,
        // The duplicate key is rejected before attempting to decode its value.
        r#"{"1":1,"1":null}"#,
    ] {
        let file = file(lines);
        let contents = format!(r#"{{"version":1,"files":{{"a.ts":{file}}}}}"#);
        let error = load_raw(&contents).unwrap_err();
        assert!(
            contains_message(&error, "duplicate coverage line: 1"),
            "{contents}: {error:#}"
        );
    }
    let zero = file(r#"{"1":0}"#);
    let hit = file(r#"{"1":1}"#);
    for entries in [
        format!(r#""a.ts":{zero},"a.ts":{hit}"#),
        format!(r#""a.ts":{hit},"a.ts":{zero}"#),
        format!(r#""a.ts":{hit},"a\u002ets":{hit}"#),
        format!(r#""a.ts":{zero},"a.ts":null"#),
    ] {
        let contents = format!(r#"{{"version":1,"files":{{{entries}}}}}"#);
        let error = load_raw(&contents).unwrap_err();
        assert!(
            contains_message(&error, "duplicate coverage file path: a.ts"),
            "{contents}: {error:#}"
        );
    }
    let empty = file("{}");
    let valid = format!(r#"{{"version":1,"files":{{"a.ts":{empty},"b.ts":{hit}}}}}"#);
    let loaded = load_raw(&valid).unwrap();
    assert_eq!(loaded.files.len(), 2);
    assert_eq!(loaded.files["b.ts"].lines[&1], 1);
}

#[test]
fn owned_line_bounds_reject_zero_and_past_eof_with_or_without_hits() {
    let source = "one();\n";
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let coverage = load(serde_json::json!({"version":1,"files":{"a.ts":{
        "sha256":hash,"lines":{"1":1}
    }}}))
    .unwrap();
    for path in ["a.ts", "absent.ts"] {
        let view = coverage.for_source(path, source).unwrap();
        for line in [0, 2] {
            assert!(view.evidence(&BTreeSet::from([line])).is_err());
        }
    }
    assert_eq!(
        coverage
            .for_source("a.ts", source)
            .unwrap()
            .evidence(&BTreeSet::from([1]))
            .unwrap()
            .fraction,
        1.0
    );
}

#[test]
fn malformed_reports_are_rejected() {
    let hash = "0".repeat(64);
    for value in [
        serde_json::json!({"version":2,"files":{}}),
        serde_json::json!({"version":1,"files":{},"unexpected":true}),
        serde_json::json!({"version":1,"files":[]}),
        serde_json::json!({"version":1,"files":null}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":"not a hash","lines":{}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"0":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"01":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"+1":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"-1":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":-1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":1.5}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":true}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":[]}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":null}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{},"extra":0}}}),
        serde_json::json!({"version":1,"files":{"../a.ts":{"sha256":hash,"lines":{}}}}),
    ] {
        assert!(
            load(value.clone()).is_err(),
            "accepted malformed report: {value}"
        );
    }
}

#[test]
fn out_of_source_lines_are_rejected_even_without_owned_statements() {
    let source = "one();\n";
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let coverage = load(serde_json::json!({"version":1,"files":{"a.ts":{
        "sha256":hash,"lines":{"2":0}
    }}}))
    .unwrap();
    assert!(coverage.for_source("a.ts", source).is_err());
}

#[test]
fn fractions_count_owned_statement_starts_not_all_reported_lines() {
    let source = "first();\nsecond();\nthird();\n";
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let coverage = load(serde_json::json!({"version":1,"files":{"src/a.ts":{
        "sha256":hash,"lines":{"1":8,"2":0}
    }}}))
    .unwrap();
    assert_eq!(
        coverage
            .for_source("src/a.ts", source)
            .unwrap()
            .evidence(&BTreeSet::from([1, 2, 3]))
            .unwrap()
            .fraction,
        1.0 / 3.0
    );
    assert_eq!(
        coverage
            .for_source("absent.ts", source)
            .unwrap()
            .evidence(&BTreeSet::from([1]))
            .unwrap()
            .fraction,
        0.0
    );
    assert_eq!(
        coverage
            .for_source("src/a.ts", source)
            .unwrap()
            .evidence(&BTreeSet::new())
            .unwrap()
            .fraction,
        0.0
    );
    assert!(coverage.for_source("src/a.ts", "changed").is_err());
}

#[test]
fn coverage_evidence_is_bounded_monotonic_and_distinguishes_absent_from_zero() {
    // Exhaustively vary absent/zero/covered evidence for three owned statements.
    let source = "a();\nb();\nc();\n";
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let owned = BTreeSet::from([1, 2, 3]);
    for mut state in 0..27 {
        let mut lines = serde_json::Map::new();
        for line in 1..=3 {
            let evidence = state % 3;
            state /= 3;
            if evidence != 0 {
                lines.insert(line.to_string(), serde_json::json!(evidence - 1));
            }
        }
        let coverage =
            load(serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":lines}}}))
                .unwrap();
        let before = coverage
            .for_source("a.ts", source)
            .unwrap()
            .evidence(&owned)
            .unwrap();
        assert_eq!(before.complete, lines.len() == 3);
        let covered = lines.values().filter(|value| **value == 1).count();
        assert_eq!(before.fraction, covered as f64 / 3.0);
        assert!((0.0..=1.0).contains(&before.fraction));
        for promoted in 1..=3 {
            let mut increased = lines.clone();
            increased.insert(promoted.to_string(), serde_json::json!(1));
            let coverage = load(
                serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":increased}}}),
            )
            .unwrap();
            let after = coverage
                .for_source("a.ts", source)
                .unwrap()
                .evidence(&owned)
                .unwrap();
            assert!(after.fraction >= before.fraction);
            assert!(!before.complete || after.complete);
        }
        let absent = coverage
            .for_source("absent.ts", source)
            .unwrap()
            .evidence(&BTreeSet::new())
            .unwrap();
        assert!(!absent.complete);
        assert_eq!(absent.fraction, 0.0);
        let empty = coverage
            .for_source("a.ts", source)
            .unwrap()
            .evidence(&BTreeSet::new())
            .unwrap();
        assert!(empty.complete);
        assert_eq!(empty.fraction, 0.0);
    }
}
