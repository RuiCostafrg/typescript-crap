# Roadmap after v2.2.1

Drafted 2026-10-30 against `main` at `9034757` (v2.2.1). Nothing here is
implemented yet; each item lists its acceptance criteria and an effort
estimate (S ≈ ½–1 day, M ≈ 1–3 days, L ≈ 3+ days, including tests and docs).

## Scope decisions

- **TypeScript only.** ts-crap scores `.ts`, `.tsx`, `.mts` and `.cts`
  (`src/selection.rs:18-28`). Adding `.js`/`.jsx` support is explicitly out of
  scope.
- **Vitest and Jest are the priority frameworks.** Both already work through the
  coverage importer, but only one pinned version each, one Jest transformer and
  no one-step command. Phase 1 makes them first-class.
- **Lint/format tooling.** The repo maintains 16 JavaScript (`.mjs`) files,
  about 1,600 lines: 10 helper scripts, ~900 lines, and 6 `node:test` suites,
  ~680 lines. They currently have no lint or format check; CI only runs
  `node --check` on 4 of them (`.github/workflows/ci.yml:57`). That warrants
  oxlint and oxfmt (Phase 3). Rust keeps `cargo fmt`, `clippy -D warnings` and
  cargo-mutants (≥ 90 % kill rate) unchanged.

## Current state of tests

- The repo's own tests are Rust (`tests/*.rs`) plus `node:test` suites in
  `.mjs` (`tests/*.test.mjs`, run by `tests/coverage_e2e.rs` under
  `cargo test`). **No repo test is written in TypeScript, and the repo does not
  use Vitest or Jest as its own test runner.**
- Vitest and Jest appear only as **fixtures under test**:
  `tests/fixtures/frameworks` has Jest 30.5.2 with babel-jest, and Vitest 5.0.5
  with the istanbul and v8 providers. It runs TypeScript/TSX test files
  (`service.test.ts`, `badge.test.tsx`, `complex.test.tsx`).
  `scripts/test-framework-coverage.mjs` drives them in the owner-only
  `framework-coverage` CI job on Linux, macOS and Windows.

## Phase 1: first-class Vitest and Jest (priority)

### 1.1 One-step coverage command (M)

Today users assemble
`node import-coverage.mjs --root … --format … --input … --output … -- npx vitest …`
by hand and must pick fresh directories themselves (`README.md:488-493`).

- Add `coverage-run.mjs --framework vitest|jest|auto [--provider istanbul|v8] [--format json|lcov] [--root .] [-- extra args]`.
- `auto` detects `vitest.config.*`, `jest.config.*` or the `jest` key in
  `package.json`. It never evaluates config files itself.
- It creates a fresh private run directory and adds the correct reporter and
  directory flags. It then imports the results and writes native coverage.
  Finally it prints the matching `ts-crap --coverage … --require-coverage`
  command, or runs it with `--scan`.
- The existing safety guarantees hold: stale coverage is never accepted, the
  helper and scanner versions must match exactly, and the project config is
  never overwritten.
- **Acceptance:** all fixture combinations pass through the new command in CI.
  Each existing manual command still works.

### 1.2 Actionable diagnostics (S)

- Replace terse failures ("Coverage command failed: 1",
  "Empty imported coverage", `scripts/import-coverage.mjs:208-216`) with
  messages that name the likely cause and fix. Likely causes include coverage
  not enabled, the wrong reporter, coverage `include` missing `src`, and
  unremapped V8 output.
- Warn when selected TypeScript files are absent from the imported coverage.
- **Acceptance:** contract tests assert each message. Exit codes are unchanged.

### 1.3 Version matrix (M)

- Test Vitest 3, 4 and 5, and Jest 29 and 30, each from its own lockfile, on
  Node 24 across Linux, macOS and Windows.
- Publish a supported-versions table in `docs/COMPATIBILITY.md`. Failures name
  the framework and version.
- **Acceptance:** the matrix runs on every PR and on `main` before a release.

### 1.4 Transformers and providers (M)

- Jest: add `ts-jest`, `@swc/jest` and `coverageProvider: 'v8'` fixtures next to
  babel-jest.
- Vitest: document the already-tested `v8` provider as a user command.
- Each combination must produce the same CRAP results as the baseline fixture.
  If a combination can't, it needs a documented rejection with a test.
- **Acceptance:** working config snippets in the docs for every supported
  combination.

### 1.5 Framework quickstarts (S)

- Add `docs/VITEST.md` and `docs/JEST.md`, each covering install, config
  snippet, one-step command, JSON vs LCOV, `package.json` scripts, a GitHub
  Actions example and a troubleshooting table. Link both from the README.
