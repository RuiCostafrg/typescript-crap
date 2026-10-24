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

#[test]
fn duplicate_coverage_is_an_input_error_and_preserves_the_previous_report() {
    use sha2::{Digest, Sha256};
    let source = "export function f(a:boolean,b:boolean){if(a)return 1;if(b)return 2;return 0;}\n";
    let dir = project(&[("a.ts", source)]);
    let coverage = dir.path().join("coverage.json");
    let output = dir.path().join("report.json");
    let hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let file = |lines: &str| format!(r#"{{"sha256":"{hash}","lines":{lines}}}"#);
    let run = || {
        at(
            dir.path(),
            &[
                "--coverage",
                coverage.to_str().unwrap(),
                "--require-coverage",
                "--format",
                "json",
                "--output",
                output.to_str().unwrap(),
            ],
        )
    };
    // Controls prove the same selected function passes with hits and fails
    // without them; malformed duplicate evidence must use the input-error exit.
    for (lines, exit) in [(r#"{"1":1}"#, 0), (r#"{"1":0}"#, 1)] {
        let file = file(lines);
        fs::write(
            &coverage,
            format!(r#"{{"version":1,"files":{{"a.ts":{file}}}}}"#),
        )
        .unwrap();
        let result = run();
        assert_eq!(result.status.code(), Some(exit), "{result:?}");
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["failed"], exit == 1);
    }
    let zero = file(r#"{"1":0}"#);
    let hit = file(r#"{"1":1}"#);
    for entries in [
        format!(r#""a.ts":{}"#, file(r#"{"1":0,"1":1}"#)),
        format!(r#""a.ts":{}"#, file(r#"{"1":1,"1":0}"#)),
        format!(r#""a.ts":{zero},"a.ts":{hit}"#),
        format!(r#""a.ts":{hit},"a.ts":{zero}"#),
    ] {
        fs::write(
            &coverage,
            format!(r#"{{"version":1,"files":{{{entries}}}}}"#),
        )
        .unwrap();
        fs::write(&output, "previous complete report").unwrap();
        let result = run();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("duplicate coverage"));
        assert!(result.stdout.is_empty());
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "previous complete report"
        );
    }
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

#[test]
fn report_rejects_selected_unselected_and_ignored_source_hardlinks() {
    let source = "export function f(){return 1;}";
    let dir = project(&[
        ("selected.ts", source),
        ("hidden ü/unselected.ts", source),
        (".gitignore", "hidden ü/\n"),
    ]);
    for file in ["selected.ts", "hidden ü/unselected.ts"] {
        let output = dir.path().join("report.json");
        fs::hard_link(dir.path().join(file), &output).unwrap();
        let result = at(
            dir.path(),
            &["selected.ts", "--output", output.to_str().unwrap()],
        );
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("source file"),
            "{result:?}"
        );
        assert_eq!(fs::read_to_string(dir.path().join(file)).unwrap(), source);
        assert_eq!(fs::read_to_string(&output).unwrap(), source);
        fs::remove_file(output).unwrap();
    }
}

#[test]
fn report_rejects_typescript_destinations_even_when_missing_or_unselected() {
    let source = "export function f(){return 1;}";
    let dir = project(&[("selected.ts", source), ("unselected.ts", source)]);
    for file in [
        "unselected.ts",
        "new.tsx",
        "new.mts",
        "new.cts",
        "new.d.ts",
        "new.TS",
        ".ts",
    ] {
        let output = dir.path().join(file);
        let result = at(
            dir.path(),
            &["selected.ts", "--output", output.to_str().unwrap()],
        );
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("source destination"));
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("unselected.ts")).unwrap(),
        source
    );
    assert!(!dir.path().join("new.tsx").exists());
}

#[test]
fn report_rejects_coverage_input_and_hardlink_alias() {
    let dir = project(&[("a.ts", "export function f(){return 1;}")]);
    let coverage = dir.path().join("coverage.json");
    let original = r#"{"version":1,"files":{}}"#;
    fs::write(&coverage, original).unwrap();
    let alias = dir.path().join("alias.json");
    fs::hard_link(&coverage, &alias).unwrap();
    for output in [&coverage, &alias] {
        let result = at(
            dir.path(),
            &[
                "--coverage",
                coverage.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
            ],
        );
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("coverage input"));
        assert_eq!(fs::read_to_string(&coverage).unwrap(), original);
    }
}

#[test]
fn atomic_report_replacement_preserves_other_links_and_cleans_temporary_files() {
    let dir = project(&[("path with ü/a.ts", "export function f(){return 1;}")]);
    let output = dir.path().join("report ü.json");
    let previous = dir.path().join("previous.json");
    fs::write(&output, "previous report").unwrap();
    fs::hard_link(&output, &previous).unwrap();
    let result = at(
        dir.path(),
        &[
            "path with ü/a.ts",
            "--format",
            "json",
            "--output",
            output.to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(report["functions"][0]["path"], "path with ü/a.ts");
    assert_eq!(fs::read_to_string(previous).unwrap(), "previous report");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    // A failed scan must leave a previously published report byte-for-byte intact.
    let bytes = fs::read(&output).unwrap();
    fs::write(dir.path().join("broken.ts"), "function {").unwrap();
    assert_eq!(
        at(dir.path(), &["--output", output.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(output).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn report_rejects_symlinks_including_extension_changing_and_dangling_links() {
    use std::os::unix::fs::symlink;
    let dir = project(&[("a.ts", "export function f(){return 1;}")]);
    for target in ["a.ts", "missing.ts"] {
        let output = dir.path().join("alias.json");
        symlink(dir.path().join(target), &output).unwrap();
        let result = at(dir.path(), &["--output", output.to_str().unwrap()]);
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("symbolic link"));
        fs::remove_file(output).unwrap();
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("a.ts")).unwrap(),
        "export function f(){return 1;}"
    );
    assert!(!dir.path().join("missing.ts").exists());
}

#[test]
fn deeply_nested_valid_expressions_do_not_abort() {
    let dir = tempfile::tempdir().unwrap();
    for depth in [1, 64, 512, 4_096, 20_000] {
        let source = format!(
            "export function f(){{return {}1{};}}",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        fs::write(dir.path().join("deep.ts"), source).unwrap();
        let result = at(dir.path(), &["--format", "json"]);
        assert_eq!(result.status.code(), Some(0), "depth {depth}: {result:?}");
        assert_eq!(json(&result)["functions"][0]["complexity"], 1);
    }
}

#[test]
fn deeply_nested_function_names_do_not_recurse() {
    let dir = tempfile::tempdir().unwrap();
    let depth = 64;
    let source = format!(
        "{}return 1;{}",
        "function f(){".repeat(depth),
        "}".repeat(depth)
    );
    fs::write(dir.path().join("deep.ts"), source).unwrap();
    let result = at(dir.path(), &["--format", "json"]);
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    assert_eq!(json(&result)["functions"].as_array().unwrap().len(), depth);
    assert_eq!(
        json(&result)["functions"][depth - 1]["name"],
        vec!["f"; depth].join("::")
    );
}

#[cfg(unix)]
#[test]
fn failed_report_write_preserves_existing_report_and_removes_staging_file() {
    let dir = tempfile::tempdir().unwrap();
    let source: String = (0..100)
        .map(|i| format!("export function f{i}(){{return 1;}}\n"))
        .collect();
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let output = dir.path().join("report.json");
    fs::write(&output, "previous complete report").unwrap();
    let result = Command::new("python3")
        // The file-size limit also truncates LLVM's exit-time profile. Keep
        // this one failure-injection child out of the coverage merge inputs.
        .env("LLVM_PROFILE_FILE", "/dev/null")
        .args([
            "-c",
            r#"
import resource, signal, subprocess, sys
signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
resource.setrlimit(resource.RLIMIT_FSIZE, (1024, 1024))
sys.exit(subprocess.call(sys.argv[1:], restore_signals=False))
"#,
        ])
        .arg(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "json", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot write report"));
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        "previous complete report"
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}
