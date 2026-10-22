#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::fs;
#[test]
fn validation_propagates_metadata_errors_and_rejects_directories() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("file"), "regular").unwrap();
    assert!(super::validate(root.path(), &root.path().join("\0report.json"), None).is_err());
    assert!(super::validate(root.path(), root.path(), None).is_err());
    assert!(super::validate(root.path(), &root.path().join("missing.json"), None).is_ok());
}
