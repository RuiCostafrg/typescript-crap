#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;
fn at(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .args(["--root", root.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}
fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} / {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}
fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let file = dir.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    dir
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .args(["--root", fixture().to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn real_typescript_functions_have_known_scores_and_default_gate_fails() {
    let out = run(&["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 3);
    let scores: Vec<_> = functions
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap(),
                f["complexity"].as_u64().unwrap(),
                f["crap"].as_f64().unwrap(),
            )
        })
        .collect();
    assert!(scores.contains(&("identity", 1, 2.0)));
    assert!(scores.contains(&("choose", 2, 6.0)));
    assert!(scores.contains(&("classify", 4, 20.0)));
    assert_eq!(report["failed"], true);
    assert_eq!(functions[0]["path"], "complex.ts");
    assert_eq!(functions[0]["line"], 1);
}

#[test]
fn threshold_is_strict_and_selected_files_are_deduplicated() {
    let out = run(&[
        "simple.ts",
        "simple.ts",
        "--format",
        "json",
        "--threshold",
        "6",
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["files_scanned"], 1);
    assert_eq!(json(&out)["functions"].as_array().unwrap().len(), 2);
    assert_eq!(
        run(&["simple.ts", "--threshold", "5.999"]).status.code(),
        Some(1)
    );
    assert_eq!(run(&["--threshold", "20"]).status.code(), Some(0));
    for value in ["-1", "NaN", "inf", "nonsense"] {
        assert_eq!(run(&["--threshold", value]).status.code(), Some(2));
    }
}

#[test]
fn zero_threshold_is_valid_and_invalid_thresholds_fail_during_argument_parsing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(at(dir.path(), &["--threshold=0"]).status.code(), Some(0));
    for value in ["-1", "NaN", "inf", "-inf"] {
        let out = at(
            &dir.path().join("nonexistent-root"),
            &[&format!("--threshold={value}")],
        );
        assert_eq!(out.status.code(), Some(2));
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("invalid value"), "{error}");
        assert!(
            error.contains("threshold must be a finite nonnegative number"),
            "{error}"
        );
        assert!(!error.contains("cannot resolve project root"));
    }
}

#[test]
fn real_parser_ignores_strings_comments_types_and_nested_function_branches() {
    let dir = project(&[(
        "shapes.ts",
        r#"
type Choice<T> = T extends string ? true : false;
export function outer() {
  const text = "if (x) { while(true) {} } && || ??";
  // if and while should never count
  function inner(flag: boolean) { if(flag) return 1; return 0; }
  return inner;
}
export class Rules {
  constructor(public enabled: boolean) {}
  check(n: number) { return this.enabled && n > 0 ? n : 0; }
  get active() { return this.enabled; }
  set active(value: boolean) { this.enabled = value; }
}
export const fallback = (a?: number, b?: number) => a ?? b ?? 0;
export const object = { run(a: boolean) { return a || false; } };
export function overload(a: number): number;
export function overload(a: number) { return a; }
"#,
    )]);
    let out = at(dir.path(), &["--format", "json", "--threshold", "100"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = json(&out);
    let functions = value["functions"].as_array().unwrap();
    let cc = |name: &str| {
        functions.iter().find(|f| f["name"] == name).unwrap()["complexity"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(cc("outer"), 1);
    assert_eq!(cc("outer::inner"), 2);
    assert_eq!(cc("Rules::constructor"), 1);
    assert_eq!(cc("Rules::check"), 3);
    assert_eq!(cc("Rules::active"), 1);
    assert_eq!(cc("fallback"), 3);
    assert_eq!(cc("run"), 2);
    assert_eq!(cc("overload"), 1);
    assert_eq!(functions.len(), 9);
}

#[test]
fn runtime_decisions_have_defined_complexity() {
    let dir = project(&[(
        "flow.ts",
        r#"
function flow(values: number[], flag: boolean) {
  let x = 0;
  for (const n of values) { if (n && flag) x++; }
  for (let i = 0; i < 2; i++) x++;
  while (x < 3) x++;
  do { x--; } while (x > 5);
  try { x++; } catch (error) { x--; }
  switch (x) { case 1: x++; break; case 2: x--; break; default: break; }
  flag ||= x > 1;
  return flag ? x : 0;
}
"#,
    )]);
    let out = at(dir.path(), &["--format", "json", "--threshold", "1000"]);
    assert_eq!(out.status.code(), Some(0));
    // Base + for-of + if + && + for + while + do + catch + 2 cases + ||= + ternary.
    assert_eq!(json(&out)["functions"][0]["complexity"], 12);
    assert_eq!(json(&out)["functions"][0]["crap"], 156.0);
}

#[test]
fn directories_skip_generated_declaration_ignored_and_dependency_files() {
    let dir = project(&[
        ("src/a.ts", "export function a(){ return 1; }"),
        ("src/b.tsx", "export const View = () => <div>Hello</div>;"),
        ("src/c.mts", "export const c = () => 3;"),
        ("src/d.cts", "export function d(){ return 4; }"),
        ("src/types.d.ts", "declare function declared(): void;"),
        ("node_modules/broken.ts", "this isn't valid TypeScript"),
        ("dist/broken.ts", "this isn't valid TypeScript"),
        ("ignored.ts", "this isn't valid TypeScript"),
        (".gitignore", "ignored.ts\n"),
    ]);
    let out = at(dir.path(), &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out)["files_scanned"], 4);
    assert_eq!(json(&out)["functions"].as_array().unwrap().len(), 4);
    assert_eq!(at(dir.path(), &["src/types.d.ts"]).status.code(), Some(2));
}

#[test]
fn missing_stale_invalid_and_outside_files_are_errors_not_quality_failures() {
    let dir = project(&[("invalid.ts", "function broken( {")]);
    for args in [
        &["gone.ts"][..],
        &["invalid.ts"][..],
        &["../escape.ts"][..],
        &["--diff", "not-a-ref"][..],
    ] {
        let out = at(dir.path(), args);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn symlinks_and_existing_outside_sources_cannot_escape_root() {
    use std::os::unix::fs::symlink;
    let dir = project(&[("inside.ts", "export const inside = () => 1;")]);
    let outside = project(&[(
        "outside.ts",
        "export function unsafe(a: boolean,b: boolean){ if(a && b) return 1; return 0; }",
    )]);
    symlink(
        outside.path().join("outside.ts"),
        dir.path().join("link.ts"),
    )
    .unwrap();
    symlink(outside.path(), dir.path().join("linked-dir")).unwrap();
    let out = at(dir.path(), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["files_scanned"], 1);
    assert_eq!(at(dir.path(), &["link.ts"]).status.code(), Some(2));
    assert_eq!(
        at(
            dir.path(),
            &[outside.path().join("outside.ts").to_str().unwrap()]
        )
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn empty_and_type_only_projects_succeed_with_explicit_zero_counts() {
    let dir = project(&[("types.ts", "export interface Thing { name: string }")]);
    let out = at(dir.path(), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["files_scanned"], 1);
    assert_eq!(json(&out)["functions"], serde_json::json!([]));
    fs::remove_file(dir.path().join("types.ts")).unwrap();
    let out = at(dir.path(), &["--format", "json"]);
    assert_eq!(json(&out)["files_scanned"], 0);
}
