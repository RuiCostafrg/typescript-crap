//! Proof-only crate: compile the production comparator without Git or C FFI.
#![cfg(kani)]
#[path = "../../../src/selection/committed/compare.rs"]
#[allow(dead_code)]
mod compare;

// Deliberately independent specification: remove each CR in a CRLF pair,
// retaining every other byte. This is not the production iterator/state machine.
fn specification(input: &[u8]) -> ([u8; 4], usize) {
    let mut result = [0; 4];
    let mut length = 0;
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\r' && i + 1 < input.len() && input[i + 1] == b'\n' {
            i += 1;
        }
        result[length] = input[i];
        length += 1;
        i += 1;
    }
    (result, length)
}

// Each length pair is a separate harness. Keeping lengths concrete avoids
// spending solver resources on symbolic standard-library buffer bookkeeping.
fn comparison<const B: usize, const S: usize>() {
    let blob: [u8; 4] = kani::any();
    let source: [u8; 4] = kani::any();
    let mut framed = [0; 5];
    for i in 0..B {
        framed[i] = blob[i];
    }
    framed[B] = b'\n';
    let mut reader = std::io::Cursor::new(&framed[..]);
    let actual = compare::matches_committed_with_buffer::<2>(&mut reader, B as u64, &source[..S]);
    assert_eq!(
        actual.unwrap(),
        specification(&blob[..B]) == specification(&source[..S])
    );
    assert_eq!(reader.position(), B as u64);
}

macro_rules! comparison_proof {
    ($name:ident, $blob:literal, $source:literal) => {
        #[kani::proof]
        #[kani::unwind(6)]
        fn $name() {
            comparison::<$blob, $source>();
        }
    };
}

macro_rules! truncation_proof {
    ($name:ident, $length:literal) => {
        #[kani::proof]
        #[kani::unwind(6)]
        fn $name() {
            let blob: [u8; $length] = kani::any();
            let mut reader = std::io::Cursor::new(&blob[..]);
            assert!(
                compare::matches_committed_with_buffer::<2>(&mut reader, ($length + 1) as u64, &[])
                    .is_err()
            );
            assert_eq!(reader.position(), $length as u64);
        }
    };
}

comparison_proof!(comparison_0_0, 0, 0);
comparison_proof!(comparison_0_1, 0, 1);
comparison_proof!(comparison_0_2, 0, 2);
comparison_proof!(comparison_0_3, 0, 3);
comparison_proof!(comparison_0_4, 0, 4);
comparison_proof!(comparison_1_0, 1, 0);
comparison_proof!(comparison_1_1, 1, 1);
comparison_proof!(comparison_1_2, 1, 2);
comparison_proof!(comparison_1_3, 1, 3);
comparison_proof!(comparison_1_4, 1, 4);
comparison_proof!(comparison_2_0, 2, 0);
comparison_proof!(comparison_2_1, 2, 1);
comparison_proof!(comparison_2_2, 2, 2);
comparison_proof!(comparison_2_3, 2, 3);
comparison_proof!(comparison_2_4, 2, 4);
comparison_proof!(comparison_3_0, 3, 0);
comparison_proof!(comparison_3_1, 3, 1);
comparison_proof!(comparison_3_2, 3, 2);
comparison_proof!(comparison_3_3, 3, 3);
comparison_proof!(comparison_3_4, 3, 4);
comparison_proof!(comparison_4_0, 4, 0);
comparison_proof!(comparison_4_1, 4, 1);
comparison_proof!(comparison_4_2, 4, 2);
comparison_proof!(comparison_4_3, 4, 3);
comparison_proof!(comparison_4_4, 4, 4);

truncation_proof!(truncated_0, 0);
truncation_proof!(truncated_1, 1);
truncation_proof!(truncated_2, 2);
truncation_proof!(truncated_3, 3);
truncation_proof!(truncated_4, 4);
