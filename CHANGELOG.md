# Changelog

## Unreleased

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
