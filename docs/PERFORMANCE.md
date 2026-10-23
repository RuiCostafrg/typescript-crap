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

Rust verification runs directly with Cargo: `cargo clippy`, `cargo test`,
`cargo doc`, `cargo llvm-cov` and `cargo mutants`. GitHub Actions records each
step's start/end times and conclusion, including failed steps. Compare those
native timings for each Rust-version matrix leg, platform, coverage and mutation
job. No additional runtime wraps Cargo.

Local measurements recorded 29.437 s for the initial debug run and
31.205 s for the final debug run; the first release build plus tests took
377.535 s, including 5 min 49 s of compilation. Build caches and concurrent
validation workloads differed, so these numbers do not justify removing a
profile or matrix leg. Hosted timings should be compared under like conditions.

A redundant Python coverage-summary run was removed: the same tests already run
in the full Python discovery step.

The debug/release matrix, full PR/main mutation runs, release prerequisites,
95% coverage floor and 90% mutation floor remain in force. The initial hosted
run took about two minutes for each Rust matrix leg and thirteen minutes for
the full mutation job. Compare multiple hosted step timings before changing
required checks or moving mutations off PRs; local proot timings cannot estimate
GitHub runner costs reliably.

## Remaining review measurements

Measured 2026-10-23 on Android/aarch64 in proot, Rust 1.96.0, Node 26.3.1.
[Raw samples](measurements/review-2026-10-23.json) include all five sorted samples,
fixture sizes, output bytes, process peak RSS and benchmark executable SHA256s.
Builds and measurements ran serially, with no concurrent Cargo builds. Each
process warms up once, then takes five samples. Correctness assertions, fixture
construction and the initial scan are outside timed regions. RSS is the process
high-water mark (`VmHWM` for Rust, `resourceUsage().maxRSS` for Node), **including
setup and prescan**, not stage allocation counts. Do not subtract these peaks to
claim memory saved by a stage. Mobile scheduling, thermal state and filesystem
caches still make these results noisy; these are not CI performance gates.

### Rendering and publication

Medians in milliseconds; each cell is baseline → current. Rendering measures only
serialization of a prescanned report, in reversed caller order. Both revisions
encode the same report; machine-format ordering is checked separately by tests.

| Functions | JSON | XML | SARIF | Text | HTML |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 2,000 | 1.266 → 3.268 | 1.416 → 0.735 | 32.179 → 26.029 | 1.196 → 1.194 | 1.243 → 2.032 |
| 5,000 (current only) | 4.547 | 2.672 | 79.843 | 2.503 | 2.999 |
| 10,000 | 12.884 → 10.941 | 4.232 → 4.364 | 143.034 → 146.254 | 4.930 → 6.353 | 7.397 → 9.157 |

At 10,000 functions, process RSS was 55,940 → 57,036 KiB for JSON and
200,584 → 201,656 KiB for SARIF. These peaks are dominated by setup/serialization;
they do not demonstrate a memory improvement from removing the discarded sort.
The code now does no presentation collection/sort for machine formats, but these
small, variable timing differences do **not** establish a general speedup.
Human reports still sort normally; no top-N heap was introduced.

The validation fixture contains ten functions, two existing destinations, two
protected JSON inputs and the stated number of ignored TypeScript dependency
files. Its walk deliberately includes ignored files for alias safety. The
scan-plus-publication case includes selection/read/parse/scoring, baseline and
JSON rendering, validation, staging, syncing and two renames; it does **not** load
coverage or parse the dummy protected JSON files.

| Ignored files | Validation baseline → current (ms) | Scan + publication baseline → current (ms) |
| ---: | ---: | ---: |
| 100 | 147.938 → 62.929 | 410.245 → 90.696 |
| 500 (current only) | 419.116 | 730.508 |
| 1,000 | 1,431.574 → 380.587 | 3,209.400 → 1,140.447 |

At 1,000 ignored files validation RSS was 4,296 → 5,908 KiB, and combined-case
RSS 4,832 → 5,852 KiB. Batching reduces repeated walks, not necessarily peak RSS.
The 500-file timings illustrate substantial variance; do not fit a throughput
model to these medians.

