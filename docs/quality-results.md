# Quality validation record

This record describes local runs on the Termux Android aarch64 Rust 1.96.0
toolchain, accessed inside Ubuntu/proot. CI configuration is not a claim that
GitHub Actions has already run.

## Rust layout and policy

The flat module layout was already valid Rust. The binary is now a thin process
entry point, with CLI I/O in `src/cli.rs` and reusable analysis/rendering behind
`src/lib.rs`. Public-interface tests remain under `tests/`, parser input under
`tests/fixtures/`, and an optimized benchmark under `benches/`.
See CONTRIBUTING.md and Cargo.toml for enforced rules.

- Production unsafe Rust is forbidden.
- Must-use results are enforced; strict Clippy passes.
- Explicit production unwrap/expect/panic/placeholders/debug/exit are denied.
- Third-party/native dependencies remain trusted code; this is not a safety proof.
- The actual lockfile requires Rust 1.90; the previously stated 1.88 was corrected.

## Ordinary validation

Locally passed:
- The initial **33 tests** passed in debug and release profiles (including actual Node/V8 and Git).
- Mutation-driven additions expanded the suite to **39 Rust tests**; the expanded debug, release, and instrumented suites all pass, along with strict Clippy.
- All three Python job-summary helper tests also pass.
- `cargo fmt --check`.
- `cargo clippy --locked -j 1 --all-targets -- -D warnings`.
- `cargo doc --locked --no-deps -j 1`.
- Node syntax checks and installer shell syntax checks.

## Current measured Rust source coverage

With the expanded **39-test** suite, cargo-llvm-cov 0.9.1 reports:

- **Lines:** 597 / 614 = **97.23%**.
- **Functions:** 56 / 59 = **94.92%**.
- **Regions:** 1,007 / 1,067 = **94.38%**.

Scope is the seven production Rust files. The coverage unit-test module now lives
in `src/coverage/tests.rs`, which is excluded alongside integration tests and
benchmarks. Separating it prevents test assertions themselves inflating the
production-source totals. The CI regex applies the same exclusions.

Branch coverage was not collected. Artifacts are `coverage/lcov.info`,
`coverage/summary.json`, and `coverage/html/index.html`. All 39 instrumented Rust
tests passed; the Markdown summary helper was checked against the generated JSON.

### Earlier baseline (different scope)

The earlier report below included inline unit-test code located inside
`src/coverage.rs`. It is retained for historical accuracy, not as a directly
comparable pure-production baseline.

Ran cargo-llvm-cov **0.9.1** locally with matching LLVM **21.1.8** tools and
instrumentation enabled. All **33 Rust tests** passed. Scope is the seven
production `src/*.rs` files; integration-test and benchmark source is excluded.

- **Lines:** 654 / 675 = **96.89%**.
- **Functions:** 59 / 64 = **92.19%**.
- **Regions:** 1,162 / 1,239 = **93.79%**.

Branch coverage was not collected; zero branch counters are not a measured 0%
result. This measures Rust tests, not TypeScript execution coverage used for CRAP.

Generated artifacts: `coverage/lcov.info`, `coverage/summary.json`, and
`coverage/html/index.html` (all ignored). The job-summary helper was also checked
against this actual JSON and has three passing Python CLI tests. GitHub workflow
files pass local actionlint **1.7.12** validation; hosted runs remain unexecuted.

## Dependency audit

cargo-audit **0.22.2** (official RustSec Linux aarch64 release binary, GitHub asset
SHA256 verified) initially found **RUSTSEC-2026-0194** and **RUSTSEC-2026-0195** in
the test-only quick-xml 0.38.4. Upgraded quick-xml to **0.42.0** and updated XML test
APIs. Rerun: **0 known vulnerabilities, no warnings**, with no advisory ignores.

Database revision: `4b587f00996ebe5c05a73eaef105ef241e876414`.
The machine-readable result is generated locally at `reports/audit.json` (ignored).

## Performance measurement

Command: `cargo bench --locked -j 1 --bench scan`.
Workload: 2,000 functions, one 152,890-byte TypeScript source file, complete
source-hashed coverage; warmup + 5 measured scans.

