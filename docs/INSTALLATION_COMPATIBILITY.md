# Installation and platform verification

v2.2.0 adds a separate static musl and Debian package matrix described
in [Linux distribution](LINUX_DISTRIBUTION.md). The GNU archive baseline below
continues to apply to existing releases and GNU artifacts. Musl/package checks
are additional; they do not retroactively certify older archives.

The supported Linux reference runtime for v0.4.0 and newer prebuilt archives is
**Ubuntu 26.04**. Keep this minimum until a separately built and tested older
runtime is introduced. A glibc version alone does not establish compatibility
with every distribution. Releases through v2.1.0 have no Alpine/musl archive;
v2.2.0 adds separate musl builds. Android/Termux has no native release
archive. Older v0.3.0 archives retain their earlier glibc 2.35+ baseline.

The [installation workflow](../.github/workflows/installation.yml) verifies a
pinned published upgrade pair, v0.3.0 to v0.4.0. The
[release verification workflow](../.github/workflows/verify-release.yml) repeats
installation and upgrades using the candidate release assets before publication.
The installer and Homebrew checks upgrade candidates from the published v2.1.0
release; Debian package checks use the synthetic revision-zero fixture described
in the Linux distribution guide. Update the published-version fixture
explicitly if its supported upgrade range changes. Tagged publication requires
successful installation CI on the source commit as well as the existing Rust,
coverage, mutation, and release checks.

## Matrix and assertions

- Shell: macOS 26 Apple Silicon/Intel and Ubuntu 26.04 ARM64/x86-64.
- PowerShell: Windows Server 2022 x86-64, both Windows PowerShell and PowerShell
  Core in published-pair CI; candidate release verification uses Windows PowerShell.
- Homebrew: macOS 26 Apple Silicon/Intel and Ubuntu 26.04 x86-64. Linux ARM64
  archives are verified through shell/manual installation; Homebrew on that
  architecture is not established by this matrix.
- Linux runtime: both GNU architectures execute the archive in the stock
  `ubuntu:26.04` container, with networking disabled and only the binary and
  sample TypeScript project mounted read-only. No Rust, compiler, Node, or
  dependency installation is performed in that container.

The [verification script](../scripts/verify-installation.py) stages verified
archive bytes behind a loopback HTTP server and uses the generated installers'
`TS_CRAP_DOWNLOAD_URL` override. Shell and PowerShell installer contents remain
unchanged. Homebrew formula URLs are redirected to the same staging server;
formula versions, SHA-256 values, and installation logic remain unchanged.
This exercises installer behavior without requiring a candidate to be published.
It does not establish GitHub HTTPS availability or artifact provenance; existing
release provenance checks remain separate.

Each run verifies a fresh install, reinstall, and upgrade between two actual
versions. It compares installed executable bytes with the checksum-verified
archive and checks installer receipts. Homebrew uses a temporary tap, refuses to
replace an existing ts-crap installation, and removes its test installation and
tap afterward. The temporary tap uses `tap-new --no-git`, so it creates no Git
commits and does not require an author identity. Other installers use temporary prefixes/configuration directories,
including spaces and Unicode, with profile/PATH modification disabled. Default
user profile and persistent registry PATH changes are not exercised.

Installed binaries must report the expected version, show help, scan TypeScript
and TSX, return gate failure for threshold zero, accept source-bound full coverage,
reject uncovered code at a strict threshold, and reject stale source. Checksums,
version identity, target identity, and ambiguous executable entries are validated
before running an installer. Python regression tests cover malformed staged
assets independently of networked installation tests.

## Reproduce

Download each version's matching archive, `.sha256` file, and generated installer
into separate directories. For Homebrew, also download `ts-crap.rb` and ensure
Homebrew is installed with no existing ts-crap installation.

```sh
python3 scripts/verify-installation.py \
  --assets /tmp/current-assets --version 0.4.0 \
  --previous-assets /tmp/previous-assets --previous-version 0.3.0 \
  --target aarch64-unknown-linux-gnu --installer shell
```

Use the matching target triple from `--help`. Windows uses `--installer powershell`
(and optionally `--powershell pwsh`); formula checks use `--installer homebrew`.
The script requires Python 3 and the actual installer's prerequisites. It never
changes the caller's HOME, profiles, or global environment variables.

## Verification evidence

Local validation on 2026-10-26 exercised the published ARM64 Linux shell
installers on Ubuntu 26.04/Termux proot: fresh install, reinstall, and v0.3.0 to
v0.4.0 upgrade, including TS/TSX and coverage-gate smoke checks. Docker, PowerShell,
and Homebrew are unavailable in that local environment, so their results must
come from hosted CI. This matrix is not evidence of compatibility with older
macOS or Windows releases.

