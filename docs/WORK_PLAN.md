# Work plan and handoff (post-v2.2.1)

Last updated 2026-11-02. This file preserves decisions and the parallel work
split so any session can resume after a context reset. Update the status table
when a stream changes state.

## Source documents

- [ROADMAP_V2_3.md](ROADMAP_V2_3.md): feature roadmap (Vitest/Jest first-class,
  scanner gaps, oxlint/oxfmt tooling).
- [LINUX_DISTRIBUTION_STRATEGY.md](LINUX_DISTRIBUTION_STRATEGY.md):
  distribution options. A = npm package, B = shell installer hardening,
  C = GitHub Action, D = crates.io.

## Owner decisions (2026-10-30)

- **TypeScript only.** No `.js`/`.jsx` scanning support.
- **Vitest and Jest are the priority frameworks.**
- **Linting and formatting.**
  - oxlint and oxfmt apply only to the repository's own `.mjs` files.
  - Rust keeps `cargo fmt`, `clippy -D warnings` and cargo-mutants (≥ 90 %).
- **Distribution now:**
  - B (shell installer hardening) and C (GitHub Action) start now, in parallel
    with the roadmap. Each ships in the first release after it merges.
  - The Vitest/Jest one-step command ships in the coverage helper bundle
    (`ts-crap-coverage-helpers.zip`), and in the npm package as
    `npx typescript-crap coverage` from v2.3.2.
- **Distribution A (npm):** originally deferred until the owner's npm account
  existed. Unblocked on 2026-11-01 (below); merged in #48 and #49.
- **Decided 2026-10-31:**
  - Linux will use the static musl binary, in the installer and every other
    channel. This is pending implementation in the distribution stream.
  - The GitHub Action docs show all three reference styles (full commit SHA,
    immutable `@vX.Y.Z`, floating `@v2`) and recommend the full commit SHA as
    the most secure. Offering `@v2` needs a separate tag pattern and owner
    approval of any ruleset change.
  - The owner will create the npm account. npm work starts once the account
    exists with 2FA and trusted publishing configured, matching the
    Distribution A gate above.
- **Decided 2026-11-01:** the npm package is `typescript-crap`, with platform
  packages `typescript-crap-{linux-x64,linux-arm64,darwin-x64,darwin-arm64,win32-x64}`.
  The owner created npm account `lotinha` (2FA), published 0.0.0 placeholders and
  configured trusted publishing for `release.yml` with environment `release`.
- **Still open:**
  - Whether to add an APT repository (recommendation: no, keep `.deb` assets
    as direct downloads).

## Work streams (T3 threads)

