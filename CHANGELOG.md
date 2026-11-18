# Changelog

## Unreleased

### Changed

- Function analysis walks each syntax tree once instead of three times (an
  outer walk plus, for every function, a metrics walk and an identity-hash
  walk). Identities, names, complexity and owned lines are unchanged: the old
  implementation stays in the tests as an oracle, and it agrees on all 371,152
  functions of a full VS Code checkout.

- `--diff` and `--pr` scans send all committed-blob requests to their one
  `git cat-file --batch` process up front from a helper thread, so Git
  streams each file's blob while the previous file is read and compared,
  instead of one request-and-reply round trip per file. Reports, errors and
  the byte-for-byte check against HEAD are unchanged.

- Release archives, Debian packages and the Linux npm packages ship
  mimalloc's MIT notice (`mimalloc-LICENSE`) next to musl's, because the static
  musl build links mimalloc from 2.5.0. Debian's `copyright` file includes it
  too.

## 2.5.0 — 2026-11-16

Faster scans with byte-identical reports: bounded parallel analysis
(`--jobs`, default up to 4 threads), cheaper function analysis, fewer
filesystem calls and lower peak memory on baseline and stdout scans. On a
full VS Code scan on one hosted x86-64 runner, scans take about half the time
of v2.4.0 on glibc. Release archives are smaller and the GitHub Action
installs faster, and it now binds each asset to its own attested digest.

### Changed

- The static musl build (every Linux install channel) uses mimalloc instead of
  musl's allocator, which takes a global lock. On a full VS Code scan on a
  hosted x86-64 runner this cut wall time by 25% (38.5 s to 29.1 s) and system
  CPU from 12 s to 0.5 s, for about 12% more peak memory. Reports are byte for
  byte unchanged. glibc, macOS and Windows builds keep the system allocator.
- Faster coverage conversion and import on large or minified sources; output is unchanged.
- The corpus benchmark can build an older git ref with the same toolchain,
  profile, targets and `--locked` flag and measure it interleaved with the
  current commit, so before/after ratios come from one runner instead of two
  different CPU models.
- Function analysis evaluates node kinds through per-grammar tables of symbol
  ids, hashes each function's tokens as one buffered stream, keeps owned lines
  in a sorted vector and tracks ancestors during the tree walk instead of
  asking tree-sitter for parents. Reports, identities and scores are byte for
  byte unchanged (pinned by the golden fixtures and checked on a full VS Code
  scan).
- The GitHub Action fetches the Sigstore trusted root while the release assets
  download, instead of afterwards. On GitHub-hosted runners this saves a median
  0.55–0.67 seconds per install. What is verified, and the rule that nothing is
  extracted or run before verification, are unchanged.
- The GitHub Action binds each downloaded asset to its own name in the verified
  build-provenance attestation: every subject named like the asset must carry
  the asset's SHA-256. `gh attestation verify` alone accepts any attested
  subject with a matching digest, so another attested release asset could have
  stood in for the requested one. The unsigned `.sha256` sidecar is no longer
  downloaded (it is still published for manual checks). All releases the Action
  accepts (2.2.1 and later) name every asset in their attestations.
- Release binaries no longer carry a symbol table (`strip = "symbols"` in the
  `dist` profile). The Linux x86-64 archive shrinks by 7.5% (1.42 MB to 1.31 MB),
  the Linux ARM64 archive by 10.3% and the macOS ARM64 archive by 7.3%; the
  glibc Linux and macOS x86-64 archives shrink by 6.5–10%. The Windows archive is
  unchanged, since MSVC already keeps symbols out of the executable. Panic
  messages keep their file and line, but backtraces no longer show function
  names. Builds from source with `--release` are unchanged.
- The README compares install times and what each install path verifies, with
  hosted-runner measurements.
- Lower peak memory on baseline and stdout scans, with byte-identical output.
  On the VS Code repository (431,594 functions, static Linux build), a
  `--write-baseline` scan printing JSON to stdout fell from 480 MB to 178 MB, and
  a `--baseline` scan from 466 MB to 432 MB. Stdout reports are staged in an
  anonymous temporary file (in memory if none can be created) and still appear
  only after every file is published.
- Directory scans check each directory for symbolic links once per scan instead
  of once per file. On Linux, walked files are no longer resolved one by one:
  the walk starts at the resolved project directory and never follows links,
  so each path is already canonical. Explicitly named paths and every `--diff`
  and `--pr` path keep the full per-file check, and macOS and Windows keep
  resolving walked files. Selection, symbolic-link rejection and report bytes
  are unchanged.
