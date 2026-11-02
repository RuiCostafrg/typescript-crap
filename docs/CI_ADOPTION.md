# Adopt the CI gate with installed tools

For Node projects on releases after v2.2.1, the simplest setup is the npm
package: add `typescript-crap` as a dev dependency, so `npm ci` installs the
scanner and the matching helpers, and run `npx ts-crap ...` or
`npx typescript-crap coverage --framework vitest --scan` (or `--framework jest`)
in CI. The coverage command needs Node.js
24 or later in the CI job (the scanner alone runs on any maintained Node.js).
Until the first release after v2.2.1 is published, npm has only a `0.0.0`
placeholder. The rest of this page uses the release archives, which also suit
non-Node pipelines.

Use a published CLI and matching `ts-crap-coverage-helpers.zip` from the same
release. The [copyable workflow](../examples/ci/github-actions.yml) targets the
published [v1.0.0 release](https://github.com/RuiCostafrg/typescript-crap/releases/tag/v1.0.0),
which includes the standalone helper bundle and persisted gate policy.
The v0.4.0 release predates both features. When upgrading the example, select a
reviewed release and keep its CLI/helper versions identical.

The example uses Node 24, Vitest's installed Istanbul provider, and an Ubuntu
26.04 runner for the prebuilt GNU binary. Install/configure Vitest and
`@vitest/coverage-istanbul` in your project first and commit the lockfile. Adapt
`npm ci` and the test command to your existing project; the generic importer can
also run Jest or another compatible reporter. Use JSON when default-argument
initializers matter: LCOV line records can omit their declaration-line evidence.

## Establish the reviewed baseline

Install the CLI using a documented release installer or checksum-verified archive,
and extract the matching helper bundle. Set `TS_CRAP_HELPERS` to its directory.
Start with a clean checkout of the intended base revision and commit policy:

```json
{
  "version": 1,
  "include": ["src/**/*.ts", "src/**/*.tsx"],
  "exclude": ["**/*.test.ts", "**/*.test.tsx"],
  "threshold": 10,
  "require_coverage": true,
  "fail_on_empty": false
}
```

`fail_on_empty: false` allows PRs with no selected implemented functions. Enable
it if empty selections should block your workflow. Missing coverage never means
fully covered code. A zero hit is valid evidence of an uncovered statement.

```sh
run_dir=$(mktemp -d)
node "$TS_CRAP_HELPERS/import-coverage.mjs" --root . --format istanbul \
  --input "$run_dir/raw/coverage-final.json" --output "$run_dir/native.json" -- \
  npx vitest run --coverage --coverage.provider=istanbul \
  --coverage.reporter=json --coverage.reportsDirectory="$run_dir/raw"
set +e
ts-crap --coverage "$run_dir/native.json" --write-baseline crap-baseline.json
status=$?
set -e
if [ "$status" -gt 1 ]; then exit "$status"; fi
```

Baseline creation can exit 1 for existing score/completeness violations and still
write a valid snapshot. Review the absolute report, fix missing evidence, and
commit `ts-crap.json` and `crap-baseline.json` through your normal review process.
An empty or invalid report is not a baseline. Baselines never waive completeness
requirements. Keep the baseline and gate policy under code-owner review; update
them deliberately when policy changes or accepted debt is reviewed. CI must not
automatically rewrite a baseline to make a failing change pass.

## Gate committed PR changes

Copy the example workflow into your repository. It checks out the PR head SHA,
fetches full history, and disables persisted checkout credentials. `--diff` uses
the merge-base of the supplied base ref and HEAD; a shallow/missing base or a
working-tree source edit is an input error. A default synthetic merge checkout can
select a different history, so the workflow explicitly selects the PR head.

The workflow reads baseline and policy from the base commit into its temporary
run directory. Gate decisions therefore use the approved policy while analyzing
committed PR source. Policy/baseline changes need review and landing on the base
before they affect later PRs; threshold mismatches fail with exit 2.

Every collection gets a new directory. Both the raw report and native output must
be absent before the importer starts; never recycle a previous coverage report.
The importer snapshots TypeScript before running tests and verifies unchanged
bytes afterward. Stale source, a failing test command, or an unmappable location
prevents native report publication. Source edits made by code generation or tests
must be completed and committed before collection/diff gating.

Reports are generated twice from the same fresh native coverage: JSON and SARIF.
A valid gate failure still writes both reports. The workflow preserves their exit
codes and uses `if: always()` for artifact upload. Collector/input failures can
leave fewer artifacts; a successful upload does not change the gate result.

| Exit | Meaning | Action |
| --- | --- | --- |
| 0 | Gate passes | Continue CI. |
| 1 | Score/regression/completeness/empty-selection failure | Review reports and fix code/evidence or review policy separately. |
| 2 | Invalid arguments, inputs, source identity, history, or output | Fix the setup; recollect coverage after source changes. |

Git is required for diff mode. The `--pr` convenience lookup additionally requires
authenticated `gh` and a checkout of that PR's exact head; the example uses an
explicit base SHA so the gate itself needs no GitHub API credential. Collection
executes trusted project commands with the runner's permissions, so use normal PR
isolation and read-only permissions for untrusted contributions. Rust/compiler
installation is unnecessary for the downloaded CLI.

## GitHub Action

The repository root contains a composite [setup action](../action.yml). It
replaces the manual install step in the example workflow. It installs the
`version` you choose (2.2.1 or newer, no `latest`) for the runner:

| Runner | Release archive |
| --- | --- |
| Linux x86-64 / ARM64 | static musl (`*-unknown-linux-musl`), so any glibc version works |
| macOS Apple Silicon / Intel | `*-apple-darwin` |
| Windows x86-64 | `x86_64-pc-windows-msvc` |

Before anything downloaded runs, the action:

1. Downloads the archive and its `.sha256` with `gh` into a fresh directory
   under `RUNNER_TEMP`.
2. Runs `gh attestation verify` with an exact `--cert-identity` for this
   repository's `release.yml` at `refs/tags/vVERSION`, plus `--source-ref` and
   `--deny-self-hosted-runners`. An exact identity is used because `gh`
   2.101.0 and older match `--signer-workflow` only as a prefix
   ([GHSA-wjmr-j3rp-mh2g](https://github.com/cli/cli/security/advisories/GHSA-wjmr-j3rp-mh2g))
   and compare `--source-ref` case-insensitively
   ([GHSA-4mq3-hpgx-9cx8](https://github.com/cli/cli/security/advisories/GHSA-4mq3-hpgx-9cx8)).
   Some hosted images still ship such versions, for example 2.98.0 on
   `macos-26` Intel.
3. Checks the SHA-256, and that the checksum file names that exact archive.
4. Requires exactly one executable that reports `ts-crap VERSION`, then adds
   its directory to `PATH`.

With `helpers: true` it verifies and extracts the matching
`ts-crap-coverage-helpers.zip` the same way and sets `TS_CRAP_HELPERS`.
The release repository and signer workflow are fixed in the action and are
not inputs. No remote script is piped into a shell. The action needs `gh` (preinstalled on
GitHub-hosted runners) and only read access through the default `token`.

```yaml
permissions:
  contents: read
steps:
  - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
    with:
      ref: ${{ github.event.pull_request.head.sha }}
      fetch-depth: 0
      persist-credentials: false
  - uses: RuiCostafrg/typescript-crap@<full commit SHA> # vX.Y.Z
    with:
      version: 2.2.1
      helpers: true
  # ts-crap and $TS_CRAP_HELPERS are now available to later steps.
```

To run the gate in the action, set `scan: true` and give the arguments in
`args`, one per line. Each line is passed as one argument and never evaluated by a
shell, so paths with spaces need no quoting. With `sarif: true` the scan
runs once with `--format sarif --output` placed before your `args`, so the
report and the gate result always come from the same run. When `--root`
is a subdirectory of the checkout, the action rewrites the report's paths to be
relative to the checkout so code scanning can find the files (this needs
`python3`, which hosted runners have). It then prints a one-line summary per
SARIF result. Scan output is printed with workflow commands suspended, so
paths or function names in a PR cannot forge annotations. In that mode, `--format`,
`--output`, `-o` and the text-only `--only-failing`, `--sort` and `--top`
are rejected before `--`. `upload-sarif: true` uploads the report to code
scanning with a pinned `github/codeql-action/upload-sarif`; only that job needs
`security-events: write`:

```yaml
jobs:
  crap:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: RuiCostafrg/typescript-crap@<full commit SHA> # vX.Y.Z
        with:
          version: 2.2.1
          scan: true
          sarif: true
          upload-sarif: true
          args: |
            --threshold
            15
            src
```

The scan's exit code is the `exit-code` output (0 pass, 1 gate failure, 2
invalid input). When a report is produced, the action uploads it before
failing on a non-zero result, so gate failures still reach code scanning.
Invalid input can stop the scan before any report exists, and then nothing is
uploaded. Use `continue-on-error`
on the step to report without blocking. Other outputs: `version`, `target`,
`bin-dir`, `executable`, `helpers-dir` and `sarif-file`. For coverage-gated
PRs, install with the action and keep the collection and gate commands from
the example workflow.

`sarif-category` defaults to upload-sarif's own category for the workflow
and job. Set distinct categories when one job uploads more than one scan,
otherwise later uploads replace earlier ones. `sarif` requires `scan`, and
`upload-sarif` requires `sarif`. Inconsistent or non-boolean inputs fail
before anything is downloaded. The `token` is passed only to `gh`; the
downloaded binary runs without it.

### Choosing the action ref

The action ref (after `@`) and the `version` input are independent. The ref
selects the action's own scripts, which do the verification. `version` selects
the ts-crap release they install. There are three ways to reference the action:

| Ref | Example | Changes when | Trade-off |
| --- | --- | --- | --- |
| Full commit SHA (**recommended**) | `@<full commit SHA> # vX.Y.Z` | Never | Most secure: the code you reviewed is the code that runs, even if a tag or branch moved. It is the only form accepted where SHA pinning is enforced, as it is in this repository. Dependabot can update it, and the comment records the release it came from. |
| Release tag | `@vX.Y.Z` | Never here | Readable and immutable in this repository, where release tags cannot move or be deleted. You still trust that rule; a SHA does not. Works from the first release that contains the action. |
| Floating major | `@v2` | With every 2.x release | Picks up fixes automatically, but the code that verifies your downloads can change without review. Rejected where SHA pinning is enforced. **Not offered yet:** release tags matching `v*` are immutable, so a moving `v2` would need a separate ref scheme. |

Use the full commit SHA of a release commit with a `# vX.Y.Z` comment, as the
examples above do. Until a release contains the action, use a commit on `main`. Get it with
`git ls-remote https://github.com/RuiCostafrg/typescript-crap 'refs/tags/vX.Y.Z^{}'`.
Release tags are annotated, and the `^{}` suffix returns the commit rather
than the tag object.

For the owner's commits and PRs, [Action CI](../.github/workflows/github-action.yml)
runs the action against the published v2.2.1 release on Ubuntu 26.04 and 24.04
(x86-64 and ARM64), macOS Apple Silicon and Intel, and Windows x86-64. It
checks the Linux binary is static, scans with SARIF, uploads to code scanning
(on pushes to `main` and manual runs only, so PR runs keep read-only tokens),
and checks that gate failures, invalid input, `latest`, unsupported and
unpublished versions fail. Like the other workflows, it skips jobs for other
actors. Unit tests in `tests/test_github_action.py` cover
tampered, misbound and unattested assets with a fake `gh`.

## Executable verification and release evidence

`verify-ci-guide.py` creates a separate clean Git checkout, extracts installed
helper bytes, and exercises reviewed baseline creation, empty PRs, comment-only
changes, a regression, deterministic fresh repeats, JSON/SARIF artifacts on exit 1,
and stale-input rejection while preserving prior artifacts. It verifies the
baseline stays unchanged. CI runs it with both direct Node and actual Vitest JSON
collection; release archive verification runs it with the packaged CLI/helpers.

```sh
python3 scripts/bundle-coverage.py target/helper-bundle
python3 scripts/verify-ci-guide.py \
  --archive target/helper-bundle/ts-crap-coverage-helpers.zip \
  --scanner target/debug/ts-crap
# Exact Vitest collection path (after npm ci --prefix tests/tooling):
python3 scripts/verify-ci-guide.py \
  --archive target/helper-bundle/ts-crap-coverage-helpers.zip \
  --scanner target/debug/ts-crap --vitest tests/tooling/node_modules/vitest/vitest.mjs
```

A [clean published-tool run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40494926411)
passed with v1.0.0's checksum/provenance-verified CLI and helper archive on all
five targets: Linux ARM64/x86-64, macOS ARM64/Intel, and Windows x86-64. The Linux
x86-64 job additionally passed actual Vitest/Istanbul collection. The
[acceptance ledger](RELEASE_V1.md) records the immutable tag/source, helper
checksum, and related installation evidence.

For each new release, after publication and live Homebrew synchronization, dispatch
[Verify published adoption](../.github/workflows/verify-published.yml) with the
published version and previous upgrade fixture. Keep guide verification enabled.
It downloads checksum/provenance-verified published CLI/helpers, runs this clean
project on all five archive targets (plus actual Vitest on Linux x86-64), and
checks unchanged live Homebrew URLs on its three supported targets. Record the
run URL and source commit in the roadmap; a skipped guide check for legacy
releases does not satisfy the new-release acceptance criterion.

Hosted [contract/guide CI](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847597)
passed on `bf8a52d7be37c871939746bfe3ceae807e2bdf49` with Node 24 and both
collection paths. That run used candidate helpers before publication; the v1.0.0
published-tool run above supplies the subsequent public-download evidence.
