#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{BlobReader, without_crlf};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn normalization_changes_only_crlf_pairs() {
    for (input, expected) in [
        ("", ""),
        ("a\nb\n", "a\nb\n"),
        ("a\r\nb\r\n", "a\nb\n"),
        ("a\rb\r", "a\rb\r"),
        ("\r\r\n", "\r\n"),
        ("😀\0\r\n", "😀\0\n"),
    ] {
        assert_eq!(
            without_crlf(input.as_bytes()).collect::<Vec<_>>(),
            expected.as_bytes()
        );
    }
}

fn blob(root: &Path, source: &str) -> String {
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn verification_uses_supplied_snapshot_and_consumes_each_complete_blob() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let source = "export const f = () => '😀';\n";
    let oid = blob(dir.path(), source);
    let path = dir.path().join("a.ts");
    let mut reader = BlobReader::new(dir.path()).unwrap();
    // Disk content does not replace the caller's captured analysis snapshot.
    fs::write(&path, "different on disk").unwrap();
    reader.verify(&oid, &path, source).unwrap();
    reader
        .verify(&oid, &path, &source.replace('\n', "\r\n"))
        .unwrap();
    let large = format!("{}\n", "x".repeat(128 * 1024));
    let large_oid = blob(dir.path(), &large);
    reader.verify(&large_oid, &path, &large).unwrap();
    reader.verify(&oid, &path, source).unwrap();
    reader.finish().unwrap();
    // Conversely, restoring disk content cannot repair a stale captured buffer.
    fs::write(&path, source).unwrap();
    let mut reader = BlobReader::new(dir.path()).unwrap();
    assert!(
        reader
            .verify(&oid, &path, "changed snapshot")
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
}

#[test]
fn missing_blob_and_nonblob_objects_fail_verification() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    // Ask Git for the empty blob identity without storing it; do not assume a
    // hash length, so the verifier also supports SHA-256 repositories.
    let output = Command::new("git")
        .current_dir(dir.path())
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let oid = String::from_utf8(output.stdout).unwrap();
    let mut reader = BlobReader::new(dir.path()).unwrap();
    assert!(
        reader
            .verify(oid.trim(), &dir.path().join("absent.ts"), "")
            .is_err()
    );
    reader.finish().unwrap();
    let output = Command::new("git")
        .current_dir(dir.path())
        .args(["mktree"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let oid = String::from_utf8(output.stdout).unwrap();
    let mut reader = BlobReader::new(dir.path()).unwrap();
    assert!(
        reader
            .verify(oid.trim(), &dir.path().join("tree.ts"), "")
            .is_err()
    );
    reader.finish().unwrap();
}

#[test]
fn failed_batch_process_is_rejected_and_reaped() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let mut reader = BlobReader::new(dir.path()).unwrap();
    reader.child.kill().unwrap();
    assert!(reader.finish().is_err());
    assert!(reader.child.stdin.is_none());
    assert!(!reader.child.try_wait().unwrap().unwrap().success());
}

#[test]
fn dropping_a_reader_terminates_and_reaps_a_waiting_batch_process() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let mut reader = BlobReader::new(dir.path()).unwrap();
    let pid = reader.child.id();
    // Keep the input pipe open outside the reader. Dropping a handle alone would
    // leave Git waiting for more input, so successful cleanup must kill and reap.
    let input = reader.child.stdin.take().unwrap();
    drop(reader);
    let probe = r#"
        try {
            const pid = Number(process.argv[1]);
            process.kill(pid, 0);
            process.kill(pid, 'SIGKILL');
            process.exitCode = 1;
        } catch (error) {
            if (error.code !== 'ESRCH') throw error;
        }
    "#;
    let status = Command::new("node")
        .args(["-e", probe, &pid.to_string()])
        .status()
        .unwrap();
    drop(input);
    assert!(
        status.success(),
        "Git batch process survived reader cleanup"
    );
}
