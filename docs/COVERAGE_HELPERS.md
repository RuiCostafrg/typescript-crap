# Standalone coverage helpers

Starting with v1.0.0, releases include
`ts-crap-coverage-helpers.zip` and its `.sha256` checksum. The bundle contains both
collectors and every supporting module; no repository checkout, npm install, or
Rust compiler is needed. Install the standalone `ts-crap` binary separately.

## Install and compatibility

Download the ZIP and checksum from the **same release tag** as your CLI at
https://github.com/RuiCostafrg/typescript-crap/releases. Verify the ZIP's SHA-256
against the checksum before extracting. On macOS use
`shasum -a 256 -c ts-crap-coverage-helpers.zip.sha256`; on Linux use
`sha256sum -c ts-crap-coverage-helpers.zip.sha256`. In PowerShell compare
`Get-FileHash ts-crap-coverage-helpers.zip -Algorithm SHA256` with the downloaded
checksum, then use `Expand-Archive ts-crap-coverage-helpers.zip`.

Keep the extracted `ts-crap-coverage-helpers` directory intact. Both entry points
use sibling modules, including the loader hook. Move the directory anywhere you
prefer and invoke its scripts with Node using their absolute paths.

- Node.js **24 or later** is required for the helpers and Node test commands.
- Use the **same exact release version** of helpers and scanner. Other version
  combinations are not supported, even when their schema version is the same.
- `version.json` records helper version, matching scanner version, Node minimum,
  and native coverage schema version. Compare it with `ts-crap --version`.
- Upgrade by downloading and verifying the new bundle, then replacing the whole
  helper directory along with the scanner. Do not mix modules from releases.
- Python is used to build/test release assets, but is not required by users.

## Collect native Node coverage

In these examples replace `/tools/ts-crap-coverage-helpers` with the extracted
directory and `/project` with your project root. On Windows use Windows paths in
the same quoted arguments. Output paths are relative to your calling directory;
the child command runs inside the project root.

Both helpers reject destinations at `ROOT/ts-crap.json`, whether or not it
already exists. If the scanner uses an alternative policy, pass the same
caller-relative `--config FILE` to the helper before `--` to protect that path as
well. The default remains reserved. This option protects files from publication;
it does not apply scanner policy to collection or forward arguments to tests.
For imports, it protects both raw input and native output. Existing directory
aliases and resolved config symlink targets are included in the checks.
Protected filename case variants are reserved on every platform for portability.

```sh
node "/tools/ts-crap-coverage-helpers/coverage.mjs" --root /project --output native-coverage.json -- node test.mjs
ts-crap --root /project --coverage native-coverage.json --require-coverage
```

Native collection supports Node's direct TypeScript execution, including `.mts`.
It does not transpile TSX or remap source maps. It snapshots source before tests
and verifies loaded TypeScript through the bundled loader hook. Failed collection
preserves an existing report; always check the command's exit status before scanning.

## One-step Vitest or Jest coverage

`coverage-run.mjs` wraps the importer below for Vitest and Jest. It finds the
framework installed in the project (or a parent workspace), creates a new
private run directory, adds the reporter and directory flags, imports the
result and prints the matching scanner command:

```sh
node "/tools/ts-crap-coverage-helpers/coverage-run.mjs" --root /project
ts-crap --root /project --coverage /tmp/ts-crap-run-AbC123/native-coverage.json --require-coverage
```

Add `--scan` to run that scanner command directly. The helper's exit status is
then the scanner's: 0 pass, 1 quality gate, 2 error. Before any test runs,
`--scan` checks that `ts-crap --version` (or `--scanner PATH`) matches this
bundle's `version.json` exactly.

| Option | Default | Meaning |
| --- | --- | --- |
| `--framework vitest\|jest\|auto` | `auto` | `auto` looks for `vitest.config.*`, `vite.config.*` when Vitest is installed (Vitest also loads it, but plain Vite apps have one too), `jest.config.*` or a `jest` key in `package.json`. It never evaluates config files. If both frameworks match it stops and asks for `--framework`. |
| `--provider PROVIDER` | see below | Each framework's own names: `istanbul` or `v8` for Vitest (`--coverage.provider`), `babel` or `v8` for Jest (`--coverageProvider`). Other combinations are rejected. |
| `--format json\|lcov` | `json` | Reporter: Istanbul `json` or `lcovonly`. JSON keeps default-argument evidence. |
| `--root DIR` | `.` | Project root; the framework runs here. |
| `--output FILE` | inside the run directory | Native coverage path. It must not exist yet, and missing parent directories are created before tests run. With `--output` the run directory is removed after success. Without it, the report stays in the run directory (kept, so the printed command works) until you delete it. |
| `--config FILE` | — | Alternative ts-crap policy path. It is protected from publication and passed to the scanner. |
| `--scan`, `--scanner PATH` | off, `ts-crap` | Run the scanner after a successful import. |
| `-- ARGS` | — | Extra framework arguments, for example `-- --config vitest.ci.ts` or `-- --runInBand`. Arguments that change the coverage reporter, directory, provider or coverage switch are rejected. |

