//! Compare the analyzed worktree snapshot with pinned HEAD blobs, not index hints.
#[path = "committed/compare.rs"]
mod compare;
use anyhow::{Context, Result, ensure};
use compare::matches_committed;
use std::{
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread::{self, JoinHandle},
};

/// One long-lived `git cat-file --batch` process, fed ahead of the comparison.
///
/// Every object id is written by a dedicated thread as soon as the reader is
/// created, so Git fetches blobs while this thread reads and compares worktree
/// sources. Writing from the reading thread instead would stall as soon as
/// Git's output pipe filled: one round trip per file.
#[derive(Debug)]
pub(crate) struct BlobReader {
    child: Child,
    output: BufReader<ChildStdout>,
    /// Replies Git still owes: one per id written, consumed by `verify`.
    pending: usize,
    /// Owns the input thread until `finish` or `Drop`; joined, never detached.
    writer: Option<JoinHandle<()>>,
}

impl BlobReader {
    /// Start a reader that will verify `ids`, in the order `verify` is called.
    pub(crate) fn new(root: &Path, ids: Vec<String>) -> Result<Self> {
        let mut child = Command::new("git")
            .current_dir(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("cannot execute git to verify committed sources")?;
        let input = child.stdin.take().context("Git blob input closed")?;
        let output = BufReader::new(child.stdout.take().context("Git blob output unavailable")?);
        let pending = ids.len();
        let writer = match thread::Builder::new()
            .name("ts-crap-git-blob-input".to_owned())
            .spawn(move || write_ids(input, ids))
        {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("cannot start Git blob writer");
            }
        };
        Ok(Self {
            child,
            output,
            pending,
            writer: Some(writer),
        })
    }

    pub(crate) fn verify(&mut self, oid: &str, path: &Path, source: &str) -> Result<()> {
        // The id is already on its way to Git; only the reply is still to read.
        // A caller/queue order mismatch fails the header check below.
        self.pending = self
            .pending
            .checked_sub(1)
            .context("Git blob input closed")?;
        let mut header = String::new();
        self.output.read_line(&mut header)?;
        let mut fields = header.split_whitespace();
        ensure!(
            fields.next() == Some(oid) && fields.next() == Some("blob"),
            "cannot read committed source {}",
            path.display()
        );
        let size: u64 = fields
            .next()
            .context("Git blob size unavailable")?
            .parse()?;
        // Keep one batch process, but only a fixed-size comparison buffer. The
        // worktree snapshot remains the exact buffer subsequently analyzed/hashed.
        let matches = matches_committed(&mut self.output, size, source.as_bytes())?;
        let mut terminator = [0];
        self.output.read_exact(&mut terminator)?;
        ensure!(terminator == *b"\n", "invalid Git blob response");
        ensure!(
            matches,
            "uncommitted changes in selected source {}; commit or restore it before a diff/PR scan (only CRLF/LF differences are allowed)",
            path.display()
        );
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        // Unread replies would block Git on a full pipe; `Drop` kills it instead.
        ensure!(self.pending == 0, "Git source verification failed");
        self.join_writer();
        ensure!(
            self.child.wait()?.success(),
            "Git source verification failed"
        );
        Ok(())
    }

    /// Wait for the input thread. It reports errors by exiting; Git's status decides.
    fn join_writer(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// Write every id in one buffered pass, then close Git's input so it can exit.
///
/// A killed child closes the pipe, so a failed write just ends the thread.
fn write_ids(mut input: ChildStdin, ids: Vec<String>) {
    let mut writer = BufWriter::new(&mut input);
    for id in ids {
        if writeln!(writer, "{id}").is_err() {
            return;
        }
    }
    let _ = writer.flush();
    // Dropping `input` closes stdin, ending `git cat-file --batch`.
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        // Error paths must also close and reap a batch process waiting for input.
        // Kill first: the writer then fails with EPIPE instead of blocking on a
        // pipe that nobody drains. Joining keeps no thread behind a dead reader.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_writer();
    }
}

#[cfg(test)]
#[path = "committed/tests.rs"]
mod tests;
