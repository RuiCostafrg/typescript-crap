# Rust development and quality gates

## Layout

This is a single Cargo package with both a library and a binary. Flat modules are
idiomatic here; do not add a directory per file or a workspace without a real need.

- `src/main.rs`: process entry point and exit-code mapping only.
- `src/cli.rs`: Clap arguments, stdout/report-file publication, user diagnostics.
- `src/lib.rs`: public `scan`, `render`, `ScanOptions`, and report types.
- `src/analysis.rs`: private Tree-sitter TypeScript analysis.
- `src/coverage.rs`: private native coverage validation and borrowed per-source view.
- `src/coverage/tests.rs`: its `#[cfg(test)]` unit module; separate physical file
  keeps test code out of production coverage measurements.
- `src/selection.rs`: private source discovery and Git/gh integration.
- `src/report.rs`: private report encoders.
- `tests/`: public-interface integration and real CLI/Node/Git end-to-end tests.
- `tests/fixtures/`: TypeScript input data, not Rust compilation targets.
- `benches/scan.rs`: repeatable optimized scan benchmark.
- `examples/shop/`: executable TypeScript example, not a Rust example target.
- `scripts/`: optional Node coverage adapter and its loader-integrity hook.
- `.cargo/`: Cargo developer-tool configuration.
- `.github/workflows/`: automated validation.

Tests should exercise public behavior, not expose private modules just to increase
coverage. `tests/core.rs` is a fast public-library contract suite; keep the separate
CLI, filesystem, Git, and actual Node/V8 tests as well.

## Commands

```sh
cargo fmt --check
cargo clippy --locked -j 1 --all-targets -- -D warnings
cargo test --locked -j 1
cargo test --locked -j 1 --release
cargo doc --locked --no-deps -j 1
cargo bench --locked -j 1 --bench scan
node --check scripts/coverage.mjs
node --check scripts/coverage-hook.mjs
```

Use `-j 1` on the Termux/proot development machine to limit compiler/linker memory.
The minimum supported Rust version is **1.90**, matching the current lockfile's
`tree-sitter-language` requirement; CI also checks that toolchain. Stable is tested
separately. Do not claim an MSRV based only on the syntax in our own crate.

## GitHub Actions

All workflows use read-only repository permissions, immutable action commit SHAs,
and disabled checkout credential persistence. Obsolete runs on the same ref are
cancelled. They require no publishing credentials or external coverage-service token.

- **`.github/workflows/ci.yml`**: push, PR, and manual runs; Rust 1.90/stable
  matrix; formatting, strict Clippy, debug/release tests, Python helper tests,
  publishable-package content checks, rustdoc and Node syntax checks. Separate
  jobs run RustSec auditing and actionlint. macOS and Windows jobs run
  CLI/path/output, Git selection, report and real Node coverage behavior tests,
  including spaces, Unicode and native path separators. These jobs retain the
  owner-only actor gate and receive no publishing credentials.
- **`.github/workflows/coverage.yml`**: push, PR, and manual runs; actual Rust
  instrumentation through cargo-llvm-cov 0.9.1 and llvm-tools-preview. The complete
  library/binary/integration suite runs, including spawned CLI binaries and Node/V8
  tests. Production Rust files are reported; tests/benchmarks are excluded.
  Uploads `rust-coverage` (LCOV, JSON summary, and HTML) and adds totals to the job
  summary. Available reports are uploaded even when tests fail.
- **`.github/workflows/mutation.yml`**: all PRs, main pushes and manual runs;
  full production mutation analysis, including Node/Git tests; uploads
  `mutation-results` with diffs and logs even on failure.

Coverage and mutation artifacts are retained for 14 days. Coverage enforces the production-source line floor documented in
`docs/QUALITY_BASELINE.md`. Mutation score must be at least 90%; only assertion-killed
mutants count as caught, unbuildable mutants are excluded, and timeouts or partial
runs fail independently. Audit warnings/advisories fail. There is no
`continue-on-error` hiding failures.

The Cargo manifest has an explicit package allowlist: local reports, mutation logs,
coverage data, and Python caches must not be included in a crate archive. This does
not yet create/publish a release or install the Node helper alongside the executable.

Rust source coverage from CI is distinct from the TypeScript coverage used in CRAP
scores. Inspect `coverage/html/index.html` locally or download the Actions artifact.
`scripts/coverage-summary.py` converts LLVM JSON totals into the job's Markdown
summary; its CLI is exercised by three Python standard-library tests.

