# Security policy

Only **RuiCostafrg** maintains code and releases. Public bug reports and feature
requests belong in issues. Pull requests are limited to collaborators; do not
add collaborators unless intentionally changing this trust policy. Public forks
and redistribution remain permitted by the MIT license and GitHub’s public repo
model; forks cannot publish into this repository.

Report vulnerabilities privately through the repository’s Security tab rather
than disclosing exploit details in a public issue. Account passkeys/hardware-key
2FA and recovery codes remain the owner’s responsibility; repository automation
cannot enable or verify them.

## Release trust boundary

- CI runs read-only, with no publishing secrets. Issues/comments never execute
  workflows. The release workflow does not run on pull_request_target,
  workflow_run, issues, or comments.
- Only the numeric owner account ID may initiate release builds here. Version
  tags must be SemVer tags on commits already in main with successful main CI.
  Manual Release dispatch performs a dry run and never publishes.
- Main requires passing checks and a PR. The sole owner can bypass the independent
  review requirement **through a PR**; an owner cannot independently approve
  their own PR. CODEOWNERS documents the owner. This is an explicit single-owner
  trust model, not two-person review. Required checks remain a separate rule.
- New releases are immutable after publication: assets cannot be replaced.
- Release tags cannot be changed/deleted, and only repository admins (currently
  the sole owner) may create matching tags. Do not add other admins casually.
- Rust is pinned to 1.90.0 for distribution, Cargo.lock pins crate versions and
  registry checksums, dependencies are fetched with --locked, distribution builds run offline and
  reject any lockfile changes, and cargo-audit checks
  known advisories. Upgrades require maintainer review; an audit is not proof that
  a crate is free of malicious behavior.
- Actions are pinned to commits and an allowlist is enforced in repository
  settings. cargo-dist 0.33.1 archives are verified against committed SHA-256
  hashes before execution. No remote installer is piped into a shell in CI.
- Builds and smoke tests have contents:read only. Only publish receives
  contents:write, id-token:write and attestations:write, after owner approval in
  the release environment, which accepts only v* tags and disallows admin bypass.
- Publishing uses a fresh hosted runner. It neither checks out source nor runs
  cargo-dist, built binaries, or downloaded scripts. Artifacts are data. GitHub
  provenance is generated with ephemeral OIDC credentials, then assets publish
  through the job’s temporary GITHUB_TOKEN. There are no persistent deploy keys,
  GitHub App private keys, personal tokens, or inherited secrets.
- The Homebrew tap independently verifies a newer stable formula’s attestation
  against this workflow, tag and commit, then requires owner approval in its own
  homebrew environment. It writes using its own temporary token. Sync checks run
  hourly or can be manually dispatched; GitHub can delay scheduled runs.

## Verifying downloads

For releases published by the hardened workflow, use:

```sh
gh attestation verify ./ts-crap-aarch64-apple-darwin.tar.xz \
  --repo RuiCostafrg/typescript-crap \
  --signer-workflow RuiCostafrg/typescript-crap/.github/workflows/release.yml \
  --source-ref refs/tags/vVERSION --deny-self-hosted-runners
```

Replace the platform archive and vVERSION with the desired release. For a strict
commit binding, add --source-digest with the expected 40-character commit SHA.
Checksums alone detect corruption, not a compromised publisher. Attestations
identify origin, not whether the program is harmless. The original v0.1.0 release
predates this hardening and is not retroactively attested.

## Limits

GitHub, pinned action authors, toolchains, dependency sources and the owner account
are trust dependencies. No deployment design prevents all supply-chain attacks.
A repository admin can change protections. A malicious approved dependency can
still influence a build; least privilege limits credential exposure but does not
prove binary correctness. Release installers and installed tools run with the
user’s permissions; they are not a sandbox. Code signing/notarization requires
separate Apple/Windows signing credentials and is not configured here.
