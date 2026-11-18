#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::compare::without_crlf;
use super::{BlobReader, matches_committed};
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn streaming_comparison_preserves_boundaries_lone_cr_and_response_framing() {
    for prefix in [0, 8190, 8191, 8192, 16383] {
        for suffix in ["", "\r", "\n", "\r\n", "\r\r\n", "\rX\n", "😀\0\r\n"] {
            let committed = format!("{}{suffix}", "x".repeat(prefix));
            let normalized: Vec<_> = without_crlf(committed.as_bytes()).collect();
            for source in [committed.as_bytes(), normalized.as_slice()] {
                let mut response = committed.as_bytes().to_vec();
                response.extend(b"\nnext response");
                let mut reader = Cursor::new(response);
                assert_eq!(
                    matches_committed(&mut reader, committed.len() as u64, source).unwrap(),
                    without_crlf(committed.as_bytes()).eq(without_crlf(source)),
                    "{prefix}, {suffix:?}"
                );
                assert_eq!(reader.position(), committed.len() as u64);
                for different in [b"".as_slice(), b"different", &[source, b"x"].concat()] {
                    if different == normalized {
                        continue;
                    }
                    reader.set_position(0);
                    assert!(
                        !matches_committed(&mut reader, committed.len() as u64, different).unwrap()
                    );
                    assert_eq!(reader.position(), committed.len() as u64);
                }
            }
        }
    }
    // CRLF equivalence works in either direction; lone CR is never discarded.
    assert!(matches_committed(&mut Cursor::new(b"a\nb"), 3, b"a\r\nb").unwrap());
    assert!(!matches_committed(&mut Cursor::new(b"a\rb"), 3, b"a\nb").unwrap());
    assert!(!matches_committed(&mut Cursor::new(b"\r\r\n"), 3, b"\r\n").unwrap());
    assert!(matches_committed(&mut Cursor::new(b"short"), 6, b"short").is_err());
}

#[test]
fn malformed_batch_framing_and_truncated_blobs_are_rejected() {
    use std::io::BufReader;
    for response in [
        "id blob 3\nabc!",
        "id blob 4\nabc",
        "id blob 3\nabc",
        "id blob nope\n",
        "id blob\n",
    ] {
        let mut child = Command::new("node")
            .args(["-e", "process.stdin.once('data',()=>{process.stdout.end(process.argv[1]);process.stdin.destroy();});", response])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        // Requests are written ahead by `new`; stand in for its writer here.
        writeln!(child.stdin.as_mut().unwrap(), "id").unwrap();
        let mut reader = BlobReader {
            child,
            output,
            pending: 1,
            writer: None,
        };
        assert!(
            reader.verify("id", Path::new("a.ts"), "abc").is_err(),
            "{response:?}"
        );
        // Drop must also terminate/reap malformed-response processes.
    }
}

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
    let large = format!("{}\n", "x".repeat(128 * 1024));
    let large_oid = blob(dir.path(), &large);
    let ids = [&oid, &oid, &large_oid, &oid].map(|id| id.clone()).to_vec();
    let mut reader = BlobReader::new(dir.path(), ids).unwrap();
    // Disk content does not replace the caller's captured analysis snapshot.
    fs::write(&path, "different on disk").unwrap();
    reader.verify(&oid, &path, source).unwrap();
    reader
        .verify(&oid, &path, &source.replace('\n', "\r\n"))
        .unwrap();
    reader.verify(&large_oid, &path, &large).unwrap();
    reader.verify(&oid, &path, source).unwrap();
    reader.finish().unwrap();
    // Conversely, restoring disk content cannot repair a stale captured buffer.
    fs::write(&path, source).unwrap();
    let mut reader = BlobReader::new(dir.path(), vec![oid.clone()]).unwrap();
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
    let mut reader = BlobReader::new(dir.path(), vec![oid.trim().to_owned()]).unwrap();
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
    let mut reader = BlobReader::new(dir.path(), vec![oid.trim().to_owned()]).unwrap();
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
    let mut reader = idle(dir.path());
    reader.child.kill().unwrap();
    assert!(reader.finish().is_err());
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
    let mut reader = idle(dir.path());
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

