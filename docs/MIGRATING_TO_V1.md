# Upgrade from v0.4.0 to v1.0.0

The CLI's default threshold remains 10. Missing coverage remains 0%, the threshold
comparison remains strict, and schema versions for native coverage, configuration,
baselines, and reports remain 1. Existing command lines keep their defaults unless
you add project gate policy. Read the [1.x contract](COMPATIBILITY.md) before
binding an integration to output fields or baseline identities.

## Install the CLI and matching helpers

Use the documented release installer or checksum-verified platform archive. The
Linux prebuilt minimum remains Ubuntu 26.04; Node 24+ is required only for coverage
collection. The first standalone helper bundle is included with v1.0.0; published
v0.4.0 assets are unchanged and do not contain it. Extract the helper ZIP from the
same release as the CLI and keep its files together. Verify both checksums and,
when using GitHub provenance, bind verification to the expected release tag/commit.

Replace repository-relative `scripts/coverage.mjs` / `scripts/import-coverage.mjs`
paths with paths inside that extracted bundle. Use a new temporary raw/native
output directory for each Istanbul/LCOV collection. Do not reuse an earlier raw
report or manufacture a new source hash for it. The [helper guide](COVERAGE_HELPERS.md)
and [CI workflow](CI_ADOPTION.md) show complete commands.

## Migrate Rust callers and persisted policy

`ScanOptions.threshold`, `require_coverage`, and `fail_on_empty` now use `Option`.
For a caller that intentionally fixed policy in v0.4.0:

```rust
let options = ts_crap::ScanOptions {
    threshold: Some(10.0),
    require_coverage: Some(false),
    fail_on_empty: Some(false),
    ..Default::default()
};
```

Use `None` (or omit those fields) to inherit `ts-crap.json` policy, then built-in
defaults. `Some(false)` deliberately overrides configured `true`. The resolved
report threshold is still a scalar. The minimum supported Rust version is 1.90.

CLI users may add `threshold`, `require_coverage`, and `fail_on_empty` to the
root project's existing version-1 config. Explicit CLI options override each field
independently. Bare boolean flags enable the setting; `--require-coverage=false`
and `--fail-on-empty=false` explicitly disable configured gates. No ancestor or
environment policy is introduced. Review policy before committing it, and keep
CI baseline/policy reads anchored to the reviewed base revision.

## Recollect coverage and review baseline decisions

Existing valid version-1 documents remain readable, subject to the same strict
validation, current-source hashes, and threshold matching. The baseline format
and owned-token identity algorithm have not changed. Do not rewrite a baseline
solely because the application version changed. If you change threshold/policy,
review and recreate the baseline deliberately with the new scanner; keep its
prior version in Git for review. Completeness failures cannot be waived by a
baseline.

The importer now consumes direct Istanbul JSON `default-arg` counters. Default
initializers that previously lacked statement evidence can now have real covered
or zero-hit evidence, changing the coverage-derived score for those inputs. Use
fresh collection and review the resulting report; do not copy old report hashes.
LCOV can still omit default-initializer declaration lines and fails completeness
when that evidence is absent. Unknown source mappings still reject the import.

The parser now accepts valid generic overloads in interface/object types and
TypeScript 5 type-only wildcard exports that v0.4.0 rejected. This does not change
the scoring/identity rules for previously supported source. Remaining malformed
syntax still fails the entire scan with exit 2 and preserves existing outputs.

The [schemas and checked examples](../schemas/README.md) help structural validation.
They do not replace duplicate-key checks, source freshness, or runtime cross-field
validation. JSON report consumers must tolerate additive output fields; human
formatting and exact error text are outside the compatibility contract.
