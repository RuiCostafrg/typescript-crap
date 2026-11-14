# Mutation evidence plan

Drafted 2026-11-13. This is a plan, not a record of shipped behaviour. The
implementing thread should correct it wherever the code or Oximut's report
contract contradicts it.

**Status 2026-11-14: M1–M4 implemented and locally validated. The recommended
behavior is recorded in [MUTATION_EVIDENCE.md](MUTATION_EVIDENCE.md).
M5 is complete: documentation and local checks are recorded below, and
[draft PR #81](https://github.com/RuiCostafrg/typescript-crap/pull/81) is open.**

## Goal

`ts-crap` accepts an Oximut mutation report as optional extra evidence, so a
function with surviving mutants is reported as under-tested even when its line
coverage is 100%.

## Known constraints

- Oximut's report (v1.0.0-rc.1) carries no source hash, no machine-readable
  mutator set, and absolute paths only. Freshness and path mapping therefore
  cannot copy the coverage design directly.
- A report produced with `--mutators` describes only that subset.
- The scoring contract, report schemas and exit codes are public interfaces
  under `docs/COMPATIBILITY.md`. Everything here is additive and opt-in.
- Oximut itself is not changed by this work.

## Milestones

We chose one draft PR with one commit per milestone, as permitted by the owner.
The reader and wiring overlap in implementation; milestone commits group the
reader, opt-in report surfaces, freshness conformance, gate behavior, and final
documentation. Final acceptance checks apply to the integrated slice.

### M1 — Design note and report reader

**Status: Complete: malformed/version/path rejection and nested same-line ownership tests pass.**

- Write `docs/MUTATION_EVIDENCE.md`: mutant-to-function mapping, freshness
  rule, treatment of functions with no mutants, treatment of filtered reports,
  and the scoring rule with the rejected alternatives.
- Parse and validate an Oximut report: schema version, required fields,
  duplicate and unknown keys, path containment under `--root`.
- Map each mutant to exactly one function span; nested functions own their
  own mutants.

**Acceptance:** malformed, wrong-version and out-of-root reports exit 2 with a
specific message; a mutant on a line shared by two nested functions has a
documented, tested owner.

### M2 — Opt-in flag and report fields

**Status: Complete: default behavior tests and additive JSON Schema validation pass.**

- Add the opt-in flag. Without it, output is byte-identical to v2.4.0.
- Add per-function counts to the JSON report: killed, survived, timed out,
  not covered, and other outcomes kept separate.
- Record at file and report level whether the evidence is complete or comes
  from a filtered mutator subset.

**Acceptance:** existing snapshot and contract tests pass unchanged with the
flag absent; the JSON Schema change is additive and validated in CI.

### M3 — Freshness

**Status: Complete: in-span and whole-file stale-edit rejection tests pass.**

- Reject evidence that does not match the scanned source. Use the strongest
  binding the report actually offers; if that is weaker than a source hash,
  say so in the design note and in the report output.
- Write down exactly what Oximut would need to emit for a hash-level binding.

**Acceptance:** editing a source file after the mutation run causes a
rejection or an explicit "unverified" marking, never silent acceptance.

**Outcome:** the strongest available binding is reproducing Oximut rc1's
opaque mutant-ID digest, which covers the root-relative path, the whole source
and the edit, plus the documented per-mutant byte-range/original check. It is
not a public stable hash contract: a future Oximut that derives IDs
differently is rejected with a specific message. Files with zero mutants carry
no freshness binding at all. A hash-level binding would need a per-file source
hash and a machine-readable mutator list from Oximut. See
[MUTATION_EVIDENCE.md](MUTATION_EVIDENCE.md#freshness-binding).

### M4 — Scoring or gate rule

**Status: Complete: fully covered survivor, all-killed, baseline and format tests pass.**

- Apply the rule chosen in M1 (effective coverage capped by kill ratio, or a
  separate gate beside the CRAP score).
- Functions with no mutants keep their line-coverage result and are marked as
  having no mutation evidence.

**Acceptance:** a function at 100% line coverage with surviving mutants fails
or is flagged; a function with all mutants killed is unchanged; text, JSON,
SARIF and HTML reports agree.

**Outcome:** the implementation recommends the **separate mutation gate**: any surviving or
uncovered mutant fails the function regardless of complexity or the CRAP
threshold; `effective_coverage`/`effective_crap` are informational estimates
only. Existing CRAP scores, coverage and baselines are unchanged, and
baselines cannot suppress mutation-gate failures. The rejected alternatives
are recorded in
[MUTATION_EVIDENCE.md](MUTATION_EVIDENCE.md#rejected-alternatives).

### M5 — Documentation and draft PR

**Status: complete. One draft PR, five milestone commits; no merge or release.**

- README section, schema docs, CHANGELOG entry, a reproducible example.
- Run the repo's local checks; state which were skipped on this host and why.
- Open a draft pull request. No merge, tag, release or npm publish.

## Out of scope

- Changes to Oximut.
- Baselines and move matching for mutation evidence.
- Mutation evidence in `dotnet-crap`.

## Decisions for the owner

1. Scoring rule: capped effective coverage versus a separate gate.

   **Resolved: separate gate.** Any survived or no-coverage mutant fails the
   function regardless of complexity or CRAP threshold;
   `effective_coverage`/`effective_crap` stay informational.
2. Whether filtered-subset evidence may ever lower a score, or only be shown.

   **Resolved: subset/unknown reports may fail functions but never prove
   complete adequacy.** With the separate gate, nothing is "lowered":
   `subset`/`unknown` is labelled partial evidence in every report.
3. Whether to ask Oximut for source hashes before this ships as stable.

   **Recommendation for this opt-in slice: do not block on upstream changes.** The rc1 mutant-ID digest is the
   strongest available binding and is used as documented; a per-file source
   hash and machine-readable mutator list remain the written-down ask for a
   future Oximut release. Oximut stays unchanged.

## Local verification (2026-11-14)

Passed: 148 Python helper tests; full debug and release Rust suites and Node
unit/integration tests through the local tests stage; the final full debug suite;
12 mutation integration tests in debug and release; strict all-target Clippy;
Rust formatting; rustdoc with warnings denied; npm launcher syntax checks;
JSON Schema/executable contracts; JavaScript lint and formatting checks.
The local lane was run with `--no-signoff`; no statuses were posted.
Android native addon loading required copying the pinned oxlint/oxfmt bindings
to the host-accessible temporary directory and setting
`NAPI_RS_NATIVE_LIBRARY_PATH`; no lockfile or dependency changes were made.

Not run: full cargo-mutants, LLVM coverage, audit, benchmarks, framework-version
matrices, cross-platform CI, and release verification. Those remain hosted/heavy
checks and this draft is not release acceptance. A real browser check was not
run: no browser executable is installed here; HTML output and gate markers are
covered by the integration suite. Oximut test execution is blocked by PRoot's
process-tree checks, so reconstructed fixture provenance is recorded explicitly.
