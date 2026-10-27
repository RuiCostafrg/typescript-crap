//! Proof-only crate: compile the production comparator without Git or C FFI.
#![cfg(kani)]
#[path = "../../../src/selection/committed/compare.rs"]
mod compare;

// Deliberately independent specification: remove each CR in a CRLF pair,
// retaining every other byte. This is not the production iterator/state machine.
fn specification(input: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\r' && i + 1 < input.len() && input[i + 1] == b'\n' {
            i += 1;
        }
        result.push(input[i]);
        i += 1;
    }
    result
}

#[kani::proof]
#[kani::unwind(16)]
fn comparison_matches_specification_and_preserves_framing() {
    let blob: [u8; 4] = kani::any();
    let source: [u8; 4] = kani::any();
    let blob_len: usize = kani::any();
    let source_len: usize = kani::any();
    kani::assume(blob_len <= blob.len());
    kani::assume(source_len <= source.len());
    // The trailing byte models the batch-protocol terminator, which must remain
    // unread even when an unequal source has more or fewer normalized bytes.
    let mut framed = blob.to_vec();
    framed.insert(blob_len, b'\n');
    let mut reader = std::io::Cursor::new(framed);
    let actual = compare::matches_committed(&mut reader, blob_len as u64, &source[..source_len]);
    assert_eq!(
        actual.unwrap(),
        specification(&blob[..blob_len]) == specification(&source[..source_len])
    );
    assert_eq!(reader.position(), blob_len as u64);
}

#[kani::proof]
#[kani::unwind(16)]
fn truncated_blob_is_an_error() {
    let blob: [u8; 4] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= blob.len());
    let mut reader = std::io::Cursor::new(&blob[..len]);
    let actual = compare::matches_committed(&mut reader, (len + 1) as u64, &[]);
    assert!(actual.is_err());
    assert_eq!(reader.position(), len as u64);
}