**Baseline attribution:** production source is `6ecbe804dbca93466771de25ea15558cfadd120a`.
A benchmark-only adapter invokes its original per-destination `validate` and
`write_report`; no baseline production algorithm was changed. Only the external
unit-module path annotation was adjusted for inclusion by the benchmark. Its
synthetic preflight is two destinations × two protected inputs = four walks.
The original CLI sequences three preflight walks plus one later walk; both total
eight walks including writes, but their sequencing differs. This is a
**synthetic equivalent-policy comparison, not an exact old-CLI latency result**.
Current preflight batches handles in one walk; publication refreshes the complete
plan before each rename (three walks total). It stages all contents first, but
individual atomic replacements are **not** an atomic multi-output transaction.

The recorded baseline executable was copied from the completed baseline build
log and hashed before rebuilding current. Baseline tracked production files were
verified against `git show 6ecbe80:<path>`. The current package was explicitly
cleaned and rebuilt, including the final benchmark, before this run. Earlier
shared-target runs and the unrelated v0.1.1 `/tmp/ts-crap-p2-before` fixture are
not used as comparison evidence.

### Adversarial scaling and decision

| Workload | Size | Median (ms) | Process RSS (KiB) |
| --- | ---: | ---: | ---: |
| Unicode, single-line functions | 500 | 30.162 | 7,940 |
| Unicode, single-line functions | 1,000 | 86.508 | 9,908 |
| Unicode, single-line functions | 2,000 | 232.925 | 14,284 |
| Nested functions | 32 | 24.118 | 4,796 |
| Nested functions | 64 | 18.199 | 4,844 |
| Nested functions | 128 | 161.567 | 6,044 |
| Overlapping V8 ranges | 250 | 5.821 | 45,392 |
| Overlapping V8 ranges | 500 | 15.176 | 52,940 |
| Overlapping V8 ranges | 1,000 | 8.275 | 50,324 |

The Unicode fixture contains an astral character before many functions on one
line; nested fixtures include qualified-name growth, and conversion has dense
nested overlapping ranges with a checked one-line hit result. Name/source/output
byte counts are in the raw data. Unicode and deep nesting warrant future
profiling, but nested and converter timings are non-monotonic and noisy. No
profile identifies a dominant cost yet. A per-line UTF-16 index, ancestor-name
cache, or active-range heap would add correctness-sensitive state without a
proven practical benefit. Item 5 therefore delivers measurement infrastructure
and a documented decision, **not** a claim that worst-case behavior was optimized.
Revisit with profiles, larger controlled runs and representative workloads.

### Reproduce

```sh
cargo bench --locked -j 1 --bench review --no-run
# Cargo prints the executable path; invoke that path directly for stage timing:
target/release/deps/review-<hash> unicode 1000
target/release/deps/review-<hash> nested 64
target/release/deps/review-<hash> render-json 5000
# Also render-xml, render-sarif, render-text, render-html.
target/release/deps/review-<hash> validation 500
target/release/deps/review-<hash> end-to-end 500
node scripts/bench-coverage-lines.mjs 500
```

Use Unicode sizes 500/1000/2000, nesting 32/64/128, rendering 2000/5000/10000,
validation/publication 100/500/1000 and conversion 250/500/1000. Run each in a
separate process, sequentially, without builds or tests competing for resources.

For a baseline reproduction, make a detached worktree at `6ecbe80`, copy the
current `benches/review.rs`, and register its `harness = false` bench target.
Replace only the benchmark's `mod output` inclusion with an `original_output`
inclusion and a local adapter: `DestinationPlan::new` calls original `validate`
for every destination/input pair; `publish` calls original `write_report` for
each destination, passing the first protected input. Set the original output's
test module path to `output/tests.rs` so benchmark test discovery still compiles.
Use **distinct `CARGO_TARGET_DIR` values** for both builds, retain logs and
executable hashes, and compare production files against the baseline commit.
This preserves the synthetic sequencing caveat above; use an actual CLI harness
instead if exact historical CLI latency is the question.
