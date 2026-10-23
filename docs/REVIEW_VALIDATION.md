# Remaining review validation (2026-10-23)

Scope: proposal items 2–8 after PR #11, based on `6ecbe80`. No public API,
format version, coverage floor or mutation floor is relaxed. Private unit suites
remain under `#[cfg(test)]`; shared integration support ships in `tests/common/`.

## Behavioral invariants and deliberate faults

These focused experiments temporarily changed production implementations, ran the
named test, verified a nonzero exit **with an assertion failure**, then restored
the original bytes. They are examples of assertion sensitivity, not a substitute
for the full CI mutation run or a measured full-suite mutation score.

| Suite | Invariant / test | Deliberate fault killed |
| --- | --- | --- |
| Analysis | `owned_identity_and_metrics_survive_trivia_and_nested_edits`: trivia preserves identity/complexity, nested edits change only the nested identity/metrics; Unicode/default arguments/TSX and depth 64 included | Stop pruning descendant functions from the enclosing function's token hash |
| Coverage | `coverage_evidence_is_bounded_monotonic_and_distinguishes_absent_from_zero`: exhaustive 27 three-line states, promoting covered lines, empty/missing evidence | Count zero-hit lines as covered instead of positive-hit lines |
| Baseline | `ambiguous_names_and_fingerprints_cannot_suppress_new_violations` plus reorder/completeness/conflict cases | Allow multiple historical fingerprint candidates to match a current function |
| Reports | `every_presentation_combination_preserves_report_gate_totals_and_machine_order`: text/HTML × filters × sorts × limits; full gate/totals unchanged, exact JSON/XML/SARIF name ordering | Truncate at `top + 1` rather than `top` |
| Publication | `final_validation_refreshes_protected_inputs_and_destinations`: newly introduced source/protected/output hardlink aliases cannot publish; sentinels and cleanup asserted | Skip the final plan validation before each rename |
| V8 conversion | `generated overlapping`: 300 deterministic cases (also reversed range order) against an independent per-character oracle | Reverse the tie-breaking hit-count comparison |
| Snapshot | `coverage-snapshot.test.mjs`: BOM-preserving strict UTF-8/hash behavior and explicit caller selection/storage policies | Set the decoder's `ignoreBOM` option false instead of true |

The first report truncation experiment survived because the new combination test
checked the full gate but not row counts. The test was strengthened to count
actual text/HTML rows under every combination; the same off-by-one fault then
failed an assertion. An inverted output-conflict predicate was not useful evidence
(it rejected valid paths early and the conflict test still passed); the final
publication experiment instead removed revalidation and failed on sentinel policy.
Neither survivor is counted as a killed mutant.

Analysis tests retain subprocess watchdog isolation for traversal faults; no
private module is made public for testing. Coverage tests retain stale-hash,
malformed, out-of-range, absent/zero and aggregate completeness cases. Publication
integration tests also cover missing destinations through directory symlinks,
protected inputs and both-output conflicts, and staging failure before either
output is replaced. Replacements are atomic **per file**, not as a group; a later
rename failure can leave an earlier output published. Hostile concurrent parent
directory substitution remains outside the ordinary local-filesystem guarantee.

## Final local checks

Rust 1.96.0, Node 26.3.1, Android/aarch64 in proot; Cargo uses `-j 1`.
All below passed on the restored current implementation:

```sh
cargo fmt --check
cargo clippy --locked -j 1 --all-targets -- -D warnings
cargo test --locked -j 1
cargo test --locked -j 1 --release
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps -j 1
node --test tests/*.test.mjs
for f in scripts/*.mjs; do node --check "$f"; done
python3 -B -m unittest discover -s tests -p 'test_*.py'
cargo package --list --allow-dirty
cargo bench --locked -j 1 --bench review --no-run
```

Each Rust profile passed 88 tests (14 library, 2 binary, 72 integration); these
also invoke the Node suites and real Node/Git/CLI behavior. Standalone Node ran
26 tests, Python 12. The package listing includes `tests/common/mod.rs` and the
new Node helper/tests; no reports, coverage, target, Python caches or bytecode
were packaged. The stage benchmarks ran serially after builds completed; see
[measurement data and limitations](PERFORMANCE.md#remaining-review-measurements).

### Production Rust coverage

cargo-llvm-cov 0.9.1 with native LLVM tools ran the complete library, binary and
integration suites in a **fresh, distinct target directory**. The unchanged CI
production-source filter and 95% floor passed:

- Lines: **1,095 / 1,122 = 97.59%**.
- Functions: 102 / 107 = 95.33%.
- Regions: 1,855 / 1,962 = 94.55% (not the line gate).

An initial reused-target report contained stale instrumentation/dependency
mappings and was discarded, not used as a coverage result. Reproduction where
the tool is not on PATH (substitute its installation path):

```sh
export CARGO_TARGET_DIR=/tmp/ts-crap-review-fresh-cov
export LLVM_COV="$(command -v llvm-cov)"
export LLVM_PROFDATA="$(command -v llvm-profdata)"
/tmp/ts-crap-llvmcov091/cargo-llvm-cov llvm-cov --locked -j 1 \
  --lib --bins --tests --no-report
/tmp/ts-crap-llvmcov091/cargo-llvm-cov llvm-cov report \
  --ignore-filename-regex '(^|/)(tests|benches)/|(^|/)src/.*/tests[.]rs$' \
  --json --summary-only --output-path /tmp/review-coverage.json
python3 scripts/coverage-summary.py /tmp/review-coverage.json --min-lines 95
```

The full production mutation run, 90% assertion-killed score gate, MSRV/platform
matrix, advisory audit and hosted workflow results remain required CI checks.
No full local mutation score, new dependency audit or supported-platform matrix
result is claimed here. No exclusions or lower thresholds were added.

## Real-browser limitation and smoke recipe

Generated HTML markup, styles and JavaScript are unchanged by this refactor;
only the ownership of presentation-row preparation changed. The proposal's real
browser smoke condition applies on the next report UI change. An attempted T3
`preview_open` could not launch the host browser because GLib/NSS/X11/GBM shared
libraries were missing; it failed **before loading the page**. No system-library
installation or browser-sandbox workaround was attempted. This is not a passing
browser test. Existing VM fake-DOM tests validate filter logic, not real DOM
parsing/layout.

On a machine with a functioning browser, reproduce against actual generated HTML:

```sh
fixture=$(mktemp -d)
printf '%s\n' 'export const café = (x: boolean) => x ? "😀" : "<&>";' \
  'export function quiet() { return 1; }' > "$fixture/view<&>.ts"
# Exit 1 means the expected CRAP gate failure, not generation failure.
cargo run --locked -- --root "$fixture" --format html \
  --output "$fixture/report.html" --threshold 1
python3 -m http.server 8765 --bind 127.0.0.1 --directory "$fixture"
```

Open `http://127.0.0.1:8765/report.html` in an ordinary supported browser (or T3
preview when available). Verify two parsed function rows, escaped filename `<&>` and Unicode function name
without injected nodes, a failing gate, and the full function/coverage totals.
Search for `café` and then an absent name; toggle “Only failing”, clear search,
and confirm row visibility changes while full gate/totals remain unchanged.
Inspect the browser console for errors. Repeat with CLI `--top 1` and
`--only-failing` presentation options; the report's full gate must not change.
Stop the server and remove the disposable fixture afterwards. Attach the actual
browser result when making a UI change; this recipe alone is not evidence it ran.
