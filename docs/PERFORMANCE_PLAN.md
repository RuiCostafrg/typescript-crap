# Performance plan: faster scans, lower CPU and memory, faster installs

Written **2026-11-12** against `9907f22` (v2.4.0). Status: **investigation
complete, nothing implemented**. Raw numbers are in
[the measurements file](measurements/vscode-profile-2026-11-12.json).

This plan supersedes the sentence in [PERFORMANCE.md](PERFORMANCE.md) that
"parallelism and caching remain deferred because these measurements do not
justify their added complexity". That decision was based on synthetic fixtures
of at most 300 small files. The first measurement on a large real project is
below and it changes the picture.

## What was measured

A full scan of `microsoft/vscode` at `f42f0a11` (11,473 selected files, 186 MB
of TypeScript, 370,009 functions, 158 MB JSON report), with benchmark-only
timing spans around each stage.

| Stage | Time | Share | What it does |
| --- | ---: | ---: | --- |
| Selection | 57–101 s | ~45% | 50 s or more is `checked()`: per walked file, one `lstat` per ancestor directory plus `canonicalize` |
| Read sources | 10–13 s | ~8% | `read_to_string` per file |
| Parse | 25–31 s | ~20% | tree-sitter, one thread |
| Collect functions | 20–24 s | ~15% | three separate AST walks per function; `kind()` string compares |
| Output preflight | 19 s per walk | ~12% | only when the destination already exists: walks the whole root and opens every TypeScript file |
| Render + publish | under 1 s | — | plus one more preflight walk per destination |
| Everything else | under 0.5 s | — | coverage hashing, scoring, sorting |

Total: **152–162 s wall, 63–64 s user CPU, 199 MB peak RSS.**

**Read these numbers with care.** The machine is an aarch64 phone running
Ubuntu under proot, which intercepts every syscall. Filesystem stages are
inflated compared with a native Linux runner, and run-to-run noise is 20–40%.
On native Linux, parsing and collection will be the largest share and the
filesystem stages will be smaller, but slow-filesystem environments are real
(Windows with antivirus, network filesystems, containers on macOS). Nothing
here was measured on x86-64, on a hosted runner, or on the static musl build
that Linux users actually install. WP0 closes that gap before any claim is
published.

## Prototype results

Four changes were prototyped in a scratch copy outside the repository and
toggled at run time. **Every variant produced a byte-identical report**
(same SHA-256 as the unmodified build).

| Variant (cores 4–7 pinned, output absent) | Wall | User CPU | Peak RSS |
| --- | ---: | ---: | ---: |
| Baseline | 152 s | 63 s | 199 MB |
| Kind-id tables only | 159 s | 62 s | 199 MB |
| Tables + selection cache + single-`lstat` preflight | **68 s** | **49 s** | 199 MB |
| The above + 2 worker threads | 55 s | 69 s | 320 MB |
| The above + 4 worker threads | **42 s** | 78 s | 397 MB |

- **Selection cache:** `checked` fell from 50 s to 1.6 s; selection overall
  from 57–101 s to 9–11 s.
- **Single `lstat` preflight:** 19 s to 10 s per walk (measured unpinned, with
  an existing output file).
- **Kind-id tables:** collection fell from 20–24 s to 14–15 s. Whole-process
  user CPU is too noisy on this device to show it reliably (46 s in one run,
  62 s in another).
- **Threads:** gains flatten after 2–4 workers here. The phone has four slow
  cores, and eight workers used 60% more CPU for no wall-time gain. The naive
  prototype doubled peak RSS, which is why WP4 specifies a bounded design.

## Constraints every change must keep

1. **No output byte changes.** Function identities are SHA-256 values stored
   in users' baselines. Scores, ordering, names, columns and error messages
   stay the same.
2. **No weaker gate or publication check.** Symlink rejection, root
   containment, HEAD-blob verification, coverage hash binding, destination
   alias checks and the per-rename revalidation all stay.
3. **No public API break in 2.x.** `FunctionScore` field types are public and
   [COMPATIBILITY.md](COMPATIBILITY.md) requires a major release to change them.
4. **Existing policy stays:** `unsafe_code = "forbid"`, the Clippy
   panic/unwrap/expect denials, `overflow-checks = true`, unwinding panics.
5. **Measure on the shipped target.** Accept a change on paired measurements
   from a hosted x86-64 runner, glibc and static musl, not on phone timings.

## Work packages

Each package is one pull request. Ownership follows the usual split: **Opus
5.5 implements, Sonnet 5.5 reviews independently, Haiku 5.5 does mechanical
sweeps**. Packages marked "parallel" can run at the same time in separate
worktrees.

