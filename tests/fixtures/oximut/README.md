# Oximut fixtures

Reconstructed conformance fixtures for Oximut 1.0.0-rc.1, used by
`tests/mutation.rs`. Fresh full and `--mutators boolean-literal` invocations
produced current report envelopes, locations, and IDs, but test execution failed
under PRoot's process-tree restrictions. `full.json` and `subset.json` copy the
historical M1 baseline and mutant outcomes for the same source and matching IDs;
they are not untouched successful current-run reports. The failed-baseline
`infrastructure-error.json` is the fresh run with its original outcomes.
See `docs/MUTATION_EVIDENCE.md` for the binding and its limitations.

| File | Content |
| --- | --- |
| `subject.ts` | The scanned TypeScript source: three small functions |
| `full.json` | Complete run with all families: two killed mutants, one survived in `uncalled` |
| `subset.json` | The same run filtered to `--mutators boolean-literal`: partial evidence |
| `infrastructure-error.json` | The baseline failed, so every mutant was skipped |

## The `<ROOT>` placeholder

Oximut records canonical absolute paths. The fixtures were recorded under a
project root whose location differs per machine, so every absolute path is
stored as `<ROOT>/...`. To use a fixture:

1. Create a fresh project directory and write `subject.ts` at
   `<root>/src/subject.ts` (its recorded location; the mutant ids and byte
   offsets are derived from exactly those bytes).
2. Replace every `<ROOT>` occurrence with the canonical absolute path of that
   directory before handing the report to ts-crap.

The scan root and the substituted `<ROOT>` must resolve to the same canonical
directory, otherwise every mutant file is rejected as outside the project
root.

## Mutant identity

Each mutant `id` is the digest Oximut 1.0.0-rc.1 derives from the
root-relative source path, the whole source bytes, and the edit (start/end
byte offsets, operator, replacement). `tests/mutation.rs` reproduces that
derivation to build synthetic reports for cases the recorded runs do not
cover (nested same-line functions, empty runs, stale sources, malformed
documents). This is a pinned behaviour of one rc release, not a public
contract; see `Mutation::for_source` in `src/mutation.rs`.
