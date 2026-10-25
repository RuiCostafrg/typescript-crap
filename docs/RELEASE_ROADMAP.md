# Roadmap to public beta and v1.0.0

Assessment date: 2026-10-25. Starting point: v0.4.0.

The core feature set is sufficient to move toward beta: TypeScript CRAP scoring,
source-bound coverage validation, regression baselines, Git diff/PR selection,
SARIF, HTML reports, and release automation already exist. The next priorities
are installation, integration, and dependable compatibility. This document is a
proposed release checklist; unchecked items are not implemented or verified by
this roadmap.

## Before public beta

### 1. Distribute the coverage helpers

The documented coverage workflow uses repository-local Node scripts, but the
[release asset allowlist](../scripts/prepare-release.py) does not include them.
Provide a versioned Node package or bundle the helpers with releases, including
their supporting modules. Preserve pre-test source snapshots and validation.

- [ ] Users can install, collect coverage, and scan without cloning this repository.
- [ ] Document supported CLI/helper version combinations and Node requirements.
- [ ] Verify the published package or bundle in an installation smoke test.

### 2. Exercise real Jest and Vitest projects

The [importer regression tests](../tests/coverage_import.rs) exercise constructed
Istanbul/LCOV fixtures and source maps. Add small projects that actually run Jest
and Vitest to validate the advertised integration paths.

- [ ] Collect and scan actual Jest and Vitest coverage end to end.
- [ ] Include TSX and a workspace layout across the fixtures.
- [ ] Assert passing coverage, uncovered code, stale-source rejection, and safe
  repeated execution with fresh input/output paths.
- [ ] Record tested framework/reporter versions and unsupported source-map cases.

### 3. Persist project gate policy

The [project config](../src/config.rs) currently stores version, includes, and
excludes. Allow teams to commit their gate policy alongside selection rules.

- [ ] Add threshold, require-coverage, and fail-on-empty settings.
- [ ] Define and test CLI precedence, including explicit boolean overrides.
- [ ] Preserve existing configuration behavior and reject invalid settings.
- [ ] Document a complete config example. An `init` command is optional polish.

### 4. Verify installation and platform compatibility

The [archive smoke workflow](../.github/workflows/verify-release.yml) verifies
checksums and runs unpacked binaries. Extend verification to the advertised
installation and upgrade paths.

- [ ] Test shell and PowerShell installation and upgrades in clean environments.
- [ ] Verify Homebrew formula publication, installation, and upgrades.
- [ ] Decide the supported Linux runtime baseline: current builds require Ubuntu
  26.04 or compatible glibc. Broaden compatibility or prominently document that
  minimum, and test the chosen minimum runtime explicitly.
- [ ] Confirm the documented commands work on each advertised platform.

## During beta, before v1.0.0

### 5. Define the 1.x compatibility contract

- [ ] Document stability expectations for scoring rules, exit codes,
  configuration, JSON/SARIF output, baseline identities, and the public Rust API.
- [ ] Publish machine-readable JSON Schemas for configuration, native coverage,
  baselines, and JSON reports, with examples checked against them.
- [ ] Define migration and versioning rules for scoring or identity changes.

### 6. Validate independent project adoption

- [ ] Exercise 3–5 independent projects, including a Node service, a TSX
  application, and a monorepo.
- [ ] Record setup friction, surprising scores, coverage mapping failures,
  runtime, and peak memory, with reproducible project/version details.
- [ ] Resolve adoption-blocking defects and document supported limitations.
- [ ] Use those measurements to decide whether configurable resource budgets and
  subprocess timeouts must block v1.0.0. The current local trusted-command CLI
  does not promise sandboxing or bounded resource use.

### 7. Publish a complete CI adoption guide

- [ ] Provide a working example covering fresh coverage collection, reviewed
  baseline creation, PR gating, and report upload even when the quality gate fails.
- [ ] Explain exit codes, stale reports, Git history requirements, and safe
  repeated execution under the importer's fresh-destination requirement.
- [ ] Verify the guide from a clean checkout using published installation paths.

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
current advisory results, and fresh coverage/mutation measurements were not
revalidated for this assessment.

The earlier security/correctness review's main findings are addressed by the
current release; see [P2 implementation and validation](P2_IMPLEMENTATION.md).
