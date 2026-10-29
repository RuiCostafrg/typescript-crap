# Next improvements for approval

Reviewed 2026-10-28. Items 1–3 were reproduced against code unchanged between
`main` (`bd621ca9b7e10c1b80548f154e6d26535e9c7b48`, v2.0.1) and
[PR #32](https://github.com/RuiCostafrg/typescript-crap/pull/32)
(`06768012d18d9d80d4f2070dd5d16e7407371c36`). Item 4 concerns the performance
harness introduced by that PR; its changes are included in PR #34.

All four items below are **implemented on the follow-up branch**, with regression
tests; hosted checks and code-owner review remain merge requirements. The original
reproductions and acceptance scope are retained below. The previous five
[post-v1 priorities](IMPROVEMENT_PRIORITIES.md) are already implemented.

| Item | Implementation | Regression evidence |
| --- | --- | --- |
| 1 | Shared config-destination protection in both helpers; additive `--config FILE` | [Helper config tests](../tests/coverage-config.test.mjs), wired into Rust tests on all three CI platforms |
| 2 | Fall back only when the default config entry itself is missing | [Library/CLI policy tests](../tests/project_policy.rs) cover all gates and preserve existing outputs |
| 3 | Validate every record/metric before aggregation | [Coverage-summary tests](../tests/test_coverage_summary.py) reject cancellation, booleans and invalid mixed metrics |
| 4 | Invalidate stale results before setup; stage/sync/replace complete evidence | [Performance tests](../tests/test_performance.py) cover metadata/build/run failures, budget rejection, reruns and write failures |

PR #34 targets `main` directly and includes all changes from the superseded
PR #32. It does not depend on #32 merging. The original sequencing recommendation
below is superseded by this combined merge path. Code-owner review remains a
merge requirement; closing the superseded PR does not replace that review.

P1 means the next correctness fix; P2 means subsequent evidence hardening.
S estimates half to one engineer-day, M one to three days, including regressions
but excluding hosted CI/review waits. These are planning estimates.

| Order | Priority | Proposal | Observed impact | Effort |
| --- | --- | --- | --- | --- |
| 1 | P1 | Reserve the project config path in coverage helpers | Native collection overwrites existing project policy | M |
| 2 | P1 | Reject a dangling default-config symlink | An unchanged source changes from failing to passing under fallback defaults | S |
| 3 | P2 | Validate individual LLVM coverage records before summing | Invalid records can cancel into an apparently valid 100% total | S |
| 4 | P2 | Invalidate stale performance evidence before metadata collection | A failed local rerun leaves an older `passed: true` report | S |

## 1. Protect project configuration from coverage publication

Code: [`validateOutput()` and publication](../scripts/coverage.mjs);
[import destination handling](../scripts/import-coverage.mjs).

The native collector protects TypeScript destinations and rejects non-regular
output entries. An existing regular `ROOT/ts-crap.json` remains an allowed output.
The importer requires fresh paths, but can create a previously absent default
config as raw input or native output.

**Reproduced native-collector case:**

1. Create `a.mts` containing `export function f(){return 1;}` and `ts-crap.json`
   containing `{"version":1,"threshold":0}`.
2. `ts-crap --root ROOT --format json` exits 1 at CRAP 2, as expected.
3. Run `node scripts/coverage.mjs --root ROOT --output ROOT/ts-crap.json -- node -e ''`.
   Collection exits 0 and replaces the policy with a coverage document whose
   top-level keys are `version` and `files`.
4. The next scan exits 2 with `invalid project configuration`.

**Implemented change:** reserve the canonical default config location for both
helpers, including when absent, before executing tests and immediately before
publishing. Resolve existing parent-directory aliases when comparing locations.
Keep normal replacement of existing native coverage reports. An additive,
caller-relative `--config FILE` explicitly declares an alternative protected
policy path without loading it or forwarding policy to tests. Arbitrary config
paths are not inferred.

**Acceptance:** existing/absent default configs, equivalent path spellings,
directory symlink aliases, importer raw/native destinations, and successful
ordinary report destinations. Preflight failures must preserve policy bytes and
avoid executing the child. Existing hardlink publication behavior must remain
safe: replacing an output entry must not modify its other links.

The native overwrite was reproduced before implementation. Both importer
absent-config cases are now exercised by the helper integration regressions.

## 2. Distinguish missing configuration from a broken config link

Code: [`Rules::load()`](../src/config.rs), especially the default-config
`ErrorKind::NotFound` fallback.

Reading a symlink whose target is missing returns `NotFound`. The loader treats
that exactly like an absent `ts-crap.json` and silently installs the built-in
threshold and completeness policy.

**Reproduced:** point `ts-crap.json` at a policy containing
`{"version":1,"threshold":0}`. A source with one CRAP-2 function exits 1.
Remove the target policy, leaving the link in place, and repeat the identical
scan. It exits 0 with threshold 10. The source and symlink entry are unchanged.

**Proposed change:** allow defaults only when the default config entry itself is
absent. A present dangling link must produce a config-read error. Preserve
working config symlinks and the existing behavior for an absent default file;
an explicitly selected missing config remains an error.

**Acceptance:** direct library and CLI cases for absent/regular/valid-symlink/
dangling-symlink configs, explicit missing configs, and configured threshold,
require-coverage and fail-on-empty policies. A broken link must exit 2 without
publishing or replacing existing reports/baselines. Exercise symlinks on supported
Windows CI as well as POSIX.

## 3. Validate each LLVM coverage record before aggregation

Code: [`render()` and `enforce_floor()`](../scripts/coverage-summary.py).

Validation currently checks aggregate `covered` and `count` values. It does not
check each input entry first; `sum()` also turns boolean inputs into integers.

**Reproduced:** provide two `data` entries with `(count, covered)` equal to
`(10, 20)` and `(10, 0)` for each metric. The first entry is impossible, but their
aggregate is `(20, 20)`. Rendering reports 100.00%, and the 95% line gate accepts
the report.

**Proposed change:** validate integer types excluding booleans, nonnegative
counts, and `covered <= count` independently for every metric of every entry,
then aggregate validated values. Preserve valid multi-entry reports and the
existing no-data/zero-line rejection policy.

**Acceptance:** invalid entry hidden by another entry, negative values that
cancel, booleans, missing metrics, mixed valid/invalid metrics, all-zero reports,
and valid multi-entry totals exactly at and below the floor. Both summary output
and floor enforcement must reject malformed input.

This is defensive handling of malformed evidence. The probe does not show that
LLVM emits those invalid records or that an actual coverage run passed wrongly.

## 4. Prevent stale success reports after performance setup failures

Code: [`main()` metadata collection](https://github.com/RuiCostafrg/typescript-crap/blob/06768012d18d9d80d4f2070dd5d16e7407371c36/scripts/check-performance.py)
in [PR #32](https://github.com/RuiCostafrg/typescript-crap/pull/32).

The harness constructs metadata by invoking Git, Rust, Node and GNU time before
entering its report-writing `try/finally`. If that setup fails in a reused output
directory, a previous `results.json` is left untouched.

**Reproduced:** prepopulate an output directory with
`{"passed":true,"candidate_sha":"old-run"}`. Invoke `main()` with that directory
while injecting a metadata-command exception through Python's `unittest.mock`.
The exception propagates, but the old success document remains. The command
fails correctly; the problem is misleading retained evidence, not a passing CI
status. Fresh GitHub-hosted workspaces do not have the reused-directory condition.

**Proposed change:** invalidate the previous report before metadata collection,
start with a minimal `passed: false` result, and collect metadata inside the
failure-reporting scope. Publish the final JSON through a sibling staging file
and atomic replacement. Filesystem failures must still return nonzero and leave
no report claiming success for the attempted run.

**Acceptance:** seed a previous success, fail Git/Rust/Node/time metadata
collection separately, and assert that the prior success is removed or replaced
by a clearly failed current attempt. Also cover build failures, timeout,
malformed observations, budget rejection, successful reruns, and report-write
failure without modifying benchmark limits.

## Implementation order and validation

The original plan ordered 1 and 2 first, followed by 3, then 4 after PR #32.
All four are now included with the performance harness in PR #34, targeting
`main` together. Review changed behavior
against [the compatibility policy](COMPATIBILITY.md), and choose a release version
only after the final implementation scope is known. Existing releases remain
immutable.

Keep required coverage, mutation, API, bounded-proof, platform and performance
gates. Each implementation PR should include the failing reproduction as a
regression and verify the unaffected success case. The local review used
Node 26.3.1, Rust 1.96.0 and Android/aarch64 with Ubuntu/proot; hosted checks supply
the supported-platform acceptance evidence when fixes are implemented.

The disposable probe outputs are retained locally in
`target/next-improvements/reproductions.json`. The scenarios above describe the
inputs independently of that ignored artifact. This is a targeted review of
configuration/publication and CI evidence, not an exhaustive correctness claim.