- Before replacing an existing report or baseline, the check that no
  TypeScript file under the project root aliases the destination identifies
  regular files with one `lstat` instead of opening each one (Unix). Symbolic
  links keep the full check, ignored and hidden files are still checked, and
  the check still runs again before each rename.
- **Behaviour change:** an unreadable TypeScript file under the project root no
  longer aborts publication of `--output` or `--write-baseline` on Unix. Its
  device and inode prove it is not a destination without opening it.

- The CI performance guard compares report bytes: on pull requests the
  candidate must reproduce the base's report for every scanner workload unless
  the pull request is labelled `output-change`, and repeated runs must agree.
  New workloads cover a mixed corpus with multi-MiB files, an existing
  `--output` destination, a deep directory tree and a `--baseline` scan.
- An opt-in `corpus-benchmark` workflow measures full scans of a pinned
  VS Code commit with the x86-64 glibc and static musl builds
  (`scripts/bench-corpus.py`).

### Added

- `--jobs N` analyzes selections of 32 files or more on up to N worker threads.
  The CLI defaults to the available cores, at most 4. Files are still read and
  verified against Git in path order on one thread. Workers only parse, bind
  coverage and score, and results are merged in path order. Reports, baselines,
  exit codes and the reported error (the first failing file in path order,
  including panics) are the same for every N. In-flight source text is capped
  at 64 MiB, and a larger file is analyzed alone. Sources over 128 KiB are
  analyzed on the calling thread and worker results are copied into its heap,
  which keeps glibc's per-thread heaps small: on the VS Code repository
  (hosted x86-64, 4 jobs) peak memory was 1.14x a sequential scan with glibc
  and unchanged with the static musl build, at 1.64x and 1.37x the speed
  (`docs/measurements/wp4-parallel-rss-2026-11-15.json`).
- `ScanExtensions::jobs` for library callers. It defaults to 1, so `scan` and
  `scan_with` start no threads unless asked.
- `Baseline::render_report_to` streams a report's baseline snapshot without
  copying it first; it writes the same bytes as `Baseline::capture` followed by
  `render_to`. It is part of the stable 2.x Rust API, so together with
  `ScanExtensions::jobs` the next release is a minor one.

## 2.4.0 — 2026-11-10

Adds configurable excluded directories, syntax-error reporting that keeps
scanning, and coverage report merging. The GitHub Action now verifies with one
trusted-root fetch per install and retries transient download errors.

### Added

- README guidance on why to use the GitHub Action in CI and when npm is the
  better fit, with measured install-and-scan times.
- The GitHub Action fetches the Sigstore trusted root once per install and verifies
  the release archive and coverage helpers in parallel, so `helpers: true` no
  longer adds a second full verification. What is verified is unchanged.
- The coverage importer accepts repeated `--input` files or a fresh report
  directory, conservatively reduces each report before adding independent counts,
  and rejects duplicate evidence or invalid inputs without publishing a report.
  Concurrent overlapping imports are reserved, and report reads are bound to
  discovered file identities. Explicit symlink report inputs, previously followed,
  are now rejected.

- Pinned oxlint/oxfmt checks for the repository’s own `.mjs` files and an
  explicit `node --test` CI step, alongside the existing Rust checks.

- `excluded_directories` in `ts-crap.json` replaces the built-in excluded
  directory names (`node_modules`, `.git`, `target`, `dist`, `build`,
  `coverage`), for example to scan a real `src/build/` directory. The coverage
  helpers read the same key, so collection skips the same directory names as
  the scanner. Omitting it keeps the defaults.
- Vitest and Jest quickstarts (`docs/VITEST.md`, `docs/JEST.md`) covering setup,
  the one-step command, JSON or LCOV, `package.json` scripts, GitHub Actions and
  troubleshooting, linked from the README.
- A Jest variant of the copyable CI workflow (`examples/ci/github-actions-jest.yml`),
  whose commands CI replays through `verify-ci-guide.py --jest` with the installed
  helpers.
- `--on-syntax-error report` scores every parsable file instead of aborting the
  scan, and lists each unparsable file in every report format (JSON
  `syntax_errors`, text and HTML sections, XML `<syntax-error>`, located SARIF
  `TSCRAP002`). Unparsable files fail the completeness gate, and baselines are
  refused until they parse. The default `fail` is unchanged. Rust callers use
  `scan_with` with `ScanExtensions`.