### WP0 — Safety net and trustworthy measurement (do first)

Nothing pins function identities today: no test or fixture contains a
committed identity value, and existing identity tests only compare two runs
with each other.

- **Golden identities.** Commit a TypeScript and a TSX fixture covering every
  construct the analyzer names or counts, with pinned identity hex, complexity,
  owned lines, names and columns.
- **Kind consistency test.** Assert `node_kind_for_id(node.kind_id()) ==
  node.kind()` for every node of both grammars, including trees with ERROR and
  MISSING nodes.
- **Guard compares output.** `scripts/check-performance.py` records
  `output_sha256` but never compares baseline with candidate. Compare it for
  every scanner workload.
- **Guard workloads that are missing today:** several hundred files of mixed
  size including multi-MiB files (so RSS grows with concurrency); an existing
  `--output` destination (so the preflight walk is measured); a deep directory
  tree (selection); a `--baseline` run.
- **Real-corpus benchmark.** An opt-in script that checks out a pinned VS Code
  commit and reports stage times, CPU and RSS. Run it on a hosted runner for
  x86-64 glibc and static musl. Record the results in `docs/measurements/`.
- **Models:** Opus implements; Haiku writes the fixture inventory (one case per
  construct in `analysis.rs`); Sonnet reviews that the goldens fail when a hash
  input is perturbed.
- **Done when:** the goldens pass on `main` unchanged and a deliberately
  altered hash input fails them.

### WP1 — Selection: check each directory once (parallel with WP2, WP3)

File: `src/selection.rs`.

- In the directory-walk branch only, verify each directory for symlinks once
  per scan and reuse the result. Skip `canonicalize` for walked entries: the
  walk starts at a canonical directory, never follows links, and yields real
  directory entries.
- **Keep the full `checked()`** for explicitly requested paths and for
  `--diff` / `--pr` paths. That is where the confirmed symlink false pass was.
- **Platform gate.** Verified on Linux only (same report bytes). macOS
  normalization and case folding, and Windows verbatim prefixes, 8.3 names and
  reparse points, are unverified. Either gate the `canonicalize` skip to Linux
  or add hosted macOS and Windows tests that assert identical report paths and
  `Report::is_source_path` results. The directory cache itself applies
  everywhere.
- **Tests to add:** symlinked directory and symlinked file at depth 3 or more
  inside a walked tree; non-ASCII and mixed-case names on macOS; junction and
  verbatim-root walks on Windows.
- **Also here (small):** `excluded()` rechecks every ancestor for each entry
  although `filter_entry` already pruned them.
- **Measured:** `checked` 50 s to 1.6 s on this device.

### WP2 — Output preflight: one `lstat` per file (parallel)

File: `src/output.rs`.

- For entries whose `lstat` says "regular file", compare `(dev, ino)` with the
  destinations instead of `stat` + open + `fstat` + close. Symlink entries and
  `lstat` errors take the existing full path. Non-Unix keeps `same_file`.
- **Take the destination identity with the same method** (`symlink_metadata`)
  while keeping its handle open. Mixing path `lstat` with handle `fstat` can
  disagree on overlayfs and would produce false negatives in container CI.
- **Do not** shortcut with readdir inode numbers or `nlink == 1`. Both are
  unreliable on overlayfs, FUSE and under proot.
- **Keep** the walk over ignored files and the revalidation before each rename.
- **Behaviour change to pin with a test:** an unreadable TypeScript file no
  longer aborts publication (it cannot be an alias of a destination we just
  identified by inode).
- **Tests to add:** `alias.ts` symlink to an existing destination at depth;
  dangling `.ts` symlink; `.ts` symlink to a protected input outside the root.
- **Also here (small, byte-identical):** `typescript()` allocates two strings
  per walked entry; use a 1 MiB `BufWriter` for staged reports.
- **Measured:** 19 s to 10 s per walk.

### WP3 — Analyzer CPU (parallel; needs WP0)

File: `src/analysis.rs`.

1. **Kind-id tables.** Build one flag table per grammar from
   `Language::node_kind_for_id` using the exact string matches that exist
   today, then index by `kind_id()`. Cache field ids and use
   `child_by_field_id`. Conditions:
   - Look up with `.get()`: ERROR is id 65535 and is outside the table.
   - Build flags over **all** ids. `class` and `type` exist as both named and
     anonymous symbols; matching named ids only would change behaviour.
   - Keep hashing the kind **string**, taken from the table.
   - `"return_type"` in the type-pruning match is a field name, not a node
     kind, so that arm never fires. Leave it. Turning it into a real check
     would change complexity and owned lines.
   - Never hard-code ids. Select the table with the same flag that selects the
     parser.
