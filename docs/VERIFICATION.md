# Verification contracts

The [agentic development article](https://smallcultfollowing.com/babysteps/blog/2026/10/14/agentic-dev-and-rust/)
recommends explicit specifications, stronger correctness checks and measured
optimization. Footnote 6 compares language tradeoffs; it adds no tool requirement.
The existing fmt, strict Clippy, dependency audit, cross-platform integration,
coverage, mutation and executable schema/CLI contracts remain in force.

## Bounded proof of committed-source comparison

`verification/kani` compiles the actual production comparator from
`src/selection/committed/compare.rs`, without a copy of its implementation or the
Git subprocess/parser dependencies. An independent specification removes CR only
when immediately followed by LF. Every other byte, including lone CR, matters.

Thirty Kani harnesses cover both properties over arbitrary bytes and every length
combination from zero through four, using a two-byte buffer to cover transitions
between chunks. The production
entry point instantiates the same const-generic algorithm with 8192 bytes. They
assert that comparison agrees with the specification, consumes exactly
the declared blob without touching batch framing even on mismatch, and rejects a
truncated blob. Unwinding assertions remain enabled; exceeding the harness unwind
bound fails verification. These are bounded proofs, not proofs for arbitrary
lengths or the 8192-byte instantiation, process cleanup, filesystem behavior, the
parser, or the complete CLI.
The harnesses use an in-memory `Cursor`; OS I/O errors and short reads remain
integration-test responsibilities. The ordinary tests exercise large blobs and
CRLF split across the 8192-byte buffer.

On a Kani-supported host, install and run the same pinned tool as CI:

```sh
cargo install --locked kani-verifier --version 0.68.0
cargo kani setup
cd verification/kani
cargo kani --output-format terse
```

The separate proof crate has no dependencies and is not published in the scanner
package. Its Cargo.lock is committed. It does not alter the scanner's MSRV or
require users to install a proof tool. See the [Kani documentation](https://model-checking.github.io/kani/).

## Rust API compatibility

The Verification workflow also runs pinned `cargo-semver-checks` 0.51.0 against
the immutable `v2.0.0` tag, explicitly enforcing patch-level API compatibility:

```sh
cargo semver-checks --baseline-rev v2.0.0 --release-type patch
```

Update that baseline deliberately when adopting another major API contract.
This catches supported Rust API removals and incompatible signature changes;
it does not prove CLI, schema, scoring or behavioral compatibility. The existing
executable compatibility contracts cover those interfaces separately.

## Performance and other proof systems

Use the existing `cargo bench --bench scan`, `--bench review` and `--bench committed`
feedback loop, retain raw measurements, and profile the relevant executable with
the host profiler before optimizing. `committed` takes an optional fixture size in
MiB (`cargo bench --bench committed -- 32`) and reports Linux parent-process RSS;
that value does not include Git children or establish whole-scan memory usage.

Dial9 targets async services; this CLI has no async runtime. Adding it would not
measure this workload. Verus, Creusot and Aeneas are alternatives for deeper proof
work, rather than additional mandatory tools for the same small invariant. Start
with the production-code Kani boundary above and expand specifications when new
critical invariants justify them. No unsafe-code exemption or quality-gate
reduction accompanies this tooling.