### Fixed

- The GitHub Action makes up to three attempts at a release download when
  GitHub answers with a transient error (HTTP 5xx, 408, 429 or a network
  error, or `release not found`, which gh also reports for a transient lookup
  failure), instead of failing the job. A missing asset in a found release and
  other HTTP 4xx errors still fail at once.
- The published-release npm check disables Yarn's one-day `npmMinimalAgeGate`
  for its own install projects, so it passes right after a release. The README
  explains the gate for Yarn 4.15+ users.

## 2.3.2 — 2026-11-02

First release published to npm. The v2.3.0 and v2.3.1 GitHub releases,
installers, Homebrew formula and GitHub Action are unaffected.

### Changed

- npm packages are staged instead of published directly: the release workflow
  runs `npm stage publish` (trusted publishing, with provenance), and nothing is
  public until the owner approves each staged version with 2FA, platform
  packages first. The job summary lists every stage id. The npm trusted
  publisher allows only staged publishing.

## 2.3.1 — 2026-11-02

The GitHub release is published. Its npm publish was rejected because the npm
trusted publisher allows only staged publishing, so npm starts at 2.3.2.

### Added

- Jest `ts-jest` and `@swc/jest` transformer fixtures and the Jest `v8` coverage
  provider. Each pair must give exactly the `babel-jest`/`babel` CRAP results.
  The docs include a configuration snippet for every supported combination.

### Fixed

- The release workflow publishes npm packages from `./npm-packages/…`. npm 12
  read the bare `npm-packages/…` path as a GitHub shorthand and refused the
  git fetch, so v2.3.0 never reached npm. CI now dry-runs the exact publish
  command with the release's pinned npm CLI on every change.

## 2.3.0 — 2026-11-02

The GitHub release is published. Its npm publish failed before any package
was uploaded, so npm starts at 2.3.2.

### Added

- npm package `typescript-crap`: `npm install --save-dev typescript-crap`,
  then `npx ts-crap`. Platform packages carry the release executables (static
  musl on Linux), selected by `os` and `cpu`, with no install scripts. The main
  package bundles the coverage helpers at the same version, and
  `npx typescript-crap coverage --framework vitest|jest|auto` runs the one-step
  coverage command (Node.js 24+). Packages publish from the release workflow
  with npm trusted publishing and provenance, after each executable is checked
  against the attested release archive. CI installs them with npm, pnpm, Yarn
  and Bun on Linux (Ubuntu, Debian, Alpine, Fedora; x64 and arm64), macOS and
  Windows before publishing, and the published-release check
  (`verify-published.yml` with `verify-npm=true`) repeats them from the registry.
- `coverage-run.mjs`, a one-step Vitest/Jest coverage command in the helper
  bundle. It detects the framework from config filenames or the `jest` key in
  `package.json`, without evaluating them. It uses a fresh private
  run directory, imports the result, and prints the matching `ts-crap`
  command. With `--scan` it checks that the helper and scanner versions match
  exactly, then runs that command.
- `import-coverage.mjs` names the likely cause and fix for failed, missing,
  empty and unremapped coverage, and warns about TypeScript files without
  coverage records. Exit codes are unchanged.
- A Vitest 3/4/5 and Jest 29/30 coverage matrix, each from its own lockfile, on
  Linux, macOS and Windows, and a supported-versions table in
  `docs/COMPATIBILITY.md`.
- Documented version-pinned shell installer URLs for CI and a verify-before-run
  flow (`gh attestation verify` before running the downloaded installer).
- Installation CI runs that flow against the published v2.2.1 installer and
  checks its GNU/musl archive selection on Ubuntu 26.04 and 24.04 runners and in
  Debian 12 and Alpine 3.22 containers.
- GitHub Action (`action.yml` at the repository root) that installs a pinned
  release (static musl on Linux) after verifying its build provenance and
  SHA-256, adds `ts-crap` to `PATH`, and can run the gate with SARIF upload.

### Changed

- `import-coverage.mjs` forwards SIGINT, SIGTERM and SIGHUP to the test command
  and waits for it to stop, so cancelling an import also stops the tests. A
  cancelled import never publishes, even if the test command then exits 0. The
  exit code stays 2.
- The shell installer installs the static musl build on every Linux system,
  instead of the GNU build when glibc is 2.43 or newer. GNU archives are still
  published, and the Homebrew formula is unchanged.