These definitions are local until committed and pushed to GitHub. Running them
locally or validating YAML is not evidence of a successful hosted Actions run.

## Mutation tests

Pinned tool version: **cargo-mutants 27.1.0**.

```sh
# Ordinary Linux:
cargo install cargo-mutants --version 27.1.0 --locked
# Termux toolchain in proot (Android reflink dependency compatibility fallback):
bash scripts/install-mutants-termux.sh /root/.local
export PATH=/root/.local/bin:$PATH

# List mutations without changing files:
cargo mutants --list
# Full run: production mutations against the entire test suite:
cargo mutants -j 1 --jobserver-tasks 1 --timeout 60 --build-timeout 180
# Focused development run (not a replacement for the full run):
cargo mutants --file src/coverage.rs -j 1 -- --test core
```

`.cargo/mutants.toml` selects **all production `src/**/*.rs`** and does not mutate
test assertions or benchmarks. The full run includes the real Node/V8 and Git tests.
The default skip of capacity-only allocation calls is the tool's built-in
behavior, not an application-logic exclusion. Baseline tests must pass first.
The mutation tool caps lints only in its scratch builds: a lint failure must not
masquerade as a test killing a mutant. Normal CI retains all strict lints.

Mutations are tested in isolated copies, never with `--in-place`. Results in
`mutants.out/` include `outcomes.json`, diffs and per-mutant build/test logs. The
GitHub mutation workflow runs on every PR, main pushes or manual dispatch and uploads
these artifacts even on failure. The explicit score gate accepts at least 90%
assertion-killed viable mutants; investigate every survivor and aim for none.
Do not whitelist survivors or claim timeouts/unbuildable mutants were caught by assertions.
Review equivalent mutants explicitly. Scores measure the selected operators and
tests, not proof of correctness, security, or complete branch coverage.

The Termux installer modifies only temporary local copies of the tool and
`reflink`. It retains the original locked versions, asserting the only lockfile
change is replacing reflink's registry source/checksum with the patched local path.
Android then uses `reflink`'s existing unsupported path and cargo-mutants uses
ordinary `fs::copy`. No application dependency is patched.

## Dependency auditing

```sh
cargo install cargo-audit --version 0.22.2 --locked
cargo audit --deny warnings
```

CI uses the same pinned audit-tool version with the current RustSec database and no
advisory ignores. The audit includes dev-dependencies. Results describe known
advisories at the database revision used, not an assurance of no unknown flaws.

## Safety and lint policy

Cargo.toml is the source of truth. It applies to both library and binary targets.

- **Forbid unsafe Rust** in this crate.
- **Deny ignored must-use results**.
- Clippy correctness, suspicious, and performance groups are denied.
- Clippy's remaining default lints are warnings; CI promotes all warnings to errors.
- Deny production `unwrap`, `expect`, explicit `panic!`, `todo!`,
  `unimplemented!`, `dbg!`, and `process::exit`.
- Test/benchmark assertion code explicitly allows unwrap/expect/panic. This exception
  must remain local to test targets or `#[cfg(test)]` modules, never the whole library.
- Prefer `Result` with context for user/filesystem/parser errors.
- Use `write!`/`writeln!` when appending formatted text to an existing String;
  do not create an intermediate `format!` allocation per row.
- Commit Cargo.lock for this CLI; build/test/audit with `--locked`.

These checks do **not** prove memory safety of dependencies, the absence of every
panic, or freedom from vulnerabilities. Tree-sitter includes native C and FFI;
forbidding our own unsafe blocks does not remove that trusted dependency.
Rust string indexing is bounds checked; AST byte ranges originate from the parser
of the same source. Do not introduce unchecked indexing or unsafe shortcuts.

Source freshness hashes are integrity checks, not authentication. Test commands
are trusted executable code, not sandboxed input. Git and gh are launched with
argument arrays, not shell interpolation.

## Performance policy

Measure before optimizing. There is no lint switch for “maximum performance.”

The release profile uses thin LTO, one codegen unit, and overflow checks. We keep
the default unwinding panic behavior rather than hiding faults with abort or
unchecked operations. LTO/codegen settings increase build cost, so benchmark
release builds separately from debug tests.