2. **One hash update per token.** Write `kind\0text\0` into a reused buffer.
   SHA-256 is a stream, so the digest is unchanged.
3. **Owned lines as a sorted, deduplicated `Vec`** instead of a `BTreeSet` per
   function. `Method` is crate-private and `evidence` only iterates in order.
4. **Parent lookups.** `Node::parent()` searches down from the root on every
   call. `local_name` calls it for every unnamed function, and the repair loop
   calls it repeatedly. Carry an ancestor stack in `walk` instead. Profile
   first; add a test that the stack parent equals `parent()`.
- **Measured for items 1–2:** collection 20–24 s to 14–15 s. Items 3–4 are
  unmeasured estimates.
- **Review focus (Sonnet):** every flag against the original match arms, and
  the TSX table.

### WP4 — Bounded parallel analysis (after WP0 and WP3)

Files: `src/lib.rs`, `src/cli.rs`.

**Design: ordered producer, pure workers.** The main thread reads each file
and, in diff mode, verifies it against the HEAD blob, strictly in file order,
exactly as today. It hands `(index, source)` to workers over a bounded queue.
Workers only parse, collect, hash for coverage and score. The main thread
merges results in index order.

This keeps the Git verifier sequential, keeps read and verification errors in
today's order, and means a worker never touches the filesystem.

- **Memory bound.** Workers reduce each file to its `FunctionScore`s before
  returning (no trees, sources or line sets in the reorder buffer). Bound the
  queue by item count and by in-flight source bytes (start with 64 MiB; a
  larger file runs alone). Reserve the final vector once.
- **Deterministic errors.** Report the outcome of the lowest-index file. Catch
  panics per file with `catch_unwind` and resume the panic only if that file
  is the lowest failing one; otherwise an error in file 2 and a panic in file
  5 would race for the exit code.
- **Thread creation.** Use `Builder::spawn_scoped` with an explicit stack size
  and fall back to fewer workers. `Scope::spawn` panics when the process
  cannot create a thread.
- **Defaults.** Add `--jobs N` and a `jobs` field on `ScanExtensions` (the
  struct is `#[non_exhaustive]`, so this is a minor change). Library default
  is **1**, so embedders see no new threads. CLI default is
  `min(available_parallelism, 4)`, and stays sequential for small selections
  (start with fewer than 32 files) so typical PR diff scans are unchanged.
  Confirm the cap of 4 on hosted runners before release.
- **Tests to add:** `--jobs 1` and `--jobs N` byte-identical in every format;
  lowest-index error wins with several failing files, repeated many times;
  alternating TS and TSX across workers; deep nesting with several workers;
  panic-versus-error ordering through a test hook.
- **Acceptance:** peak RSS on the VS Code corpus at the default job count is
  at most 1.25× the sequential run, measured on glibc and musl. The prototype
  without bounding reached 2×.
- **Measured (naive prototype):** 68 s to 42 s with four workers on this
  device. Expect better scaling on machines with uniform cores, and measure
  musl separately: its allocator takes a global lock.

### WP5 — Memory on the report and baseline paths (parallel with WP4)

Peak RSS is dominated by the report itself: three heap strings per function.
Those field types cannot change in 2.x, so the wins are around them.

- `Baseline::capture` clones path, name and identity for every function
  (about 1.1 million allocations at this scale). Serialize from borrowed
  records with the same field order.
- Stdout reports are rendered into one `String` (158 MB here). Render to an
  anonymous temporary file and copy to stdout after publication. Same
  all-or-error behaviour, bounded memory.
- `Report.source_paths` is a clone of the file list; move it.
- `Baseline::apply` builds four ordered maps keyed by path strings and never
  uses their order; use hash maps.
- Coverage line keys allocate twice per entry in `deserialize_lines`. Borrow
  the key and check canonical form without allocating. Keep rejecting `+5`,
  which `parse::<usize>()` accepts.
- **Measure first:** peak RSS of a `--baseline` plus `--write-baseline` scan on
  the VS Code corpus. That is the usual CI invocation and has not been
  measured.
- **For 3.0, not now:** identity as 32 bytes and a shared path string would
  cut the per-function cost by roughly half. Record it as a 3.0 candidate.

### WP6 — Only if measurements justify it

