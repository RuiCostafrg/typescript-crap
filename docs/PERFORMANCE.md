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

This records the initial investigation. The three lookup optimizations were
implemented in a subsequent follow-up; see "Lookup optimization follow-up" below.

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

## Report streaming measurements

The v0.4.0 rendering benchmark ran on 2026-10-25 in fresh optimized processes,
with one warmup and five samples per case. The
[raw measurements](measurements/p2-rendering-2026-10-25.json) retain timings,
output sizes, toolchain, and process peak RSS.

| Functions | SARIF output | Median (ms) | Process peak RSS (KiB) |
| --- | --- | ---: | ---: |
| 2,000 | String wrapper | 3.544 | 18,128 |
| 2,000 | Stream to `io::sink` | 1.338 | 14,556 |
| 10,000 | String wrapper | 20.033 | 59,372 |
| 10,000 | Stream to `io::sink` | 6.660 | 49,584 |

Reproduce with the release `review` benchmark's `render-sarif` and
`stream-sarif` modes at sizes 2000 and 10000. Output byte counts agree between
the two modes. These measurements describe the current implementation at that
revision, rather than a controlled before/after comparison. Sink output omits
file I/O and can allow compiler optimizations; peak RSS includes fixture setup
and parsing. No general throughput or memory reduction percentage is inferred.

## Lookup optimization follow-up

The three lookup changes are now implemented. This supersedes the initial
deferral above; that investigation and its raw data remain historical evidence.
No public report fields, scoring rules, source identities, or coverage tie rules
change.

- **UTF-16 columns:** preorder visits function starts in increasing source order.
  A cursor counts only the slice between starts on the same line and resets at
  a different line start. It uses constant state, without building a per-byte
  index or repeatedly decoding a whole line prefix. Total prefix decoding is
  linear in the visited source segments.
- **Scope names:** a stack expires completed scopes at the next scope event.
  Named classes reset qualification and anonymous classes inherit the enclosing
  prefix, matching the original ancestor algorithm. Function entries reference
  names already emitted in the report; class entries borrow their source names.
  Auxiliary storage is O(depth), and name construction copies the required
  output once. Deeply nested qualified names still have quadratic total output
  size; caching does not remove that inherent cost.
- **Coverage ranges:** a min-heap orders ranges by clipped length, then hit
  count. Expired entries are removed lazily before selecting the root. Every
  range is inserted once and removed at most once, bounding heap operations by
  O(R log R) with O(R) heap storage. End boundaries still split source segments.
  Equal-length ranges retain the lower count, including uncovered zero hits.

### Paired measurements

Baseline is `821229ad9302a6a683ca6f45a40ec738fd190104`. The release benchmark was
built before editing the Rust implementation, copied outside Cargo's target
directory and hashed. The current implementation was then rebuilt and copied
separately. Both compiler logs identified the workspace package and the saved
executables have distinct SHA256 hashes. The Node baseline module and benchmark
were also copied before changing the converter. This comparison uses the same
benchmark source at both revisions, with no output-validation adapter.

Measured on Android 12/aarch64 in proot, Rust 1.96.0 and Node 26.3.1. All builds
completed before timing. Each fresh process performs one warmup and five samples.
Two paired rounds alternate baseline/current, then current/baseline, to reduce
ordering bias. The table shows the range of the **two round medians**, in ms;
all individual samples, process peak RSS, source hashes and executable hashes
are in [the raw data](measurements/lookup-optimizations-2026-10-23.json).

| Workload | Size | Baseline median range (ms) | Optimized median range (ms) |
| --- | ---: | ---: | ---: |
| Unicode, single-line functions | 1,000 | 98.072–119.175 | 60.576–80.607 |
| Unicode, single-line functions | 4,000 | 682.880–688.422 | 201.230–216.094 |
| Unicode, single-line functions | 8,000 | 2,222.354–2,242.225 | 291.985–309.502 |
| Nested functions | 64 | 17.914–32.001 | 26.382–36.684 |
| Nested functions | 128 | 160.057–160.157 | 7.395–15.627 |
| Nested functions | 256 | 742.949–778.759 | 20.838–35.669 |
| Overlapping V8 ranges | 32 | 0.255–0.497 | 0.217–0.313 |
| Overlapping V8 ranges | 500 | 10.717–12.529 | 2.869–4.658 |
| Overlapping V8 ranges | 2,000 | 32.634–36.866 | 3.938–4.408 |
| Overlapping V8 ranges | 8,000 | 450.738–467.276 | 17.200–21.702 |

The larger fixtures consistently improve in both rounds. The smallest nested
case does not improve consistently; filesystem/scheduling variance is still
visible. These are bounded synthetic workloads, not a general throughput or
memory guarantee. Rust timings include selection, filesystem reads, parsing,
scoring and sorting. The nested fixture uses separate lines and therefore does
not benefit from avoiding long UTF-16 prefixes. Its emitted name bytes are
unchanged (173,699 at depth 256). Both Rust changes are present in each optimized
run, so the data is not an isolated instruction-level profile.

RSS is the process high-water mark including fixture setup, initial scanning and
runtime overhead. Keep those values as observations; do not infer per-stage
allocation savings from them. No unstable timing assertions are added to CI.

Reproduce with the existing `review` executable in release mode, using Unicode
sizes 1000/4000/8000 and nesting 64/128/256. For Node use
`node scripts/bench-coverage-lines.mjs SIZE` with 32/500/2000/8000. Preserve each
revision's executable/module before rebuilding the other, verify source and
artifact hashes, and run without concurrent builds.

### Behavior verification

The new analyzer reference test computes names by walking ancestors and computes
UTF-16 columns from full prefixes, independently of the production caches. It
covers named/anonymous/nested classes, inheritance expressions, default-parameter
functions, computed names, adjacent siblings, abstract methods, TSX, BOM, astral
Unicode and CRLF. Existing identity, complexity and source-ownership tests remain
in place. A separate read-only code review found no correctness/security issues.

The converter retains its 300 generated range cases against an independent
per-code-unit oracle and adds a larger expired-priority/gap/tie case, each checked
in both input orders. An isolated copy with the heap's count tie comparison
reversed fails with an assertion, demonstrating that the tests reject that fault.

Local formatting, strict all-target Clippy, the complete debug suite (89 Rust
tests, including real Node/Git/CLI cases), rustdoc with warnings denied, Node
syntax checking and package listing passed. All 15 library tests also passed
in the release profile, including the analyzer reference test. The complete
hosted release/platform/coverage/mutation/audit checks remain merge requirements;
a local benchmark is not a replacement for those gates.
