# Changelog

## Unreleased

### Added

- Standalone `ts-crap-coverage-helpers.zip` release asset with SHA-256 checksum,
  all native V8 and Istanbul/LCOV collector modules, matching-version metadata,
  and installation instructions. Requires Node 24+ and no repository checkout.
- Installed-bundle smoke tests on Linux, macOS, and Windows, including actual
  native collection, Istanbul/LCOV conversion, and source-integrity rejection.

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