- **Merge the three AST walks into one pass.** Saves at most two of three node
  visits, and is the riskiest change for identities: expression-bodied arrows,
  functions without a body, single bare arrow parameters and class bodies
  inside functions all have special cases. Do it only if collection is still a
  large share after WP3, and keep the current implementation under
  `#[cfg(test)]` as an oracle.
- **Allocator for the musl build.** tree-sitter allocates through libc
  `malloc`, so a Rust global allocator alone does not help, and
  `tree_sitter::set_allocator` is `unsafe`. The only policy-compatible route
  is an allocator crate that overrides the `malloc` symbol. That is a new
  dependency with a C build on every cross target. Measure glibc against musl
  on the corpus first; skip this if musl is within about 15%.
- **Pipeline `git cat-file --batch`** for large diffs.
- **Node coverage helpers** (`scripts/`): per-segment `trim()` in `lineHits`,
  whole-object copies in `mergeLineHits`, per-record source-map decoding in the
  importer, linear `findLast` per hit. Each is a contained fix with the
  existing generated-case oracle.

## Install and download track (independent of the scan track)

The download itself is already small: the Linux archive is 1.42 MB and takes
about half a second. **The time goes to verification round trips, not bytes.**
In the Action, `gh attestation trusted-root` took about 5 s here and the
verification call about 2 s, against 2 s for the download. An Action run makes
roughly 11–14 HTTP requests before the tool first executes.

| # | Change | Expected saving | Security property kept |
| --- | --- | --- | --- |
| D0 | Time each Action step on a hosted runner | none; sets the order below | — |
| D1 | Fetch the trusted root in the background while the release assets download (`action/install.sh`) | about 2 s here | Same bytes, same root, verified before first execution |
| D2 | Strip the symbol table (`strip = "symbols"`) | 577 KB of the binary; about 109 KB (7.7%) of the archive, similar for npm | No verification touched. Backtraces lose function names; panic messages keep file and line |
| D3 | Stop downloading the `.sha256` sidecar in the Action; compare the archive digest with the attested subject digest | one request, 0.2–0.5 s | Stronger source for the digest. Keep publishing the sidecar for manual users |
| D4 | Cache the TUF metadata directory between runs, **only if** the client still refreshes its timestamp against Sigstore | up to about 3.5 s; unmeasured | Full TUF verification and freshness are unchanged, or the change is dropped |
| D5 | Document npm as the fastest CI install | about 2 s against 5 s for the Action (README figures) | Registry integrity plus publish-time provenance, already in place |

Decisions taken as maintainer, with reasons:

- **No vendored or pinned Sigstore trusted root.** It would save the most time
  and it trades away revocation freshness. That is a security regression.
- **No tool cache keyed by anything the cache itself supplies.** A committed
  per-release digest table cannot work either: the Action lives in the same
  tag as the binary it installs, so the digest cannot be in that tag's source.
- **Keep `.tar.xz`.** Recompressing the binary: xz 1.40 MB, zstd -19 1.53 MB,
  gzip -9 1.91 MB. xz -9e gains nothing.
- **Keep unwinding panics.** `panic = "abort"` would shrink the binary a
  little, break the per-file panic handling WP4 needs, and leak staging files.
- **No executable packers.** They break the byte-for-byte comparison between
  the npm executable and the attested archive.
- **No fat LTO or feature trimming for now.** The hot parse path is C that
  Rust LTO cannot reach, and trimming clap features changes help and error
  text.

Models: Opus implements D1–D3 in one PR after D0; Sonnet reviews the Action
shell changes against `tests/test_github_action.py`; D4 is a time-boxed
investigation.

## Order and expected outcome

| Step | Packages | Runs in parallel |
| --- | --- | --- |
| 1 | WP0, D0 | yes |
| 2 | WP1, WP2, WP3, D1–D3 | yes, separate worktrees |
| 3 | WP4, WP5 | yes |
| 4 | Re-measure on hosted runners; decide WP6 and D4 | — |
| 5 | Update PERFORMANCE.md and the changelog with hosted numbers; release | — |

Steps 1–2 alone took this device from about 155 s to 68 s on one thread with
unchanged memory, and step 3 to about 42 s. Treat those as upper bounds for
the filesystem share and lower bounds for thread scaling until the hosted
numbers exist.

## Not doing

- Persistent cross-run caches. They add an invalidation and trust problem to
  a gate, and steps 1–3 should make them unnecessary.
- The `ignore` crate's parallel walker. Which walk error is reported first
  would become nondeterministic, for a stage WP1 already shrinks.
- Upgrading the grammar for speed. It can change trees, scores and identities.
- Validating the tree once for all renames.
- Defaulting to all cores, or spawning threads from the library by default.
