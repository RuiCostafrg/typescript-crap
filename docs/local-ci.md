# Local CI — `gh-signoff` (Linux only)

Run the core Linux CI checks on your own machine (a fast subset — see Notes for
what stays hosted) and post green `signoff/<stage>` commit statuses with
[basecamp/gh-signoff](https://github.com/basecamp/gh-signoff).

Use it when free Actions minutes run out — or as a fast pre-push gate. The GitHub
Actions workflows stay untouched; this lane covers **Linux only** (the macOS/Windows
`platform-behavior` and `framework-coverage` matrix jobs remain hosted).

## Setup (Linux)

```bash
gh auth login
gh extension install basecamp/gh-signoff
```

Toolchains: Rust (*1.90* minimum — CI tests on a 1.90/stable matrix), Node ≥ 24, Python 3.
`cargo-llvm-cov` only for the optional `coverage` stage.

## Usage

```bash
scripts/local-ci.sh                # stages: tests lint docs
scripts/local-ci.sh tests lint     # run selected stages only
scripts/local-ci.sh --list         # all stages (incl. optional: coverage security)
scripts/local-ci.sh --no-signoff   # checks only, no statuses posted
```

Push your commits first: `gh signoff` only attests pushed work. Failing stages post
a red `signoff/<stage>` status automatically. Each status (green or red) is bound
with `--commit` to the exact commit captured at the start of its stage, so a
commit or push landing mid-run can never be green-lit for code it did not test.
a red `signoff/<stage>` status automatically.

## Optional: make sign-off a merge requirement

```bash
gh signoff install tests lint docs
gh signoff uninstall
```

## Notes

- `scripts/mutation-score.py` / runtime mutation counting and cargo-kani proofs
  (`verification.yml`, `mutation.yml`) are deliberately left out of the local lane —
  they are slow hosted jobs that run automatically on every PR and push to `main`.
- Release-tag builds (`release.yml`, `verify-*.yml`) stay CI-only.
