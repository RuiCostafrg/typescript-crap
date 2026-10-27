//! Compare the analyzed worktree snapshot with pinned HEAD blobs, not index hints.
use anyhow::{Context, Result, ensure};
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

fn matches_committed(reader: &mut impl Read, size: u64, source: &[u8]) -> Result<bool> {
    let mut committed = reader.take(size);
    let mut expected = without_crlf(source);
    let mut buffer = [0; 8192];
    let mut pending_cr = false;
    let mut matches = true;
    for offset in (0..size).step_by(buffer.len()) {
        let length = (size - offset).min(buffer.len() as u64) as usize;
        // Read exactly the advertised blob, never its framing or the next reply.
        committed.read_exact(&mut buffer[..length])?;
        for &byte in &buffer[..length] {
            if pending_cr && byte != b'\n' {
                matches &= expected.next() == Some(b'\r');
            }
            pending_cr = byte == b'\r';
            if !pending_cr {
                matches &= expected.next() == Some(byte);
            }
        }
    }
    if pending_cr {
        matches &= expected.next() == Some(b'\r');
    }
    Ok(matches && expected.next().is_none())
}

// Preserve lone CR bytes and all other content. Parsing and source hashes still
// use the original worktree buffer; this equivalence applies only to HEAD checks.
fn without_crlf(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, byte)| {
            (byte != b'\r' || bytes.get(index + 1) != Some(&b'\n')).then_some(byte)
        })
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
