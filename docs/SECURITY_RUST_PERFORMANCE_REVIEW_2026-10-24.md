# Security, Rust practices, and performance review

Reviewed **2026-10-24**, commit **`01d04c062ffe5423471e960a2dd1d0ff5b90c69e`** (v0.3.0). Scope: production Rust, Node coverage helpers, existing tests/benchmarks, Cargo settings, and CI/release workflow definitions. This report proposes changes; production code was not modified.

The most valuable next work is to make Git selection independent of user configuration and verify that diff scans actually score committed content. Both currently have reproducible false passes. Several smaller input/identity issues also remain. Keep the existing single-crate architecture and safety checks.

## Status (checked 2026-11-20, v2.6.1)

This review is kept as a historical record. Its code links and line numbers point at v0.3.0. Eight of the nine findings were fixed in 0.4.0. Finding 9 remains a deliberate deferral.

| ID | Status | Fixed in | Regression test |
| --- | --- | --- | --- |
| 1 | Fixed: the committed diff passes `--no-relative`. The dirty-worktree check is replaced by blob verification (item 2). | #17 (0.4.0) | `tests/git_diff.rs` `relative_diff_config_cannot_hide_nested_sources_or_dirty_edits` |
| 2 | Fixed: selected worktree bytes are compared with their HEAD blobs through one `git cat-file --batch` process (`src/selection/committed.rs`), independent of index hints. Only CRLF/LF differences are allowed. Pipelined in 2.6.0 (#99). | #17 (0.4.0) | `tests/git_diff.rs` (index hints, staged/unstaged, clean filters, CRLF) |
| 3 | Fixed: paths are encoded from components. Backslashes and colons are rejected, and duplicate identities are an input error. | #19 (0.4.0) | `tests/adoption.rs` `unrepresentable_source_paths_cannot_borrow_coverage_or_selection_identity` |
| 4 | Fixed: the fingerprint fallback requires equal stable inferred names. | #19 (0.4.0) | `tests/adoption.rs` `inferred_renames_are_new_but_unchanged_moves_and_anonymous_shifts_match` |
| 5 | Fixed: separate XML attribute escaping (`src/report.rs` `xml_attribute`). | #19 (0.4.0) | `tests/core.rs`, `tests/adoption.rs` XML parser check |
| 6 | Fixed: duplicate keys, including escaped equivalents, are rejected before `JSON.parse` (`scripts/strict-json.mjs`). | #19 (0.4.0) | `tests/strict-json.test.mjs`, `tests/coverage_import.rs` |
| 7 | Fixed: decisions are recomputed from absolute scores on every application. | #19 (0.4.0) | `src/baseline/tests.rs` `baseline_application_is_independent_of_prior_decisions` |
| 8 | Fixed: SARIF is serialized from borrowed typed structs, every format has writer-based `render_to`, and the CLI streams to staged files. | #19 (0.4.0) | `tests/adoption.rs` `streaming_formats_match_wrappers_and_propagate_writer_errors` |
| 9 | Open by decision: there are no configurable byte, parser or subprocess budgets. [ADOPTION.md](ADOPTION.md#resource-budget-decision-for-v1) defers them for the trusted local CLI. Revisit if ts-crap scans untrusted projects as a service. | — | — |

## Priorities, gains, and change size

**P1:** next fix, confirmed gate-integrity failure. **P2:** subsequent improvement cycle. **P3:** opportunistic. These are delivery priorities, not CVSS ratings. The findings do not establish remote execution or privilege escalation; Git flags/configuration and test commands remain under the caller's control.

**S:** up to 1 engineer-day; **M:** 2–3 days; **L:** 4–7 days. Approximate line counts include implementation, focused regression tests, and documentation. Estimates exclude hosted CI time and are not measured implementation costs. Sizes overlap where fixes share code.

| ID | Priority | Area | Proposed change | Expected gain | Size |
| --- | --- | --- | --- | --- | --- |
| 1 | P1 | Security / correctness | Force repository-relative Git diff paths | Prevent changed files disappearing into a passing empty selection | S · 0.5–1 day · 40–90 lines |
| 2 | P1 | Security / correctness | Verify selected worktree bytes against HEAD independently of index hints | Prevent committed violations being replaced by passing local edits | M · 2–3 days · 120–240 lines |
| 3 | P2 | Security / Rust invariants | Preserve unique source-path identities | Prevent two files sharing coverage, baseline keys, and SARIF locations | M · 2–3 days · 80–180 lines |
| 4 | P2 | Baseline correctness | Include inferred names in rename matching policy | Make renamed arrows/expressions obey the documented new-function rule | M · 2–3 days · 80–180 lines |
| 5 | P2 | Reporting / Rust practices | Separate XML character and attribute escaping from HTML | Produce valid XML for all accepted source/path text | S · 0.5–1 day · 40–90 lines |
| 6 | P2 | Coverage integrity | Reject duplicate Istanbul JSON keys | Keep contradictory raw coverage from becoming valid positive evidence | M · 2–3 days · 100–220 lines |
| 7 | P2 | Rust API correctness | Derive baseline decisions from absolute scores on every application | Make repeated baseline comparisons independent of earlier comparisons | S · 0.5–1 day · 30–70 lines |
| 8 | P2 | Rust / performance | Serialize SARIF through borrowed typed structures; add writer-based rendering | Remove intermediate JSON trees and avoid retaining full output strings in CLI publication | M · 2–3 days for SARIF; L · 4–7 for all formats · 150–400 lines |
| 9 | P2 conditional | Availability / performance | Add configurable input and execution budgets | Bound oversized input memory and long parser/subprocess work | L · 4–7 days · 200–450 lines |

Items 1–7 have concrete correctness consequences. Item 8 is an optimization opportunity: no percentage speedup is established. Item 9 is most valuable if scanning large or externally supplied projects becomes a supported service; for the current local trusted-command CLI it can follow the correctness fixes.

## 1. Git configuration can hide changed files — P1 / S

**Code:** [`selection.rs:74`](../src/selection.rs#L74), [`selection.rs:86`](../src/selection.rs#L86), [`selection.rs:200`](../src/selection.rs#L200).

Both Git diff commands inherit `diff.relative`. Their filenames are then joined to the repository top level as though Git always returned repository-relative paths.

**Reproduced:** create a Git project containing `pkg/a.ts`, branch `base`, then commit a change making `f` complexity 3 / CRAP 12. Scanning `--root pkg --diff base` normally exits **1**, with one failing function. After `git config diff.relative true`, the identical scan exits **0**, reports **zero files**, and passes. Git returns `a.ts`; joining it to the repository root produces the wrong path, which is discarded by the `starts_with(root)` check.

**Change:** pass `--no-relative` to both the committed-change and dirty-worktree diff commands. Keep NUL-delimited filenames and the existing root intersection. Git explicitly documents that [`--no-relative` overrides `diff.relative`](https://git-scm.com/docs/git-diff#Documentation/git-diff.txt---relativeltpathgt).

**Acceptance:** the same nested-root selection, paths, exit code, and dirty-file rejection with `diff.relative` true/false, including configured relative prefixes. An empty diff must still pass by default; adding `--fail-on-empty` alone would mask the selection bug rather than fix it.

## 2. A clean Git diff does not prove the scanned bytes match HEAD — P1 / M

**Code:** [`selection.rs:198`](../src/selection.rs#L198), [`lib.rs:178`](../src/lib.rs#L178).

The dirty-file guard trusts `git diff --name-only HEAD`, but analysis reads the worktree separately. Git's `assume-unchanged` bit can suppress checking a modified file; [Git documents this behavior](https://git-scm.com/docs/git-update-index#_using_assume_unchanged_bit).

**Reproduced:** commit the same CRAP-12 function, mark it with `git update-index --assume-unchanged pkg/a.ts`, then replace the worktree contents with `function f(){return 0;}`. `--diff base` exits **0**, scoring the local replacement at **2**, rather than rejecting the edit or scoring the committed violation. No concurrent mutation is needed.

**Change:** validate the exact source snapshot passed to analysis against its HEAD blob, independently of index stat/hint state. Batch object retrieval (for example with `git cat-file --batch`) so verification does not spawn a Git process per file. Define how CRLF and clean/smudge transformations relate to the README's HEAD-match contract; do not silently introduce platform-specific mismatches. Preserve the existing policy that dirty selected sources are input errors. Avoid modifying the user's index flags as part of verification.

**Acceptance:** reject selected modifications under `assume-unchanged` and `skip-worktree`; cover staged and unstaged edits, normal clean checkouts, nested roots, spaces/Unicode, and checkout line-ending behavior. Assert the verified bytes are the bytes analyzed. Ordinary full-project scans must continue to support work in progress.

## 3. Unix backslashes cause source-path identity collisions — P2 / M

**Code:** [`lib.rs:154`](../src/lib.rs#L154), [`lib.rs:173`](../src/lib.rs#L173), [`validation.rs:2`](../src/validation.rs#L2).

Replacing every backslash with `/` conflates a legitimate Unix filename containing a literal backslash with a directory separator. The serialized-path validator rejects backslashes, but scanning applies this lossy transformation before coverage lookup and report generation.

**Reproduced:** create distinct files `src/a.ts` and `src\a.ts`, with identical CRAP-12 source. Supply coverage only for `src/a.ts`. `--require-coverage` exits **0**, reports two functions with the same `src/a.ts` path, and grants both 100% coverage / CRAP 3. There is no evidence entry for the second physical file.

**Change:** encode root-relative paths from `Path::components`, distinguishing actual platform separators from filename characters. Given the existing schema forbids literal backslashes and colons, the smallest compatible policy is to reject unrepresentable selected paths with a clear input error. Validate uniqueness before producing a report. A new escaping scheme would require an explicit schema/version and coordination with both collectors.

**Acceptance:** Unix backslash collisions and colon filenames fail explicitly or round-trip uniquely; ordinary Windows separators and Unicode continue working. Coverage, baseline, selection rules, and SARIF must agree on the same identity. A small private validated path type is reasonable if it enforces this boundary; a generic path framework is unnecessary.

## 4. Renamed arrow functions can inherit an old baseline exemption — P2 / M

**Code:** [`analysis.rs:34`](../src/analysis.rs#L34), [`analysis.rs:237`](../src/analysis.rs#L237), [`baseline.rs:101`](../src/baseline.rs#L101).

Inferred names can come from a variable/property/assignment outside the function AST node. Token hashing begins at the function node, so those names do not participate in its fingerprint. Fingerprint fallback can therefore match renamed arrows even though the README says renamed functions are new identities.

**Reproduced:** write a baseline for `export const oldName = (a:boolean,b:boolean) => {if(a && b)return 1;return 0;};` (CRAP 12). Rename only `oldName` to `newName`. The baseline scan exits **0** and exempts `newName`. The analogous declaration-name change is included in the hashed node and does not have this behavior.

**Change:** define one consistent rule for inferred names during fingerprint fallback. Prefer requiring compatible stable inferred names for named functions while retaining location-independent matching for genuinely anonymous callbacks. Alternatively version fingerprints to include stable inferred names, with an explicit migration for existing baselines. Do not include anonymous line/column labels in a supposedly location-independent identity.

**Acceptance:** renamed arrows, assigned expressions, object properties, and class fields are treated consistently with the documented policy; unchanged moves, trivia changes, and shifted anonymous callbacks still match. Ambiguous matches remain conservative.

## 5. XML escaping misses forbidden Unicode characters — P2 / S

**Code:** [`report.rs:19`](../src/report.rs#L19), [`report.rs:137`](../src/report.rs#L137).

The shared HTML/XML helper replaces controls, but U+FFFE and U+FFFF are not covered by `is_control()`. Both are forbidden by the [XML 1.0 character range](https://www.w3.org/TR/xml/#charsets). Literal whitespace in attributes also undergoes XML attribute-value normalization, so multiline computed names or unusual filenames need an explicit preservation policy.

**Reproduced:** analyze `const obj = { "<U+FFFF>"(){return 1;} };`, where the quoted key contains the actual U+FFFF character. The CLI exits **0** and emits XML that Python's `xml.etree.ElementTree` rejects as not well-formed.

**Change:** use a separate XML attribute encoder, replace forbidden scalar values consistently, and use character references for tab/newline/carriage return where exact attribute values should survive parsing. Preserve HTML's independent escaping semantics. No new rendering dependency is required.

**Acceptance:** a standards-compliant XML parser accepts U+FFFE/U+FFFF and control-character fixtures after replacement; attribute values round-trip permitted whitespace and Unicode; HTML escaping and XSS regression cases remain intact.

## 6. Istanbul import can reintroduce contradictory coverage — P2 / M

**Code:** [`import-coverage.mjs:108`](../scripts/import-coverage.mjs#L108), [`import-coverage.mjs:159`](../scripts/import-coverage.mjs#L159). Compare the duplicate rejection already present in [`coverage.rs:31`](../src/coverage.rs#L31).

The importer parses raw Istanbul evidence using `JSON.parse`, which has already discarded duplicate object members before counters are inspected. The resulting native report passes Rust's stricter duplicate checks because the contradiction is gone.

**Reproduced:** import an otherwise valid record with `"s":{"0":0,"0":1}` for the CRAP-12 fixture's statement. Collection succeeds, publishes a native line hit of 1, and `--require-coverage` passes at CRAP 3. The zero evidence is silently discarded. This is malformed-input integrity hardening, not authentication against hostile test commands, which already run with the user's privileges.

**Change:** reject duplicate decoded keys before materializing Istanbul objects. Use a deliberately chosen duplicate-aware JSON parser or a narrowly scoped, thoroughly tested duplicate-detection pass; a `JSON.parse` reviver cannot recover discarded duplicates. Preserve rejection of escaped-equivalent keys, invalid counters, and unknown/unmappable evidence.

**Acceptance:** reject zero/positive duplicates in either order, duplicate file and statement keys, and equivalent escaped keys; publish no output on failure. Verify real Jest/Vitest Istanbul reports still import. Review dependency cost if introducing a parser rather than maintaining another handwritten parser.

## 7. Baseline application depends on prior mutable gate state — P2 / S

**Code:** [`baseline.rs:99`](../src/baseline.rs#L99), [`baseline.rs:114`](../src/baseline.rs#L114).

`Baseline::apply` starts with `f.failed`, then combines its comparison with that flag. After one baseline clears a failure, a later stricter baseline cannot restore it because the second comparison is ANDed with the already-cleared flag. This affects reusable-library callers; the CLI currently applies only one baseline.

**Reproduced through the library:** compare a current CRAP-12 function first against its CRAP-20 historical version, then against its CRAP-2 version. The first application clears `failed`; the second retains false. Applying only the CRAP-2 baseline to a fresh scan fails. A disposable Cargo project depending on the local library printed `reused_failed=false fresh_failed=true`.

**Change:** derive the absolute violation from `f.crap > report.threshold` for every invocation, then apply the selected historical score. Keep completeness failure independent. This is a small invariant repair; splitting the whole public API into many state types is unnecessary.

**Acceptance:** applying B to a fresh report and applying A then B produce identical decisions; repeat and reorder baselines with matching thresholds, plus error paths and completeness failures. Document whether composition is supported; otherwise explicitly reject reuse rather than silently keeping a stale pass.

## 8. SARIF rendering builds large intermediate JSON trees — P2 / M–L

**Code:** [`report.rs:57`](../src/report.rs#L57), [`report.rs:75`](../src/report.rs#L75), [`cli.rs:133`](../src/cli.rs#L133), [`output.rs:139`](../src/output.rs#L139).

SARIF first creates a vector of `serde_json::Value` results, embeds those into another JSON value, and pretty-serializes the whole document. The CLI retains rendered strings for both report and baseline before staging. JSON rendering also wraps an already allocated output string in `format!` merely to append a newline.

**Change, first increment:** introduce private borrowed `Serialize` structs for the fixed SARIF schema and a sequence serializer/iterator over failing functions. Keep `render(...) -> Result<String>` as a compatibility wrapper. Append newlines to the existing buffer rather than constructing another complete string.

**Change, second increment:** add `render_to<W: std::io::Write>` and stream through `BufWriter` into the existing owned temporary files. [`serde_json::to_writer_pretty`](https://docs.rs/serde_json/latest/serde_json/fn.to_writer_pretty.html) supports this directly. Stage every output successfully before any publication, then retain final policy checks, syncing, and per-file atomic replacement. Writer APIs must not expose a partially rendered stdout report on processing errors without documenting that behavior.

**Gain:** structurally removes full intermediate JSON trees and makes CLI output memory less dependent on the serialized report length. It does not remove the in-memory analyzed `Report`. Fresh measurements below establish current rendering cost, not the improvement an implementation would achieve. Historical measurements in [PERFORMANCE.md](PERFORMANCE.md#rendering-and-publication) remain separate evidence.

**Acceptance:** semantic SARIF equivalence, existing function order/fingerprints/UTF-16 locations, completeness results, existing JSON/XML contracts, injected writer failures, and previous-output preservation. Measure release render time and allocations/RSS at multiple report sizes; report actual results rather than promise a percentage improvement.

## 9. Resource budgets remain an optional availability improvement — P2 conditional / L

**Code:** [`lib.rs:178`](../src/lib.rs#L178), [`coverage.rs:112`](../src/coverage.rs#L112), [`analysis.rs:306`](../src/analysis.rs#L306), [`selection.rs:46`](../src/selection.rs#L46), [`selection.rs:111`](../src/selection.rs#L111), [`coverage.mjs:69`](../scripts/coverage.mjs#L69).

Source/coverage/config/baseline inputs and subprocess output are read without explicit application size limits. Tree-sitter parsing and Git/gh/test subprocesses have no configurable per-invocation deadline. Deep qualified names can have quadratic total output size, even though traversal itself is iterative. These are observable design limits; this review did not run a memory-exhaustion attack or establish a new parser vulnerability. Earlier reports already noted the general concern, and it remains unresolved.

**Change:** add an explicit options structure for maximum per-file/aggregate input bytes, output/function/name budget, parser cancellation, and subprocess timeout/output limits. Enforce reads while consuming bytes rather than trusting a metadata precheck. The pinned Tree-sitter version exposes a [parse progress callback](https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.ParseOptions.html) for cooperative cancellation; this alone does not bound filesystem I/O or analysis after parsing. Kill and reap timed-out children; account for child process trees where supported. Keep trusted test commands explicitly outside a sandbox guarantee.

**Gain:** predictable errors and bounded resource use for configured workloads. No normal-case throughput gain is promised. Default limits need representative-project measurements to avoid breaking legitimate large generated sources or test suites.

**Acceptance:** oversized valid/invalid inputs, over-budget nested names, parser cancellation, sleeping children, excessive subprocess output, cleanup, and preservation of prior reports. Test small deterministic budgets rather than consuming machine-wide memory in ordinary CI.

## Current rendering measurements

Fresh optimized `benches/review.rs` executable, built at the reviewed commit. Each case ran in a fresh process with one warmup and five samples; builds and tests finished before measurements. The fixture is a prescanned report with all functions failing, and rendering preserves its reversed caller order. Medians cover serialization only.

| Functions | JSON median | SARIF median | JSON process peak RSS | SARIF process peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 2,000 | 1.290 ms | 32.046 ms | 17,224 KiB | 46,192 KiB |
| 10,000 | 13.842 ms | 142.950 ms | 57,100 KiB | 201,652 KiB |

At 10,000 functions, JSON output is 3,226,882 bytes and SARIF output is 7,247,304 bytes. Formats produce different schemas and output sizes, so these figures are not an efficiency comparison for equivalent work. Process RSS includes fixture construction, prescan, warmup, and rendering; it is not rendering allocation volume. Do not subtract peaks to estimate memory saved. Scheduling, filesystem caching, and mobile thermal behavior still affect these local samples.

The [measurement artifact](measurements/security-rust-performance-review-2026-10-24.json) records all samples, benchmark executable SHA-256, environment, checks, and the seven reproduction observations. There is no optimized implementation or before/after speedup in this review.

Reproduce by building `cargo bench --locked --offline -j 1 --bench review --no-run`, then running the printed executable with `render-json 2000`, `render-sarif 2000`, `render-json 10000`, and `render-sarif 10000` sequentially. Hash the built executable before comparing runs.

## Delivery order

1. Ship item 1 independently; it is small and directly prevents a false empty pass.
2. Implement item 2 with a deliberate checkout-content contract and Git regression fixtures.
3. Address path identity and baseline correctness (3, 4, 7), and independently land XML/import fixes (5, 6).
4. Measure and implement item 8 in increments; take item 9 forward if large/untrusted scanning is a product requirement.

Do not replace existing protections with faster but weaker checks. Retain forbidden unsafe code, strict Clippy, checked release arithmetic, parser reuse, iterative traversal, per-source coverage validation, duplicate native-JSON rejection, final publication revalidation, pinned Actions, read-only build jobs, and separated privileged publication. Existing deterministic/generated tests and coverage/mutation gates already provide substantial value; adding stricter lints or chasing coverage percentages alone would not have found these failures.

## Validation and limits

Local environment: Rust/Cargo **1.96.0**, Node **26.3.1**, Git **2.54.0**, Android/aarch64 toolchain in proot. Initial tracked worktree was clean. Reproductions used disposable projects and repository-local Git settings; no real repository index/configuration was changed.

Passed during this review:

- `cargo test --locked --offline -j 1`: **94 Rust tests** (17 library, 2 binary, 75 integration), also exercising the bundled Node suites and real Git/Node/CLI behavior.
- `cargo fmt --check`.
- `cargo clippy --locked --offline -j 1 --all-targets -- -D warnings`.
- `python3 -B -m unittest discover -s tests -p 'test_*.py'`: **12 tests**. Printed low mutation scores in this suite are intentional helper fixtures, not a fresh production mutation result.
- Disposable reproductions for items 1–7, including a dependent Rust-library probe for item 7.
- `cargo bench --locked --offline -j 1 --bench review --no-run`; fresh optimized rendering measurements described above.

`cargo-audit` is **not installed**, so this report does not claim a clean current advisory result. Existing audit workflows are present, but their latest hosted results were not checked. The supported-platform/MSRV matrix, full release tests, coverage measurement, mutation run, repository settings, and deployed release provenance were not revalidated. External documentation links above support API/format behavior; findings themselves are grounded in local code and reproductions.
