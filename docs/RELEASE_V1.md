# v1.0.0 acceptance ledger

Status on 2026-10-27: implementation complete, release preparation in progress.
The package declares 1.0.0. No v1 tag or immutable release has been published by
this preparation change. The [roadmap](RELEASE_ROADMAP.md) retains the final
published-guide check until actual published tools pass it.

## Completed implementation evidence

The implementation was merged in [PR #25](https://github.com/RuiCostafrg/typescript-crap/pull/25)
as `9fca2f9e5daab58965248e4b9889f639f06ee2ce`. All checks passed on its final
head `bf8a52d7be37c871939746bfe3ceae807e2bdf49`:

| Area | Evidence |
| --- | --- |
| Rust 1.90/stable, debug/release, Clippy, rustdoc, Python, advisory audit, workflows | [CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597) |
| Linux/macOS/Windows real Jest/Vitest, installed helpers, paths and policy | [CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597) |
| Schemas and installed CI guide, direct Node and actual Vitest | [contracts-and-guide](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597/job/121914606913) |
| Hono service, TanStack monorepo, Vite TSX app, pinned unmodified source | [independent-adoption](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597/job/121914606771), [reproduction and limits](ADOPTION.md) |
| Published installers, clean Linux runtime, live tap fresh/reinstall/upgrades on three Homebrew targets | [installation matrix](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847643) |
| Production Rust coverage: 1517 / 1555 lines, **97.56%** | [coverage](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847587), floor 95% |
| Mutation testing: 337 caught, 18 missed, 30 unviable, **94.93%**, zero timeouts | [mutations](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847618), floor 90% |

Coverage/mutation numbers belong to that exact implementation head. The final
1.0.0 release commit must pass its own full checks. The package version change
adds no scoring, identity, input-schema, or production-algorithm change.

## Mutation survivor review

All 18 survivors in that hosted run were inspected, rather than being counted as
catches or excluded from the score:

- One baseline comparison survivor exposed a missing exact-threshold test. The
  release preparation adds public scans for both a new function and an existing
  previously covered function: a score equal to threshold must pass regression
  comparison even when it exceeds the old score.
- Two Git batch-reader survivors remove exit-status validation or drop cleanup.
  New tests reject a killed child and check that dropping a reader terminates and
  reaps a real waiting Git process while an external input pipe stays open.
- Nine parser survivors weaken normalization guards or change whitespace-span
  arithmetic. The complete-file reparse remains the final syntax check; no tested
  malformed source was accepted. A new valid overload corpus puts line breaks
  inside block comments and includes Unicode/CRLF. Residual permissive-guard
  survivors are documented test-boundary gaps, not evidence that altered parser
  code was shipped. No guard is removed to improve a mutation score.
- Five output-path error-guard survivors overlap later canonicalization and
  staging failures. Existing tests reject metadata errors and preserve prior
  reports. Exact incidental diagnostic wording is outside the 1.x API.
- One JSON visitor survivor removes an expected-type diagnostic string. Runtime
  deserialization still rejects the invalid value; exact error prose is outside
  the compatibility contract.

A focused local rerun of four reviewed mutations caught the exact-threshold and
both Git lifecycle mutations with assertion failures. The parser whitespace
arithmetic mutation still survived the new valid corpus because another recovery
path accepts it; it remains a reported survivor, with all guards retained. This
focused run is not substituted for the complete hosted score.

The final run retains every production mutation and the same score/timeout gates.

## Exact-release and publication checklist

- [ ] Merge the version-preparation PR after every check passes.
- [ ] Successful main CI, Rust coverage, mutations, and installation workflows
  for the exact commit selected for `v1.0.0`.
- [ ] Successful manual Release dry run for that exact main commit: five archives,
  checksums, helper bundle, installed guide, real installers/upgrades, three
  Homebrew targets, and clean Linux runtimes.
- [ ] Owner-approved protected publication of the immutable `v1.0.0` tag/assets.
- [ ] Provenance-verified stable Homebrew sync, formula PR checks, and merge.
- [ ] `Verify published adoption` with version `1.0.0`, previous version `0.4.0`,
  and guide verification enabled: all five installed-guide targets, actual Vitest
  on Linux x86-64, and all three live-tap targets.
- [ ] Record the final tag/commit and workflow URLs, then close the roadmap's
  published-guide item.

Publish through the existing protected workflow described in [SECURITY.md](../SECURITY.md).
Manual Release dispatch is a dry run and cannot publish. Existing release assets
must not be replaced, and quality floors/actor/environment restrictions stay in
place. The final published-path check runs after publication because it downloads
the immutable public assets, rather than a local substitute.
