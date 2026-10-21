#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{fs, path::Path, process::Command};

fn runner(root: &Path, output: &Path, entry: &str) -> std::process::Output {
    Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/coverage.mjs"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--",
            "node",
            "--experimental-strip-types",
            entry,
        ])
        .current_dir(root)
        .output()
        .expect("Node must be installed to run real V8 coverage tests")
}

#[test]
fn snapshot_hash_survives_source_mutation_and_uncovered_same_line_branch_is_zero() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::Builder::new()
        .prefix("ts crap ü ")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let source = "export function branch(flag: boolean) { const emoji = '😀'; return flag ? emoji.length : 0; }\n";
    fs::write(root.join("branch ü.mts"), source).unwrap();
    fs::write(root.join("unused.cts"), "const unused: number = 1;\n").unwrap();
    for excluded in ["node_modules", ".git", "target"] {
        fs::create_dir(root.join(excluded)).unwrap();
        fs::write(root.join(excluded).join("ignored.ts"), "ignored();").unwrap();
    }
    fs::write(root.join("test.mjs"), "import { branch } from './branch ü.mts';\nimport { writeFileSync } from 'node:fs';\nimport assert from 'node:assert/strict';\nassert.equal(branch(true), 2);\nwriteFileSync('branch ü.mts', '// mutated after import\\n');\n").unwrap();
    let report = root.join("coverage.json");
    let result = runner(root, &report, "test.mjs");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let native: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(
        native["files"]["branch ü.mts"]["sha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    assert_eq!(native["files"]["branch ü.mts"]["lines"]["1"], 0);
    assert_eq!(
        native["files"]["unused.cts"]["lines"],
        serde_json::json!({})
    );
    assert_eq!(native["files"].as_object().unwrap().len(), 2);
}

#[test]
fn modified_then_restored_source_cannot_receive_original_source_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("a.mts"),
        "export function f(a: boolean, b: boolean) { if(a) return 1; if(b) return 2; return 3; }\n",
    )
    .unwrap();
    fs::write(root.join("test.mjs"),r#"
import { readFileSync, writeFileSync } from 'node:fs';
const original=readFileSync('a.mts','utf8');
try {
  const altered=original.replace('if(a) return 1;', 'void 0;        ').replace('if(b) return 2;', 'void 0;        ');
  writeFileSync('a.mts',altered);
  const { f } = await import('./a.mts');
  f(true,true);
} finally { writeFileSync('a.mts',original); }
"#).unwrap();
    let report = root.join("coverage.json");
    let result = runner(root, &report, "test.mjs");
    assert!(
        !result.status.success(),
        "modified source must be rejected at module load"
    );
    assert!(!report.exists());
}

#[test]
fn default_initializer_is_owned_coverage_not_just_complexity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("defaults.mts"),
        "export function f(\n  x = globalThis.flag ? 1 : 0\n) {\n  return x;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("test.mjs"),
        "import { f } from './defaults.mts';\nf(1);\n",
    )
    .unwrap();
    let coverage = root.join("coverage.json");
    assert!(runner(root, &coverage, "test.mjs").status.success());
    let analyze = || {
        let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(root)
            .arg("--coverage")
            .arg(&coverage)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let value = analyze();
    assert_eq!(value["functions"][0]["complexity"], 2);
    assert_eq!(value["functions"][0]["coverage"], 0.5);
    assert_eq!(value["functions"][0]["crap"], 2.5);
    fs::write(root.join("test.mjs"), "import { f } from './defaults.mts';\nf(1); globalThis.flag = true; f(); globalThis.flag = false; f();\n").unwrap();
    assert!(runner(root, &coverage, "test.mjs").status.success());
    assert_eq!(analyze()["functions"][0]["coverage"], 1.0);
}

#[test]
fn commonjs_typescript_crlf_and_bom_sources_are_collected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("c.cts"),
        "function c(n: number) {\r\n  return n + 1;\r\n}\r\nmodule.exports = { c };\r\n",
    )
    .unwrap();
    fs::write(
        root.join("bom.mts"),
        "\u{feff}export const bom = (n: number) => n + 1;\n",
    )
    .unwrap();
    fs::write(root.join("test.mjs"),"import { createRequire } from 'node:module';\nimport { bom } from './bom.mts';\nconst require=createRequire(import.meta.url);\nif(require('./c.cts').c(1)!==2 || bom(1)!==2) throw Error('bad');\n").unwrap();
    let report = root.join("coverage.json");
    let result = runner(root, &report, "test.mjs");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--coverage")
        .arg(report)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["functions"].as_array().unwrap().len(), 2);
    assert_eq!(value["functions"][0]["coverage"], 1.0);
    assert_eq!(value["functions"][1]["coverage"], 1.0);
}

#[test]
fn failing_runner_propagates_exit_code_without_publishing_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("test.mjs"), "process.exit(7);\n").unwrap();
    let report = root.join("coverage.json");
    let result = runner(root, &report, "test.mjs");
    assert_eq!(result.status.code(), Some(7));
    assert!(!report.exists());
    fs::write(&report, "existing report").unwrap();
    assert_eq!(runner(root, &report, "test.mjs").status.code(), Some(7));
    assert_eq!(fs::read_to_string(report).unwrap(), "existing report");
}

#[test]
fn real_typescript_coverage_is_accepted_by_cli_and_rejects_stale_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("src")).unwrap();
    let source = "export function choose(value: boolean): number {\n  const emoji = '😀';\n  if (value) {\n    return emoji.length;\n  }\n  return 0;\n}\n";
    fs::write(root.join("src/a.mts"), source).unwrap();
    fs::write(root.join("test.mjs"), "import assert from 'node:assert/strict';\nimport { choose } from './src/a.mts';\nassert.equal(choose(true), 2);\n").unwrap();
    let report = root.join("coverage.json");
    let result = runner(root, &report, "test.mjs");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let native: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(native["version"], 1);
    assert!(native["files"]["src/a.mts"]["lines"]["4"].as_u64().unwrap() > 0);
    assert_eq!(native["files"]["src/a.mts"]["lines"]["6"], 0);
    assert_eq!(
        native["files"]["src/a.mts"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let analyze = || {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("src/a.mts")
            .arg("--root")
            .arg(root)
            .arg("--coverage")
            .arg(&report)
            .args(["--format", "json", "--threshold", "1000"])
            .output()
            .unwrap()
    };
    let result = analyze();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let analyzed: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("CLI emits JSON");
    let functions = analyzed["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "choose");
    // Four executable statement starts: declaration, if, and two returns.
    assert_eq!(functions[0]["coverage"], 0.75);
    assert_eq!(functions[0]["complexity"], 2);
    assert_eq!(functions[0]["crap"], 2.0625);
    fs::write(root.join("src/a.mts"), format!("{source}// changed\n")).unwrap();
    assert!(!analyze().status.success(), "stale coverage must fail");
}
