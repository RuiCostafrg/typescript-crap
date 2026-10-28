//! Exact Git-blob comparison; only CRLF/LF differences are equivalent.
use std::io::Read;

pub(super) fn matches_committed(
    reader: &mut impl Read,
    size: u64,
    source: &[u8],
) -> std::io::Result<bool> {
    matches_committed_with_buffer::<8192>(reader, size, source)
}

// Const-generic buffer size lets bounded proofs exercise chunk transitions with
// the same algorithm; the production entry point always uses 8192 bytes.
pub(super) fn matches_committed_with_buffer<const N: usize>(
    reader: &mut impl Read,
    size: u64,
    source: &[u8],
) -> std::io::Result<bool> {
    const {
        assert!(N > 0);
    };
    let mut committed = reader.take(size);
    let mut expected = without_crlf(source);
    let mut buffer = [0; N];
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
pub(super) fn without_crlf(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, byte)| {
            (byte != b'\r' || bytes.get(index + 1) != Some(&b'\n')).then_some(byte)
        })
}
