//! Compare the analyzed worktree snapshot with pinned HEAD blobs, not index hints.
#[path = "committed/compare.rs"]
mod compare;
use anyhow::{Context, Result, ensure};
use compare::matches_committed;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};

#[derive(Debug)]
pub(crate) struct BlobReader {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl BlobReader {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        let mut child = Command::new("git")
            .current_dir(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("cannot execute git to verify committed sources")?;
        let output = BufReader::new(child.stdout.take().context("Git blob output unavailable")?);
        Ok(Self { child, output })
    }

    pub(crate) fn verify(&mut self, oid: &str, path: &Path, source: &str) -> Result<()> {
        let input = self.child.stdin.as_mut().context("Git blob input closed")?;
        writeln!(input, "{oid}")?;
        input.flush()?;
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
        drop(self.child.stdin.take());
        ensure!(
            self.child.wait()?.success(),
            "Git source verification failed"
        );
        Ok(())
    }
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        // Error paths must also close and reap a batch process waiting for input.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
#[path = "committed/tests.rs"]
mod tests;
