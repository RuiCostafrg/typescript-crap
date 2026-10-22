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