- Add a Jest variant to `examples/ci/` and to `scripts/verify-ci-guide.py`, so
  both guides are executed in CI rather than only documented.

### 1.6 Monorepos, workspaces and shards (L)

- The importer accepts multiple inputs or a directory. Counts for files covered
  by more than one input are merged conservatively.
- Documented and tested recipes for:
  - Vitest `projects`
  - Jest `projects`
  - Sharded runs: `vitest --shard` with `--merge-reports`, and `jest --shard`
- **Acceptance:** a fixture with two or more projects proves the merged counts.

## Phase 2: scanner gaps

| # | Item | Acceptance | Effort |
| --- | --- | --- | --- |
| 2.1 | Make the hard-coded excluded directories configurable (`node_modules`, `.git`, `target`, `dist`, `build`, `coverage`; `src/selection.rs:30-39`). A real `src/build/` is currently skipped silently with no override. | `ts-crap.json` can replace the default list. Defaults are unchanged and documented. Tests cover nested `build/` source. | S |
| 2.2 | Option to report unparsable files instead of aborting the whole scan (`src/analysis.rs:463`). | `--on-syntax-error fail\|report` (default `fail`). Reported files appear in every output format and count as incomplete under the completeness gate. | S–M |
| 2.3 | Inline suppression comment, e.g. `// ts-crap-ignore-next-function: reason`. | A reason is required. Suppressions are listed in every report format. Suppression affects the gate but never coverage completeness. | S–M |
| 2.4 | Changed-function-only diff mode. Currently every function in a changed file is scored. | `--diff-scope functions` scores only functions overlapping changed lines. The current file scope remains the default. | M |
| 2.5 | Per-path thresholds for monorepos. | Glob overrides in `ts-crap.json`. The effective threshold is shown per function in JSON and SARIF. | M |
| 2.6 | Publish to crates.io so `cargo install ts-crap` works. | Publish job in the release workflow. Uses the same owner approval and provenance model as the GitHub release, with a crates.io trusted-publishing token and no long-lived secret. | S |
| 2.7 | Optional resource limits: maximum file size and child-process timeout. This is the open item from the 2026-10-24 review. | Opt-in flags. Defaults are unchanged. Documented for scanning untrusted repositories. | M |

## Phase 3: repository tooling

| # | Item | Acceptance | Effort |
| --- | --- | --- | --- |
| 3.1 | oxlint (1.90.0 at the time of writing) for `scripts/*.mjs`, `tests/*.test.mjs` and `examples/**/*.mjs`, pinned in the tooling lockfile. | Zero warnings. Runs as a CI step. Replaces the partial `node --check` step. | S |
| 3.2 | oxfmt (0.75.0, pre-1.0) for the same files. | One-time formatting commit, then a CI check. If an oxfmt update would reformat the code, CI fails instead of reformatting silently. | S |
| 3.3 | Run `node --test` as its own CI step, in addition to the indirect run through `cargo test`. | Clearer failure attribution. | S |
| 3.4 | Rust stays on `cargo fmt --check`, `clippy -D warnings` and cargo-mutants. | No change. | — |

## Housekeeping

- Merge or close the open PRs #36 (Dependabot `taiki-e/install-action` bump)
  and #28 (obsolete docs cleanup).
- `docs/SECURITY_RUST_PERFORMANCE_REVIEW_2026-10-24.md` (untracked). 8 of its 9
  findings are fixed. Commit it with a status table, or delete it.

## Distribution

Linux and npm distribution options are evaluated separately in
[LINUX_DISTRIBUTION_STRATEGY.md](LINUX_DISTRIBUTION_STRATEGY.md). Its
recommended npm package and installer hardening are proposed for v2.3.0, so the
Vitest/Jest one-step command ships through `npx`. The GitHub Action and
crates.io (2.6) are proposed for v2.4.0.

## Proposed release grouping

- **v2.3.0:** 1.1, 1.2, 1.3, 1.4, 1.5, 2.1, 2.2, Phase 3.
  Makes Vitest and Jest first-class and removes the two ways a scan can
  silently skip code or abort entirely.
- **v2.4.0:** 1.6, 2.3, 2.4, 2.5, 2.6.
  Adds monorepo, shard and PR-noise improvements, plus crates.io.
- **Later / on demand:** 2.7.

All changes are additive under the 2.x compatibility contract
(`docs/COMPATIBILITY.md`). Defaults stay as they are, and new behaviour is
opt-in.