## 2.2.1 — 2026-10-30

First published release containing the 2.2.0 changes below.

### Fixed

- Release assembly accepts cargo-dist's aggregate `sha256.sum`, which ends with
  a blank line, when adding Debian package checksums.

## 2.2.0 — 2026-10-30 (not published)

The v2.2.0 tag exists, but release assembly failed before publication; it has
no release assets. Use v2.2.1.

### Added

- Static musl Linux archives and reproducible Debian packages for x86-64 and
  ARM64, including coverage helpers and license notices.
- Native container runtime checks on Ubuntu, Debian, Alpine and Fedora, with
  Debian install, revision upgrade, reinstall and removal checks before release.
- Optional provenance-verified published Linux package adoption checks.

## 2.1.0 — 2026-10-29

### Added

- Required paired time/RSS regression CI across seven validated workloads,
  with exact baseline builds, retained observations and behavioral regressions.
- Coverage helpers accept caller-relative `--config FILE` to protect an
  alternative project policy path alongside the always-reserved default config.

### Fixed

- Prevent native coverage publication from replacing project configuration;
  reserve absent config paths for both importer input and output as well.
- Reject dangling default-config links instead of silently weakening gate policy
  with built-in settings; preserve working links and absent-config defaults.
- Validate each LLVM coverage record before summing, rejecting invalid counts
  and boolean values that could otherwise hide in apparently valid totals.
- Invalidate old performance evidence before metadata setup and atomically
  publish complete success/failure reports without retaining stale success.

These changes preserve scoring, fingerprints, coverage document versions, and
the public Rust API. No coverage recollection or baseline migration is required.
Use a regular generated-report path instead of a protected configuration path,
and repair or remove a dangling config link before scanning.

## 2.0.1

### Added

- Bounded Kani verification of committed-source comparison against an independent
  CRLF specification, including framing and truncation properties.
- Pinned Rust API compatibility checks against the released v2.0.0 API.
- Documented verification contracts, proof limits, and comparator invariants.

### Changed

- Require successful main-branch verification alongside the existing quality
  workflows before tag publication; verify upgrades from v2.0.0.

Scoring, coverage schemas, fingerprints, CLI behavior, and the public Rust API
remain compatible with v2.0.0. No coverage recollection or baseline migration is
needed when upgrading from v2.0.0.

## 2.0.0

### Fixed

- Reject symlinked source ancestors before Git identity and selection rules can
  substitute a different baseline path or silently omit a changed violation.
- Preserve whitespace in Git repository-root names when selecting committed
  changes, including nested project roots.
- Combine independent LCOV test counters after conservative source-map overlap
  reduction. Repeated captures of the same test/source add before mapping;
  duplicate lines within one record and unsafe counter sums are rejected.
- Reserve default and explicit config destinations before they exist, preventing
  reports and baseline snapshots from creating an invalid future project config.

### Changed

- Compare committed Git blobs with a fixed-size buffer instead of retaining a
  second full source copy, preserving CRLF/LF equivalence and batch verification.

These fixes do not change native coverage/config/baseline document versions,
function scoring, or fingerprints. LCOV users should recollect native coverage
with the matching updated helper and review resulting gate/baseline differences;
old imported evidence cannot be repaired by rerunning the scanner alone.
Because accepted LCOV input can produce different gate outcomes, this is a major
release under the previous 1.x compatibility contract. See the
[v2 migration guide](docs/MIGRATING_TO_V2.md) before upgrading collected evidence.

## 1.0.0

### Added

- A 1.x compatibility contract and versioned JSON Schemas for configuration,
  native coverage, baselines, and reports, with executable contract checks.
- Reproducible adoption tests against pinned Hono, TanStack Table, and Vite React
  TypeScript sources, recording real Vitest coverage, runtime, and peak memory.
- A complete CI adoption guide and copyable workflow with reviewed policy and
  baselines, fresh coverage, PR selection, and JSON/SARIF upload on gate failure.
- Installed-guide verification in CI and candidate releases, plus a manual
  published-release verification workflow for the same path and live Homebrew.

- Installation compatibility checks using real shell, PowerShell, and Homebrew
  installers: fresh install, reinstall, upgrade, and installed TS/TSX/coverage
  smoke tests. Release verification checks candidate assets and the clean Ubuntu
  26.04 Linux runtime before publication.