Coverage SHA256 and line-range validation happen **once per selected file**.
Line keys are parsed to numeric keys once at load; function scoring borrows the
validated file view. Avoid rehashing a whole file for every function or repeatedly
allocating decimal line-number strings.

Keep deterministic ordering and source/error validation when optimizing.
The benchmark creates TypeScript and source-hashed coverage for 2,000 functions,
warms up once, then reports five samples (median/min/max). It includes selection,
file/coverage reads, parsing, scoring, and sorting—not JSON/HTML serialization.
Use `TS_CRAP_BENCH_FUNCTIONS` to change the workload. Local wall-clock measurements
are informational: mobile CPU throttling, concurrent compilation, filesystem
caches, and proot affect results. There is no flaky timing assertion in CI.


## Publishing releases

Only RuiCostafrg maintains the code. Report bugs and requests through issues;
public pull requests are restricted to collaborators. Read [SECURITY.md](SECURITY.md)
for the trust model and protections.

Distribution uses checksum-pinned cargo-dist 0.33.1, pinned Rust 1.90.0 and the
committed Cargo.lock. The release workflow is deliberately hand-maintained to
separate builds from privileged publication. `allow-dirty = ["ci"]` prevents
cargo-dist from requiring its default workflow; do not overwrite release.yml
with `dist generate`. `dist plan` still validates the packaging configuration.

1. Update Cargo.toml/Cargo.lock in an owner PR and merge after CI passes.
2. Wait for successful main CI on the merged commit, then create/push the matching
   SemVer tag, for example `git tag v0.2.0` and `git push origin v0.2.0`.
   Protected tags cannot be moved or deleted.
3. The Release workflow builds all five targets and verifies the archives.
   Approve the pending **release** environment deployment after reviewing the
   source SHA and checks. This publishes attested archives, installers and formula.
4. The Homebrew tap checks hourly or can be manually dispatched via its
   **Sync verified release** workflow. It rejects missing/wrong provenance or
   downgrades. Approve its **homebrew** environment deployment to open an update PR, then
   review and merge it after Tap CI passes.
   Prereleases do not update Homebrew.

Manual dispatch of **Release** on main is a dry run: it builds and tests without
publishing or requiring publishing credentials. It cannot publish an arbitrary
input version. Workflow/action/dependency changes need owner inspection.

Both publishers use their own repository’s temporary GITHUB_TOKEN. No personal
access token, deploy key, or GitHub App setup is required. No plaintext publishing
credentials should exist locally, in artifacts, or in repository secrets.

The owner can use the PR-only review bypass because self-authored PRs cannot
receive an independent owner review. Mandatory CI remains enforced separately.
Future collaborators require an explicit reconsideration of this policy.

## P2 adoption features

See the README for versioned selection config, source-bound Istanbul/LCOV imports,
regression baselines, completeness flags, terminal escaping and SARIF. Baseline
matching indexes identities once per report rather than scanning all prior
functions for each current function. Coverage hashes and validation remain once
per source; source-map rows and line counts are prepared once per imported file.

Daily advisory scans and weekly reviewed dependency/action updates are documented
in [docs/DEPENDENCY_UPDATES.md](docs/DEPENDENCY_UPDATES.md). Bot PR code never runs
with trusted credentials. Release validation requires successful main CI, coverage
and mutation runs for the exact tagged SHA, and all PR checks must pass before merge.


## Remaining review improvements

See [docs/PERFORMANCE.md](docs/PERFORMANCE.md) for the expanded multi-file and
mixed-grammar benchmarks, parser reuse measurements and native Actions step timings.
Required test/mutation checks and floors are preserved; the duplicate Python
coverage-summary invocation was removed.

`scripts/coverage-lines.mjs` contains the pure V8 range sweep and independent-process
line merging. `scripts/coverage.mjs` owns snapshots, child commands and publishing.
Run the Node unit tests with `node --test tests/*.test.mjs`; `cargo test` also
invokes them, and retains the real loader/process freshness tests. Node 24+ is
required. Test files ship in the crate so packaged test runs retain these checks.

Human report controls are exposed through `RenderOptions` and
`render_with_options`; default `render` preserves the existing full report.
Machine formats reject non-default presentation settings. Filtering never mutates
the `Report` used for exit codes or baselines. HTML uses escaped DOM text for
search and fixed JavaScript, without interpolating source into executable code.
