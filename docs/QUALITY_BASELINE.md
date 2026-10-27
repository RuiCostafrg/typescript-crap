# Quality gates

CI enforces these gates for pull requests and main. Floors must not be lowered
to make a failing change pass. Do not add production application-logic exclusions
to mutation runs.

## Rust source coverage

The minimum is **95% production line coverage**, calculated from integer
covered/total counts without rounding. Empty, malformed, or missing reports fail.
The complete library, binary, and integration suite is instrumented; test modules,
benchmarks, dependencies, and runtime sources are excluded from the production
report. Use the pinned tools in [the coverage workflow](../.github/workflows/coverage.yml)
and fresh profiles for each source revision.

The `rust-coverage` artifact includes LCOV, JSON totals, and an HTML report.
Inspect uncovered lines and add behavioral tests for meaningful gaps. Rust source
coverage is distinct from the TypeScript coverage used to calculate CRAP scores.

## Mutation testing

The minimum is **90%**, calculated as caught / (caught + missed + timeout).
Unviable mutants are reported separately and never counted as catches. Baseline
failures, incomplete outcomes, infrastructure errors, and timeouts fail
independently. Inspect every survivor, aim for zero survivors, and add behavioral
tests for meaningful gaps. A focused local run does not replace the full
production mutation run.

The [mutation workflow](../.github/workflows/mutation.yml) uploads
`mutation-results` with outcomes, individual diffs, and logs, including on failure.
Coverage and mutation artifacts are retained for 14 days.

## Verification and release evidence

[CONTRIBUTING.md](../CONTRIBUTING.md) documents the local commands, supported
platform/MSRV checks, and publication process. The [v1.0.0 acceptance ledger](RELEASE_V1.md)
records that release's exact source and hosted results. Historical release
measurements are evidence for their recorded commits, not the current branch.
Use the latest exact-commit workflow artifacts when assessing a new change.