Without `--provider`, Jest uses the project's configured provider. Vitest offers
to install a missing provider package and edit `package.json`, so for Vitest the
helper first checks that `@vitest/coverage-v8` or `@vitest/coverage-istanbul`
is installed. If only one is installed it selects that provider explicitly. If
both are installed the configured provider is used.

The importer's guarantees still apply. Every run uses fresh reporter and native
paths, source is hashed before the tests and checked after them, failing tests
publish nothing, and the project configuration is never overwritten. SIGINT,
SIGTERM and SIGHUP are forwarded through the importer to the test command, and
the helper waits for it to stop, so a cancelled run never publishes later.

The printed command is quoted for POSIX shells, and on Windows for PowerShell.
`cmd.exe` cannot quote `%` literally, so use `--scan` there for paths with
unusual characters. On failure
the run directory is kept and its path is printed for inspection.

## Collect Istanbul or LCOV

For transpiled TypeScript/TSX, run your existing reporter through the importer:

```sh
node "/tools/ts-crap-coverage-helpers/import-coverage.mjs" --root /project --format istanbul --input /project/coverage/coverage-final.json --output imported-coverage.json -- npx jest --coverage --coverageReporters=json
ts-crap --root /project --coverage imported-coverage.json --require-coverage
```

For LCOV, use `--format lcov --input /project/coverage/lcov.info`, with a command
that emits LCOV (for example `npx vitest run --coverage --coverage.reporter=lcov`).
Install/configure the test framework and coverage reporter in your project first.

LCOV `TN:` test groups are aggregated independently. Repeated records for the
same test name and resolved `SF:` source add execution counts before mapping;
missing/empty `TN:` belongs to the unnamed group for that record, without
inheriting the preceding record's test name. Within a test, overlapping
statements or generated locations mapped to the same source line take the
minimum count. The resulting per-test line counts then add. Duplicate `DA:` line
numbers within one source record and sums outside JavaScript's safe-integer range
are errors. Istanbul's already aggregated statement counts retain conservative
overlap behavior. Recollect coverage with the updated helper to benefit from
aggregation fixes; review changed gate results before updating baselines.

Both raw input and native output must be **fresh paths** before each import.
Use a new run directory and configure your reporter to write there, or remove
only those previous generated reports before rerunning. The importer runs tests
after snapshotting source; it cannot validate freshness of an old coverage file.
Changed source, failing tests, malformed or unmappable evidence abort publication.

Already remapped TypeScript coverage works directly. Generated JavaScript needs
flat source maps v3 with `sourcesContent` matching the snapshot; webpack virtual
URLs and indexed maps are unsupported. Coverage is a conservative line-based
approximation, not branch coverage. Istanbul JSON also maps direct `default-arg`
initializer counters; LCOV can omit that declaration-line evidence. Use JSON for
complete default-argument evidence. Missing evidence is never treated as covered.
These helpers run trusted test commands with your permissions; they are not a sandbox.

## Troubleshooting imports

Import failures exit with status 2, publish nothing, and name the likely fix.

| Message starts with | Likely cause | Fix |
| --- | --- | --- |
| `Coverage command failed: N` | Tests failed, or a coverage threshold failed. Any raw report the command wrote is not imported. | Fix the tests or thresholds, then rerun with fresh paths. |
| `Coverage command succeeded but wrote no report` | Coverage is off, the reporter is wrong, or the report went elsewhere. | Pass `--coverage`; use the `json` (Istanbul) or `lcovonly` reporter; set the reports directory (Vitest `coverage.reportsDirectory`, Jest `coverageDirectory`) to the directory that contains the `--input` file. |
| `Empty imported coverage` | No tests ran, or the coverage include misses your sources. | Set Vitest `coverage.include` or Jest `collectCoverageFrom`, for example `src/**/*.ts`. |
| `No source map for generated input`, `could not be source-mapped` | The report covers generated JavaScript. | Use a provider that remaps to TypeScript (Vitest `istanbul`/`v8`; Jest `babel`/`v8` with source maps). Do not import raw `NODE_V8_COVERAGE` output. |
| `Unmappable source outside collected TypeScript snapshot` | The report names a file outside `--root`, or one under it that is not collected: in an excluded directory (`node_modules`, `.git`, `target`, `dist`, `build`, `coverage`) or not `.ts`/`.tsx`/`.mts`/`.cts`. | The message says which. Pass the project root that contains your sources, or limit coverage to collected TypeScript sources. |

After a successful import, a warning lists TypeScript files under the root that
have no coverage records. It leaves out `*.test.*`, `*.spec.*`, `*.mock.*` and
`*.fixture.*` files (plural forms too), `*.config.*`
files, and anything under `__tests__`, `__mocks__`, `__fixtures__`, `test`,
`tests`, `mocks` or `fixtures` directories. Names are shown with control
characters escaped.
Those files count as missing coverage if the scanner selects them. The warning
never changes the exit status.
