# Performance benchmarks

The [README](../README.md#performance) summarizes the compact-score build on
a pinned VS Code checkout. [Raw measurements](measurements/) retain the
workload, samples, output checks, and tool and machine details for historical
runs. Compare builds measured together on the same runner.

## Real-corpus benchmark

The CI guard uses synthetic fixtures. For numbers on a real project, the
opt-in [corpus benchmark workflow](https://github.com/RuiCostafrg/typescript-crap/blob/main/.github/workflows/corpus-benchmark.yml) checks out
`microsoft/vscode` at the configured commit, builds the
shipped `dist` profile for `x86_64-unknown-linux-gnu` and static
`x86_64-unknown-linux-musl`, and runs
`ts-crap --root <corpus> --on-syntax-error report --format json --output <file>`
with each build. Each build is measured with the destination absent and with
it present (which adds the output preflight walk), after one warmup per mode,
alternating builds between rounds. It reports median, minimum and maximum
wall time, user and system CPU and GNU time peak RSS, and fails if any run's
report SHA-256 differs. No budget is applied. The job summary shows the
medians; the `corpus-benchmark` artifact keeps every observation, binary
hashes and runner details. Recorded runs live in `docs/measurements/`.

The optional `baseline_ref` input (for example `v2.4.0`) builds an older ref
with the same toolchain, `dist` profile, targets and `--locked` flag and
measures it in the same run. Its tracked files are exported with `git archive`
into a separate directory (untracked files are never copied) and built in a
separate `CARGO_TARGET_DIR`; the extra builds appear as `baseline-glibc` and
`baseline-musl`, keeping the `glibc` and `musl` names for the current commit,
and take part in the existing alternating rounds. Hosted runners differ by CPU
model: the same code took 14.4 s on one runner and 25 s on another, so only
builds measured on one runner can be compared. The record gains the baseline
ref and its resolved commit SHA, and the summary gains a table of medians and
current-to-baseline ratios. Each current build's report SHA-256 must equal its
baseline's; older versions can legitimately change output, so pass
`allow_report_change` to record the difference instead of failing.

On Linux with GNU time, pass the same corpus and repeat `--scanner` for each
build to compare:

```sh
python3 scripts/bench-corpus.py --corpus /path/to/vscode \
  --scanner candidate=target/dist/ts-crap --runs 3 --output results.json
```

## Local stage benchmarks

Run these from a source checkout. The scan benchmark uses real temporary files
and source-bound coverage, validates function counts, warms up once, and records
five samples. Set `TS_CRAP_BENCH_FUNCTIONS` or `TS_CRAP_BENCH_FILES` to size
the synthetic fixtures.

```sh
cargo bench --locked -j 1 --bench scan
TS_CRAP_BENCH_FILES=250 TS_CRAP_BENCH_MIXED=1 cargo bench --locked -j 1 --bench scan
TS_CRAP_BENCH_FILES=300 TS_CRAP_BENCH_REAL=1 cargo bench --locked -j 1 --bench scan
cargo bench --locked -j 1 --bench committed -- 32
cargo bench --locked -j 1 --bench review -- render-json 4000
```

The committed-source benchmark measures Git blob comparison in isolation. Its
argument is a fixture size in MiB. The Linux memory figure is the parent
process high-water mark. For stage timings, build `review` with
`cargo bench --locked --bench review --no-run` and run the resulting executable
directly to avoid Cargo overhead. See [verification](VERIFICATION.md).

## Continuous performance regression checks

The Rust 1.90 CI job runs `scripts/check-performance.py`.

The baseline is the exact PR base SHA, the preceding main SHA on pushes, or
`v2.0.1` on manual dispatch. Empty, all-zero or locally unavailable event
references fall back to `v2.0.1`; failure to resolve that tag still fails the job.
A full checkout makes those sources available. The
runner exports tracked baseline files, builds baseline and candidate sequentially
with separate Cargo target directories, `--locked --release -j 1`, and the same
Rust/Node tools. All builds finish before measurement. Untracked user files are
never copied into the baseline. No previous runner's timings are used as a gate.

Eleven fixed, validated workloads protect important optimization boundaries:

| Workload | Fixture | What it exercises |
| --- | --- | --- |
| Covered scan | 4,000 functions in 40 TS/TSX files | Source hashes, coverage lookup, parser reuse and scoring |
| Unicode TSX | 4,000 arrows on one line with astral characters and JSX | UTF-16 columns and long-line scanning |
| Nested functions | Depth 192 | Scope naming, child ownership and hashing |
| SARIF | 4,000 failing functions | Complete finding serialization and fingerprints |
| Git batch | 3,840 functions in 48 committed files, CRLF worktree snapshots | One batch verifier, pinned identities and newline equivalence |
| Large Git blob | A 32 MiB comment and one function | Bounded blob verification memory without a second full blob copy |
| Coverage conversion | 16,000 overlapping V8 ranges | Active-range heap scaling and conservative line evidence |
| Mixed corpus | 397 small TS/TSX files and three 2–4 MiB files (about 9 MiB, 7,504 functions) | Per-file overhead next to large trees; peak RSS from the largest file |
| Existing output | 3,000 functions in 1,500 files, written with `--output` to an existing file | The preflight walk that checks TypeScript source aliases before publication |
| Deep tree | 3,840 functions in 384 files, eight directory chains 24 levels deep | Selection cost per directory level |
| Baseline | 3,000 failing functions, all matched by a `--baseline` snapshot | Baseline loading and matching; the gate passes |

Every scanner result must retain the known function count and exact fixture
scores/coverage. Every scanner workload must also produce the same report bytes
in every run, and on pull requests the candidate's report SHA-256 must equal the
baseline's: a performance change must not change output. A pull request that
changes reports on purpose carries the `output-change` label. A re-run keeps
the labels of the original event, so push a commit (or close and reopen the
pull request) after adding it. With the label, and on pushes, manual runs and
the `v2.0.1` fallback, a difference (including SARIF fingerprints) is printed as
a notice and recorded as `output_changed` in `results.json`, and does not fail.
Runs that disagree with each other fail in every case. SARIF must retain all 4,000 expected findings; the Node benchmark
asserts its known line-hit result. Errors, malformed results, missing samples and
process timeouts fail instead of being treated as speedups.
Unicode fixture locations must match independently encoded source prefixes,
including exact byte and UTF-16 columns by function name. Every measured SARIF
run must retain the baseline warmup's ordered `function/v1` identities; a hash of
those fingerprints is retained with each observation. Distinct but incorrect
columns or replacement fingerprints cannot pass as successful optimization.

Each side gets one validated warmup, then five fresh-process observations in
alternating baseline/candidate order. Only the five paired observations count.
This is a warm-filesystem benchmark and includes CLI startup, scan and output;
it does not isolate parser instructions. The Node case includes process startup
and its benchmark's warmup and five conversions. No compiler or other test runs
concurrently on that job's runner during measurements.

A metric fails only if its candidate median exceeds **both** the relative and
absolute budget, and a majority of individual pairs also exceed both:

| Metric | Relative limit | Absolute slack |
| --- | ---: | ---: |
| Process wall time | 1.5× baseline | 50 ms |
| Peak RSS | 1.3× baseline | 16 MiB |

These intentionally broad limits target substantial regressions on shared CI
runners. They are not promises of stable latency or maximum performance. RSS is
GNU time's per-run `%M` value in KiB, including kernel child accounting; it is
neither summed process-tree memory nor a stage allocation measurement. Setup in
the Python parent is outside the measured scanner/helper process. Each command
has a 60-second watchdog that kills its process group on timeout.

The `performance-regressions` artifact retains `results.json` for 14 days even
when a budget fails: source SHAs, dirty-tree status, compiler/runtime versions,
binary/module/harness hashes, every observation and budget decision. Inspect the
raw pairs and reproduce before changing code or reviewing a budget adjustment;
do not loosen a limit merely to make a failing change pass.
The harness removes previous `results.json` evidence before setup, records
metadata/build/measurement failures as failed attempts, and publishes complete
JSON by syncing a sibling staging file and atomically replacing the result.
If publication itself fails, the command fails and cleans its staging file;
an earlier success is not reused after successful initial invalidation.

```sh
# Linux needs GNU time; local Android/proot can use its GNU /usr/bin/time too.
python3 scripts/check-performance.py --baseline v2.0.1
# Focused development run; CI always uses all eleven workloads and five pairs.
python3 scripts/check-performance.py --baseline HEAD --case unicode-tsx --pairs 3
python3 -B -m unittest discover -s tests -p test_performance.py
```

Ordinary Rust regressions independently check a long Unicode/TSX line against
full-prefix UTF-16 references, isolate one missing coverage statement among
1,000 functions, reject stale evidence, and exercise seven-byte reads with
interrupted reads and real I/O errors across the production 8192-byte boundary.
Those tests assert behavior without wall-clock limits and run in debug, release,
coverage and mutation CI. The Python guard tests reject known slowdowns and
memory growth, tolerate isolated outliers, reject corrupt observations, verify
per-process RSS isolation, and check command failures and timeout cleanup.
