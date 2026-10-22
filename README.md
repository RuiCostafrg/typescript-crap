<img src="assets/icon.svg" width="64" height="64" alt="">

# ts-crap

A Rust command-line quality gate for **TypeScript function CRAP scores**.
Scans a project, selected files/directories, or files changed in a Git/GitHub PR diff.
Reports each function's relative path, one-based start line/column, end line,
cyclomatic complexity, coverage, CRAP score, and gate result.

## Install

Prebuilt releases require **no Rust toolchain, C compiler, or Node.js** to scan
TypeScript. Available for macOS (Apple Silicon / Intel), Linux with glibc 2.35+
(ARM64 / x86-64), and Windows (x86-64).

### macOS and Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/RuiCostafrg/typescript-crap/releases/latest/download/ts-crap-installer.sh | sh
```

The shell installer uses `curl` (or `wget`) and `tar`; Linux also needs `xz`
(usually provided by the `xz-utils` package). It detects your platform and
installs to `~/.local/bin`. Follow its
PATH instructions, or open a new terminal, then run `ts-crap --help`.
Run the same installer again to update to the latest release.

### Homebrew (macOS and Linux)

```sh
brew install RuiCostafrg/tap/ts-crap
# Upgrade later:
brew update && brew upgrade ts-crap
```

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/RuiCostafrg/typescript-crap/releases/latest/download/ts-crap-installer.ps1 | iex"
```

### Manual download

