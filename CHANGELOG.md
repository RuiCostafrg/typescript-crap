# Changelog

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
