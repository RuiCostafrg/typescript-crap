#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[test]
fn validation_propagates_metadata_errors_and_rejects_directories() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "regular").unwrap();
    assert!(
        super::DestinationPlan::new(
            root.path(),
            vec![&root.path().join("\0report.json")],
            vec![]
        )
        .is_err()
    );
    assert!(super::DestinationPlan::new(root.path(), vec![root.path()], vec![]).is_err());
    assert!(
        super::DestinationPlan::new(root.path(), vec![&root.path().join("missing.json")], vec![])
            .is_ok()
    );
}

#[test]
fn final_validation_refreshes_protected_inputs_and_destinations() {
    for change in ["source", "protected", "destination"] {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        let protected = root.path().join("input.json");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        std::fs::write(&protected, "input").unwrap();
        let plan =
            super::DestinationPlan::new(root.path(), vec![&first, &second], vec![&protected])
                .unwrap();
        match change {
            "source" => std::fs::hard_link(&first, root.path().join("newly-created.TS")).unwrap(),
            "protected" => {
                std::fs::remove_file(&protected).unwrap();
                std::fs::hard_link(&first, &protected).unwrap();
            }
            _ => {
                std::fs::remove_file(&second).unwrap();
                std::fs::hard_link(&first, &second).unwrap();
            }
        }
        assert!(
            plan.publish_with(2, |index, writer| {
                writer.write_all(
                    [b"replacement one".as_slice(), b"replacement two".as_slice()][index],
                )?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
        assert!(
            !std::fs::read_to_string(&second)
                .unwrap()
                .starts_with("replacement")
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            if change == "source" { 4 } else { 3 }
        );
    }
}

#[test]
fn streaming_failure_leaves_every_previous_output_and_cleans_staging() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.json");
    let second = root.path().join("second.json");
    std::fs::write(&first, "first").unwrap();
    std::fs::write(&second, "second").unwrap();
    let plan = super::DestinationPlan::new(root.path(), vec![&first, &second], vec![]).unwrap();
    assert!(
        plan.publish_with(2, |index, writer| {
            writer.write_all(b"partial")?;
            if index == 1 {
                anyhow::bail!("injected rendering failure");
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(first).unwrap(), "first");
    assert_eq!(std::fs::read_to_string(second).unwrap(), "second");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn typescript_names_match_every_case_variant_and_nothing_else() {
    use std::path::Path;
    for name in [
        "a.ts", "a.TS", "a.Tsx", "a.d.ts", "a.MTS", "a.cTs", ".ts", "ü.tsx", "x/y.cts",
    ] {
        assert!(super::typescript(Path::new(name)), "{name}");
    }
    for name in [
        "a.json",
        "ts",
        "a.tsx.json",
        "a.t",
        "a.mt",
        "a.tsy",
        "",
        "a.ts/",
        "a.ts.",
        "aTS",
        "a.js",
    ] {
        // `a.ts/` names the file `a.ts`, the rest are not TypeScript names.
        assert_eq!(
            super::typescript(Path::new(name)),
            name == "a.ts/",
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn walked_aliases_of_a_destination_are_rejected_at_any_depth() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("report.json");
    std::fs::write(&destination, "previous").unwrap();
    let deep = root.path().join("a/b/c");
    std::fs::create_dir_all(&deep).unwrap();
    for alias in ["link", "hard"] {
        let path = deep.join(format!("{alias}.Ts"));
        if alias == "link" {
            symlink(&destination, &path).unwrap();
        } else {
            std::fs::hard_link(&destination, &path).unwrap();
        }
        let error = format!(
            "{:#}",
            super::DestinationPlan::new(root.path(), vec![&destination], vec![],).unwrap_err()
        );
        assert_eq!(
            error, "report output cannot overwrite a source file",
            "{alias}"
        );
        std::fs::remove_file(&path).unwrap();
    }
    // A link to another regular file, or with a non-TypeScript name, is no alias.
    std::fs::write(deep.join("real.ts"), "export const real = 1;").unwrap();
    symlink(deep.join("real.ts"), deep.join("other.ts")).unwrap();
    symlink(&destination, deep.join("alias.json")).unwrap();
    std::fs::hard_link(&destination, deep.join("hard.json")).unwrap();
    super::DestinationPlan::new(root.path(), vec![&destination], vec![]).unwrap();
}

#[cfg(unix)]
#[test]
fn dangling_and_outside_links_are_skipped_without_weakening_input_protection() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let destination = root.path().join("report.json");
    let input = outside.path().join("coverage.json");
    std::fs::write(&destination, "previous").unwrap();
    std::fs::write(&input, "input").unwrap();
    let deep = root.path().join("x/y/z");
    std::fs::create_dir_all(&deep).unwrap();
    symlink(deep.join("missing.ts"), deep.join("dangling.ts")).unwrap();
    symlink(&input, deep.join("input.ts")).unwrap();
    symlink(outside.path(), deep.join("outside.tsx")).unwrap();
    let plan = super::DestinationPlan::new(root.path(), vec![&destination], vec![&input]).unwrap();
    plan.publish_with(1, |_, writer| Ok(writer.write_all(b"replacement")?))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "replacement"
    );
    assert_eq!(std::fs::read_to_string(&input).unwrap(), "input");
    // The protected input itself, or a hard link to it, is still refused.
    let alias = root.path().join("alias.json");
    std::fs::hard_link(&input, &alias).unwrap();
    for target in [&input, &alias] {
        let error = format!(
            "{:#}",
            super::DestinationPlan::new(root.path(), vec![target], vec![&input],).unwrap_err()
        );
        assert_eq!(
            error,
            "report output cannot overwrite coverage input, config, or baseline"
        );
    }
    assert_eq!(std::fs::read_to_string(&input).unwrap(), "input");
}

#[cfg(unix)]
#[test]
/// Deliberate change: an unreadable TypeScript file cannot be a destination we
/// identified by inode, so it no longer aborts publication. (Root can read the
/// file anyway, so this passes trivially there; hosted CI runs unprivileged.)
fn unreadable_sources_no_longer_abort_publication() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("report.json");
    std::fs::write(&destination, "previous").unwrap();
    let source = root.path().join("src/locked.ts");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "export const locked = 1;").unwrap();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = super::DestinationPlan::new(root.path(), vec![&destination], vec![])
        .and_then(|plan| plan.publish_with(1, |_, writer| Ok(writer.write_all(b"replacement")?)));
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644)).unwrap();
    result.unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "replacement"
    );
    // Permissions belong to the inode, so a hard link of the destination cannot
    // be unreadable while the destination is readable: locking the alias locks
    // the destination too, and publication is still refused before any write.
    let alias = root.path().join("src/locked-alias.ts");
    std::fs::hard_link(&destination, &alias).unwrap();
    std::fs::set_permissions(&alias, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = super::DestinationPlan::new(root.path(), vec![&destination], vec![]);
    std::fs::set_permissions(&alias, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "replacement"
    );
}

#[cfg(unix)]
#[test]
fn typescript_names_are_matched_on_bytes_including_non_utf8_names() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};
    for (name, expected) in [
        (b"\xff.ts".as_slice(), true),
        (b"\xff\xfe.TSX", true),
        (b"a\xff.d.mts", true),
        (b"a.ts\xff", false),
        (b"\xff", false),
        (b".t\xffs", false),
    ] {
        let path = Path::new(OsStr::from_bytes(name));
        assert_eq!(super::typescript(path), expected, "{name:?}");
    }
}
