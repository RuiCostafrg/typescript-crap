# CLI reference

Run `ts-crap --help` for the full list of flags. See [the README](../README.md) for installation and a quick start.

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

### Finding and navigating failures

```sh
ts-crap --root . --only-failing --sort crap --top 20
ts-crap --root . --format html --sort crap --output report.html
```

Text and HTML reports support `--only-failing`, descending `--sort crap` (ties
use path/line/column), and positive `--top N`. Filtering happens before sorting
and limiting. Default ordering remains path/line/column. The full selection
always determines gate exits, completeness counts, totals and written baselines;
rows hidden from view cannot hide a failure. JSON/XML/SARIF keep their complete
schemas and reject presentation options that would filter, reorder or limit them.

Standalone HTML includes a path/function search and an “Only failing” checkbox.
Search is literal and case-insensitive; both controls combine. All rows remain
readable with JavaScript disabled. Syntax errors include a one-based line and
UTF-8 byte column plus a bounded source-line snippet, with terminal controls
escaped in CLI diagnostics.

### Exit codes

- **0:** all analyzed functions pass (also for an empty selection).
- **1:** at least one score is strictly greater than the threshold, or a
  function fails the mutation gate (`--mutation-report`).
- **2:** invalid arguments, invalid TypeScript, missing/stale inputs or coverage,
  Git/gh errors, or report I/O errors.

By default a selected file that does not parse aborts the whole scan (exit 2).
`--on-syntax-error report` (ts-crap 2.4.0 or later) scores every other file
instead and lists each unparsable file at its first syntax error. It appears as
`syntax_errors` in JSON, a `Syntax errors` section in text and HTML, a
`<syntax-error>` element in XML, and a located `TSCRAP002` result in SARIF. Any
unparsable file fails the completeness gate (exit 1) whatever the coverage
policy, and `--write-baseline` is refused until every selected file parses.
Reports for scans without syntax errors are byte-identical in both modes.

The default text report explicitly identifies empty selections. JSON/XML/HTML
include file/function counts or records, so an empty result is distinguishable
from a nonempty passing scan. Thresholds must be finite and nonnegative.

### File and diff selection

- Parses `.ts`, `.tsx`, `.mts`, and `.cts` using Tree-sitter—not regular
  expressions. Excludes `.d.ts`, `.d.mts`, and `.d.cts`.
- Directory scans honor `.gitignore`/`.ignore`, skip hidden entries and symbolic
  links, and skip directories named `node_modules`, `.git`, `target`, `dist`,
  `build` or `coverage` at any depth. `excluded_directories` in `ts-crap.json`
  replaces that list, for example to scan a real `src/build/` directory.
  An explicitly named supported source file bypasses ignore rules but cannot
  bypass the excluded directories. Missing explicit files are errors.
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
  Git selection ignores `diff.relative`; committed object IDs pin the source
  comparison independently of `assume-unchanged` and `skip-worktree` flags.
  CRLF/LF differences are allowed; all other bytes must match the HEAD blob,
  including when clean/smudge filters would hide a difference from `git diff`.
  Parsing and coverage hashes use the actual worktree bytes. Verification does
  not run filters or modify index flags, and uses one Git batch process per scan.
- PR mode obtains base/head OIDs with `gh pr view`, verifies local HEAD is the
  PR head, then uses the same merge-base selection. Both objects must be locally
  available: fetch/checkout the PR yourself. The tool never fetches, changes
  branches, creates a PR, or uploads reports.
- `--diff` and `--pr` are mutually exclusive. A shallow checkout missing a base
  commit is an error; fetch sufficient history (e.g. `fetch-depth: 0` in CI).
- Selections of 32 files or more are parsed and scored on up to `--jobs N`
  worker threads (default: the available cores, at most 4). Files are still read
  and verified against Git in order on one thread. Reports, baselines, exit
  codes and the reported error (always the first failing file in path order) do
  not depend on `N`. `--jobs 1` analyzes on one thread.

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

## Native coverage schema (v1)

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

Paths must be normalized root-relative slash paths. Source paths containing literal
backslashes or colons cannot be represented by schema v1 and are rejected by the
scanner and collectors; actual Windows directory separators are encoded as `/`.
Line keys are canonical,
positive one-based decimal integers; hits are nonnegative integers. Duplicate file
paths or line keys are errors, including equivalent JSON-escaped keys. Unknown
fields, invalid hashes/versions/counts, out-of-range lines in selected sources, and
selected source/hash mismatches are errors. Missing files/line hits mean 0% coverage.
Unselected coverage entries may refer to files outside the scan; their source
hashes are not validated because those files are not analyzed. Hashes ensure
freshness, not trust: use coverage generated by your own test pipeline.

## Project configuration

