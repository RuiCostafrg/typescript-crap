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

#[test]
fn relative_diff_config_cannot_hide_nested_sources_or_dirty_edits() {
    let dir = repo();
    let root = dir.path();
    fs::create_dir(root.join("pkg")).unwrap();
    fs::write(
        root.join("pkg/a.ts"),
        "export function changed(a:boolean,b:boolean){if(a && b)return 1;return 0;}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "nested violation"]);
    let nested = root.join("pkg");
    for relative in ["false", "true"] {
        git(root, &["config", "diff.relative", relative]);
        let out = run(&nested, &["--diff", "base"]);
        assert_eq!(out.status.code(), Some(1), "{relative}: {out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["files_scanned"], 1);
        assert_eq!(report["functions"][0]["path"], "a.ts");
        assert_eq!(report["functions"][0]["crap"], 12.0);
        fs::write(nested.join("a.ts"), "export const local = () => 0;\n").unwrap();
        let out = run(&nested, &["--diff", "base"]);
        assert_eq!(out.status.code(), Some(2), "{relative}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted"));
        assert!(out.stdout.is_empty());
        git(root, &["restore", "pkg/a.ts"]);
    }
}

#[test]
fn index_hints_cannot_replace_committed_violations_with_local_passing_sources() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let dir = repo();
        let root = dir.path();
        fs::write(
            root.join("changed.ts"),
            "export function changed(a:boolean,b:boolean){if(a && b)return 1;return 0;}\n",
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "committed violation"]);
        git(root, &["update-index", flag, "changed.ts"]);
        let before = git(root, &["ls-files", "-v"]);
        assert_eq!(run(root, &["--diff", "base"]).status.code(), Some(1));
        fs::write(root.join("changed.ts"), "export const passing = () => 0;\n").unwrap();
        assert!(git(root, &["diff", "--name-only", "HEAD"]).is_empty());
        // The ordinary project scan still supports work in progress.
        assert_eq!(run(root, &[]).status.code(), Some(0));
        fs::write(root.join("report.json"), "previous report").unwrap();
        fs::write(root.join("baseline.json"), "previous baseline").unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .current_dir(root)
            .args([
                "--root",
                root.to_str().unwrap(),
                "--diff",
                "base",
                "--output",
                "report.json",
                "--write-baseline",
                "baseline.json",
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{flag}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted"));
        assert!(out.stdout.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("report.json")).unwrap(),
            "previous report"
        );
        assert_eq!(
            fs::read_to_string(root.join("baseline.json")).unwrap(),
            "previous baseline"
        );
        assert_eq!(git(root, &["ls-files", "-v"]), before);
    }
}

#[test]
fn staged_and_unstaged_selected_edits_are_input_errors() {
    for staged in [false, true] {
        let dir = repo();
        let root = dir.path();
        fs::write(
            root.join("changed.ts"),
            "export const committed = () => 2;\n",
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "selected source"]);
        fs::write(root.join("changed.ts"), "export const local = () => 1;\n").unwrap();
        if staged {
            git(root, &["add", "changed.ts"]);
        }
        let out = run(root, &["--diff", "base"]);
        assert_eq!(out.status.code(), Some(2), "{staged}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted"));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn clean_crlf_checkout_uses_worktree_bytes_for_coverage() {
    use sha2::{Digest, Sha256};
    let dir = repo();
    let root = dir.path();
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join(".gitattributes"), "*.ts text eol=crlf\n").unwrap();
    let lf = "export function changed(a:boolean,b:boolean){\nif(a && b)return 1;\nreturn 0;\n}\n";
    fs::write(root.join("changed.ts"), lf).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "CRLF checkout attributes"]);
    fs::remove_file(root.join("changed.ts")).unwrap();
    git(root, &["checkout", "--", "changed.ts"]);
    let crlf = fs::read(root.join("changed.ts")).unwrap();
    assert_eq!(crlf, lf.replace('\n', "\r\n").as_bytes());
    assert_eq!(run(root, &["--diff", "base"]).status.code(), Some(1));
    let coverage = root.join("coverage.json");
    fs::write(
        &coverage,
        serde_json::json!({"version":1,"files":{"changed.ts":{
            "sha256":hex::encode(Sha256::digest(&crlf)),"lines":{"2":1,"3":1}
        }}})
        .to_string(),
    )
    .unwrap();
    let out = run(
        root,
        &["--diff", "base", "--coverage", coverage.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["functions"][0]["coverage"], 1.0);
    assert_eq!(report["functions"][0]["crap"], 3.0);
}

