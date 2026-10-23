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
            plan.publish(&["replacement one".into(), "replacement two".into()])
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
