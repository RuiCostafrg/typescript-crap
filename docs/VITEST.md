# Vitest quickstart

Score your TypeScript functions by CRAP (complexity and coverage) using the
coverage Vitest already produces. Tested with Vitest 3, 4 and 5 and both coverage
providers on Linux, macOS and Windows (see the
[supported versions](COMPATIBILITY.md#supported-test-frameworks)).

## 1. Install

```sh
npm install --save-dev --save-exact typescript-crap @vitest/coverage-v8@VITEST_VERSION
```

Replace `VITEST_VERSION` with your installed Vitest version (`npx vitest --version`):
the coverage provider must match it exactly, as in the tested fixtures.

`typescript-crap` installs the `ts-crap` scanner and the coverage helpers at the
same version. The helpers need Node.js 24 or later. Install
`@vitest/coverage-istanbul` instead of (or as well as) `@vitest/coverage-v8` if
you prefer the Istanbul provider. Without npm, download the scanner and the
`ts-crap-coverage-helpers.zip` bundle from the
[same release](COVERAGE_HELPERS.md#install-and-compatibility) and replace
`npx typescript-crap coverage` below with `node /path/to/ts-crap-coverage-helpers/coverage-run.mjs`.
With `--scan`, put the downloaded `ts-crap` on `PATH` or pass `--scanner /path/to/ts-crap`.

## 2. Configure

Tell Vitest which sources to measure, so files that no test imports still get
(zero-hit) coverage records:

```ts
// vitest.config.ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    coverage: { include: ['src/**/*.{ts,tsx}'] },
  },
});
```

Tell `ts-crap` which files to score. Without `include`, the scan also selects
test and config files, which have no coverage and fail `--require-coverage`:

```json
{
  "version": 1,
  "include": ["src/**"],
  "exclude": ["**/*.test.ts", "**/*.test.tsx"],
  "threshold": 30,
  "require_coverage": true
}
```

Save it as `ts-crap.json` in the project root.

## 3. Run

```sh
npx typescript-crap coverage --framework vitest --scan
```

This runs `vitest run` with coverage enabled, the JSON reporter and a fresh
private reports directory. It then converts the report into source-hashed native
coverage and runs `ts-crap --coverage … --require-coverage`. The exit status is the
scanner's: 0 pass, 1 quality gate failed, 2 error. Without `--scan` it prints the
scanner command instead.

- `--provider v8` or `--provider istanbul` picks the provider. Without it, the
  only installed provider is used, or your configured one if both are installed.
- Arguments after `--` go to Vitest, for example `-- --config vitest.ci.ts`.
  Arguments that would change the coverage reporter, directory or provider are
  rejected.
- Failing tests publish no coverage. Source edited during the run is rejected.

## JSON or LCOV

Use the default `--format json` (Istanbul JSON). It carries statement and
default-argument evidence, so functions with destructured defaults are complete.
`--format lcov` also works, but LCOV can omit default-argument declaration lines,
and those functions then count as missing coverage. See
[FRAMEWORK_COVERAGE.md](FRAMEWORK_COVERAGE.md) for the tested details.

## package.json scripts

```json
{
  "scripts": {
    "test": "vitest run",
    "crap": "typescript-crap coverage --framework vitest --scan"
  }
}
```

Run `npm run crap` locally and in CI. To keep the native report, pass
`--output coverage/ts-crap.json` (it must not exist yet), then scan it separately,
for example with `--format sarif`.

## GitHub Actions

```yaml
name: CRAP
on: pull_request
permissions:
  contents: read
jobs:
  crap:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1 # v7.1.0
        with:
          node-version: 24
      - run: npm ci
      - run: npx typescript-crap coverage --framework vitest --scan
```

Actions are pinned to commit SHAs, as in this repository's workflows. For a
reviewed baseline, changed-file (`--diff`) gating and retained JSON/SARIF reports,
start from [examples/ci/github-actions.yml](../examples/ci/github-actions.yml) and
[CI_ADOPTION.md](CI_ADOPTION.md). CI replays equivalent collection, baseline and gate
commands with real Vitest coverage, using `scripts/verify-ci-guide.py` in a synthetic
Git checkout with the installed helper bundle. The YAML file itself is not run.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `Vitest coverage needs @vitest/coverage-v8 or @vitest/coverage-istanbul` | Install one coverage provider package at your Vitest version. |
| `Coverage command failed: N` | A test or a coverage threshold failed. Fix it and rerun; nothing was published. |
| `Empty imported coverage` | No tests ran, or `coverage.include` misses your sources. |
| Gate fails with `files_missing_coverage` on test or config files | Add `include` (and `exclude`) to `ts-crap.json` so only sources are scored. |
| Warning lists files with no coverage records | Add them to `coverage.include`, or exclude them in `ts-crap.json`. |
| `Both Vitest and Jest configuration found` | Pass `--framework vitest`. |
| `could not be source-mapped` | Use the `v8` or `istanbul` provider; both remap to TypeScript. |

More messages and their fixes are in the
[helper guide](COVERAGE_HELPERS.md#troubleshooting-imports).
