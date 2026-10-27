# Roadmap to public beta and v1.0.0

Assessment date: 2026-10-25. Starting point: v0.4.0.
Completion date: 2026-10-27. Released version: v1.0.0.

The starting v0.4.0 core already provided TypeScript CRAP scoring, source-bound
coverage validation, regression baselines, Git diff/PR selection, SARIF, HTML
reports, and release automation. This roadmap added installation, integration,
and compatibility verification. All items now have implementation and hosted
verification evidence recorded below and in the [v1 acceptance ledger](RELEASE_V1.md).

## Before public beta

### 1. Distribute the coverage helpers

Implementation: release assembly now builds an allowlisted
`ts-crap-coverage-helpers.zip` with both collectors, supporting modules, version
metadata, and a standalone guide. The [release asset allowlist](../scripts/prepare-release.py)
requires the bundle and verifies its SHA-256 before publication. Pre-test source
snapshots and validation remain in the distributed, unchanged collector modules.

- [x] Users can extract a standalone bundle, collect coverage, and scan without
  cloning this repository; local smoke tests exercise that workflow.
- [x] Document exact-match CLI/helper versions and Node 24+ requirements in the
  [standalone guide](COVERAGE_HELPERS.md), also shipped inside the ZIP.
- [x] Add installed-bundle smoke tests to cross-platform CI and release
  verification, using the assembled release assets before publication.

