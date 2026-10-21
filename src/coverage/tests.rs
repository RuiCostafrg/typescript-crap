#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::Coverage;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn load(value: serde_json::Value) -> anyhow::Result<Coverage> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), serde_json::to_vec(&value).unwrap()).unwrap();
    Coverage::load(file.path())
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
            assert!(view.fraction(&BTreeSet::from([line])).is_err());
        }
    }
    assert_eq!(
        coverage
            .for_source("a.ts", source)
            .unwrap()
            .fraction(&BTreeSet::from([1]))
            .unwrap(),
        1.0
    );
}

#[test]
fn malformed_reports_are_rejected() {
    let hash = "0".repeat(64);
    for value in [
        serde_json::json!({"version":2,"files":{}}),
        serde_json::json!({"version":1,"files":{},"unexpected":true}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":"not a hash","lines":{}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"0":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"01":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"-1":1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":-1}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":1.5}}}}),
        serde_json::json!({"version":1,"files":{"a.ts":{"sha256":hash,"lines":{"1":true}}}}),
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
            .fraction(&BTreeSet::from([1, 2, 3]))
            .unwrap(),
        1.0 / 3.0
    );
    assert_eq!(
        coverage
            .for_source("absent.ts", source)
            .unwrap()
            .fraction(&BTreeSet::from([1]))
            .unwrap(),
        0.0
    );
    assert_eq!(
        coverage
            .for_source("src/a.ts", source)
            .unwrap()
            .fraction(&BTreeSet::new())
            .unwrap(),
        0.0
    );
    assert!(coverage.for_source("src/a.ts", "changed").is_err());
}
