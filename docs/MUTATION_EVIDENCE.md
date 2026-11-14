# Mutation evidence

This note records the proposed design for the opt-in Oximut mutation evidence in
the next minor ts-crap release. It supersedes the corresponding parts of
[MUTATION_EVIDENCE_PLAN.md](MUTATION_EVIDENCE_PLAN.md), which was the draft
plan. Everything here is additive and opt-in: without a mutation report the
scanner's output is unchanged. Oximut is read-only for ts-crap — the scanner
consumes an Oximut report and never launches, configures, or modifies Oximut.
Oximut itself was not changed by this work.

## Supplying evidence

- CLI: `ts-crap --mutation-report FILE ...` with an Oximut report whose
  `schemaVersion` is 1.
- Rust API: `ScanExtensions::mutation_report` with `scan_with`.

The report must describe the same checkout and the same `--root`: Oximut
records canonical absolute paths, so both tools must have run on this checkout
with this root, and every mutant file must resolve to a valid root-relative
source identity under it. Like coverage, a mutation report is trusted local
input from your own pipeline; freshness and consistency checks below are
integrity checks, not authentication.

## Load-time validation

A report is rejected (exit 2) unless:

- `schemaVersion` is present and equals 1; other versions are unsupported.
- `baseline.outcome` is `passed`. Mutant results are evidence only when the
  unmutated tests passed.
- Every mutant has a unique SHA-256 `id`, a one-based location whose byte
  offsets are ordered, and a file inside the project root.
- The `summary` agrees with the mutant entries: per-outcome counts and total,
  the score denominator (`killed + survived + no_coverage`), Oximut's own
  score, and `complete` implies no inconclusive mutants. A truncated or edited
  mutant list cannot pass as the run's evidence.

Consumed fields are named exactly; Serde rejects a duplicate of any consumed
field. Unknown fields are ignored, as Oximut asks consumers to do.

## Mutant-to-function ownership

Each mutant belongs to the innermost implemented function whose span contains
the mutant's whole edit, matching CRAP ownership: nested functions own their
own mutants and are scored separately. A mutant with no owning function — for
example module-level code, or an edit spanning a function boundary — belongs to
no function; it is counted in `mutants_outside_functions` and fails no gate.
Ownership uses Tree-sitter LF rows and UTF-8 byte columns. The redundant input
locations are checked separately using Oximut's JavaScript line separators
(LF, CR, CRLF, U+2028 and U+2029) and UTF-16 columns.
Ownership uses the analyzed function spans, so an unparsable selected file
(retained under `--on-syntax-error report`) owns no functions: its mutants are
still validated for freshness and counted, but never scored.

## The mutation gate (decision)

**A function fails a separate mutation gate when it owns at least one mutant
with outcome `survived` or `no_coverage`.** The gate is independent of the CRAP
threshold, of complexity, and of the CRAP score:

- A function whose CRAP score passes but that owns a surviving or uncovered
  mutant fails the scan (exit 1). It is marked `FAIL (mutation)` in text and
  HTML, reported as `TSCRAP003` in SARIF, and counted in
  `functions_failed_by_mutation_only`.
- **Existing CRAP scores, coverage, and thresholds are unchanged.** The gate is
  additive: `FunctionScore::crap` and `coverage` mean exactly what they meant
  before, and a function that previously passed still passes the CRAP gate.
- `FunctionScore::failed` includes the mutation-gate result, so report-level
  `failed` and exit codes cover both gates. CRAP-only failures retain their existing rule.
- **Regression baselines cannot suppress mutation-gate failures.** Baseline
  snapshots hold CRAP scores only; a surviving or uncovered mutant always
  fails, in every scan, baseline or not.
- **Functions with no mutants are unchanged**: their line-coverage result
  stands alone (`evidence: "none"`). No-mutant status is not evidence of test
  adequacy — the mutator simply produced nothing there, and the text report
  counts these functions explicitly.
- **Functions whose every mutant is inconclusive** (timeout, invalid,
  infrastructure error, resource exhaustion, skipped) are unchanged:
  inconclusive outcomes count neither for nor against, as in Oximut's own
  score (`evidence: "inconclusive"`). An incomplete run (`run_complete: false`) is labelled in reports; cleanup
  failures can also make a run incomplete without inconclusive mutants.

### Informational estimates, not gate inputs

`kill_ratio` is `killed / (killed + survived + no_coverage)`, Oximut's own
score definition. `effective_coverage` is `min(coverage, kill_ratio)` — a kill
is not a line hit, so mutation evidence can only lower the estimate.
`effective_crap` is the CRAP formula at `effective_coverage`, never below the
function's real `crap`. **These three fields are informational estimates only
and do not decide the mutation gate.** They illustrate the rejected coverage-cap alternative without rescoring functions.

### Partial mutator scope