#[test]
/// Preserve blob boundaries through fragmented reads, interruptions and fatal I/O errors.
fn short_reads_and_interruptions_preserve_normalization_and_framing() {
    use std::io::{self, Read};
    struct Fragmented {
        input: Cursor<Vec<u8>>,
        calls: usize,
        fail_at: Option<u64>,
    }
    impl Read for Fragmented {
        /// Limit reads and inject recoverable interruptions or a terminal permission error.
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls.is_multiple_of(3) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.fail_at.is_some_and(|at| self.input.position() >= at) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let length = buffer.len().min(7);
            self.input.read(&mut buffer[..length])
        }
    }
    let committed = format!("{}\r\n😀\rX\r\n{}\r", "a".repeat(8191), "b".repeat(8192));
    let normalized = committed.replace("\r\n", "\n");
    let mut framed = committed.as_bytes().to_vec();
    framed.extend_from_slice(b"\nnext response");
    for source in [normalized.as_bytes(), b"mismatch".as_slice()] {
        let mut reader = Fragmented {
            input: Cursor::new(framed.clone()),
            calls: 0,
            fail_at: None,
        };
        assert_eq!(
            matches_committed(&mut reader, committed.len() as u64, source).unwrap(),
            source == normalized.as_bytes()
        );
        assert_eq!(reader.input.position(), committed.len() as u64);
    }
    let mut reader = Fragmented {
        input: Cursor::new(framed),
        calls: 0,
        fail_at: Some(8),
    };
    assert_eq!(
        matches_committed(&mut reader, committed.len() as u64, normalized.as_bytes())
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(reader.input.position() < committed.len() as u64);
}

/// A reader whose Git process keeps waiting: its input pipe stays open in `child`.
fn idle(root: &Path) -> BlobReader {
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let output = std::io::BufReader::new(child.stdout.take().unwrap());
    BlobReader {
        child,
        output,
        pending: 0,
        writer: None,
    }
}

fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    dir
}

#[test]
fn requests_written_ahead_are_answered_in_order_beyond_pipe_capacity() {
    let dir = git_repo();
    // Far more request and reply bytes than one pipe buffer holds each, so
    // writing every request ahead from the reading thread would deadlock.
    let sources: Vec<String> = (0..300)
        .map(|index| {
            format!(
                "export const f{index} = {};\n",
                "1 + ".repeat(index * 40) + "1"
            )
        })
        .collect();
    let ids: Vec<String> = sources
        .iter()
        .map(|source| blob(dir.path(), source))
        .collect();
    let path = dir.path().join("a.ts");
    // 3,000 requests of 41+ bytes also exceed a 64 KiB input pipe.
    let order: Vec<usize> = (0..3000).map(|index| index % sources.len()).collect();
    let requests = order.iter().map(|&index| ids[index].clone()).collect();
    let mut reader = BlobReader::new(dir.path(), requests).unwrap();
    for &index in &order {
        reader.verify(&ids[index], &path, &sources[index]).unwrap();
    }
    reader.finish().unwrap();
}

#[test]
fn early_failures_with_requests_outstanding_do_not_hang() {
    let dir = git_repo();
    let large = format!("{}\n", "x".repeat(256 * 1024));
    let id = blob(dir.path(), &large);
    let path = dir.path().join("a.ts");
    // A mismatch on the first file leaves many large replies unread.
    let mut reader = BlobReader::new(dir.path(), vec![id.clone(); 64]).unwrap();
    assert!(
        reader
            .verify(&id, &path, "changed")
            .unwrap_err()
            .to_string()
            .contains("uncommitted")
    );
    drop(reader);
    // Finishing before every reply was read is an error, never a wait.
    let mut reader = BlobReader::new(dir.path(), vec![id.clone(); 64]).unwrap();
    reader.verify(&id, &path, &large).unwrap();
    assert!(reader.finish().is_err());
    // A reply for another object than the caller expects is rejected.
    let other = blob(dir.path(), "other\n");
    let mut reader = BlobReader::new(dir.path(), vec![other]).unwrap();
    assert!(reader.verify(&id, &path, &large).is_err());
}
