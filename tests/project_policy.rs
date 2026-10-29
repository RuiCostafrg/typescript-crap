#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(dead_code)]
mod common;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
use ts_crap::{ScanOptions, scan};

const SOURCE: &str =
    "export function choose(value: boolean) {\n  if (value) return 1;\n  return 0;\n}\n";

#[cfg(any(unix, windows))]
#[test]
/// A present broken policy link must never relax any configured gate or publish outputs.
fn dangling_default_policy_links_fail_closed_in_library_and_cli() {
    for policy in [
        json!({"version":1,"threshold":0}),
        json!({"version":1,"threshold":100,"require_coverage":true}),
        json!({"version":1,"exclude":["**"],"fail_on_empty":true}),
    ] {
        let project = common::project(&[("a.ts", SOURCE)]);
        let root = project.path();
        let target = root.join("policy.json");
        let link = root.join("ts-crap.json");
        let options = ScanOptions {
            root: root.into(),
            ..Default::default()
        };
        assert!(!scan(&options).unwrap().failed);
        fs::write(&target, policy.to_string()).unwrap();
        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&target, &link);
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_file(&target, &link);
        if let Err(error) = created {
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            eprintln!("config symlink creation unavailable: {error}");
            return;
        }
        assert!(scan(&options).unwrap().failed);
        cli(root, &[], 1);
        fs::remove_file(&target).unwrap();
        assert_eq!(
            scan(&options).unwrap_err().to_string(),
            "cannot read project configuration"
        );
        let report = root.join("report.json");
        let baseline = root.join("baseline.json");
        fs::write(&report, "old report").unwrap();
        fs::write(&baseline, "old baseline").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(root)
            .arg("--output")
            .arg(&report)
            .arg("--write-baseline")
            .arg(&baseline)
            .args([
                "--threshold",
                "100",
                "--require-coverage=false",
                "--fail-on-empty=false",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("cannot read project configuration")
        );
        assert_eq!(fs::read_to_string(report).unwrap(), "old report");
        assert_eq!(fs::read_to_string(baseline).unwrap(), "old baseline");
        fs::remove_file(link).unwrap();
        assert!(!scan(&options).unwrap().failed);
        let explicit = ScanOptions {
            config: Some(target),
            ..options
        };
        assert_eq!(
            scan(&explicit).unwrap_err().to_string(),
            "cannot read project configuration"
        );
    }
}

fn config(root: &Path, value: Value) {
    fs::write(root.join("ts-crap.json"), value.to_string()).unwrap();
}

