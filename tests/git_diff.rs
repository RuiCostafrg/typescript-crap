#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Test Author"]);
    git(
        dir.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    fs::write(
        dir.path().join("unchanged.ts"),
        "export const stable = () => 1;",
    )
    .unwrap();
    fs::write(dir.path().join("changed.ts"), "export const old = () => 1;").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "base"]);
    git(dir.path(), &["branch", "base"]);
    dir
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn diff_scores_only_changed_files_and_intersects_explicit_selection() {
    let dir = repo();
    let root = dir.path();
    fs::write(
        root.join("changed.ts"),
        "export function changed(a: boolean,b: boolean) { if (a && b) return 1; return 0; }",
    )
    .unwrap();
    fs::write(root.join("new file ü.ts"), "export const fresh = () => 1;").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "feature"]);
    let out = run(root, &["--diff", "base"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["files_scanned"], 2);
    assert_eq!(value["functions"][0]["name"], "changed");
    assert_eq!(value["functions"][0]["crap"], 12.0);
    assert_eq!(value["functions"][1]["path"], "new file ü.ts");
    let out = run(root, &["unchanged.ts", "--diff", "base"]);
    assert_eq!(out.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["files_scanned"], 0);
    fs::remove_file(root.join("changed.ts")).unwrap();
    assert_eq!(
        run(root, &["--diff", "base"]).status.code(),
        Some(2),
        "missing changed source must not silently pass"
    );
}

#[test]
fn rename_deletion_and_diverged_base_use_merge_base() {
    let dir = repo();
    let root = dir.path();
    git(root, &["checkout", "-b", "feature"]);
    git(root, &["mv", "changed.ts", "renamed file.ts"]);
    git(root, &["rm", "unchanged.ts"]);
    git(root, &["commit", "-m", "rename and delete"]);
    git(root, &["checkout", "main"]);
    fs::write(root.join("base-only.ts"), "not valid TS ::::").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base advances independently"]);
    git(root, &["checkout", "feature"]);
    let out = run(root, &["--diff", "main"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["files_scanned"], 1);
    assert_eq!(value["functions"][0]["path"], "renamed file.ts");
    let out = run(root, &["--diff", "HEAD"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["files_scanned"],
        0
    );
}

#[test]
fn tracked_ignored_and_hidden_changes_are_not_silently_skipped() {
    let dir = repo();
    let root = dir.path();
    fs::write(
        root.join("changed.ts"),
        "export function changed(a: boolean, b: boolean) { if(a && b) return 1; return 0; }",
    )
    .unwrap();
    fs::write(root.join(".hidden.ts"), "export const hidden = () => 1;").unwrap();
    fs::write(root.join(".gitignore"), "changed.ts\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "ignored tracked source"]);
    let out = run(root, &["--diff", "base"]);
    assert_eq!(out.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["files_scanned"], 2);
    assert_eq!(value["functions"][0]["path"], ".hidden.ts");
    let out = run(root, &[".", "--diff", "base"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn diff_from_a_project_subdirectory_returns_paths_relative_to_that_root() {
    let dir = repo();
    let root = dir.path();
    fs::create_dir(root.join("pkg")).unwrap();
    fs::write(root.join("pkg/a.ts"), "export const a = () => 1;").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "package"]);
    let out = run(&root.join("pkg"), &["--diff", "base"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["functions"][0]["path"], "a.ts");
}

#[cfg(unix)]
#[test]
fn github_pr_lookup_uses_gh_and_refuses_a_mismatching_checkout() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let root = dir.path();
    let base = git(root, &["rev-parse", "HEAD"]);
    fs::write(root.join("changed.ts"), "export const updated = () => 42;").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "feature"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("gh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" != pr ] || [ \"$2\" != view ]; then exit 91; fi\ncase \"$EXPECT_PR\" in\n current) [ \"$#\" -eq 4 ] && [ \"$3\" = --json ] && [ \"$4\" = baseRefOid,headRefOid ] || exit 92 ;;\n *) [ \"$#\" -eq 5 ] && [ \"$3\" = \"$EXPECT_PR\" ] && [ \"$4\" = --json ] && [ \"$5\" = baseRefOid,headRefOid ] || exit 93 ;;\nesac\nprintf '%s' '{{\"baseRefOid\":\"{base}\",\"headRefOid\":\"{head}\"}}'\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .env("PATH", &path)
            .env("EXPECT_PR", "42")
            .args([
                "--root",
                root.to_str().unwrap(),
                "--pr=42",
                "--format",
                "json",
            ])
            .output()
            .unwrap()
    };
    let out = invoke();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["functions"][0]["name"],
        "updated"
    );
    for (arg, expected) in [
        ("--pr", "current"),
        (
            "--pr=https://github.com/example/project/pull/42",
            "https://github.com/example/project/pull/42",
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .env("PATH", &path)
            .env("EXPECT_PR", expected)
            .arg("--root")
            .arg(root)
            .args([arg, "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fs::write(root.join("changed.ts"), "export const dirty = () => 0;").unwrap();
    let out = invoke();
    assert_eq!(
        out.status.code(),
        Some(2),
        "dirty PR source must not be reported as the PR commit"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted"));
    git(root, &["restore", "changed.ts"]);
    git(root, &["checkout", "base"]);
    let out = invoke();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not match PR head"));
}
