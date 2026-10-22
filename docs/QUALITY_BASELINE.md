# Quality gates for v0.2.0

The full Rust library/binary/integration suite was freshly instrumented on
2026-10-22 with Rust 1.96.0 and matching LLVM 21.1.8. The production-source export
measured **940 / 971 lines (96.81%)**, **92 / 95 functions (96.84%)**, and
**1588 / 1704 regions (93.19%)** before the final CLI safety tests were added.
All ten production modules were included; test modules, integration tests,
benchmarks, dependencies and Rust runtime sources were excluded. These values are
a current local measurement, not historical README percentages or hosted results.
The initial hosted run on commit `c452d65` measured **949 / 971 lines (97.73%)**,
**92 / 95 functions (96.84%)**, and **1617 / 1704 regions (94.89%)** using the pinned
coverage workflow. Both exceeded the 95% line floor. The final hosted artifact is
the authoritative measurement for the release SHA.

The CI floor is **95% production line coverage**, leaving less than two percentage
points of headroom from that measurement. The check uses integer covered/total
counts without rounding. Empty, malformed or missing coverage fails. Floors must
not be lowered to make failing changes pass. Add behavior tests for new code and
review uncovered lines in the uploaded HTML report.

The mutation floor is **90%**: caught / (caught + missed + timeout). Unviable
mutants are reported separately and never counted as catches. Baseline failure,
incomplete outcomes, infrastructure errors and timeouts fail independently. No
application-logic exclusions are added. Every survivor must be inspected, with
behavior tests added for meaningful gaps; aim for zero survivors. Full outcome
artifacts retain individual diffs, logs and counts rather than only a percentage.

Run coverage with the pinned cargo-llvm-cov version in the workflow. On the local
Android Rust toolchain, equivalent instrumentation uses `-C instrument-coverage`,
unique `%p-%m.profraw` filenames, matching `llvm-profdata merge -sparse` and
`llvm-cov export --summary-only` over all test and CLI executables, selecting only
production source paths. Do not reuse raw profiles from previous source revisions.

The initial full hosted mutation run on `c452d65` tested 260 mutants: 243 caught,
3 missed, 14 unviable and zero timeouts (**98.78%**). Follow-up behavior tests cover
missing coverage in type-only selections, duplicate qualified-name matching on
both sides of a baseline, and report staging across filesystems. Final release
results supersede this initial run; no survivors are excluded from mutation.


## Review follow-up validation (2026-10-22)

The remaining P3 implementation adds parser reuse, multi-file/real-module
benchmarks, a pure V8 conversion layer, syntax diagnostics, report navigation,
CI cost measurements, and runnable inventory/retry examples. Fresh instrumentation with
Rust 1.96.0 and matching LLVM 21.1.8 measures **1044 / 1069 production lines
(97.66%)**, **98 / 102 functions (96.08%)**, and **1777 / 1870 regions (95.03%)**.
All ten production Rust modules are included; test/benchmark/dependency sources
are excluded. The existing 95% line floor passes without modification.

The local raw profiles live in `coverage/p3-raw/`, with
`coverage/p3-summary.json` and `coverage/p3.profdata`. They were freshly collected
for this source revision, including the full instrumented integration suite.
All **76 Rust tests** pass in debug and release; **12 Python tests** and
**8 Node unit tests** pass. Strict Clippy,
formatting, rustdoc, actionlint and package-content validation are checked locally.
Initial hosted CI on `684fb5b` passed both Rust matrix legs, macOS/Windows behavior,
coverage, audit and workflow checks. Visual browser inspection is not claimed:
the T3 browser host lacks its required shared libraries. HTML filter logic and
generated markup are covered by Node and Rust tests.


Changed-line mutation testing exercised **23 mutations** against the full Rust
suite: 20 caught, 2 unviable and 1 survivor, with no timeouts. The initial
assertion-killed score was **95.24%**, passing the unchanged 90% gate. The survivor
removed the internal parser-state `Debug` output. Parser-lifecycle assertions now
check lazy grammar initialization; a separate focused rerun killed that mutation.
All **21 viable changed-code mutations** were therefore caught across the two
rounds, with no unresolved survivors or application-logic exclusions. The complete
277-case production mutation run remains a required CI check; this local run was
scoped to changed code. Detailed outcomes/diffs/logs are retained under
`reports/p3-mutations/` and `reports/p3-parser-lifecycle-mutations/`.

The first full hosted mutation run exercised all 277 candidates: 261 caught,
14 unviable and two timeouts in deliberately broken AST cursor transitions.
The original watchdog failed correctly, but newly added parser tests could hang
other threads in the same test runner. All analyzer unit tests now run in exact
subprocesses with the shared ten-second watchdog; a nonterminating child is
killed and reaped, and the parent fails an assertion. The mutation timeout and
score gates are unchanged. The full hosted mutation job must pass for the new
head before merging.

A focused local `cargo mutants --file src/analysis.rs --re ' in walk$' -- --lib`
rerun caught both formerly timing-out mutations, with zero survivors or timeouts.

Rust checks invoke Cargo directly. CI cost comparisons use GitHub Actions' native
step timings; the additional Python timing wrapper and its two tests were removed.
