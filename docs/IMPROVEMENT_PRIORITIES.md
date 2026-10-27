# Post-v1.0.0 improvement priorities

Reviewed 2026-10-27 at `d7c478a735f55931dc7f002f7130df665ed9c71f`.
Scope: Rust analysis/selection/publication, Node coverage collectors/importer,
and their existing tests. At review time production code was unchanged from the
`v1.0.0` tag; intervening commits changed documentation and verification workflows.

## Implementation status

All five items are implemented; the original review and reproductions below
describe the pre-fix behavior at the reviewed commit.

| Item | Resolution | Regression evidence |
| --- | --- | --- |
| 1 | Reject symlinked source ancestors before canonicalization and filtering | [Git selection tests](../tests/git_diff.rs) cover baseline/include false passes, excluded substitutions and root escapes |
| 2 | Remove only Git's output terminator, preserving filename whitespace | The same suite covers space, tab, CR and newline roots, nested projects and empty diffs |
| 3 | Aggregate LCOV by test/source before mapping; retain conservative overlap within tests, then add independent tests | [Importer tests](../tests/coverage_import.rs) include actual LCOV output, order reversal, duplicate records, overflow and generated-source overlap |
| 4 | Reserve both default and explicit config locations even when absent | [CLI tests](../tests/cli.rs) cover reports/baselines, existing/absent configs, equivalent paths and directory aliases |
| 5 | Stream committed blobs through an 8 KiB buffer and a size-limited reader | [Verifier tests](../src/selection/committed/tests.rs) cover chunk boundaries, lone CR, framing, mismatches and process cleanup; [measurements](PERFORMANCE.md#committed-git-blob-comparison) record fresh-process comparisons |

The LCOV correction changes outcomes for accepted inputs. Under the
[compatibility contract](COMPATIBILITY.md), it targets the next major release
(2.0.0), not a 1.x patch/minor. The [changelog](../CHANGELOG.md) explains coverage
recollection and baseline review; this implementation does not publish a release.

## Original review

Four correctness issues were reproduced in disposable projects. The two P1
findings can turn a failing Git-selected scan into a pass under the specific
filesystem conditions below. They concern local gate integrity; these probes
do not establish remote execution or privilege escalation.

P1 means the next correctness work; P2 means the next improvement cycle;
P3 means measure before implementing. S is roughly half to one engineer-day,
M roughly one to three days, including regression tests but excluding hosted
CI waits. These are planning estimates.

| Order | Priority | Improvement | Evidence | Effort |
| --- | --- | --- | --- | --- |
| 1 | P1 | Preserve Git source identity through symlinked parent directories | Confirmed baseline false pass and include-filter empty pass | M |
| 2 | P1 | Preserve whitespace in Git repository-root paths | Confirmed failing scan becomes a passing empty diff | S |
| 3 | P2 | Aggregate independent LCOV test records correctly | Confirmed positive hits become zero; CRAP 3 becomes 12 | M |
| 4 | P2 | Reserve the default config destination even before it exists | Confirmed report output creates an invalid project config | S |
| 5 | P3 | Measure streaming comparison of committed Git blobs | Source-level allocation opportunity; no measured speed/RSS gain | M |

## 1. Preserve Git identity through symlinked parent directories

**Code:** [source checking and canonicalization](../src/selection.rs), function
`checked`, lines 42–52; `diff_files`, lines 111–118;
[selection-rule application](../src/lib.rs), lines 161–172.

`symlink_metadata` checks only the last path component. A regular file reached
through a symlinked parent passes that check. Canonicalization then changes the
Git-reported path before it becomes the report identity and before include/exclude
rules run. Matching committed contents alone does not protect that identity.

**Reproduction:**

1. Commit `b/f.ts` with function `f` at CRAP 20, and capture its baseline at
   threshold 10. Create a `base` branch at that commit.
2. Commit a new `a/f.ts` with function `f` at CRAP 12. A normal
   `--diff base --baseline baseline.json` scan correctly exits 1 for `a/f.ts`.
3. Move the worktree directory `a` aside, replace it with a directory symlink
   pointing to `b`, and set `b/f.ts` to the exact committed bytes of `a/f.ts`.
4. Repeat the scan. It exits **0**, reports **`b/f.ts`**, and suppresses CRAP 12
   against the unrelated historical score 20. Git HEAD and the baseline are
   unchanged.

There is also a baseline-independent variant: `--diff base --include 'a/**'`
goes from exit **1**, one selected file, to exit **0**, **zero selected files**.
Canonicalization has moved the changed source outside its intended include rule.

**Recommended change:** reject symlinked components below the canonical project
root before rule matching, consistently with the existing unsupported-symlink
policy. Alternatively keep the Git-relative identity separate from filesystem
resolution and explicitly validate the relationship. Do not silently collapse
multiple changed paths into one canonical map key.

**Acceptance:** regressions for both false passes, symlinked ancestors,
canonical-path collisions, and paths escaping the root. Preserve clean nested
roots, supported root resolution, Unicode, ordinary moves, and CRLF/LF checkout
equivalence. Unsupported substitutions should exit 2 without publishing reports.
The reproduction was run on POSIX; Windows link behavior needs hosted coverage.

## 2. Preserve whitespace in the repository-root path

**Code:** [Git top-level lookup](../src/selection.rs), `diff_files`, lines 99–100.

`top.trim_end()` removes directory-name whitespace along with Git's output
terminator. If the trimmed path names another existing directory, the code uses
that directory as the Git root and silently filters out the actual changed paths.

**Reproduction:** create sibling directories `repo` and `repo `, where the second
name ends with one space. Initialize Git in `repo `, commit an empty base, then
commit `a.ts` with a CRAP-12 function. At threshold 10:

| Scan | Exit | Files scanned |
| --- | ---: | ---: |
| Full scan of `repo ` | 1 | 1 |
| `--diff base`, while sibling `repo` exists | **0** | **0** |
| `--diff base`, after removing the empty sibling | 2 | No report |

**Recommended change:** preserve directory-name bytes when decoding Git output;
do not use arbitrary whitespace trimming for paths. Remove only the command's
known framing or derive the root through a representation that avoids this
ambiguity. Treat commit-ID trimming separately from filename handling.

**Acceptance:** trailing-space roots with and without a trimmed-name sibling,
nested roots, and POSIX tab/newline names. Retain passing empty diffs; forcing
`--fail-on-empty` would only conceal the selection bug.

## 3. Aggregate independent LCOV records separately from overlapping statements

**Code:** [LCOV parsing and conversion](../scripts/import-coverage.mjs), `lcov`,
lines 131–146, and `convert`, lines 95–103.

The parser ignores `TN:` and conversion applies `Math.min` across every record.
That is conservative for overlapping statement mappings, but loses positive
coverage when independent test records describe the same source line.
LCOV's upstream manual describes aggregate reports and additive execution counts;
see [the upstream LCOV manual](https://github.com/linux-test-project/lcov/blob/master/docs/man/lcov.rst).

**Reproduction:** collect this tracefile for a function whose two owned lines
are 2 and 3 and whose complexity is 3:

```text
TN:test_a
SF:a.ts
DA:2,1
DA:3,1
end_of_record
TN:test_b
SF:a.ts
DA:2,0
DA:3,0
end_of_record
```

The importer succeeds but publishes `{"2":0,"3":0}`. A scan with
`--require-coverage` exits **1** at CRAP **12**. A single combined record with
the positive counts publishes covered lines and exits **0** at CRAP **3**.
The source bytes and source hashes are identical in both cases.

**Recommended change:** define independent-record/test aggregation explicitly,
separately from conservative statement/source-map overlap within a record.
Preserve testcase/record identity while parsing. Do not replace every minimum
with addition: an uncovered statement sharing a mapped source line must remain
conservative. Define duplicate-record handling and checked counter overflow.

**Acceptance:** multiple test names, reversed record order, positive/zero and
all-zero records, duplicate line records, and mapped/unmapped coverage. Keep
existing stale-source, duplicate-JSON, source-map, and fresh-output protections.
Include an aggregate produced by real LCOV tooling in addition to the small fixture.

## 4. Reserve the absent default project-config path

**Code:** [CLI protected inputs](../src/cli.rs), lines 98–115;
[config loading](../src/config.rs), `Rules::load`.

The default config path is added to output protection only when `exists()` is
true. With no existing config, a normal report can be published to the path the
next scan will automatically read as configuration.

**Reproduction:** in a new project containing `export function f(){return 1;}`,
run `ts-crap --root PROJECT --format json --output PROJECT/ts-crap.json`.
It exits **0** and creates a report at the default config path. The next normal
scan exits **2** with `invalid project configuration` because `files_scanned`
and the other report fields are not configuration fields. No existing config
was overwritten in this reproduction.

**Recommended change:** always reserve the resolved default config location,
including when it is absent. The destination planner already handles absent
protected paths; existence should not determine this collision policy.

**Acceptance:** absent and existing configs, both `--output` and
`--write-baseline`, equivalent destination spellings, explicit config paths,
and ordinary report destinations. Reject collisions before publication and
preserve pre-existing bytes. Only the `--output` absent-config case was exercised
in this review; the other cases are proposed regression coverage.

## 5. Measure streaming Git-blob comparison

**Code:** [committed-source verification](../src/selection/committed.rs),
`BlobReader::verify`, lines 45–56.

The worktree source is already held as a `String`, while the complete committed
blob is buffered in a second `Vec`. This is bounded to one file, not the whole
project, and is not a correctness defect. For large files, compare buffered
chunks while preserving CRLF/LF equivalence instead of retaining the whole blob.

**Before implementation:** measure representative large files in fresh processes.
No runtime or RSS improvement was measured here. Implement only if the temporary
blob allocation matters. Tests must cover CRLF split across chunk boundaries,
lone CR bytes, size/content mismatches, protocol terminators, and child cleanup.
Keep the single batch Git process and preserve comparison of the exact source
buffer subsequently passed to analysis.

## Delivery and compatibility

Address the two Git-selection failures first, each with its own focused
regressions. Follow with LCOV aggregation and config-output protection. The
streaming optimization can wait for measurements. Configurable resource budgets
remain conditional on the already documented trusted-local-CLI scope; these
probes do not establish a need for a workspace split, caching layer, or service.

Review outcome-changing fixes against the [1.x contract](COMPATIBILITY.md)
before choosing their release version, particularly LCOV changes affecting
accepted coverage and baseline scores. Publish through a new reviewed release
with explicit release notes; keep v1.0.0 immutable. Preserve the 95% coverage
floor, 90% mutation floor, source freshness, committed-byte checks, and atomic
publication protections.

## Original review validation record and limits

- All four correctness reproductions passed their assertions twice, including
  normal-behavior controls and the baseline-independent symlink variant.
- Environment: Node 26.3.1, Git 2.54.0, Android/aarch64 with Ubuntu/proot.
  The scanner was the retained checksum-verified v1.0.0 candidate executable;
  its SHA-256 is `5f0d5e49b79a982edd1fdcd85a7934b740eaafbb5ae86f77cbfd377aac8f6556`.
  `git diff v1.0.0 -- src scripts Cargo.toml Cargo.lock` was empty.
- The local reproducer and raw output are under `target/review-priorities/`:
  `probe_review.py` and `results.json`. Rerun with
  `python3 target/review-priorities/probe_review.py` while those local artifacts
  remain available. The scenarios above describe the inputs independently of
  those disposable artifacts.
- The initial review did not change production implementation. Full Rust tests, hosted platform
  checks, fresh coverage/mutation measurements, and an advisory audit were not
  rerun for this review. No new performance improvement is claimed.
