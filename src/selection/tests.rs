#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{WALK_IS_CANONICAL, checked, walked};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// A canonical project root, as `scan_with` passes it to selection.
fn tree(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "export const value = 1;\n").unwrap();
    }
    (directory, root)
}

fn message(result: anyhow::Result<PathBuf>) -> String {
    format!("{:#}", result.unwrap_err())
}

#[test]
fn walked_entries_equal_the_full_check_and_verify_each_directory_once() {
    let files = [
        "top.ts",
        "a/b/c/deep.ts",
        "a/b/c/second.ts",
        "a/b/other/third.ts",
        "Ünï cödé/CamelCase/MiXeD.ts",
        "日本語/ファイル.tsx",
    ];
    let (_directory, root) = tree(&files);
    let mut verified = HashSet::new();
    let mut resolving = HashSet::new();
    for file in files {
        let path = root.join(file);
        let full = checked(&path, &root).unwrap();
        assert_eq!(
            walked(&path, &root, &mut resolving, false).unwrap(),
            full,
            "{file}"
        );
        let unresolved = walked(&path, &root, &mut verified, true).unwrap();
        // Without resolution the walked path is returned untouched.
        assert_eq!(unresolved, path, "{file}");
        if WALK_IS_CANONICAL {
            assert_eq!(unresolved, full, "{file}");
        }
    }
    // Exactly the directories below the root, never the root or a file.
    let expected: HashSet<PathBuf> = [
        "a",
        "a/b",
        "a/b/c",
        "a/b/other",
        "Ünï cödé",
        "Ünï cödé/CamelCase",
        "日本語",
    ]
    .iter()
    .map(|directory| root.join(directory))
    .collect();
    assert_eq!(verified, expected);
    assert_eq!(resolving, expected);
}

#[test]
fn walked_entries_must_exist_only_when_resolved_and_stay_below_the_root() {
    let (_directory, root) = tree(&["a/real.ts"]);
    let missing = root.join("a/missing.ts");
    // The unresolved form never touches the file itself: the walk yielded it.
    assert_eq!(
        walked(&missing, &root, &mut HashSet::new(), true).unwrap(),
        missing
    );
    assert!(
        message(walked(&missing, &root, &mut HashSet::new(), false)).contains("missing source")
    );
    let (_other, outside) = tree(&["a/real.ts"]);
    for canonical in [false, true] {
        let mut verified = HashSet::new();
        let error = message(walked(
            &outside.join("a/real.ts"),
            &root,
            &mut verified,
            canonical,
        ));
        assert!(error.contains("source is outside project root"), "{error}");
        assert!(verified.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn walked_entries_reject_a_linked_directory_at_any_depth_and_cache_nothing_below_it() {
    use std::os::unix::fs::symlink;
    let (_directory, root) = tree(&["a/b/real/deeper/x.ts", "a/b/real/y.ts"]);
    let (_other, outside) = tree(&["deeper/x.ts", "y.ts"]);
    for target in [root.join("a/b/real"), outside.clone()] {
        let link = root.join("a/b/link");
        symlink(&target, &link).unwrap();
        for canonical in [false, true] {
            let mut verified = HashSet::new();
            for file in ["a/b/link/deeper/x.ts", "a/b/link/y.ts"] {
                // Twice: a rejected walk must not leave `link/deeper` trusted.
                for _ in 0..2 {
                    let error = message(walked(&root.join(file), &root, &mut verified, canonical));
                    assert_eq!(
                        error,
                        format!("symbolic link source is not supported: {}", link.display())
                    );
                    assert!(verified.is_empty(), "{verified:?}");
                }
            }
            // Directories above the link are real and stay usable.
            walked(&root.join("a/b/real/y.ts"), &root, &mut verified, canonical).unwrap();
            assert!(!verified.contains(&link));
            assert!(
                message(walked(
                    &root.join("a/b/link/y.ts"),
                    &root,
                    &mut verified,
                    canonical
                ))
                .contains("symbolic link source")
            );
        }
        fs::remove_file(&link).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn a_verified_directory_is_trusted_for_the_rest_of_the_scan_and_resolution_still_contains() {
    use std::os::unix::fs::symlink;
    let (_directory, root) = tree(&["a/real/x.ts"]);
    let (_other, outside) = tree(&["x.ts"]);
    let link = root.join("a/link");
    symlink(root.join("a/real"), &link).unwrap();
    // One check per directory and scan: a remembered directory is not examined again.
    let mut verified: HashSet<PathBuf> = [link.clone()].into();
    assert_eq!(
        walked(&link.join("x.ts"), &root, &mut verified, true).unwrap(),
        link.join("x.ts")
    );
    assert_eq!(verified, [link].into());
    // Resolution keeps the containment check `checked` applies to a file link.
    let escape = root.join("a/real/escape.ts");
    symlink(outside.join("x.ts"), &escape).unwrap();
    let error = message(walked(&escape, &root, &mut HashSet::new(), false));
    assert!(error.contains("source is outside project root"), "{error}");
    assert!(Path::new(&escape).symlink_metadata().unwrap().is_symlink());
}
