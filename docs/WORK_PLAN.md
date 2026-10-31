# Work plan and handoff (post-v2.2.1)

Last updated 2026-10-30. This file preserves decisions and the parallel work
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
  - Until npm exists, the Vitest/Jest one-step command ships in the coverage
    helper bundle (`ts-crap-coverage-helpers.zip`), not through `npx`.
- **Distribution later:**
  - A (npm package) is **deferred until the owner creates the npm account**,
    enables 2FA and sets up trusted publishing.
  - The npm package name is still undecided. `typescript-crap` is
    recommended. `ts-crap` is taken.
- **Decided 2026-10-31:**
  - Linux always uses the static musl binary, in the installer and every other
    channel.
  - The GitHub Action docs show all three reference styles (full commit SHA,
    immutable `@vX.Y.Z`, floating `@v2`) and recommend the full commit SHA as
    the most secure. Offering `@v2` needs a separate tag pattern and owner
    approval of any ruleset change.
  - The owner will create the npm account; npm work starts once it exists.
- **Still open:**
  - Whether to add an APT repository (recommendation: no, keep `.deb` assets
    as direct downloads).

## Work streams (T3 threads)

| Stream | Scope | Thread | Branch prefix | Status |
| --- | --- | --- | --- | --- |
| Roadmap v2.3 | `ROADMAP_V2_3.md` phases 1–3, in priority order (1.1 → 1.5, 2.1, 2.2, Phase 3, then later items) | Roadmap v2.3 implementation | `roadmap/` | started |
| Distribution B + C | Installer hardening and GitHub Action | Distribution: installer hardening + GitHub Action | `distribution/` | started |
| Distribution A (npm) | npm package with per-platform binaries and bundled helpers | — | — | blocked on npm account |
| Docs PR | This file and the two plans | this coordinating thread | `docs/roadmap-v2.3` | open |

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
   `verification` and `linux-distribution` all succeed on `main`.
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
     -f version=X.Y.Z -f previous-version=PREVIOUS -f verify-guide=true -f verify-linux=true
   ```

   Running it before the tap update fails the live-Homebrew jobs on the stale
   formula.
8. Release tags are immutable, so a failed release needs a new patch version.
   v2.2.0 was burned this way.

## Resume checklist

1. Read this file and `git log --oneline -15 origin/main`.
2. Run `gh pr list --repo RuiCostafrg/typescript-crap` to see open PRs.
3. Find the stream threads in T3 by title and read their latest activity.
4. Continue from the status table above, and update it.