Hosted installation CI for [PR #24](https://github.com/RuiCostafrg/typescript-crap/pull/24)
passed every advertised installer target. The
[candidate release dry run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40415478665)
on exact main commit `ed9105e820e8dddf4ba302d068389d3911114766` passed all five
archives, candidate installers/upgrades, installed helper bundles, three Homebrew
targets, and both clean Linux runtimes.

The provenance-verified live tap [v0.4.0 update](https://github.com/RuiCostafrg/homebrew-tap/pull/8)
is merged; [required Tap CI](https://github.com/RuiCostafrg/homebrew-tap/actions/runs/40418867847)
passed on its exact head. The manual `verify-published.yml` workflow additionally
checks live formula bytes against the published asset, installs from unchanged
GitHub URLs, upgrades from the exact previous formula in tap history with
`brew update`/`brew upgrade`, and repeats fresh/reinstall TS/TSX/coverage checks on
macOS ARM64/Intel and Linux x86-64. It clones the installed Homebrew implementation
into a disposable directory under the runner's home, with its own empty Cellar,
taps, cache, logs, and config. The prefix is outside OS temporary trees that
Homebrew grants sandboxed workers permission to write; runtime temporary/cache
paths are sibling directories. Homebrew's sandbox remains enabled.
The system installation supplies the initial implementation; the clone's origin
is the official Homebrew repository, so `brew update` remains real. Prefix/Cellar/
cache identities are checked before package operations. The caller's existing
packages and taps do not participate, and the temporary installation is removed
afterward. This live-path test covers a nondefault prefix; the separate staged
installer matrix exercises the hosted runner's standard Homebrew prefix.

After each new release, run that workflow with its version and an explicitly
selected, distinct previous published upgrade fixture. There is no default
previous version: use `version=2.2.0`, `previous-version=2.1.0` for v2.2.0
verification. Historical rechecks use `version=2.0.0`, `previous-version=1.0.0`
for v2.0.0, or `version=1.0.0`, `previous-version=0.4.0` for v1.
Leave guide verification enabled for releases containing helpers.
For the legacy v0.4.0 tap check only, disable guide verification and select v0.3.0
as the previous version. A green candidate run remains distinct from a green
published-path run; publication evidence must be recorded after it occurs.

The live-tap verifier allows up to ten minutes for an explicit `brew update`,
which refreshes the disposable Homebrew implementation and its live tap. Other
installer commands retain their three-minute deadline; the job retains its
20-minute limit. Timeout failures include bounded partial stdout/stderr. These
are verification-runner limits, not product subprocess guarantees.

The [full live-tap run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40420847643)
on `bf8a52d7be37c871939746bfe3ceae807e2bdf49` passed on all three Homebrew
targets, including the Intel macOS cold-update path. It verified the published
v0.4.0 formula bytes, actual GitHub downloads, v0.3.0 upgrades, fresh installs,
reinstalls, and installed TS/TSX/coverage behavior. New releases still require
verification of their own published assets after tap synchronization.

A prior Intel runner timeout was traced to the system `brew update` migrating its
preinstalled Azure CLI into a cask and compiling an unrelated OpenSSL dependency.
The verifier now has an empty disposable Cellar, so unrelated runner packages
cannot trigger that migration. It still runs unmodified Homebrew code, published
formula URLs/checksums, `brew update`, and `brew upgrade`; no upgrade assertion or
quality gate is skipped. The matrix records its tested prefix and platforms;
Homebrew's upstream platform support remains separate. The 2026-10-27 Intel
job emitted Homebrew's unsupported-platform warning; ts-crap's shell/manual
Intel archives retain their separately tested installation paths.

The immutable [v1.0.0 release](https://github.com/RuiCostafrg/typescript-crap/releases/tag/v1.0.0)
selects `4ce57b9aa3b9979166965aebbf15e16eabb6ccac`. Its
[tag-triggered workflow](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40464740215)
passed all five archives, candidate helper/guide checks, real installer upgrades,
three Homebrew targets, and both clean Linux runtimes before protected publication.
The provenance-verified [v1.0.0 tap update](https://github.com/RuiCostafrg/homebrew-tap/pull/9)
merged after all required checks passed. The subsequent
[published-adoption run](https://github.com/RuiCostafrg/typescript-crap/actions/runs/40494926411)
passed every job: installed CLI/helpers and the CI guide on all five platforms,
plus live v0.4.0 to v1.0.0 Homebrew upgrades, reinstalls, and fresh installs on
macOS ARM64/Intel and Linux x86-64. The [acceptance ledger](RELEASE_V1.md)
records the exact source, verifier, tap commits, and helper checksum.