| Stream | Scope | Thread | Branch prefix | Status |
| --- | --- | --- | --- | --- |
| Roadmap v2.3 | `ROADMAP_V2_3.md` phases 1–3, in priority order (1.1 → 1.5, 2.1, 2.2, Phase 3, then later items) | Roadmap v2.3 implementation | `roadmap/` | 1.1 one-step command (#43), 1.2 diagnostics (#46) and 1.3 version matrix (#47) merged and released in v2.3.0; 1.4 Jest transformers in review (targets v2.4.0) |
| Distribution B + C | Installer hardening and GitHub Action | Distribution: installer hardening + GitHub Action | `distribution/` | B docs/CI (#41), static musl default (#44) and GitHub Action (#42) merged; decisions recorded in #45. Next: exact `--cert-identity` in the repo's own verification commands |
| Distribution A (npm) | npm package with per-platform binaries and bundled helpers | Distribution A: npm package | `distribution/npm-` | Shipped: `typescript-crap@2.3.2` (npm `latest`, provenance, staged publishing approved with 2FA); published-release npm checks pass with npm, pnpm, Yarn and Bun (#48–#50, #53, #56, #57) |
| Docs PRs | This file and the two plans; owner decisions | this coordinating thread | `docs/` | #40 merged; #45 decisions |

Thread links are in T3: search thread titles above with `t3_thread_search`,
or read them directly:

- Roadmap v2.3 implementation: `mcp:ecf1739a-81f4-4e1a-b561-53a302c3631c`
- Distribution B + C: `mcp:9b58b187-2382-4b93-aea8-74d63bedeabc`
- Coordinating thread: `ba940b2c-0972-4bf6-a59a-1bf729c38653`

## Conventions for every stream

- **One PR per coherent item**, branched from up-to-date `main`.
  - Squash merges only.
  - Commit messages end with the Co-Authored-By line given by the session.
  - PR bodies end with the Claude Code attribution line.
- **Checks before pushing:**
  - Rust: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`
    and `cargo test --locked -j 1`.
  - Python: `python3 -B -m unittest discover -s tests -p "test_*.py"`.
  - Mutation: add tests that kill new mutants. The CI floor is 90 %.
- **Review bots:**
  - CodeRabbit, Cubic, Greptile and Sourcery review every PR.
  - Fix valid findings, reply with the reason when a finding doesn't apply,
    and resolve every thread.
- **Merging:**
  - Merge only after all checks pass and no threads remain open.
  - `main` needs an independent approval. The documented sole-owner bypass is
    `gh pr merge --squash --admin --match-head-commit <sha>` (see
    `SECURITY.md`).
  - A runner-infrastructure failure (no steps, no log) may be rerun once.
- **Model use:** Opus does the main work. Haiku handles mechanical sweeps and
  inventories. Sonnet does independent diff reviews.
- **Changes stay additive** under the 2.x compatibility contract. Defaults stay
  unchanged and new behaviour is opt-in.
- **Do not cut releases from a stream.** The coordinating thread tags releases
  after the streams merge.

## Release procedure (proven with v2.2.1)

1. Prepare the release in a PR:
   - Bump `Cargo.toml` and `Cargo.lock`.
   - Date the changelog.
   - Point `verify-release.yml` upgrade checks at the previous published version.
2. Merge, then wait until `ci`, `coverage`, `mutation`, `installation`,
   `verification`, `linux-distribution` and `npm-package` all succeed on `main`.
3. Tag on the merge commit: `git tag -a vX.Y.Z -m "ts-crap vX.Y.Z"`, then push
   the tag. This works through the owner-only tag ruleset bypass.
4. Approve the `release` environment. The REST pending-deployments call
   reports no approvable request, so use GraphQL (`RUN_ID` is the release
   workflow run):

   ```sh
   run=$(gh api repos/RuiCostafrg/typescript-crap/actions/runs/$RUN_ID --jq .node_id)
   env=$(gh api repos/RuiCostafrg/typescript-crap/environments/release --jq .node_id)
   gh api graphql -f r="$run" -f e="$env" -f query='
     mutation($r: ID!, $e: ID!) {
       approveDeployments(input: {workflowRunId: $r, environmentIds: [$e],
                                  comment: "Approve release"}) {
         deployments { environment }
       }
     }'
   ```

   The environment asks twice. Approve `publish` first: it creates the GitHub
   release. Then approve `publish-npm` with the same mutation (same run, same
   environment). It re-verifies the npm packages against the attested release
   assets and stages them with npm trusted publishing (`npm stage publish`; the
   trusted publisher allows nothing else). Nothing is public yet: the owner
   approves each staged version with 2FA, on npmjs.com or with
   `npm stage approve <id>`, the five platform packages first and
   `typescript-crap` last, only after `npm view typescript-crap-<platform>@X.Y.Z`
   shows all five (approved early, it installs without a binary). The job
   summary lists the stage ids. Check
   `npm view typescript-crap@X.Y.Z` after approval.

5. After publishing, verify the downloaded assets in a fresh empty directory:

   ```sh
   set -eu
   gh release download vX.Y.Z --repo RuiCostafrg/typescript-crap --dir assets
   cd assets
   if command -v sha256sum >/dev/null; then sum="sha256sum"; else sum="shasum -a 256"; fi
   $sum -c sha256.sum
   for f in *.sha256; do $sum -c "$f"; done
   # Every published asset is attested, including sums and the manifest.
   for f in *; do
     gh attestation verify "$f" --repo RuiCostafrg/typescript-crap \
       --signer-workflow RuiCostafrg/typescript-crap/.github/workflows/release.yml \
       --source-ref refs/tags/vX.Y.Z --deny-self-hosted-runners
   done
   ```

6. Update the Homebrew tap:
   - Dispatch `RuiCostafrg/homebrew-tap` "Sync verified release".
   - Approve its `homebrew` environment with the same mutation, using that
     repository's run and `homebrew` environment node IDs.
   - Approve the tap's update PR, then close and reopen it as the owner so
     the PR CI runs. Bot-triggered runs skip.
   - Squash-merge it.
7. Only after the tap merge, run the published-release check once:

   ```sh
   gh workflow run verify-published.yml --repo RuiCostafrg/typescript-crap \
     -f version=X.Y.Z -f previous-version=PREVIOUS -f verify-guide=true -f verify-linux=true \
     -f verify-npm=true
   ```

   Running it before the tap update fails the live-Homebrew jobs on the stale
   formula.
8. After the published-release check passes, open a PR that moves the pinned
   `version:` inputs in `.github/workflows/github-action.yml` (and the matching
   expectations) to the new release, since Action CI installs a published
   release.
9. If `publish-npm` fails after the GitHub release because of an external
   cause (registry outage, npm settings), fix the cause and rerun only
   that job within 30 days (the `npm-packages` artifact retention): versions
   already published with identical contents are skipped. A version that is
   still staged can't be staged again: approve or `npm stage reject` it first.
   The dist-tag is fixed when a version is staged and is chosen against the
   approved `latest`, so approve releases newest first; if an older backport
   was staged while a newer release was unapproved, reject and re-stage it. Trusted publishing
   can't change dist-tags, so if a backport rerun skips packages published
   before a newer release, add the tag by hand:
   `npm dist-tag add typescript-crap-<platform>@X.Y.Z backport`.
   A published npm version can't change; deprecate it if it is bad.
   A rerun uses the workflow file at the tag, so a bug in `release.yml`
   itself can't be fixed by rerunning: fix it on `main` and cut a new patch
   release. v2.3.0 (path bug) and v2.3.1 (direct publish, which the staged-only
   trusted publisher refuses) reached GitHub but not npm; npm starts at 2.3.2.
10. Release tags are immutable, so a failed release needs a new patch version.
   v2.2.0 was burned this way.

## Resume checklist

1. Read this file and `git log --oneline -15 origin/main`.
2. Run `gh pr list --repo RuiCostafrg/typescript-crap` to see open PRs.
3. Find the stream threads in T3 by title and read their latest activity.
4. Continue from the status table above, and update it.
