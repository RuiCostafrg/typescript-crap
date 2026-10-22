# Performance and CI measurement

The scan benchmark now covers both a large file and collections of small files,
including alternating TypeScript/TSX grammars and copies of the runnable shop,
inventory and async retry modules. Every case uses real files and source-hashed
coverage, warms up once, checks the expected function count, and reports five
samples. There are no timing assertions.

```sh
cargo bench --locked --bench scan -j 1
TS_CRAP_BENCH_FILES=250 cargo bench --locked --bench scan -j 1
TS_CRAP_BENCH_FILES=250 TS_CRAP_BENCH_MIXED=1 cargo bench --locked --bench scan -j 1
TS_CRAP_BENCH_FILES=300 TS_CRAP_BENCH_REAL=1 cargo bench --locked --bench scan -j 1
```

`TS_CRAP_BENCH_FUNCTIONS` sets the synthetic function count (default 2,000).
`TS_CRAP_BENCH_FILES` defaults to one and distributes those functions evenly.
Real-module mode instead repeats the three modules, with 800 functions in 300
files. Real and mixed modes are mutually exclusive.

Local measurements on 2026-10-22 used Rust 1.96.0 on Android/aarch64 in proot.
Before parser reuse, medians for 2,000 functions were 102.403 ms in one file,
223.389 ms in 250 TypeScript files, and 299.948 ms in 250 mixed-grammar files.
A follow-up run with reuse measured 79.849, 263.102 and 281.315 ms respectively.
The 300-file real-module case (800 functions, 275,200 source bytes) measured
2,056.412 ms median, 1,594.575 ms minimum and 2,844.011 ms maximum while other
validation builds were running. CPU contention and thermal variance were
substantial (one mixed run had a 2,199.721 ms maximum), so these samples do not establish a general speedup.

Each scan now lazily initializes one parser per grammar and reuses it across
files. Source trees are parsed independently, with no incremental tree reuse or
persistent cache. Regression tests alternate grammars, syntax errors and empty
files to check that state does not leak between sources. Selection, hashes,
function identity and ordering are unchanged. Parallelism and caching remain
deferred because these measurements do not justify their added complexity.

## CI costs

`scripts/time-command.py` runs an argument array directly, records monotonic wall
time and the command exit code in append-only JSONL, and preserves failure exits.
When `GITHUB_STEP_SUMMARY` is set, it also appends a readable timing line. Tests
exercise success, failure, missing executables and summary output.

The CI Rust-version matrix records Clippy, debug tests, release tests and rustdoc
individually. macOS/Windows behavior jobs, instrumented coverage tests and the
full mutation job also record their own costs. Artifacts retain timings even
when a check fails, allowing comparisons across actual hosted matrix legs.
Local wall-time telemetry recorded 29.437 s for the initial debug run and
31.205 s for the final debug run; the first release build plus tests took
377.535 s, including 5 min 49 s of compilation. Build caches and concurrent
validation workloads differed, so these numbers do not justify removing a
profile or matrix leg. Hosted timings should be compared under like conditions.

A redundant Python coverage-summary run was removed: the same tests already run
in the full Python discovery step.

The debug/release matrix, full PR/main mutation runs, release prerequisites,
95% coverage floor and 90% mutation floor remain in force. No hosted timings were
available for this change, so scheduling full mutations instead of PR runs would
be premature. Compare multiple hosted timing artifacts before changing required
checks; local proot timings cannot estimate GitHub runner costs reliably.
