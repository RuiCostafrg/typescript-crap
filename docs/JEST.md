# Jest quickstart

Score your TypeScript functions by CRAP (complexity and coverage) using the
coverage Jest already produces. Tested with Jest 29 and 30 on Linux, macOS and
Windows. On Jest 30, `babel-jest`, `ts-jest` and `@swc/jest` are tested with both
the `babel` and `v8` coverage providers (see the
[supported versions](COMPATIBILITY.md#supported-test-frameworks)).

## 1. Install

```sh
npm install --save-dev --save-exact typescript-crap
```

`typescript-crap` installs the `ts-crap` scanner and the coverage helpers at the
same version. The helpers need Node.js 24 or later. Without npm, download the
scanner and the `ts-crap-coverage-helpers.zip` bundle from the
[same release](COVERAGE_HELPERS.md#install-and-compatibility) and replace
`npx typescript-crap coverage` below with `node /path/to/ts-crap-coverage-helpers/coverage-run.mjs`.
With `--scan`, put the downloaded `ts-crap` on `PATH` or pass `--scanner /path/to/ts-crap`.

## 2. Configure

Use your existing TypeScript transformer and set `collectCoverageFrom`, so files
that no test imports still get (zero-hit) coverage records:

```js
// jest.config.cjs (babel-jest; needs @babel/preset-typescript and
// @babel/plugin-transform-modules-commonjs)
module.exports = {
  testEnvironment: 'node',
  transform: { '^.+\\.ts$': ['babel-jest', {
    presets: ['@babel/preset-typescript'],
    plugins: ['@babel/plugin-transform-modules-commonjs'],
  }] },
  collectCoverageFrom: ['src/**/*.ts'],
};
```

[FRAMEWORK_COVERAGE.md](FRAMEWORK_COVERAGE.md#jest-transformers-and-coverage-providers)
has tested `ts-jest` and `@swc/jest` configurations, plus the extra options for
`.tsx` files with JSX.

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
npx typescript-crap coverage --framework jest --scan
```

This runs Jest with `--coverage`, the JSON reporter and a fresh private coverage
directory. It then converts the report into source-hashed native coverage and runs
`ts-crap --coverage … --require-coverage`. The exit status is the scanner's: 0 pass,
1 quality gate failed, 2 error. Without `--scan` it prints the scanner command
instead.

- `--provider babel` or `--provider v8` sets Jest's `--coverageProvider`. Without
  it, your configured provider is used. The CRAP results are the same either way.
- Arguments after `--` go to Jest, for example `-- --config jest.ci.config.cjs --runInBand`.
  Arguments that would change the coverage reporter, directory or provider are
  rejected.
- Failing tests, or a failing `coverageThreshold`, publish no coverage. Source
  edited during the run is rejected.

## JSON or LCOV

Use the default `--format json` (Istanbul JSON). It carries statement and
default-argument evidence. `--format lcov` gives the same results for ordinary
code, but LCOV can omit default-argument declaration lines, and those functions
then count as missing coverage.

## package.json scripts

```json
{
  "scripts": {
    "test": "jest",
    "crap": "typescript-crap coverage --framework jest --scan -- --runInBand"
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
      - run: npx typescript-crap coverage --framework jest --scan -- --runInBand
```

Actions are pinned to commit SHAs, as in this repository's workflows. For a
reviewed baseline, changed-file (`--diff`) gating and retained JSON/SARIF reports,
start from [examples/ci/github-actions-jest.yml](../examples/ci/github-actions-jest.yml) and
[CI_ADOPTION.md](CI_ADOPTION.md). CI runs that workflow's collection, baseline and
gate commands with real Jest coverage through the one-step command, using `scripts/verify-ci-guide.py` in a synthetic
Git checkout with the installed helper bundle. The YAML file itself is not run.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `Cannot find jest installed` | Install Jest in the project, or run from the package that depends on it. |
| `Coverage command failed: N` | A test or a `coverageThreshold` failed. Fix it and rerun; nothing was published. |
| `Empty imported coverage` | No tests ran, or `collectCoverageFrom` misses your sources. |
| Gate fails with `files_missing_coverage` on test or config files | Add `include` (and `exclude`) to `ts-crap.json` so only sources are scored. |
| Warning lists files with no coverage records | Add them to `collectCoverageFrom`, or exclude them in `ts-crap.json`. |
| `Both Vitest and Jest configuration found` | Pass `--framework jest`. |
| `--provider istanbul is not a Jest coverage provider` | Jest's providers are `babel` and `v8`. |
| `could not be source-mapped` | Make the transformer emit source maps (`@swc/jest`: `sourceMaps: 'inline'`). |

More messages and their fixes are in the
[helper guide](COVERAGE_HELPERS.md#troubleshooting-imports).