| Implementation | Median | Minimum | Maximum |
|---|---:|---:|---:|
| Per-function file hashing/range validation | 1305.647 ms | 1162.411 ms | 1356.811 ms |
| Once-per-file validation + numeric line keys | 90.226 ms | 81.491 ms | 117.695 ms |

Observed median speedup: about **14.5×** for this workload. These runs occurred on a
mobile/proot host with concurrent developer-tool compilation; they are not a
controlled cross-machine result or CI performance gate. The architectural change
removes repeated whole-file work; report rendering also avoids per-character and
per-row intermediate String allocations. Rendering speed was not separately timed.

## Mutation testing

Tool: **cargo-mutants 27.1.0**, Android-only reflink compatibility fallback applied
to a temporary local tool copy (not to the application). The manifest/source
configuration enumerated **154 mutants across all seven production Rust files**
before the initial run. No production source file or mutant regex was excluded.

The initial **full run completed all 154 mutants** in 29m 11s:

- **135 caught by tests**.
- **10 survived**.
- **9 unviable** (compilation failures, not counted as caught).
- **0 timeouts**.
- Viable detection rate: **135 / 145 = 93.1%**.
- Full baseline passed all 33 original tests; exit 2 indicates survivors, not an
  interrupted scan. No source file or mutant regex was excluded.

Exact run command:
```sh
PATH=/root/.local/bin:$PATH CARGO_BUILD_JOBS=1 cargo mutants \
  -j 1 --jobserver-tasks 1 --timeout 60 --build-timeout 180 \
  --output reports/mutation-initial
```

Raw artifacts are preserved at `reports/mutation-initial/mutants.out/`, including
`outcomes.json`, `mutants.json`, diffs, individual logs and category lists.

### Mutation-driven follow-up

Added behavioral assertions for:
- Multiline object/array destructuring defaults on different source lines.
- Invalid SHA256 syntax in **unselected** coverage entries, so later selected-source
  hash mismatches cannot mask a missing schema check.
- The coverage view's owned-line bounds (zero and beyond EOF), even for missing hits.
- XML/HTML forbidden controls versus legal newline/CR/tab.
- Exact `gh` arguments for current PR, explicit number, and URL.
- CLI zero-threshold acceptance and invalid finite/non-finite thresholds failing
  during argument parsing, before root resolution.
- Nested/new/replaced report files, canonical source aliases, and coverage-input
  overwrite protection.

One survivor toggled the parent-path filter used only for **nonexistent** outputs.
Its resolved value could not alias an existing scanned input; writing still used
the original path. Instead of inventing a diagnostic-only assertion or excluding
the mutant, removed that redundant resolution. Existing destinations are checked
canonically; `try_exists` errors propagate and `write` reports missing-parent/I/O
errors. No score behavior or input-overwrite guard was weakened.

The incremental `cargo mutants --iterate` follow-up **completed successfully**
(exit 0), using the full expanded 39-test suite and a passing current baseline.
It retested **10 mutants: all 10 caught**, with zero survivors, unviable results,
or timeouts in that follow-up. All nine still-generated initial survivors were
caught; the redundant tenth was removed from production code, not excluded.

The current source generates **153 mutants**. Reconciliation by exact mutant name:
- **134** current mutants already caught in the initial full run.
- **10** current mutants caught in the follow-up.
- **9** current mutants previously unviable; the tool's iteration mode skips these
  as well as previously caught mutants. They were **not** rerun or counted as caught.
- **0 remaining survivors** across the current generated set.

Thus all **144 viable current mutations** are accounted for as caught across the
initial full run and incremental follow-up. This is **not a fresh clean full scan**
of the final tree, nor a claim that every possible bug is covered. No production
source or mutant regex was excluded. Normal GitHub mutation CI starts a fresh run.

Initial raw artifacts were preserved unchanged. Follow-up results, diffs and logs
are at `reports/mutation-followup/mutants.out/`; the full current inventory is
`reports/mutation-current-list.json`.

```sh
cp -a reports/mutation-initial reports/mutation-followup
PATH=/root/.local/bin:$PATH CARGO_BUILD_JOBS=1 cargo mutants --iterate \
  --output reports/mutation-followup --baseline run --cap-lints true \
  -j 1 --jobserver-tasks 1 --timeout 60 --build-timeout 180
```