The helpers ship in the immutable [v1.0.0 release](https://github.com/RuiCostafrg/typescript-crap/releases/tag/v1.0.0).
Existing published releases were not retroactively changed. The
[acceptance ledger](RELEASE_V1.md) records exact-source hosted checks and
publication evidence separately from local smoke tests.

### 2. Exercise real Jest and Vitest projects

The [real framework suite](../scripts/test-framework-coverage.mjs) runs pinned
Jest and Vitest projects (including Vitest's Istanbul and V8 providers) through
JSON and LCOV reporters, the importer, and CLI gates. The
[framework coverage guide](FRAMEWORK_COVERAGE.md) records versions,
reproduction commands, assertions, and source-map limits. CI includes a separate
Linux/macOS/Windows matrix with Node 24.

- [x] Collect and scan actual Jest and Vitest coverage end to end.
- [x] Include TSX and a workspace layout across the fixtures.
- [x] Assert passing coverage, uncovered code, stale-source rejection, and safe
  repeated execution with fresh input/output paths.
- [x] Record tested framework/reporter versions and unsupported source-map cases.

Local validation on 2026-10-26 passed all six framework/provider/reporter
combinations,
including 13 Vitest behavioral cases covering complex TSX, async errors, nested
handlers, lazy imports, and Unicode paths. Direct Istanbul JSON default-argument counters now supply initializer-line
evidence; LCOV gaps are checked separately and still fail the completeness gate. Framework validation used Node 26.3.1 on Linux aarch64
(Termux/proot). All 132 Rust tests, all 14 Python tests, formatting, Node script
syntax, and actionlint 1.7.12 passed during item 2 implementation; the seven
importer regression tests were also rerun after expanding the fixtures. Hosted Node 24 and
macOS/Windows framework jobs passed on the item-5–7 PR. The exact v1.0.0 source
also passed its own [main CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657144).

### 3. Persist project gate policy

The [project config](../src/config.rs) stores gate policy alongside selection
rules. Explicit CLI values override configured values independently; omitted
settings keep the existing defaults. Boolean flags accept `=true` and `=false`,
and bare flags still enable their gates. The [README](../README.md#adoption-gates-v02)
contains a complete config example and Rust API migration notes.

- [x] Add threshold, require-coverage, and fail-on-empty settings.
- [x] Define and test CLI precedence, including explicit boolean overrides.
- [x] Preserve existing configuration behavior and reject invalid settings.
- [x] Document a complete config example. An `init` command is optional polish.

The [project policy suite](../tests/project_policy.rs) covers defaults, CLI and
library overrides, invalid policy rejection before publication, caller-relative
explicit configs, selection precedence, coverage evidence, and baseline policy.
CI includes it in both the full Rust suite and the macOS/Windows behavior job.
Local validation on 2026-10-26 passed all 141 Rust tests, all 14 Python tests,
strict Clippy, rustdoc with warnings denied, formatting, and actionlint 1.7.12.
Hosted policy tests passed on Linux, macOS, and Windows in
[PR #25 CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597);
the exact v1.0.0 source subsequently passed its own
[main CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657144).

### 4. Verify installation and platform compatibility

The [archive smoke workflow](../.github/workflows/verify-release.yml) verifies
checksums and runs unpacked binaries. Extend verification to the advertised
installation and upgrade paths.

Implemented: the [installation matrix](../.github/workflows/installation.yml)
tests a pinned published v0.3.0/v0.4.0 pair through real shell, PowerShell, and
Homebrew installers. Candidate release verification uses the same runner against
assembled assets before publication. Linux verification also runs both archive
architectures in stock Ubuntu 26.04 containers without network access or build
tools. See [installation compatibility](INSTALLATION_COMPATIBILITY.md) for exact
coverage and remaining publication evidence.

- [x] Test shell and PowerShell installation and upgrades in clean environments.
- [x] Verify Homebrew formula publication, installation, and upgrades.
- [x] Decide the supported Linux runtime baseline: current builds require Ubuntu
  26.04 or compatible glibc. Broaden compatibility or prominently document that
  minimum, and test the chosen minimum runtime explicitly.
- [x] Confirm the documented commands work on each advertised platform.

The item-4 PR passed its entire hosted installation matrix and was merged as
`ed9105e820e8dddf4ba302d068389d3911114766`. The
[candidate release dry run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40415478665)
on that exact main commit passed all five archives, installed helper bundles,
shell/PowerShell upgrades, three Homebrew targets, and both clean Linux runtimes.
The live tap's [v0.4.0 formula PR](https://github.com/RuiCostafrg/homebrew-tap/pull/8)
was provenance-verified and merged after all three required Tap CI checks passed.
The [published installer and live tap matrix](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847643)
passed on exact head `bf8a52d7be37c871939746bfe3ceae807e2bdf49`, including
unchanged live formula URLs, fresh/reinstall checks, and v0.3.0 to v0.4.0 upgrades
on macOS ARM64/Intel and Linux x86-64. All pre-beta installation items now have
hosted evidence. The v1.0.0 package also passed its own candidate and
[published-path checks](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40494926411).

## During beta, before v1.0.0

### 5. Define the 1.x compatibility contract

- [x] Document stability expectations for scoring rules, exit codes,
  configuration, JSON/SARIF output, baseline identities, and the public Rust API.
- [x] Publish machine-readable JSON Schemas for configuration, native coverage,
  baselines, and JSON reports, with examples checked against them.
- [x] Define migration and versioning rules for scoring or identity changes.

The [compatibility contract](COMPATIBILITY.md) and [versioned schemas](../schemas/README.md)
cover these promises. Executable validation checks examples, actual CLI reports,
baselines, malformed inputs, and additive report fields in CI. Schema publication
is versioned with the source package and repository release tag.

### 6. Validate independent project adoption

- [x] Exercise 3–5 independent projects, including a Node service, a TSX
  application, and a monorepo.
- [x] Record setup friction, surprising scores, coverage mapping failures,
  runtime, and peak memory, with reproducible project/version details.
- [x] Resolve adoption-blocking defects and document supported limitations.
- [x] Use those measurements to decide whether configurable resource budgets and
  subprocess timeouts must block v1.0.0. The current local trusted-command CLI
  does not promise sandboxing or bounded resource use.

The [adoption report](ADOPTION.md), pinned manifest, harness, and raw measurements
record Hono, TanStack Table, and the Vite React TSX app. Actual Vitest JSON coverage
imports and repeated scans succeeded without editing upstream source. Parser and
default-initializer evidence blockers were fixed with regressions. Resource budgets
are deferred for the documented trusted local CLI scope; no resource guarantees
are inferred from these measurements.

### 7. Publish a complete CI adoption guide

- [x] Provide a working example covering fresh coverage collection, reviewed
  baseline creation, PR gating, and report upload even when the quality gate fails.
- [x] Explain exit codes, stale reports, Git history requirements, and safe
  repeated execution under the importer's fresh-destination requirement.
- [x] Verify the guide from a clean checkout using published installation paths.

The [CI guide](CI_ADOPTION.md) and [copyable workflow](../examples/ci/github-actions.yml)
use reviewed base policy/baselines and retain reports on failed gates. Executable
clean-project validation passes with both direct Node and real Vitest collection.
Candidate release verification runs the guide with packaged CLI/helper bytes on
all five targets. After v1.0.0 publication and live tap synchronization,
[Verify published adoption](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40494926411)
passed the guide using public CLI/helper downloads on all five platforms,
including actual Vitest/Istanbul on Linux x86-64. All three live Homebrew targets
also passed v0.4.0 to v1.0.0 upgrades, reinstalls, and fresh installations.

## v1.0.0 release status

The immutable [v1.0.0 release](https://github.com/RuiCostafrg/typescript-crap/releases/tag/v1.0.0)
selects `4ce57b9aa3b9979166965aebbf15e16eabb6ccac`, merged through
[PR #26](https://github.com/RuiCostafrg/typescript-crap/pull/26). Its exact main
commit passed every required workflow, including advisories, real framework and
independent-project tests, **97.75% production Rust line coverage**, and
**95.77% assertion-killed mutations** with zero timeouts. The full candidate dry
run and protected tag-triggered release workflow passed. The provenance-verified
[Homebrew update](https://github.com/RuiCostafrg/homebrew-tap/pull/9) is merged,
and all eight published-adoption jobs passed. No quality floor or publication
protection was lowered. [The v1 acceptance ledger](RELEASE_V1.md) records exact
commits, workflow URLs, the published helper checksum, and remaining product limits.

## Release acceptance

**Public beta:** a new user can install the published tools and gate a real
project using only the published instructions; the four pre-beta areas above
have evidence of completion.

**v1.0.0:** that workflow is proven across the declared platforms and project
types, compatibility promises are documented, and no known release-blocking
correctness or adoption defects remain. Require successful hosted checks for the
exact release commit, including the existing coverage, mutation, advisory, and
release verification gates. Do not lower existing quality floors to release.

Editor extensions, hosted dashboards, additional report formats, and
changed-line-only scoring can follow v1.0.0 unless beta feedback establishes a
concrete need.

## Evidence and limits of this assessment

Local validation on 2026-10-25 passed 132 Rust tests (including the bundled Node
suites and real Git/Node integration tests), 12 Python tests, and
`cargo fmt --check`. These results describe the assessed v0.4.0 checkout; they do
not establish completion of this checklist. Hosted CI, published installers,
advisories, and fresh coverage/mutation measurements were not revalidated during
that original assessment; their subsequent v1.0.0 evidence is recorded in the
release status above and the acceptance ledger.

The earlier security/correctness review's main findings are addressed by the
current release; see [P2 implementation and validation](P2_IMPLEMENTATION.md).
