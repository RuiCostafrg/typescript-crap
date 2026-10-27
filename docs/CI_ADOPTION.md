# Adopt the CI gate with installed tools

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
