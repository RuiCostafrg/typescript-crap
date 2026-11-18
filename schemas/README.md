# Versioned JSON Schemas

The files in this directory use JSON Schema draft 2020-12:

| Document | Schema | Example |
| --- | --- | --- |
| Project config | [config-v1](config-v1.schema.json) | [config](examples/config.json) |
| Native coverage | [coverage-v1](coverage-v1.schema.json) | [coverage](examples/coverage.json) |
| Regression baseline | [baseline-v1](baseline-v1.schema.json) | [baseline](examples/baseline.json) |
| JSON report | [report-v1](report-v1.schema.json) | [report](examples/report.json) |

Pin the repository release tag or commit when fetching schemas; the `main` URL in
`$id` identifies the schema, not an immutable download. Schemas and examples also
ship in the Rust source package. Validate examples and actual CLI output with:

```sh
npm ci --ignore-scripts --prefix tests/tooling
cargo build --locked
node scripts/verify-contracts.mjs target/debug/ts-crap
```

The test validates generated reports, native coverage, captured baselines,
malformed documents, normalized Unicode/newline paths, and additive output fields.
Config defaults are annotations; the scanner applies them, not the validator.
Report schemas allow additive fields for forward-compatible readers. Input
schemas remain closed and reject unsupported document versions.

The report schema's optional `mutation` object (unreleased) is an
additive output extension: it documents the Oximut evidence ts-crap attaches
when `--mutation-report` is supplied. Its `functions` list is parallel to the
report's `functions`; the mutation gate, count sums, and scope labelling are
cross-field invariants checked by executable contract tests, not by this
schema. The `mutators` and `evidence` enumerations cover the values this
schema version's scanner emits; a newer scanner release may add values, so
pin the schema to the scanner release it documents. See
[the mutation evidence note](../docs/MUTATION_EVIDENCE.md).

These schemas do not replace runtime validation. JSON parsers must reject
duplicate decoded object keys before schema validation. Glob syntax, source hashes,
source line bounds, finite f64/u64/platform-integer bounds, baseline thresholds,
and cross-field score/completeness rules are runtime checks. JavaScript validators
cannot precisely represent every u64 count; do not use their numeric rounding to
accept an out-of-range counter. See the [1.x contract](../docs/COMPATIBILITY.md).
