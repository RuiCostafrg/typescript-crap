# v1.0.0 acceptance ledger

Status on 2026-10-27: implementation, exact-source candidate verification, and
protected publication, and Homebrew sync complete; published-path verification
is in progress. The `v1.0.0` tag selects
`4ce57b9aa3b9979166965aebbf15e16eabb6ccac`. The [roadmap](RELEASE_ROADMAP.md)
retains the final published-guide check until actual published tools pass it.

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

## Exact release source

[PR #26](https://github.com/RuiCostafrg/typescript-crap/pull/26) merged the version
preparation and survivor regression tests as
`4ce57b9aa3b9979166965aebbf15e16eabb6ccac`, selected by `v1.0.0`. Every check
passed on both its PR head and the exact main commit. Final main evidence:

| Area | Evidence |
| --- | --- |
| Rust/MSRV, real framework/platform behavior, contracts/guide, independent adoption, advisories | [CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657144) |
| All nine installer jobs and all three live Homebrew targets | [installation compatibility](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657188) |
| Production Rust coverage: 1520 / 1555 lines, **97.75%** | [coverage](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657139), floor 95% |
| Mutation testing: 340 caught, 15 missed, 30 unviable, **95.77%**, zero timeouts | [mutations](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461657129), floor 90% |
| Five archives, helpers/guide, installers/upgrades, three Homebrew targets, both clean Linux runtimes | [Release dry run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40461673913), publication skipped by design |

The complete mutation run caught the three baseline/Git lifecycle survivors
identified above. The remaining 15 are the nine parser, five output-guard, and
one diagnostic-string survivors already reviewed. No production mutation or
quality floor was removed.

## Published release

The protected [tag-triggered Release workflow](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40464740215)
passed all builds, archive/guide/installer checks, and publication. Following
owner approval in the existing release environment, it published
[immutable v1.0.0](https://github.com/RuiCostafrg/typescript-crap/releases/tag/v1.0.0)
at 2026-10-27 11:06:33 UTC with 17 assets and GitHub provenance attestations.
The tag and assets were newly created; prior releases were not changed.

The published helper archive declares matching CLI/helper version 1.0.0,
Node >=24, and coverage schema 1. Its SHA-256 is
`0d7e69f9d4c78e9c06d75b9a928eca276ca2d33159674c78f78aa783f5d718c7`.
The downloaded ZIP matches its published checksum. Both the helper ZIP and
formula attestations were verified against the release workflow,
`refs/tags/v1.0.0`, and exact source digest
`4ce57b9aa3b9979166965aebbf15e16eabb6ccac`, rejecting self-hosted provenance.

The owner-approved [Homebrew provenance sync](https://github.com/RuiCostafrg/homebrew-tap/actions/runs/40493701131)
opened [formula PR #9](https://github.com/RuiCostafrg/homebrew-tap/pull/9).
Its formula bytes matched the attested published asset. All three required
checks passed on exact head `e117750bf201ecd8e187b8a8331f0d567579aef2`
in [Tap CI](https://github.com/RuiCostafrg/homebrew-tap/actions/runs/40494039197),
including actual macOS ARM64 and Linux x86-64 installs. The PR merged as
`38c488213ceb94d823427832e0f079566fa12f24` on 2026-10-27.

## Exact-release and publication checklist

- [x] Merge the version-preparation PR after every check passes.
- [x] Successful main CI, Rust coverage, mutations, and installation workflows
  for the exact commit selected for `v1.0.0`.
- [x] Successful manual Release dry run for that exact main commit: five archives,
  checksums, helper bundle, installed guide, real installers/upgrades, three
  Homebrew targets, and clean Linux runtimes.
- [x] Owner-approved protected publication of the immutable `v1.0.0` tag/assets.
- [x] Provenance-verified stable Homebrew sync, formula PR checks, and merge.
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
