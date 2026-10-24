# Repository improvement proposal

Reviewed on 2026-10-22 at `030cb7634ce73d0d55c5e3190e0db4672a769900`.

Scope: the repository on `main` at the commit above, including the Rust library and CLI, Node coverage collectors, tests, benchmarks, and CI/security workflows. This is a repository improvement backlog, not a pull-request review. Findings are evaluated against the current code regardless of when they were introduced. References below use paths and line numbers at the reviewed commit. The original review contained no implementation changes.

Implementation status (2026-10-23): item 1 shipped separately in PR #11. This
follow-up implements items 2–4 and 6–8, and completes the measurement investigation
in item 5. A subsequent performance follow-up implements the three lookup changes:
a monotonic UTF-16 cursor, cached qualification through a scope stack, and a
priority heap for active V8 ranges. See the follow-up measurements in
`PERFORMANCE.md`; the earlier noisy investigation is retained as historical evidence.
Node coverage collection retains
regular-file replacement semantics; the importer now stages a complete report and
atomically links it into a fresh destination, preserving its no-overwrite policy.
Coverage loading additionally rejects duplicate line and file entries instead of
silently keeping the last one, so contradictory evidence can never promote a
failing gate to a pass.
Valid baseline SHA-256 identities are normalized to lowercase on load so unchanged
functions still match across file moves when a baseline uses uppercase hex.
The real-browser smoke test in item 6 remains conditional
on the next report UI change; this refactor does not change the generated UI.
See [performance evidence and reproduction](PERFORMANCE.md#remaining-review-measurements)
and [behavioral test evidence](REVIEW_VALIDATION.md).

The strongest next steps are to fix Node coverage publication safety, remove unnecessary rendering work, and consolidate repeated validation and test setup. Keep the current single Rust package. Its existing modules are small enough that a workspace split, generic framework, or wholesale rewrite would add maintenance cost.

## Priorities and sizing

- **P1:** next fix; confirmed file-safety or correctness risk.
- **P2:** next improvement cycle; meaningful performance, test, or maintenance benefit.
- **P3:** opportunistic cleanup after the above.
- **S:** up to one engineer-day; **M:** two to three days; **L:** four to seven days. Estimates include implementation, focused tests, and review, but exclude CI queue time. They are planning estimates, not commitments.

## 1. Harden Node coverage publication — P1 / M

**Evidence:** `scripts/coverage.mjs:109–115` uses a predictable `${output}.${pid}.tmp` name and ordinary `writeFile`, then renames it. Unlike Rust's `NamedTempFile` writer, this does not exclusively create the staging file.

**Confirmed reproduction:** in an isolated temporary directory, a test child created the expected staging path as a symlink to a sentinel file before exiting successfully. The collector returned **0**, replaced the sentinel contents with coverage JSON, and published a symlink as the output. All fixture files were disposable; no project files were changed.

This is a file-clobbering weakness when another process can plant the staging entry. It is not a privilege-escalation claim: the test command already executes with the caller's permissions. The issue still violates safe publication expectations in shared or contaminated output directories.

**Proposal:** create a private random staging directory beside the output, exclusively open a file inside it, write through that owned handle, close it, and atomically rename the regular file. Clean up only staging entries this invocation successfully created. Reject TypeScript destinations independently of whether they happened to be included in the source snapshot; revalidate destination policy before publication. Document that hostile concurrent replacement of parent directories is outside the ordinary local-filesystem guarantee. Keep importer “fresh destination only” semantics distinct from replacement semantics.

**Acceptance:** regression cases for planted staging symlinks and hardlinks, existing output, rejected source destination, failed child, write/rename failure, and cleanup. Assert sentinel bytes stay unchanged and successful output is a regular complete JSON file. Run platform cases where link creation is supported. Node documents exclusive `wx` creation and its filesystem caveats in [file-system flags](https://nodejs.org/api/fs.html#file-system-flags).

## 2. Avoid sorting rows for machine formats — P2 / S

**Delivered:** only text/HTML call `presentation_rows`; machine formats retain caller order. The benchmark separates rendering from scanning and records process peak RSS.


**Evidence:** `src/report.rs:35–50` allocates and sorts presentation rows before format dispatch. JSON, XML, and SARIF subsequently use `report.functions`, so this work is discarded.

**Proposal:** a private `presentation_rows` helper called only by text and HTML. Preserve machine-format ordering and full gate semantics. Do not add top-N heap selection until a benchmark shows it is worthwhile; ordinary sorting is simpler for human reports.

**Acceptance:** existing navigation and machine-report tests remain unchanged; benchmark rendering alone with large reports and record elapsed time and allocations or peak memory. The unnecessary work is confirmed by code inspection; its real-world latency impact has not been measured.

## 3. Plan and batch output validation — P2 / M

**Delivered:** one CLI-owned `DestinationPlan` holds both outputs and protected inputs. Each validation phase uses one unfiltered source walk; all outputs are staged before publication, then all policies are refreshed before each atomic file replacement. The group is not a transaction. Missing-path directory aliases and existing hardlink aliases are rejected before publication.


**Evidence:** `src/output.rs:34–52` walks the entire project for each existing destination, deliberately including ignored sources. `src/cli.rs:100–147` validates output against coverage, config, and baseline through several calls; `write_report` repeats validation before and after staging. A run with existing report and baseline paths can traverse dependency-heavy trees repeatedly.

**Proposal:** introduce a small CLI-owned destination plan containing report/baseline destinations and all protected inputs. Check all relevant destination handles in one walk per validation phase, then retain final checks and atomic publication. Resolve conflicting output aliases before the first write where possible. Avoid a global cache whose contents could become stale.

**Acceptance:** preserve ignored/unselected TypeScript, symlink, hardlink, coverage, config, and baseline protection tests. Add alias/conflict cases proving rejection precedes publication. Benchmark existing destinations with a large ignored directory. Do not gain speed by dropping protected inputs or final checks. Cost is currently inferred, not measured.

## 4. Consolidate identical validation rules — P2 / S

**Delivered:** private, specifically named serialized-path and SHA256 checks are shared by coverage/baselines; CLI threshold parsing delegates to the existing public validator. Source selection and overwrite policies remain distinct.


**Evidence:** normalized relative paths and 64-character hexadecimal digests are checked separately in `src/coverage.rs:62–72` and `src/baseline.rs:47–54`. CLI threshold checks in `src/cli.rs:70–77` repeat `src/lib.rs:89–95`.

**Proposal:** add a small private validation module with specifically named path and digest functions, preserving caller-specific error context. Have CLI parsing call `validate_threshold` after parsing. Keep validation at both public entry points. Avoid a generic validation framework or new types solely to save a few lines.

**Acceptance:** table-driven cases cover absolute/empty/traversal paths, separators, colons, valid Unicode names, digest length/nonhex input, NaN, infinity, and negative thresholds. Existing JSON and CLI error contracts remain stable.

**Deliberate differences to preserve:** source selection excludes declaration files, whereas overwrite protection must protect them and case variants too. Glob syntax is also different from serialized source-path syntax. Do not merge these policies into one permissive “is valid path” helper.

## 5. Measure adversarial input scaling — P2 / M investigation; L only if optimization is justified

**Investigation and follow-up implementation complete:** `benches/review.rs` and `scripts/bench-coverage-lines.mjs` measure multiple input sizes, with one warmup, five samples and peak RSS. Stage-separated rendering/validation and scan-plus-publication measurements accompany adversarial scan/conversion fixtures. The follow-up replaces repeated UTF-16 prefix scans with a constant-storage cursor, ancestor re-walks with a scope stack referencing emitted names, and active-range scans with a lazy-expiration priority heap. Reference tests retain the original name/column algorithms and an independent coverage oracle.


**Evidence:** `src/analysis.rs:63–80` revisits ancestors for each qualified function name, and `:207–210` re-counts the UTF-16 prefix for each function on a line. `scripts/coverage-lines.mjs:32–40` scans every active range at each boundary. The existing benchmark emphasizes multiline functions and many files, not these shapes.

**Proposal:** add benchmark fixtures for many functions on one Unicode line, deeply nested functions, and densely overlapping V8 ranges. Measure scan, conversion, rendering, and output validation separately as well as end to end. Use multiple sizes, a warmup, repeat samples, and record toolchain/platform, wall time, and peak memory. Keep correctness assertions outside timed sections where feasible.

If profiles justify it, use a per-line UTF-16 index or monotonic offset cursor, maintain scope state during traversal, and consider an ordered active-range structure. Preserve innermost-range and zero-hit tie behavior. Qualified names themselves can have quadratic output size under deep nesting; removing ancestor re-walks cannot eliminate that inherent cost.

**Acceptance:** identical names, identities, byte/UTF-16 columns, owned lines, scores, and conservative coverage conversion on reference fixtures. Report scaling evidence rather than a promised speedup. No unstable wall-clock assertions in ordinary CI. Defer parallel scanning and persistent caches until CPU and memory profiles justify their complexity.

## 6. Strengthen module tests with behavioral invariants — P2 / M

**Delivered:** isolated analysis invariants, exhaustive three-line coverage states, deterministic generated V8/reference-oracle cases, baseline ambiguity/reordering/conflict cases, every presentation-option combination, and publication alias/staging cases. Deliberately broken implementations demonstrate assertion failures; details are in `REVIEW_VALIDATION.md`. The browser condition remains deferred until a UI change; the current environment cannot launch its browser because host shared libraries are missing.


Keep private unit tests in `#[cfg(test)] mod tests` alongside their module, including external `src/<module>/tests.rs` files where needed. Keep `tests/` for public-library and CLI behavior. This matches [Rust's documented test organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html); exposing private APIs just to test them is unnecessary.

High-value additions:

- **Analysis:** whitespace/comment insertion preserves identity and complexity; nested-function changes do not alter the enclosing function's owned identity/metrics. Include Unicode, default parameters, TSX, malformed and deep syntax. Retain subprocess isolation for traversal tests that can hang under mutation.
- **Coverage:** adding genuinely covered owned lines cannot reduce coverage; absent evidence stays distinct from explicit zero hits; fractions stay within `[0,1]`. Use small generated cases plus a simple reference oracle for overlapping V8 ranges. Preserve explicit stale-source and overflow cases.
- **Baselines:** ambiguous names/fingerprints never suppress a new violation; reorder unique entries without changing decisions; threshold and rename conflicts fail consistently.
- **Reports:** every supported presentation filter leaves the full gate and totals unchanged. Keep escaping assertions and add one actual generated-HTML browser smoke test when the report UI next changes. The current VM fake-DOM tests validate filter logic but cannot establish real DOM parsing or browser behavior.
- **Publication:** add the file-safety regressions from item 1; they guard a failure not covered by line-coverage percentages.

Use table-driven tests first. Add property testing or fuzzing only for the parser/converter boundaries where generated inputs find new behaviors; keep minimized failures as permanent fixtures. Fuzz runs need input/time limits and should not replace deterministic PR tests. Avoid tests tied to Debug formatting, internal helper counts, or the precise implementation of a refactor.

**Acceptance:** each new suite explains its invariant and demonstrates a meaningful failure on a deliberately broken implementation. Keep the existing 95% production coverage and 90% mutation floors; do not target 100% by adding shallow tests. Full mutation runs already exist and should not be proposed as a new feature.

## 7. Share test fixtures and collector plumbing carefully — P2 / M

**Delivered:** integration fixture and Node-command builders live under `tests/common/`; shared snapshot/hash/UTF-8 decoding lives in `scripts/coverage-snapshot.mjs`. Explicit caller options preserve native/importer declaration selection, source retention and line-count differences. Each collector retains its own freshness and publication contracts.


**Evidence:** integration suites repeatedly create temporary TypeScript projects and spawn Node/CLI processes (`tests/coverage_e2e.rs`, `tests/coverage_import.rs`, `tests/example.rs`). The two Node collectors also duplicate source discovery, hashing, command execution, and output handling, but differ in declaration selection, freshness checks, and output replacement policy.

**Proposal:** extract small `tests/common/mod.rs` builders for temporary projects and command assertions. Keep scenario setup and assertions readable at call sites. Extract only identical Node snapshot/decoding primitives into a helper, with explicit options for intentional policy differences. Share safe publication only after item 1 defines its contracts; do not silently change importer behavior.

**Acceptance:** fewer repeated fixture/command helpers, unchanged failure diagnostics and source-freshness guarantees, and all existing integration tests pass. Preserve per-test temporary directories; no process-wide working-directory or environment mutation. Keep dependencies explicit so parallel tests remain isolated. Cargo compiles integration targets separately, so shared support belongs under a subdirectory, not a new top-level `tests/common.rs` target; see the [Cargo test reference](https://doc.rust-lang.org/cargo/commands/cargo-test.html).

## 8. Combine coverage lookups — P3 / S

**Delivered:** `SourceCoverage::evidence` validates and counts owned lines once, returning completeness and fraction together. Missing files/lines and explicit zero evidence retain their separate semantics.


**Evidence:** `src/lib.rs:179–186` calls `complete` and `fraction`; `src/coverage.rs:104–125` separately traverses owned lines for completeness, bounds, and covered counts.

**Proposal:** one internal `evidence(lines)` result with `complete` and `fraction`, validating and counting in one pass. This is a modest simplification, not a proven performance bottleneck.

**Acceptance:** preserve missing-file, missing-line, explicit-zero, empty-body, out-of-range, and aggregate-gate behavior. Keep this separate from broader coverage-format changes.

## Code structure after these changes

Retain `analysis`, `selection`, `config`, `coverage`, `baseline`, and `report` in the library. Keep argument parsing, exit codes, and filesystem publication in the binary. Add only a narrowly scoped private validation module and test support. Keep public `scan` and `render` stable; no workspace migration is warranted by the current size.

If the renderer grows materially, split format implementations under `report/` with a shared presentation selector and distinct terminal/HTML/XML escaping functions. Do not introduce a renderer trait or templating dependency merely to split a match expression. Prefer borrowed data and iterators when clear, but prioritize correctness over removing every allocation. Keep `anyhow` at orchestration boundaries; introduce typed errors only when consumers actually need to branch on error categories.

## Security assessment and boundaries

Positive controls already present: forbidden unsafe Rust, iterative AST traversal, source-bound coverage hashes, strict input versions/fields, shell-free argument passing, terminal escaping, HTML/XML escaping, atomic Rust report writes, pinned Actions, read-only CI, and separated release publication. Preserve these during cleanup.

Besides item 1, large inputs and slow subprocesses are availability concerns worth measuring, not confirmed remote exploits. Source/coverage files are currently read into memory, and collection executes the supplied test command. Document trusted-command requirements; resource limits should be configurable if hosted/untrusted scanning becomes a supported use case. Do not describe the collector or Git helpers as a sandbox.

Dependency advisory scanning is already configured. `cargo-audit` is unavailable in this local environment, so this review does **not** establish a clean current advisory result. Repository-settings enforcement and release attestations were not independently audited; source workflow checks alone cannot prove those external protections.

## Delivery order and verification

1. Item 1 with its regression tests.
2. Items 2 and 4 as independent small changes.
3. Item 3 with filesystem safety tests; items 6–7 incrementally around touched behavior.
4. Item 5 measurement report, followed only by justified optimizations.
5. Item 8 opportunistically.

Estimates overlap where a refactor includes tests; do not sum every item as independent work. No new dependency, concurrency layer, or CI gate removal is required for the initial improvements.

Review validation: an isolated publication reproduction confirmed the overwrite; `cargo test --locked -j 1` passed in the independent review, and the 12 Python helper tests and 8 Node unit tests passed. Local tools were Rust 1.96.0 and Node 26.3.1; this is not a repeat of the complete supported-platform/MSRV matrix. No new benchmark, full mutation run, release verification, or dependency audit was performed for this proposal. Prior performance figures in `docs/PERFORMANCE.md` are explicitly noisy and do not establish general speedups.