- Project gate policy in `ts-crap.json`: `threshold`, `require_coverage`, and
  `fail_on_empty`, with per-setting CLI overrides, including explicit `=false`.
- Standalone `ts-crap-coverage-helpers.zip` release asset with SHA-256 checksum,
  all native V8 and Istanbul/LCOV collector modules, matching-version metadata,
  and installation instructions. Requires Node 24+ and no repository checkout.
- Installed-bundle smoke tests on Linux, macOS, and Windows, including actual
  native collection, Istanbul/LCOV conversion, and source-integrity rejection.

### Changed

- Establish the [1.x compatibility contract](docs/COMPATIBILITY.md). See
  [the v0.4.0 migration guide](docs/MIGRATING_TO_V1.md) before upgrading library
  integrations or enabling persisted gate policy.
- Rust `ScanOptions` gate fields are now `Option` values. Use `None` to inherit
  project policy, or wrap explicit values in `Some`, including `Some(false)`.
  Resolved defaults remain threshold 10 and both completeness gates disabled.

### Fixed

- Parse valid semicolon-free generic overloads in interface/object types and
  TypeScript 5 type-only wildcard exports without changing runtime locations.
  Remaining syntax errors still reject the whole scan.
- Import direct Istanbul default-argument counters so JSON reports can supply
  declaration-line evidence for default initializers. LCOV's missing evidence
  remains a completeness failure.

## 0.4.0 — 2026-10-25

### Added

- Writer-based `render_to`, `render_to_with_options`, and baseline `render_to`
  APIs, retaining the existing string-returning interfaces.

### Improved

- Stream CLI file reports and baselines through buffered sibling temporary files;
  stage every output before publication and retain final destination revalidation.
- Serialize SARIF with borrowed typed records and one finding at a time.
- Build release binaries on Ubuntu 26.04 and macOS 26; Linux releases require
  a compatible Ubuntu 26.04 glibc runtime.

### Fixed

- Reject source paths that schema v1 cannot represent, including literal Unix
  backslashes and colons, in the scanner and both Node collectors.
- Treat renamed inferred function names as new baseline identities while retaining
  unchanged file moves and anonymous callbacks shifted by trivia.
- Recompute absolute failures on each baseline comparison so library callers can
  safely compare the same report against different baselines.
- Encode XML attributes separately from HTML, replacing forbidden XML characters
  and preserving tabs, newlines and carriage returns through character references.
- Reject duplicate decoded Istanbul/source-map JSON keys before evidence can be
  overwritten, including escaped-equivalent keys.
- Keep Git diff selection repository-relative regardless of `diff.relative`.
- Verify the actual analyzed source against pinned HEAD blobs in diff/PR scans,
  rejecting edits hidden by index hints or clean filters. CRLF/LF checkout
  differences remain supported; parsing and coverage hashes retain worktree bytes.

## 0.3.0 — 2026-10-24

### Added

- Text and HTML report options: `--only-failing`, `--sort path|crap`, and
  `--top N`. Presentation options preserve the full quality gate and totals.
- Search and failing-function filtering in HTML reports.
- Runnable inventory and retry TypeScript examples.

### Improved

- Reuse TypeScript and TSX parsers between files, index UTF-16 positions with a
  monotonic cursor, cache qualified scope names, and prioritize active V8
  coverage ranges with a heap. Reproduction details are in `docs/PERFORMANCE.md`.
- Include source locations and bounded snippets in syntax diagnostics.
- Share coverage snapshots and publication code, centralize validation, and
  expand module, integration, fault-injection, and cross-platform tests.

### Fixed

- Publish complete coverage reports through private staging files. Native Node
  collection atomically replaces regular reports; Istanbul/LCOV import requires
  a fresh destination and preserves entries created concurrently.
- Reject duplicate coverage file paths and line keys, including equivalent
  JSON escapes, rather than silently replacing earlier evidence.
- Normalize valid baseline hashes to lowercase so letter case does not prevent
  unchanged functions from matching after a file move.
- Protect report and baseline destinations from input aliases and source-file
  overwrites, with validation immediately before publication.

Coverage and baseline schemas remain version 1. Node coverage helpers require
Node.js 24 or later; the standalone scanner does not require Node.js. Importer
publication now requires hard-link support on the destination filesystem and
fails without a direct-write fallback when it is unavailable.

[Changes since v0.2.0](https://github.com/RuiCostafrg/typescript-crap/compare/v0.2.0...v0.3.0)
