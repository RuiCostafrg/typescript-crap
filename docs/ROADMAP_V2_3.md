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
- **Lint/format tooling.** The repo maintains 19 JavaScript (`.mjs`) files,
  about 1,650 lines: 10 helper scripts, ~900 lines; 6 `node:test` suites,
  ~680 lines; and 3 example tests, ~55 lines. They currently have no lint or format check; CI only runs
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
`node scripts/import-coverage.mjs --root … --format … --input … --output … -- npx vitest …`
by hand and must pick fresh directories themselves (`README.md:488-493`).

- Add `coverage-run.mjs --framework vitest|jest|auto [--provider PROVIDER] [--format json|lcov] [--root .] [--scan] [-- extra args]`.
  `PROVIDER` follows each framework's own names: `istanbul` or `v8` for Vitest,
  `babel` or `v8` for Jest. Any other combination is rejected with a clear
  error, and tests cover that.
- `auto` detects `vitest.config.*`, `vite.config.*` (which Vitest also loads),
  `jest.config.*` or the `jest` key in `package.json`. It never evaluates config
  files itself. Ambiguous projects that have both Vitest and Jest signals must
  name `--framework`. Tests cover each signal, including Vite-only projects.
- It creates a fresh private run directory and adds the correct reporter and
  directory flags. It then imports the results and writes native coverage.
  Finally it prints the matching `ts-crap --coverage … --require-coverage`
  command, or runs it with `--scan`.
- The existing safety guarantees hold: stale coverage is never accepted, the
  helper and scanner versions must match exactly, and the project config is
  never overwritten.
- **Acceptance:** all fixture combinations pass through the new command in CI.
  Each existing manual command still works. `coverage-run.mjs` is added to the
  helper bundle allowlist (`scripts/bundle-coverage.py`), and
  `scripts/verify-coverage-bundle.py` runs it from the extracted bundle.

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
- **Acceptance:** the matrix runs on every owner-authored PR and eligible
  `main` run before a release, preserving the existing numeric actor gate
  (`docs/DEPENDENCY_UPDATES.md`).

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
  - Sharded Vitest runs: each shard runs `vitest run --shard=i/n --reporter=blob`,
    then `vitest --merge-reports` merges the blob reports, with coverage enabled.
  - Sharded Jest runs: each shard runs
    `jest --shard=i/n --coverage --coverageDirectory=<fresh dir>/shard-i`, so
    shards never overwrite each other. Shard runs disable Jest's per-shard
    `coverageThreshold` check (for example `--coverageThreshold='{}'`),
    because each shard sees only partial coverage. The importer then merges
    those explicitly listed shard files. ts-crap's gates check CRAP scores
    and coverage evidence, not coverage percentages, so the recipe also
    re-checks the project's `coverageThreshold` on the merged Istanbul
    report (for example `nyc check-coverage` with the same limits). Otherwise
    the project's existing percentage requirements would be dropped.
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
- The 2026-10-24 security and performance review (kept outside the repository)
  has 8 of 9 findings fixed in current code. The remaining one, resource
  limits, is item 2.7 above.

## Distribution

Linux and npm distribution options are evaluated separately in
[LINUX_DISTRIBUTION_STRATEGY.md](LINUX_DISTRIBUTION_STRATEGY.md). The schedule
follows [WORK_PLAN.md](WORK_PLAN.md):

- Installer hardening and the GitHub Action are merged, and each ships in the
  first release after v2.2.1.
- The npm package `typescript-crap` is merged and ships in that release too. The
  Vitest/Jest one-step command (1.1) is then
  `npx typescript-crap coverage --framework vitest|jest|auto`, and it stays in
  the coverage helper bundle as `coverage-run.mjs`.
- crates.io (2.6) stays in v2.4.0.

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
