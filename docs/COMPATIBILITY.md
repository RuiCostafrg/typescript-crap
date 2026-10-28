# The 2.x compatibility contract

This contract applies starting with ts-crap 2.0.0. The CLI, helper bundle, and
schemas are versioned together. Install helpers from the exact same release as
the scanner. Native coverage, configuration, baseline, and JSON report documents currently use independent integer schema version **1**.

## Scoring and selection

The normative scoring rules are the README's [scoring contract](../README.md#scoring-contract)
and executable tests in `tests/core.rs`, `tests/cli.rs`, and `tests/adoption.rs`.
CRAP is `CC² * (1 - coverage)³ + CC`, with coverage in [0, 1]. The gate uses the
unrounded score and fails strictly above the effective threshold. Missing
coverage is zero; recorded zero hits are evidence of an uncovered line, not
missing evidence. Completeness and empty-selection gates remain independent of
score and baseline suppression.

Each implemented function starts at CC 1; decisions include the documented loops,
conditionals, catches, non-default cases, ternaries, and short-circuit operators.
Nested functions own their decisions and statement starts separately. Type-only
syntax, optional chaining, plain else/default/try/finally, comments, and strings
do not add decisions. Coverage uses owned executable statement-start lines,
including default initializers. Shared-line evidence is a conservative
approximation, not branch coverage. Line and column values are one based; JSON
`column` counts UTF-8 bytes and `column_utf16` counts UTF-16 code units.

Changing these conventions, owned-token fingerprints, or existing input-path
identity rules requires a **major application release** and a documented
migration. Corrections to inputs previously rejected or unsupported may be minor;
a patch must not silently change a supported input's gate outcome. Parser or
mapping fixes that change existing scores require explicit release notes and the
same compatibility review as other scoring changes. Do not relabel a scoring
change as a routine dependency update.

Configuration discovery stays at `ROOT/ts-crap.json`, unless `--config` explicitly
selects a caller-relative file. No environment or ancestor policy is loaded.
A default config entry that is a dangling symlink is an error; only an absent
entry uses built-in defaults. Working config symlinks remain supported.
Each explicit gate override wins over config and then built-in defaults
(threshold 10; require_coverage/fail_on_empty false). Bare boolean flags mean true;
`=false` explicitly disables a configured gate. Include/exclude replacement and
exclusion precedence are documented in the README and remain stable in 2.x.

LCOV helpers aggregate captures by testcase and source before source mapping,
retain conservative overlap within each testcase, then add independent testcase
line counts. A missing or empty `TN:` names the unnamed testcase for that source
record; it never inherits the preceding record's name. See the
[v2 migration guide](MIGRATING_TO_V2.md) for the change from v1 behavior. Native
version-1 coverage JSON retains its existing meaning; recollect rather than
relabeling old imported evidence.

## Process and output behavior

- Exit **0**: a valid scan passed its effective gate; help/version also exit 0.
- Exit **1**: a valid scan failed the score, regression, completeness, or empty gate.
- Exit **2**: arguments, configuration, source, coverage, Git identity, or output
  publication failed. A collection helper also fails when its trusted child fails.
  Native collector child-exit propagation is documented separately in its guide.
- JSON and SARIF machine output remains independent of human row filtering/sort/top
  options. Reports are produced on valid gate failures. Input errors do not
  replace existing reports or baselines with a partial result.
- Existing JSON fields retain names, types, units, and meaning. Minor releases may
  add output fields; consumers must ignore unknown output fields. Whitespace,
  object-key order, human tables/HTML markup, exact diagnostics, and displayed
  decimal precision are not APIs. Ordering of functions stays path/line/column.
- SARIF stays 2.1.0 with `TSCRAP001` score and `TSCRAP002` completeness rules,
  UTF-16 locations, relative percent-encoded URIs, and `function/v1` fingerprints.
  Optional SARIF properties may be added without changing these meanings.

## Input documents and migration

See [upgrading from v1](MIGRATING_TO_V2.md) for the current migration, or
[upgrading from v0.4.0](MIGRATING_TO_V1.md) for the earlier CLI/Rust migration.

[Versioned JSON Schemas](../schemas/README.md) cover config, native coverage,
baselines, and reports. Input objects reject unknown/duplicate fields and
unsupported versions. Schema validation is structural: duplicate decoded JSON
keys, finite runtime numeric bounds, glob syntax, current-source SHA-256 checks,
source line bounds, and baseline/report cross-field invariants also require the
CLI/helper runtime. Never treat a schema pass as proof of source freshness.

Source identities are normalized root-relative forward-slash paths with no empty,
`.`/`..`, backslash, or colon components. Coverage hashes bind the exact bytes
collected before tests; source changes require recollection. Fingerprints bind
owned tokens, excluding trivia and nested function bodies. Ambiguous baseline
matches stay new findings. Renames follow the documented unique-identity and
explicit OLD=NEW rules; threshold mismatches are errors.

Breaking an input document's meaning requires a new document version, an explicit
reader or conversion path, and migration instructions. A new application major
may keep a document version only if its interpretation is unchanged. A scoring or
fingerprint change requires reviewing and recreating baselines with the new CLI;
never transform old scores into new evidence or silently bless an updated baseline.
Keep the previous baseline in version control for review. Coverage documents must
always be recollected against unchanged source with matching-version helpers.

## Rust API and dependencies

The public Rust API follows SemVer starting with 1.0.0. Removing/renaming public
items, changing public struct fields or signatures, or adding variants to an
exhaustively matchable public enum requires a major release. Nonbreaking new
functions may be minor. `ScanOptions` gate fields use `Option`: None inherits
policy; Some(value), including Some(false), explicitly overrides it. The resolved
`Report.threshold` remains a scalar. The minimum supported Rust version is 1.90;
raising it requires a documented minor release and CI verification.

Internal modules, dependency types not exposed through the public API, private
layout, implementation algorithms, and performance timing are not stable APIs.
The CLI runs trusted local test/Git commands with the caller's permissions. It
makes no sandbox, hard CPU/memory budget, or subprocess timeout guarantee. Resource
observations and the v1 budget decision are recorded in the adoption report;
CI jobs should enforce their own time/memory limits for untrusted repositories.

## Release acceptance

A version is releasable only after the exact source commit passes main CI,
coverage and mutation floors, advisory checks, installation/adoption/contract
validation, and candidate archive/helper/installer verification. Publish through
the protected release workflow; live Homebrew synchronization and a clean
published-tool CI-guide run supply publication evidence. Do not claim release
acceptance from a branch's local tests alone or bypass failed quality checks.