Download the archive for your OS and CPU from [GitHub Releases](https://github.com/RuiCostafrg/typescript-crap/releases/latest),
verify its SHA-256 using the accompanying `.sha256` file, extract it, and place
`ts-crap` (or `ts-crap.exe`) in a directory on your PATH. Unix archives use
`.tar.xz`; Windows archives use `.zip`. Linux binaries target glibc distributions;
Alpine/musl, Android/Termux, and Windows ARM64 do not have native release builds.

Git is needed for diff mode, and authenticated `gh` for GitHub PR discovery.
The optional coverage collector lives in this repository under `scripts/` and
requires Node 24+; the prebuilt CLI itself does not.

## Build from source

Requires Rust 1.90+ and a C compiler (for the Tree-sitter parser). Node is **not**
required to analyze code; Node 24+ is required for the bundled coverage collector
and the full end-to-end test suite. Git is needed for diff mode, and authenticated
`gh` for GitHub PR discovery.

```sh
cargo build --locked -j 1
./target/debug/ts-crap --help
# Or install the CLI on your PATH:
cargo install --locked --path . -j 1
```

**Termux / Ubuntu proot:** your existing Termux Cargo/Rust/Clang can be used from
proot when they are on PATH. Single-job builds (`-j 1`) worked in the development
environment after the linker failed during a parallel build. No second toolchain
is required. A Termux-built executable targets Android; build with a native Linux
toolchain instead when distributing to ordinary Linux machines.

## Usage

```sh
# Entire project, threshold defaults to 10
ts-crap --root /path/to/project

# Several files and directories, deduplicated
ts-crap --root /path/to/project src/a.ts src/b.ts src/components

# A score strictly ABOVE 20 fails; a score equal to 20 passes
ts-crap --root /path/to/project --threshold 20

# Files changed between merge-base(origin/main, HEAD) and HEAD
ts-crap --root /path/to/project --diff origin/main

# Current PR, or a specific PR (already checked out locally)
ts-crap --root /path/to/project --pr
ts-crap --root /path/to/project --pr=42
ts-crap --root /path/to/project --pr=https://github.com/owner/repo/pull/42

# Restrict diff selection further
ts-crap --root /path/to/project src --diff origin/main

# Optional machine-readable or standalone HTML reports
ts-crap --root /path/to/project --format json --output report.json
ts-crap --root /path/to/project --format xml --output report.xml
ts-crap --root /path/to/project --format html --output report.html
```

Positional source paths are relative to `--root` (absolute paths inside that root
also work). Coverage and output paths are relative to the **calling directory**.
Without `--output`, the selected report is written to stdout. Output parent
directories must already exist. Reports are still emitted when the quality gate
fails; input/processing errors instead produce stderr and no new report.
Do not reuse an existing report after a failed command—check its exit code.

### Exit codes

- **0:** all analyzed functions pass (also for an empty selection).
- **1:** at least one score is strictly greater than the threshold.
- **2:** invalid arguments, invalid TypeScript, missing/stale inputs or coverage,
  Git/gh errors, or report I/O errors.

The default text report explicitly identifies empty selections. JSON/XML/HTML
include file/function counts or records, so an empty result is distinguishable
from a nonempty passing scan. Thresholds must be finite and nonnegative.

### File and diff selection

- Parses `.ts`, `.tsx`, `.mts`, and `.cts` using Tree-sitter—not regular
  expressions. Excludes `.d.ts`, `.d.mts`, and `.d.cts`.
- Directory scans honor `.gitignore`/`.ignore`, skip hidden entries and symbolic
  links, and skip `node_modules`, `.git`, `target`, `dist`, `build`, `coverage`.
  An explicitly named supported source file bypasses ignore rules but cannot
  bypass those excluded directories. Missing explicit files are errors.
- Diff/PR mode takes tracked changes directly from Git, so tracked ignored/hidden
  files cannot silently disappear from the gate (generated-directory exclusions
  still apply).
- Diff mode scores **all functions in changed files**, not only changed functions
  or changed lines. Added, modified, copied, renamed, and type-changed sources are
  included; committed deletions are not scored. No changes is a successful empty scan.
- Diff mode is **committed changes only**. Selected files must match HEAD:
  uncommitted selected-file edits or missing changed files are errors. Unrelated
  untracked files are not part of a diff scan. Use ordinary file/project mode to
  scan work-in-progress code.
- PR mode obtains base/head OIDs with `gh pr view`, verifies local HEAD is the
  PR head, then uses the same merge-base selection. Both objects must be locally
  available: fetch/checkout the PR yourself. The tool never fetches, changes
  branches, creates a PR, or uploads reports.
- `--diff` and `--pr` are mutually exclusive. A shallow checkout missing a base
  commit is an error; fetch sufficient history (e.g. `fetch-depth: 0` in CI).

### Report output safety

`--output` paths are relative to the caller's current directory. Reports are
written to a securely created sibling temporary file, synced, then atomically
replaced at the destination. A failed write preserves the previous report.
TypeScript destinations (`.ts`, `.tsx`, `.mts`, `.cts`, including declarations
and case variants), symbolic links, directories, coverage-input aliases, and
hardlinks to TypeScript files anywhere under the project root are rejected.
The alias check includes hidden, ignored and unselected sources; directory
symlinks are not followed. Concurrent project mutation is not supported.

Analysis uses iterative tree traversal so deeply nested valid expressions do
not consume the application's call stack.

## Scoring contract

```text
CRAP = CC² × (1 − coverage)³ + CC
```

Coverage is a fraction in `[0, 1]`. **Missing coverage is 0%, never 100%.**
For example, complexity 4 scores **20** uncovered, **6** at 50% coverage,
and **4** fully covered. Decisions about failure use the unrounded score.

Each implemented function starts at complexity **1**. Add one for each:

- `if` (including each `else if`), `for`/`for…in`/`for…of`, `while`, `do…while`;
- `catch`, non-default `case`, and ternary expression;
- short-circuit `&&`, `||`, `??`, `&&=`, `||=`, `??=`.

Plain `else`, `default`, `try`, `finally`, optional chaining and type-only syntax
do not add complexity in this tool's convention. Comments and strings never count.

Includes declarations/expressions, arrows, generators, constructors, getters,
setters, class/object methods, and implemented overloads. Declaration-only
signatures are not scored. Nested functions are separate records and their
decisions are excluded from the enclosing function. Names are inferred where
possible; anonymous callbacks use `<anonymous@line:column>`. Class and nested
function names carry scope qualifiers; locations disambiguate duplicate names.
Columns are one-based **UTF-8 byte columns**, matching Tree-sitter.

Coverage is measured over the unique **executable statement-start lines owned by
the function**, excluding nested function bodies. Default parameter/destructuring
initializers also own their expression's starting line. Expression-bodied arrows use
their expression's starting line. Missing hits are uncovered, and empty functions
conservatively get 0% coverage. This is a documented line-based approximation:
multiple statements/functions on one line share line evidence, so prefer normal
multiline formatting. It is not branch or function-invocation coverage.

## Real coverage from TypeScript tests

The optional Node helper runs your actual tests with V8 coverage and emits the
source-hashed JSON consumed by the Rust binary. It snapshots source **before**
running tests; changed source is rejected by Rust instead of using stale coverage.
A Node loader hook also checks the source returned by the loader against that
snapshot, rejecting changes made before import—even if restored later. This is an
integrity check for trusted test commands, not a sandbox against hostile tests.

```sh
mkdir -p reports
node scripts/coverage.mjs \
  --root examples/shop --output reports/shop-coverage.json -- \
  node --experimental-strip-types test.mjs

./target/debug/ts-crap --root examples/shop \
  --coverage reports/shop-coverage.json --format html --output reports/shop.html
```

The child command runs in the specified root. Use raw TypeScript supported by
Node's built-in stripping (or `.mts` as in the example); **there is no transpiler
or source-map remapping**. The Rust parser supports TSX, but Node's built-in type
stripper cannot execute JSX or every TypeScript construct. For transpiled TS/TSX
projects, supply source-mapped coverage in the native schema using an adapter;
Istanbul/LCOV inputs use the source-snapshot collection adapter described below.

V8 offsets are mapped as UTF-16 code units (including Unicode). If an uncovered
branch occupies part of a line, that line is conservatively uncovered. Hits from
multiple Node processes are combined. Sources not loaded by tests have no hits.
A failing test command propagates its exit code and publishes no new coverage
report (any pre-existing report remains unchanged).

### Native coverage schema (v1)

```json
{
  "version": 1,
  "files": {
    "src/example.ts": {
      "sha256": "<64 hex characters: SHA-256 of the exact source bytes>",
      "lines": { "2": 1, "3": 0, "4": 3 }
    }
  }
}
```

Paths must be normalized root-relative slash paths. Line keys are canonical,
positive one-based decimal integers; hits are nonnegative integers. Unknown fields,
invalid hashes/versions/counts, out-of-range lines in selected sources, and selected
source/hash mismatches are errors. Missing files/line hits mean 0% coverage.
Unselected coverage entries may refer to files outside the scan; their source
hashes are not validated because those files are not analyzed. Hashes ensure
freshness, not trust: use coverage generated by your own test pipeline.

## Rust development

The crate now exposes a reusable library via `ts_crap::scan` / `ts_crap::render`,
with a thin CLI entry point. See **[CONTRIBUTING.md](CONTRIBUTING.md)** for folder
conventions, enforced safety/lint rules, release settings, benchmarks and quality
commands. These rules reduce risk; they are not proof of maximum performance or
of safety in third-party/native dependencies.

## Reproducible example and tests

The real shop TypeScript source is in `examples/shop/src/pricing.mts`; its
assertion-based tests are in `examples/shop/test.mjs`.

```sh
# Expected exit 1: discount scores 12 and shipping scores 20 without coverage
./target/debug/ts-crap --root examples/shop

# Collect real coverage using the command above, then expected exit 0:
./target/debug/ts-crap --root examples/shop --coverage reports/shop-coverage.json
# Fully covered scores: discount=3, shipping=4, total=1

cargo test --locked -j 1
cargo fmt --check
cargo clippy --locked -j 1 --all-targets -- -D warnings
```

Tests invoke the compiled binary on actual TypeScript fixtures and temporary
projects, execute real TypeScript through Node/V8, and use real local Git repos.
They cover known scores, partial/full/missing coverage, stale source hashes,
threshold boundaries, passing/failing exits, multiple files, ignores, TSX,
nested functions/methods, syntax errors, missing files, empty scans, Git merge-base,
renames/deletions, relative paths, and JSON/XML/HTML escaping/report I/O.
Only the external GitHub lookup is stubbed at the process boundary; no live PR
or network access is needed for the tests.

GitHub Actions runs Rust 1.90/stable debug/release tests, formatting, strict
Clippy, rustdoc, Node/helper checks, dependency auditing, and workflow linting.
Separate workflows produce Rust coverage artifacts (LCOV/JSON/HTML) and run
mutation tests with downloadable diffs/logs. See CONTRIBUTING.md for triggers,
artifact names, safety settings, and the distinction from TypeScript coverage.

## Contributions and security

This project is maintained by RuiCostafrg. Please use issues for bugs and feature
requests; pull requests are restricted to collaborators. See [SECURITY.md](SECURITY.md)
for private vulnerability reporting, release protections and download verification.
The original v0.1.0 predates artifact attestations; future hardened releases include
provenance. Public source and forks remain available under MIT.

## Adoption gates (v0.2)

`ts-crap.json` in the project root configures selection. Unknown fields and unknown
versions are errors. Globs use forward slashes relative to `--root`; `*` stops at directory boundaries and `**` matches
across directories. Includes default to all supported sources and excludes default
to empty. Built-in generated/dependency exclusions and ignore rules still apply.
Explicit paths and Git selection intersect these rules. Excludes always win.
`--config FILE` selects a config relative to the caller; repeated `--include` or
`--exclude` flags replace the corresponding configured list. No environment or
ancestor config is loaded, so results do not depend on the caller's directory.

```json
{"version":1,"include":["packages/**/src/**/*.ts"],"exclude":["**/*.test.ts"]}
```

`--require-coverage` fails when any selected file has no coverage entry or any
function-owned statement start has no line entry. A recorded zero is evidence of
an uncovered line; it still affects CRAP normally. Reports count files and
functions missing evidence. `--fail-on-empty` fails selections with no implemented
functions. Both are opt-in; empty diffs keep passing by default. Completeness
failures remain failures when using a regression baseline.

### Regression baselines

```sh
ts-crap --root . --write-baseline crap-baseline.json # absolute gate exit still applies
ts-crap --root . --baseline crap-baseline.json
# A function edited while moving files needs an explicit mapping:
ts-crap --baseline crap-baseline.json --baseline-rename src/old.ts=src/new.ts
```

Review and commit the version-1 baseline. It stores absolute scores, threshold,
paths, qualified names and SHA256 token identities, without source text. The
threshold must match. Only scores strictly above the threshold and strictly above
the matched baseline score fail. New violations fail; removed/improved violations
pass. Absolute gates remain the default. Baseline creation never changes gate
exit codes or silently updates an existing baseline during comparison.

A unique path plus qualified function name matches edits. Token fingerprints
ignore comments, formatting and separately scored descendant functions and match unique unchanged functions across file
moves, including anonymous functions whose locations shifted. Renamed functions
are new identities; edited moves require the explicit mapping above. Ambiguous
matches fail conservatively as new violations. Duplicate names should be given
unique scopes before relying on name-based matching. Prefer full-project baselines;
a diff-generated baseline only records the selected functions. Baselines are
reviewed policy inputs, not authenticated proof of past code quality.

### Istanbul and LCOV collection

Jest/nyc `coverage-final.json`, Vitest's Istanbul reporter, and LCOV line records
can be collected through the dependency-free Node adapter. Use fresh input/output
paths (the adapter rejects existing destinations). Paths passed on the command
line are relative to the caller; the test command runs inside `--root`.

```sh
node scripts/import-coverage.mjs --root . --format istanbul \
  --input coverage/coverage-final.json --output imported-coverage.json -- \
  npx jest --coverage --coverageReporters=json
node scripts/import-coverage.mjs --root . --format lcov \
  --input coverage/lcov.info --output imported-coverage.json -- \
  npx vitest run --coverage --coverage.reporter=lcov
ts-crap --coverage imported-coverage.json --require-coverage
```

Source hashes are captured **before** running the trusted test command and verified
after collection. Imports never invent freshness by hashing only when importing an
old file. A failing test command, changed source, empty report, invalid counters,
unmappable location, or source outside the collected TypeScript snapshot rejects
the entire import. Missing file records remain missing coverage evidence.

Already remapped TypeScript paths work directly. Generated JavaScript uses
Istanbul's `inputSourceMap`, or its final `sourceMappingURL` (external JSON or inline
base64). Flat source maps v3, relative paths, `sourceRoot`, and file URLs are
supported. Maps must embed `sourcesContent` matching the pre-test snapshot;
URL schemes such as webpack virtual paths and indexed maps are rejected with an
error. Configure the reporter to remap these to original TypeScript instead.
Statement starts map using their UTF-16 generated column; LCOV maps line segments.
Uncovered overlapping statements/segments keep a line uncovered. This conservative
line approximation does not claim branch coverage. Existing native V8 collection
remains available for Node's direct TypeScript execution.

### Review output

`--format sarif --output crap.sarif` emits SARIF 2.1.0 with stable `TSCRAP001`
(score) and `TSCRAP002` (completeness) rule IDs, relative percent-encoded URIs and
function fingerprints. Upload it with your repository's SARIF integration; output
does not change gate exit codes. Text and error diagnostics escape controls and
bidirectional formatting characters; JSON retains raw values for machine readers.
