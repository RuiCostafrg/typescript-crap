# Standalone coverage helpers

Starting with the next release after v0.4.0, GitHub releases include
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

```sh
node "/tools/ts-crap-coverage-helpers/coverage.mjs" --root /project --output native-coverage.json -- node test.mjs
ts-crap --root /project --coverage native-coverage.json --require-coverage
```

Native collection supports Node's direct TypeScript execution, including `.mts`.
It does not transpile TSX or remap source maps. It snapshots source before tests
and verifies loaded TypeScript through the bundled loader hook. Failed collection
preserves an existing report; always check the command's exit status before scanning.

## Collect Istanbul or LCOV

For transpiled TypeScript/TSX, run your existing reporter through the importer:

```sh
node "/tools/ts-crap-coverage-helpers/import-coverage.mjs" --root /project --format istanbul --input /project/coverage/coverage-final.json --output imported-coverage.json -- npx jest --coverage --coverageReporters=json
ts-crap --root /project --coverage imported-coverage.json --require-coverage
```

For LCOV, use `--format lcov --input /project/coverage/lcov.info`, with a command
that emits LCOV (for example `npx vitest run --coverage --coverage.reporter=lcov`).
Install/configure the test framework and coverage reporter in your project first.

Both raw input and native output must be **fresh paths** before each import.
Use a new run directory and configure your reporter to write there, or remove
only those previous generated reports before rerunning. The importer runs tests
after snapshotting source; it cannot validate freshness of an old coverage file.
Changed source, failing tests, malformed or unmappable evidence abort publication.

Already remapped TypeScript coverage works directly. Generated JavaScript needs
flat source maps v3 with `sourcesContent` matching the snapshot; webpack virtual
URLs and indexed maps are unsupported. Coverage is a conservative line-based
approximation, not branch coverage. Missing evidence is never treated as covered.
These helpers run trusted test commands with your permissions; they are not a sandbox.