#[test]
fn batch_blob_verification_handles_multiple_files_and_ignores_filtered_out_sources() {
    let dir = repo();
    let root = dir.path();
    for name in ["space ü.ts", "second.ts"] {
        fs::write(root.join(name), "export const same = () => 1;\n").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "two source blobs"]);
    let out = run(root, &["--diff", "base"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["files_scanned"],
        2
    );
    git(root, &["update-index", "--assume-unchanged", "second.ts"]);
    fs::write(root.join("second.ts"), "export const local = () => 0;\n").unwrap();
    let out = run(root, &["--diff", "base", "--include", "space*"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let out = run(root, &["--diff", "base", "--include", "absent/**"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["files_scanned"],
        0
    );
}

#[cfg(unix)]
#[test]
fn newline_source_names_do_not_become_batch_commands() {
    let dir = repo();
    let root = dir.path();
    fs::write(
        root.join("line\nHEAD.ts"),
        "export const newline = () => 1;\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "newline name"]);
    let out = run(root, &["--diff", "base"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["functions"][0]["path"],
        "line\nHEAD.ts"
    );
}

#[test]
fn clean_filters_cannot_hide_content_changes_from_head_verification() {
    let dir = repo();
    let root = dir.path();
    let source = "export function changed(a:boolean,b:boolean){if(a && b)return 1;return 0;}\n";
    fs::write(root.join(".gitattributes"), "changed.ts filter=constant\n").unwrap();
    // Git's filter sees stdin but always emits the committed source. This fixture
    // demonstrates why a clean git diff is insufficient even without index hints.
    let script = format!(
        "node -e 'process.stdin.resume();process.stdin.on(\"end\",()=>process.stdout.write({}));'",
        serde_json::to_string(source).unwrap()
    );
    git(root, &["config", "filter.constant.clean", &script]);
    fs::write(root.join("changed.ts"), source).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "clean filter fixture"]);
    assert_eq!(run(root, &["--diff", "base"]).status.code(), Some(1));
    fs::write(root.join("changed.ts"), "export const local = () => 0;\n").unwrap();
    assert!(git(root, &["diff", "--name-only", "HEAD"]).is_empty());
    let out = run(root, &["--diff", "base"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("uncommitted"));
    assert!(out.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn parent_symlinks_cannot_change_git_identity_or_hide_selected_violations() {
    use std::os::unix::fs::symlink;
    let dir = repo();
    let root = dir.path();
    fs::create_dir(root.join("b")).unwrap();
    fs::write(
        root.join("b/f.ts"),
        "export function f(a:boolean,b:boolean,c:boolean){if(a && b && c)return 1;return 0;}",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "historical violation"]);
    git(root, &["branch", "-f", "base"]);
    let baseline = root.join("baseline.json");
    assert_eq!(
        run(root, &["--write-baseline", baseline.to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
    fs::create_dir(root.join("a")).unwrap();
    let source = "export function f(a:boolean,b:boolean){if(a && b)return 1;return 0;}";
    fs::write(root.join("a/f.ts"), source).unwrap();
    git(root, &["add", "a"]);
    git(root, &["commit", "-m", "new violation"]);
    for args in [
        vec!["--baseline", baseline.to_str().unwrap()],
        vec!["--include", "a/**"],
    ] {
        let mut flags = vec!["--diff", "base"];
        flags.extend(args);
        let out = run(root, &flags);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["functions"][0]["path"], "a/f.ts");
        assert_eq!(value["files_scanned"], 1);
    }
    fs::rename(root.join("a"), root.join("saved-a")).unwrap();
    fs::write(root.join("b/f.ts"), source).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("f.ts"), source).unwrap();
    // Include/exclude filters must not conceal an unsupported substitution,
    // whether its target is inside or outside the project.
    for target in [root.join("b"), outside.path().to_path_buf()] {
        symlink(target, root.join("a")).unwrap();
        for args in [
            vec!["--baseline", baseline.to_str().unwrap()],
            vec!["--include", "a/**"],
            vec!["--exclude", "a/**"],
        ] {
            let report = root.join("report.json");
            fs::write(&report, "previous report").unwrap();
            let mut flags = vec!["--diff", "base", "--output", report.to_str().unwrap()];
            flags.extend(args);
            let out = run(root, &flags);
            assert_eq!(out.status.code(), Some(2), "{out:?}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("symbolic link source"),
                "{out:?}"
            );
            assert!(out.stdout.is_empty());
            assert_eq!(fs::read_to_string(report).unwrap(), "previous report");
        }
        fs::remove_file(root.join("a")).unwrap();
    }
    // Now both changed Git identities would collapse into b/f.ts under the old
    // canonical-key map. Reject the substitution instead of deduplicating it.
    git(root, &["add", "b/f.ts"]);
    git(root, &["commit", "-m", "both identities changed"]);
    symlink(root.join("b"), root.join("a")).unwrap();
    let out = run(root, &["--diff", "base"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("symbolic link source"));
}

#[cfg(unix)]
#[test]
fn git_roots_preserve_trailing_whitespace_and_nested_project_paths() {
    for ending in [" ", "\t", "\n", "\r", " \n"] {
        let dir = repo();
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join(format!("repo{ending}"));
        fs::rename(dir.path(), &root).unwrap();
        fs::create_dir(root.join("pkg")).unwrap();
        fs::write(
            root.join("pkg/a.ts"),
            "export function f(a:boolean,b:boolean){if(a && b)return 1;return 0;}",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "nested violation"]);
        for sibling_exists in [false, true] {
            if sibling_exists {
                fs::create_dir(parent.path().join("repo")).unwrap();
            }
            for selected_root in [&root, &root.join("pkg")] {
                let out = run(selected_root, &["--diff", "base"]);
                assert_eq!(
                    out.status.code(),
                    Some(1),
                    "{ending:?}, {sibling_exists}: {out:?}"
                );
                let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
                assert_eq!(value["files_scanned"], 1);
                assert_eq!(
                    value["functions"][0]["path"],
                    if selected_root == &root {
                        "pkg/a.ts"
                    } else {
                        "a.ts"
                    }
                );
                let empty = run(selected_root, &["--diff", "HEAD"]);
                assert_eq!(empty.status.code(), Some(0), "{empty:?}");
            }
        }
    }
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
