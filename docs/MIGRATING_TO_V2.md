# Upgrading from v1 to v2

Version 2.0.0 corrects coverage import and Git selection behavior that can change
previously accepted gate outcomes. The CRAP formula, complexity rules, owned-token
fingerprints, Rust API, minimum Rust/Node versions, and native document schema
versions retain their existing meanings. The major version follows the previous
[1.x contract](https://github.com/RuiCostafrg/typescript-crap/blob/v1.0.0/docs/COMPATIBILITY.md).

1. Install the 2.0.0 scanner and helper bundle from the same release. Verify
   `ts-crap --version` and the bundle's `version.json`; do not mix v1/v2 tools.
2. Recollect coverage against unchanged source into fresh raw/native destinations
   using [the coverage helper instructions](COVERAGE_HELPERS.md). Re-running only
   the scanner cannot repair coverage previously imported by the v1 helper.
3. Compare new absolute reports with the old reports before applying regression
   baselines. Positive LCOV hits can now survive zero-hit independent tests,
   changing coverage fractions and CRAP scores. Statement/source-map overlap
   within each testcase remains conservative. Missing/empty `TN:` is per-record
   unnamed evidence; repeated captures of the same testcase/source add counters
   before mapping. Duplicate `DA:` lines within a record and unsafe counter sums
   now fail collection with exit 2 instead of publishing ambiguous evidence.
4. Keep the previous baseline in version control while reviewing changes. If a
   new baseline is appropriate, capture it with matching v2 coverage and review
   its absolute scores before committing it. Do not relabel old evidence or
   automatically accept a regenerated baseline.

Git diff/PR scans now reject symlinked source ancestors, including links whose
targets stay inside the project. Restore the committed directory structure and
rerun. Selecting a project through a root symlink remains supported. Repository
root names ending in whitespace now select the correct committed files; a
previous empty passing diff can reveal a real violation.

Report and baseline destinations cannot be the default `ROOT/ts-crap.json` or
an explicitly selected config path, even when absent or reached through a
directory alias. Choose another JSON filename for reports and snapshots.
Ordinary report locations and existing config contents are preserved.

The [2.x contract](COMPATIBILITY.md) defines future compatibility. The v1 release,
tag, assets, and historical evidence remain immutable.