`ts-crap.json` in the project root configures selection and gate policy. Unknown
fields and unknown versions are errors. Globs use forward slashes relative to
`--root`; `*` stops at directory boundaries and `**` matches
across directories. Includes default to all supported sources and excludes default
to empty. Built-in generated/dependency exclusions and ignore rules still apply.
Explicit paths and Git selection intersect these rules. Excludes always win.
`--config FILE` selects a config relative to the caller; repeated `--include` or
`--exclude` flags replace the corresponding configured list. No environment or
ancestor config is loaded, so results do not depend on the caller's directory.

```json
{
  "version": 1,
  "include": ["packages/**/src/**/*.ts", "packages/**/src/**/*.tsx"],
  "exclude": ["**/*.test.ts", "**/*.test.tsx"],
  "threshold": 10,
  "require_coverage": true,
  "fail_on_empty": true
}
```

`excluded_directories` (ts-crap 2.4.0 or later) replaces the built-in excluded
directory names. Each entry is one non-empty directory name, matched at any depth. Path
separators (`/`, `\`), NUL characters, `.`, `..` and duplicates are invalid;
`[]` excludes nothing. Omitting it keeps the
defaults `["node_modules", ".git", "target", "dist", "build", "coverage"]`. Keep
`node_modules` and `.git` when you replace the list. The coverage helpers read
this key from the same file (`ROOT/ts-crap.json` or `--config`), so they skip
the same directory names as the scanner. They do not apply `.gitignore` or skip
hidden entries, so they may record extra files the scanner never selects; those
records are unused. Older scanners reject the key as unknown.

```json
{
  "version": 1,
  "include": ["src/**"],
  "excluded_directories": ["node_modules", ".git", "target", "dist", "coverage"]
}
```

Gate settings resolve independently: an explicit CLI value overrides the project
config, which overrides the built-in defaults (`threshold: 10`,
`require_coverage: false`, `fail_on_empty: false`). A threshold must be finite and
nonnegative; zero is valid. Boolean settings must be JSON booleans. Nulls,
duplicate fields, and invalid gate settings are errors even when overridden by
the CLI. Existing selection-only configs keep their previous behavior.

Bare boolean flags enable their gates. Use `=false` to disable a configured gate,
or `=true` to enable it explicitly. Boolean values require `=` so a following path
is never consumed as a value:

```sh
ts-crap --coverage imported-coverage.json
ts-crap --threshold 20 --require-coverage=false --fail-on-empty=false
```

The resolved threshold appears in reports and baselines. Baseline comparison
still requires matching thresholds, and a baseline cannot suppress a configured
completeness failure. In the Rust API, `ScanOptions.threshold`,
`require_coverage`, and `fail_on_empty` are `Option` overrides: `None` inherits
policy; `Some(value)` explicitly overrides it, including `Some(false)`.

`--require-coverage` fails when any selected file has no coverage entry or any
function-owned statement start has no line entry. A recorded zero is evidence of
an uncovered line; it still affects CRAP normally. Reports count files and
functions missing evidence. `--fail-on-empty` fails selections with no implemented
functions. Both are opt-in through config or flags; empty diffs keep passing by
default. Completeness failures remain failures when using a regression baseline.

### Regression baselines

```sh
ts-crap --root . --write-baseline crap-baseline.json # absolute gate exit still applies
ts-crap --root . --baseline crap-baseline.json
# A function edited while moving files needs an explicit mapping:
ts-crap --baseline crap-baseline.json --baseline-rename src/old.ts=src/new.ts
```

Review and commit the version-1 baseline. It stores absolute scores, threshold,
paths, qualified names and SHA256 token identities, without source text. Valid
hexadecimal identities are normalized to lowercase when loaded, so letter case
does not affect matching. The threshold must match. Only scores strictly above
the threshold and strictly above the matched baseline score fail. New violations
fail; removed/improved violations pass. Absolute gates remain the default. Baseline creation never changes gate
exit codes or silently updates an existing baseline during comparison.

A unique path plus qualified function name matches edits. Token fingerprints
ignore comments, formatting and separately scored descendant functions and match unique unchanged functions across file
moves, including anonymous functions whose locations shifted. Renamed functions,
including inferred variable/property/field names,
are new identities; edited moves require the explicit mapping above. Ambiguous
matches fail conservatively as new violations. Duplicate names should be given
unique scopes before relying on name-based matching. Prefer full-project baselines;
a diff-generated baseline only records the selected functions. Baselines are
reviewed policy inputs, not authenticated proof of past code quality.

## SARIF

`--format sarif --output crap.sarif` emits SARIF 2.1.0 with stable `TSCRAP001`
(score), `TSCRAP002` (completeness), and — with `--mutation-report` —
`TSCRAP003` (surviving or uncovered mutants) rule IDs, relative
percent-encoded URIs and function fingerprints. Upload it with your
repository's SARIF integration; output does not change gate exit codes. Text
and error diagnostics escape controls and bidirectional formatting
characters; JSON retains raw values for machine readers.
