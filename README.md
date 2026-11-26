![typescript-crap banner](assets/readme-banner.svg)

# ts-crap

**Find TypeScript functions that are complex and under-tested.** ts-crap is a
Rust CLI that combines cyclomatic complexity and line coverage into a CRAP score
for each function. Scan a project while you work, or gate changed files in CI
and use a baseline to catch new or worsening violations.

## Install

From your TypeScript project root:

```sh
npm install --save-dev --save-exact typescript-crap
npx ts-crap --root . --only-failing --sort crap --top 20
```

The npm package is `typescript-crap` and its installed command is `ts-crap`.
For a one-off run, use `npx typescript-crap@3.0.0` to select this package. Keep optional npm
dependencies enabled so your package manager installs the platform binary.
On Yarn Plug'n'Play, run `yarn ts-crap`.

Prebuilt binaries support macOS (Apple Silicon and Intel), Linux (x86-64 and
ARM64), and Windows (x86-64). The scanner runs without Node or Rust. On
Android/Termux, build from source.

- **Homebrew** — `brew install RuiCostafrg/tap/ts-crap`.
- **Standalone** — shell and PowerShell installers and verified archives are
  available on [GitHub Releases](https://github.com/RuiCostafrg/typescript-crap/releases/latest).
  See [download verification](SECURITY.md#verifying-downloads) and
  [Linux distribution](docs/LINUX_DISTRIBUTION.md) for musl, glibc, and Debian packages.
- **Source** — run `cargo install --locked --path .` from this checkout with
  Rust 1.90+ and a C compiler.

## Use coverage from your tests

With Node 24+ and Vitest or Jest installed, configure coverage to include your
TypeScript sources and set `include` in `ts-crap.json` to scan those same sources.
The [Vitest](docs/VITEST.md) and [Jest](docs/JEST.md) quickstarts have copyable
configurations. Vitest also needs a matching provider such as
`@vitest/coverage-v8`. Then collect coverage and scan in one command.

```sh
npx typescript-crap coverage --framework vitest --scan
# For Jest, use --framework jest.
```

On Yarn Plug’n’Play, set `nodeLinker: node-modules` for coverage collection.
The collector binds coverage to the source bytes it tested. Without coverage
evidence, the scanner treats a function as uncovered. A function with
complexity 4 scores **20** at 0% coverage, **6** at 50%, and **4** at 100%.

`CRAP = complexity² × (1 − coverage)³ + complexity`, where coverage is a
fraction between 0 and 1. Scores strictly above the threshold fail. The
default threshold is 10. See the [scoring contract](docs/CLI.md#scoring-contract)
and [coverage helpers](docs/COVERAGE_HELPERS.md) for standalone collection and
Istanbul or LCOV imports.

## Scan and report

```sh
# Scan a directory and a file.
npx ts-crap --root . src packages/api/index.ts

# Score all functions in files changed since the merge base with origin/main.
npx ts-crap --root . --diff origin/main --threshold 20

# Use previously collected ts-crap coverage and write a browsable report.
npx ts-crap --root . --coverage coverage.json --format html --output report.html
```

The scanner supports `.ts`, `.tsx`, `.mts`, and `.cts` implementation files.
It writes text by default and also supports JSON, XML, HTML, and SARIF. A
valid scan writes its report even when the gate fails. Exit codes are **0**
for a pass, **1** for a gate failure, and **2** for invalid input or an error
running the scan. See the [CLI reference](docs/CLI.md) for Git and PR modes,
`ts-crap.json`, coverage completeness, and report options.

## Gate pull requests

Add this setup step to your GitHub Actions workflow. It pins both the action
commit and the scanner version.

```yaml
- uses: RuiCostafrg/typescript-crap@641bee77459d8bbf9ea981adbcbde2595c3af1b3 # v3.0.0
  with:
    version: 3.0.0
```

The action verifies release provenance and the archive digest before installing.
For an existing codebase, capture and review a baseline, then use
`--baseline` to fail new or worsening violations. See the
[CI adoption guide](docs/CI_ADOPTION.md) for coverage collection, baseline
review, checkout setup, and SARIF upload.

## Performance

On one GitHub-hosted Linux x86-64 runner, the compact-score build that preceded
v3.0.0 scanned the pinned VS Code corpus and wrote a 184 MB JSON report. It
reported 431,594 functions from 15,246 files and flagged 50 files it could
not parse. The v2.6.1 baseline and the compact-score build produced identical
report bytes.

| Linux build | Scan + JSON, median (range) | Peak RSS, v2.6.1 → compact-score build |
| --- | ---: | ---: |
| glibc | 15.2 s (15.2–15.4 s) | 259 → 242 MiB |
| static musl | 19.1 s (19.0–19.2 s) | 213 → 201 MiB |

Three runs per build after warmup with the output initially absent. Peak RSS
figures are medians. The timing ranges overlap with v2.6.1 on both builds.
See the [raw results](docs/measurements/vscode-hosted-compact-score-2026-11-20.json)
and [benchmark instructions](docs/PERFORMANCE.md#real-corpus-benchmark).

## Documentation

- [Versioned JSON schemas](schemas/README.md) and the [compatibility contract](docs/COMPATIBILITY.md).
- [Optional Oximut mutation gate](docs/MUTATION_EVIDENCE.md) for surviving and uncovered mutants.
- [3.0 migration guide](docs/MIGRATING_TO_V3.md) for Rust library users.
- [Changelog](CHANGELOG.md), [contributor guide](CONTRIBUTING.md), and [security policy](SECURITY.md).

Report bugs and feature requests through [GitHub Issues](https://github.com/RuiCostafrg/typescript-crap/issues).
Pull requests are restricted to collaborators. Licensed under [MIT](LICENSE).