`mutators` reports which mutator families the run generated, as far as the
report's diagnostic text says: `all`, `subset` (with the named
`mutator_families`), or `unknown` when the report does not say or the
diagnostics are unreadable or disagree. **`subset` and `unknown` are explicitly
partial evidence: such a report can fail functions, but it can never prove
complete adequacy.** A clean mutation result under a filtered or unnamed
mutator set shows only that those mutants were caught. Family names are taken
only from agreeing, readable `mutator families=` diagnostics; anything else is
`unknown` rather than a guess.

## Freshness binding

Coverage binds evidence with a documented source hash. Oximut 1.0.0-rc.1
offers no source hash, no machine-readable mutator set, and absolute paths
only, so mutation evidence uses the strongest binding the report actually
offers:

- **Byte-range check.** Every mutant's recorded byte range must select exactly
  its recorded `original` text from the current source.
- **Mutant-ID reproduction.** Each Oximut mutant `id` is an opaque digest. As
  observed in 1.0.0-rc.1, it is derived from the root-relative source path, the
  whole source bytes, and the edit (start/end offsets, operator, replacement).
  ts-crap recomputes that digest and rejects the report on any mismatch. Since
  the ID covers the full source, one mutant in a file binds every other byte of
  that file.

JSON records `source_binding: "oximut-1.0.0-rc.1-mutant-id"` to identify this
check. This is an integrity check that reproduces a pinned rc1 behavior. **It is not a
public stable hash contract**: Oximut does not document the ID derivation as an
interface, and ts-crap does not promise to track future derivations. An Oximut
release that derives IDs differently — or a report generated with a different
`--root` — is rejected with a specific message telling the user which version
the scan expects, never silently accepted.

Two documented limits:

- **No evidence freshness for zero mutants.** A selected file the report has no
  mutants for carries nothing that binds its bytes, so a stale report cannot be
  detected for that file. Such files have no mutation evidence either way
  (`files_without_mutants`), and nothing is claimed about their freshness.
- Editing a selected source after the mutation run is always rejected with
  exit 2 for files that do have mutants, never silently accepted.

What a stronger binding would require from Oximut: a per-file source hash
(collected against a pre-run snapshot, as the coverage helpers do) and a
machine-readable mutator-family list instead of diagnostic text.

## Rejected alternatives

1. **Gate on effective CRAP** (fail when `effective_crap` exceeds the
   threshold). Rejected: it couples the mutation outcome to complexity and
   coverage weighting, so a surviving mutant can hide behind a low-complexity
   or well-covered function, and the gate outcome would swing with Oximut's
   score denominator whenever mutants are inconclusive. It would also change
   what a function's failure means per threshold, complicating baselines for no
   safety gain.
2. **Replace coverage with effective coverage in the existing CRAP score.**
   Rejected: it silently changes every existing score, baseline meaning, and
   the documented scoring contract — a breaking change for an additive,
   opt-in feature, and one that makes scores depend on which mutator subset
   happened to run.
3. **Report-only, never fail.** Rejected: a surviving mutant with 100% line
   coverage is precisely the case line coverage cannot see; leaving it out of
   the gate makes the evidence decorative.
4. **Fail only on `no_coverage`, tolerate survivors.** Rejected: a surviving
   mutant means tests executed the mutated code and still passed, which is
   evidence of weak assertions, not of adequacy.
5. **Require a kill-ratio floor (for example 100%).** Rejected as redundant
   under this gate: any survivor already fails, so the only non-failing scored
   functions are those with all mutants killed; a floor adds threshold policy
   without changing any outcome.

## Report surfaces

All formats agree on the gate outcome; only presentation differs.

- **JSON**: the additive `mutation` object (documented in
  [report-v1](../schemas/report-v1.schema.json)), with `functions` parallel to
  the report's `functions` in the same order.
- **Text**: a `Mutation evidence (oximut)` section listing functions with
  surviving or uncovered mutants, plus the gate summary line.
- **HTML**: the same section and a table of those functions.
- **XML**: a `<mutation-evidence>` element and per-function mutant attributes.
- **SARIF**: failed functions with surviving or uncovered mutants are reported
  under the new `TSCRAP003` rule (surviving/uncovered mutants); CRAP-only failures
  keep `TSCRAP001`.

Machine output stays independent of human row filtering, sorting, and
`--top`; the JSON `mutation` object always lists every function.

## Fixture provenance

`tests/fixtures/oximut/full.json` and `subset.json` are reconstructed
conformance fixtures, not untouched outputs of successful current runs. Their
report envelopes and mutant locations/IDs came from fresh rc1 fixture runs on
this host, which could not execute tests because PRoot lacks Oximut's required
process-tree checks. Baseline and mutant outcomes were copied from the recorded
historical M1 run for the same source and matching IDs. The subset envelope was
produced with `--mutators boolean-literal`. `<ROOT>` replaces the absolute root.
`infrastructure-error.json` retains the fresh failed-baseline report. Tests also
construct synthetic reports for specific ownership and validation cases.

## What this does not prove

Mutation evidence measures the selected operators against the tests that ran.
Like the CRAP score and line coverage, it is not proof of correctness,
security, or complete branch coverage. A passing scan with `mutators: "all"`
and a complete run shows the selected mutants were caught; it does not show
that every interesting mutant was generated.
