#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{fs, path::Path, process::Command};

#[test]
fn shipped_shop_example_fails_uncovered_but_passes_with_real_full_coverage() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/pricing.mts"),
        include_str!("../examples/shop/src/pricing.mts"),
    )
    .unwrap();
    fs::write(
        dir.path().join("test.mjs"),
        include_str!("../examples/shop/test.mjs"),
    )
    .unwrap();
    let report = dir.path().join("coverage.json");
    let run = |coverage: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ts-crap"));
        cmd.arg("--root").arg(dir.path()).args(["--format", "json"]);
        if coverage {
            cmd.arg("--coverage").arg(&report);
        }
        cmd.output().unwrap()
    };
    let before = run(false);
    assert_eq!(before.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&before.stdout).unwrap();
    let functions = value["functions"].as_array().unwrap();
    assert_eq!(
        functions.iter().find(|f| f["name"] == "shipping").unwrap()["crap"],
        20.0
    );
    let coverage = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/coverage.mjs"))
        .arg("--root")
        .arg(dir.path())
        .arg("--output")
        .arg(&report)
        .args(["--", "node", "--experimental-strip-types", "test.mjs"])
        .output()
        .unwrap();
    assert!(
        coverage.status.success(),
        "{}",
        String::from_utf8_lossy(&coverage.stderr)
    );
    let after = run(true);
    assert_eq!(
        after.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&after.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&after.stdout).unwrap();
    let functions = value["functions"].as_array().unwrap();
    for f in functions {
        assert_eq!(f["coverage"], 1.0, "{}", f["name"]);
    }
    assert_eq!(
        functions.iter().find(|f| f["name"] == "shipping").unwrap()["crap"],
        4.0
    );
    assert_eq!(
        functions.iter().find(|f| f["name"] == "discount").unwrap()["crap"],
        3.0
    );
    assert_eq!(
        functions.iter().find(|f| f["name"] == "total").unwrap()["crap"],
        1.0
    );
}

fn exercise_example(name: &str, source: &str, tests: &str, expected: &[(&str, u32)]) {
    let dir = tempfile::Builder::new()
        .prefix("real TypeScript ü ")
        .tempdir()
        .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join(format!("src/{name}.mts")), source).unwrap();
    fs::write(dir.path().join("test.mjs"), tests).unwrap();
    let coverage = dir.path().join("coverage.json");
    let scan = |covered: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ts-crap"));
        command
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "json"]);
        if covered {
            command
                .arg("--coverage")
                .arg(&coverage)
                .arg("--require-coverage");
        }
        command.output().unwrap()
    };
    let uncovered = scan(false);
    assert_eq!(uncovered.status.code(), Some(1));
    let raw: serde_json::Value = serde_json::from_slice(&uncovered.stdout).unwrap();
    assert_eq!(raw["functions"].as_array().unwrap().len(), expected.len());
    for (name, complexity) in expected {
        let function = raw["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == *name)
            .unwrap();
        assert_eq!(function["complexity"], *complexity, "{name}");
    }
    let mut partial_scores = None;
    for partial in [true, false] {
        let mut collector = Command::new("node");
        collector
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/coverage.mjs"))
            .arg("--root")
            .arg(dir.path())
            .arg("--output")
            .arg(&coverage)
            .args(["--", "node", "--experimental-strip-types", "test.mjs"]);
        if partial {
            collector.arg("--partial");
        }
        let collected = collector.output().unwrap();
        assert!(
            collected.status.success(),
            "{}",
            String::from_utf8_lossy(&collected.stderr)
        );
        let result = scan(true);
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let functions = report["functions"].as_array().unwrap();
        if partial {
            assert!(
                functions
                    .iter()
                    .any(|f| f["coverage"].as_f64().unwrap() < 1.0)
            );
            partial_scores = Some(report);
        } else {
            assert_eq!(
                result.status.code(),
                Some(0),
                "{report}\n{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(report["completeness_failed"], false);
            for (function, earlier) in functions.iter().zip(
                partial_scores.as_ref().unwrap()["functions"]
                    .as_array()
                    .unwrap(),
            ) {
                assert_eq!(function["coverage"], 1.0, "{function}");
                assert!(function["crap"].as_f64().unwrap() <= earlier["crap"].as_f64().unwrap());
                assert_eq!(function["identity"], earlier["identity"]);
            }
        }
    }
    fs::write(
        dir.path().join(format!("src/{name}.mts")),
        format!("{source}\n// edited\n"),
    )
    .unwrap();
    let stale = scan(true);
    assert_eq!(stale.status.code(), Some(2));
    assert!(stale.stdout.is_empty());
}

#[test]
fn inventory_example_covers_class_methods_business_failures_and_unicode() {
    exercise_example(
        "inventory",
        include_str!("../examples/inventory/src/inventory.mts"),
        include_str!("../examples/inventory/test.mjs"),
        &[
            ("normalizeSku", 1),
            ("Inventory::constructor", 2),
            ("Inventory::restock", 4),
            ("Inventory::reserve", 5),
        ],
    );
}

#[test]
fn retry_example_covers_async_generics_exceptions_and_policy() {
    exercise_example(
        "retry",
        include_str!("../examples/retry/src/retry.mts"),
        include_str!("../examples/retry/test.mjs"),
        &[("retry", 8)],
    );
}
