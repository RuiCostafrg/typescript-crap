//! Exact Git-blob comparison; only CRLF/LF differences are equivalent.
use std::io::Read;

/// Compare one declared blob with source bytes using the production 8 KiB buffer.
///
/// CRLF and LF are equivalent. The complete blob is consumed even on mismatch;
/// I/O errors propagate without reading beyond the declared size.
pub(super) fn matches_committed(
    reader: &mut impl Read,
    size: u64,
    source: &[u8],
) -> std::io::Result<bool> {
    matches_committed_with_buffer::<8192>(reader, size, source)
}

/// Compare normalized bytes with a nonzero, fixed-size streaming buffer.
///
/// Return whether both normalized sequences match, including their lengths.
/// Consume exactly `size` bytes on success (including a mismatch), preserving
/// following protocol bytes; propagate read errors if the blob is truncated.
/// Buffer size is const-generic so bounded proofs exercise chunk transitions
/// with the same algorithm; the production entry point uses 8192 bytes.
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

/// Iterate over bytes, removing CR only when immediately followed by LF.
///
/// Preserve lone CR and all other bytes. Parsing and source hashes still use the
/// original worktree buffer; this equivalence applies only to HEAD checks.
pub(super) fn without_crlf(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, byte)| {
            (byte != b'\r' || bytes.get(index + 1) != Some(&b'\n')).then_some(byte)
        })
}