fn cli(root: &Path, args: &[&str], status: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .args(["--format", "json"])
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn omitted_gates_keep_defaults_and_threshold_overrides_are_explicit() {
    let project = common::project(&[("a.ts", SOURCE)]);
    let root = project.path();
    for policy in [
        None,
        Some(json!({"version":1})),
        Some(json!({"version":1,"include":["a.ts"],"exclude":[]})),
    ] {
        if let Some(policy) = policy {
            config(root, policy);
        }
        let report = cli(root, &[], 0);
        assert_eq!(report["threshold"], 10.0);
        assert_eq!(report["functions"][0]["crap"], 6.0);
    }
    config(root, json!({"version":1,"threshold":5}));
    assert_eq!(cli(root, &[], 1)["threshold"], 5.0);
    // An explicit value equal to the old default must still override config.
    assert_eq!(cli(root, &["--threshold", "10"], 0)["threshold"], 10.0);
    config(root, json!({"version":1,"threshold":100}));
    assert_eq!(cli(root, &["--threshold", "0"], 1)["threshold"], 0.0);
    config(root, json!({"version":1,"threshold":6}));
    assert_eq!(cli(root, &[], 0)["threshold"], 6.0);
    config(root, json!({"version":1,"threshold":5.5}));
    assert_eq!(cli(root, &[], 1)["threshold"], 5.5);
}

#[test]
fn coverage_boolean_inherits_and_supports_both_explicit_overrides() {
    let project = common::project(&[("src/a.ts", SOURCE)]);
    let root = project.path();
    config(
        root,
        json!({"version":1,"threshold":100,"require_coverage":true}),
    );
    let inherited = cli(root, &[], 1);
    assert_eq!(inherited["completeness_failed"], true);
    assert_eq!(inherited["functions"][0]["failed"], false);
    assert_eq!(
        cli(root, &["--require-coverage=false", "src"], 0)["functions_missing_coverage"],
        1
    );
    assert_eq!(
        cli(root, &["--require-coverage=true"], 1)["completeness_failed"],
        true
    );
    config(root, json!({"version":1,"require_coverage":false}));
    cli(root, &[], 0);
    // A bare boolean flag must not consume the following positional path.
    assert_eq!(
        cli(root, &["--require-coverage", "src"], 1)["files_scanned"],
        1
    );
}

#[test]
fn empty_policy_handles_type_only_and_excluded_sources() {
    let project =
        common::project(&[("types.ts", "export interface OnlyTypes { value: string }\n")]);
    let root = project.path();
    config(root, json!({"version":1,"fail_on_empty":true}));
    assert_eq!(cli(root, &[], 1)["files_scanned"], 1);
    assert_eq!(
        cli(root, &["--fail-on-empty=false"], 0)["completeness_failed"],
        false
    );
    config(root, json!({"version":1,"fail_on_empty":false}));
    cli(root, &[], 0);
    cli(root, &["--fail-on-empty"], 1);
    cli(root, &["--fail-on-empty=true"], 1);
    config(
        root,
        json!({"version":1,"exclude":["**"],"fail_on_empty":true,"require_coverage":true}),
    );
    assert_eq!(cli(root, &[], 1)["files_scanned"], 0);
    // Coverage requirements alone do not turn an empty selection into a failure.
    cli(root, &["--fail-on-empty=false"], 0);
}

#[test]
fn cli_rejects_invalid_boolean_overrides() {
    let project = common::project(&[("a.ts", SOURCE)]);
    for flag in ["--require-coverage", "--fail-on-empty"] {
        for value in ["yes", "0", "null", ""] {
            let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
                .arg("--root")
                .arg(project.path())
                .arg(format!("{flag}={value}"))
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn rust_scan_options_use_the_same_inheritance_and_false_overrides() {
    let project = common::project(&[("a.ts", SOURCE)]);
    let root = project.path();
    config(
        root,
        json!({"version":1,"threshold":0,"require_coverage":true,"fail_on_empty":true}),
    );
    let mut options = ScanOptions {
        root: root.into(),
        ..Default::default()
    };
    let report = scan(&options).unwrap();
    assert_eq!(report.threshold, 0.0);
    assert!(report.failed && report.completeness_failed);
    options.threshold = Some(10.0);
    options.require_coverage = Some(false);
    assert!(!scan(&options).unwrap().failed);
    options.exclude = vec!["**".into()];
    assert!(scan(&options).unwrap().failed);
    options.fail_on_empty = Some(false);
    assert!(!scan(&options).unwrap().failed);
}

#[test]
fn configured_gate_distinguishes_full_zero_missing_and_stale_evidence() {
    let project = common::project(&[("a.ts", SOURCE)]);
    let root = project.path();
    config(
        root,
        json!({"version":1,"threshold":2,"require_coverage":true}),
    );
    let evidence = |lines: Value| {
        json!({"version":1,"files":{"a.ts":{
            "sha256":hex::encode(Sha256::digest(SOURCE.as_bytes())),"lines":lines
        }}})
    };
    let coverage = root.join("coverage.json");
    fs::write(&coverage, evidence(json!({"2":1,"3":1})).to_string()).unwrap();
    let args = ["--coverage", coverage.to_str().unwrap()];
    assert_eq!(cli(root, &args, 0)["functions"][0]["coverage"], 1.0);
    fs::write(&coverage, evidence(json!({"2":1,"3":0})).to_string()).unwrap();
    let uncovered = cli(root, &args, 1);
    assert_eq!(uncovered["functions_missing_coverage"], 0);
    assert_eq!(uncovered["completeness_failed"], false);
    fs::write(&coverage, evidence(json!({"2":1})).to_string()).unwrap();
    assert_eq!(cli(root, &args, 1)["completeness_failed"], true);
    fs::write(root.join("a.ts"), format!("{SOURCE}// changed\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("SHA256 mismatch"));
}

#[test]
fn invalid_policy_is_rejected_even_when_cli_would_override_it() {
    let project = common::project(&[("a.ts", SOURCE)]);
    let root = project.path();
    let destination = root.join("report.json");
    fs::write(&destination, "previous report").unwrap();
    for raw in [
        r#"{"version":1,"threshold":-1}"#,
        r#"{"version":1,"threshold":null}"#,
        r#"{"version":1,"threshold":"10"}"#,
        r#"{"version":1,"threshold":true}"#,
        r#"{"version":1,"threshold":1e999}"#,
        r#"{"version":1,"require_coverage":"true"}"#,
        r#"{"version":1,"require_coverage":null}"#,
        r#"{"version":1,"fail_on_empty":1}"#,
        r#"{"version":1,"fail_on_empty":null}"#,
        r#"{"version":1,"threshold":0,"threshold":10}"#,
        r#"{"version":1,"require_coverage":true,"require_coverage":false}"#,
        r#"{"version":1,"unknown_policy":false}"#,
        r#"{"version":2,"threshold":10}"#,
        r#"{"threshold":10}"#,
    ] {
        fs::write(root.join("ts-crap.json"), raw).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(root)
            .args([
                "--threshold",
                "100",
                "--require-coverage=false",
                "--fail-on-empty=false",
                "--output",
            ])
            .arg(&destination)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{raw}: {output:?}");
        assert!(output.stdout.is_empty(), "{raw}");
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            "previous report",
            "{raw}"
        );
    }
}

#[test]
fn explicit_config_is_caller_relative_and_selection_precedence_is_preserved() {
    let project = common::project(&[
        ("src/a.ts", SOURCE),
        ("src/b.ts", "export function b(){ return 0; }"),
    ]);
    let root = project.path();
    let caller = tempfile::tempdir().unwrap();
    fs::write(
        caller.path().join("policy.json"),
        json!({
            "version":1,"threshold":0,"require_coverage":true,
            "include":["src/a.ts"],"exclude":["src/b.ts"]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(caller.path().join("ts-crap.json"), "invalid caller config").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ts-crap"));
    command
        .current_dir(caller.path())
        .arg("--root")
        .arg(root)
        .args(["--format", "json"]);
    assert!(command.output().unwrap().status.success());
    let output = command
        .args([
            "--config",
            "policy.json",
            "--threshold",
            "10",
            "--require-coverage=false",
            "--include",
            "src/*.ts",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files_scanned"], 1);
    assert_eq!(report["functions"][0]["name"], "choose");
    assert_eq!(report["threshold"], 10.0);
}

#[test]
fn baseline_uses_resolved_threshold_and_cannot_suppress_configured_completeness() {
    let project = common::project(&[("a.ts", SOURCE)]);
    let root = project.path();
    let baseline = root.join("baseline.json");
    config(root, json!({"version":1,"threshold":6}));
    cli(root, &["--write-baseline", baseline.to_str().unwrap()], 0);
    let snapshot: Value = serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
    assert_eq!(snapshot["threshold"], 6.0);
    config(root, json!({"version":1,"threshold":5}));
    let output = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--baseline")
        .arg(&baseline)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    cli(
        root,
        &["--baseline", baseline.to_str().unwrap(), "--threshold", "6"],
        0,
    );
    config(
        root,
        json!({"version":1,"threshold":6,"require_coverage":true}),
    );
    let report = cli(root, &["--baseline", baseline.to_str().unwrap()], 1);
    assert_eq!(report["completeness_failed"], true);
    assert_eq!(report["functions"][0]["failed"], false);
}
