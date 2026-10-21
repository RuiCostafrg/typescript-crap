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
