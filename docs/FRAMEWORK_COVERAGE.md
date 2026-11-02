# Real Jest and Vitest coverage integration

The networked integration suite runs actual frameworks and reporters in
[a small npm workspace](../tests/fixtures/frameworks/package.json). Jest tests a
TypeScript service, Vitest tests React TSX components, and both import the shared
TypeScript pricing package through its workspace package name.

Run from the repository root with Rust and Node 24+ installed:

```sh
cargo build --locked
node scripts/test-framework-coverage.mjs target/debug/ts-crap
# Windows: node scripts/test-framework-coverage.mjs target/debug/ts-crap.exe
```

The suite copies the fixtures into a temporary directory whose name contains
spaces, canonicalizes its path (including Windows 8.3 temporary-path aliases),
installs the committed lockfile with `npm ci --ignore-scripts`, and
removes its temporary files after execution. It needs npm registry access and is
separate from the offline Cargo tests. The `framework-coverage` CI job runs it
with Node 24 on Linux, macOS, and Windows; hosted results must be checked for the
commit being released.

## Framework and reporter matrix

The fixture pins Jest and babel-jest **30.5.2**, Vitest and
@vitest/coverage-istanbul and @vitest/coverage-v8 **5.0.5**, Babel core, TypeScript
preset and CommonJS transform **7.29.7**, and React/React DOM **19.3.1**.
Transitive versions are recorded in the
fixture's `package-lock.json`.

Each framework runs through both Istanbul JSON (`json`) and LCOV (`lcovonly`)
reporters. Jest uses Babel instrumentation and strips TypeScript with the Babel
preset; Vitest runs both Istanbul and V8 coverage providers with automatic JSX
transformation. These reporters emit coverage remapped to original TS/TSX paths.
There are six framework/provider/format combinations. This suite does not
establish compatibility with every framework version.

Local validation on 2026-10-26 passed all six combinations with Node **26.3.1**
on Linux aarch64 (Termux/proot). This is local evidence; the configured Node 24
and macOS/Windows CI matrix still requires hosted results for the release commit.

For each combination, the suite asserts:

- Fully exercised functions have coverage 1 and CRAP equal to complexity; the
  scanner passes a threshold of 4 with required coverage.
- Partial tests leave a function uncalled and pricing branches uncovered. Zero
  hits remain coverage evidence, and the scanner returns quality-gate exit 1.
- Reusing report paths is rejected without changing the previous native report.
  A new directory produces the same native coverage and scanner report.
- The one-step `coverage-run.mjs` command (`npx typescript-crap coverage` in
  the npm package), given the same framework, provider
  and format, publishes byte-identical native coverage. Automatic detection
  refuses to guess in this fixture, which has both configurations.
- Editing source after collection causes a scanner SHA-256 mismatch (exit 2).
  Editing source during collection makes the importer reject publication (exit 2).

## Version matrix

[`tests/fixtures/framework-versions`](../tests/fixtures/framework-versions)
holds one `package.json` and lockfile per supported major: `vitest-3`,
`vitest-4`, `vitest-5`, `jest-29` and `jest-30`. Each installs one framework
and reuses this suite's sources and assertions:

```sh
node scripts/test-framework-coverage.mjs target/debug/ts-crap --versions vitest-3
```

Only the installed framework's combinations run, and every label and failure
names the exact installed version (for example `vitest 3.2.7/v8/lcov`). The
`vitest-5` and `jest-30` lockfiles were seeded from the shared fixture's lockfile,
so they resolve the same transitive versions. The `framework-versions` CI job runs
all five on Linux, macOS and Windows with Node 24. It keeps the owner-only numeric
actor gate described in [DEPENDENCY_UPDATES.md](DEPENDENCY_UPDATES.md). The
supported-versions table is in [COMPATIBILITY.md](COMPATIBILITY.md#supported-test-frameworks).

## Complex Vitest/TSX scenarios

The Vitest fixture has 13 passing behavioral cases in a full run, plus one
deliberately failing case enabled only for publication-failure checks. Tests use
React DOM server rendering and invoke event handlers separately; no DOM browser
or event-dispatch environment is implied.

- Generic lists render empty/populated states, keyed children, and escaped text.
- Order components import workspace pricing, render optional labels and switch
  states, and contain separately scored nested render/event callbacks. Handler
  cases cover selected, disabled, zero-quantity, and negative-quantity orders.
- Async loaders cover successful, empty, Error, and non-Error rejection paths;
  loading, success, and accessible error UI are checked as rendered HTML.
- A dynamically imported `Résumé.tsx` covers Unicode filenames and identifiers,
  emoji and Japanese text, fragments, pluralization, and invalid input.
- A targeted partial run renders components without invoking events or importing
  the lazy module. The scanner must retain zero hits for those functions, partial
  coverage for the async error paths, and complete evidence for their parents.
  Exact event-handler source-line hits are checked in both full and partial runs.
- Failing actual Vitest assertions must prevent native coverage publication.
  Stale and during-collection mutation tests edit the Unicode TSX module itself.

The `coverage-gaps/DefaultPanel.tsx` fixture is scanned separately. Istanbul JSON
provides direct `default-arg` counters for destructured default initializers; the
importer now maps those counters alongside statement starts. JSON completeness
must pass, including when a zero count correctly records an unexecuted default.
Malformed default-argument locations/counter arrays reject the import. LCOV lacks
those records and can omit the declaration line; its completeness check must
still reject missing evidence. Use JSON for these functions, or place a fallback
in an explicit body statement (as in `OrderPanel`). Arbitrary branch totals are
never used to infer statement execution. The primary TSX fixtures still require
complete coverage and the original threshold of 4.

## Source-map limits

Already remapped TS/TSX records work directly. The separate
[importer regression suite](../tests/coverage_import.rs) covers generated
JavaScript with flat source maps v3 and embedded or external maps. The importer
also supports relative paths, `sourceRoot`, and file URLs.
Generated JavaScript maps must contain `sourcesContent`
matching the pre-test TypeScript snapshot. Indexed maps, webpack/other virtual
URL schemes, missing or mismatched source content, and unmappable locations are
unsupported and reject the import. Configure the reporter to remap to original
TypeScript where possible. These framework fixtures exercise reporter remapping;
they do not exercise every supported generated-JavaScript source-map form.

Coverage remains a conservative line approximation rather than branch coverage;
overlapping uncovered evidence keeps a line uncovered. See the
[helper guide](COVERAGE_HELPERS.md) for collection and freshness requirements.
