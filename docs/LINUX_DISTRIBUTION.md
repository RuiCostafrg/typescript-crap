# Linux distribution

v2.2.1 adds static musl archives and Debian packages for x86-64 and
ARM64. v2.1.0 and earlier contain only the existing GNU Linux archives; their
Ubuntu 26.04 runtime boundary remains unchanged. Check a release's asset list
before using the commands below. No signed APT repository or RPM is provided.

## Portable archives

Select `ts-crap-x86_64-unknown-linux-musl.tar.xz` for x86-64 or
`ts-crap-aarch64-unknown-linux-musl.tar.xz` for ARM64. Download the archive and
its `.sha256` file from the same release. For example, after downloading the
x86-64 pair into an empty directory:

```sh
sha256sum --check ts-crap-x86_64-unknown-linux-musl.tar.xz.sha256
tar -xf ts-crap-x86_64-unknown-linux-musl.tar.xz
find . -type f -name ts-crap
```

Place the extracted executable in a directory on your PATH, retaining the
included license notices. The CLI needs neither Node, Rust nor a dynamic libc
loader. Git remains necessary for Git diff features. These archives are useful
on Alpine and older glibc userlands; they are not Android/Termux builds. The
container matrix does not establish compatibility with older Linux kernels.
The generated shell installer retains its platform selection: the GNU build
when the system glibc is 2.43 or newer, otherwise the static musl build. The
portable matrix tests manual archive execution. Installation CI separately runs
the published, provenance-verified v2.2.1 installer on hosted Ubuntu 26.04
(GNU selected) and Ubuntu 24.04 (musl fallback) on both architectures and
compares the installed bytes with the attested archive. A companion job runs
the attested installer inside stock Debian 12 (glibc 2.36) and Alpine 3.22 images on
both architectures and checks that it installs the static musl build. Other
distributions are expected to follow the same glibc/musl selection but are not
verified by these jobs.

## Debian and Ubuntu packages

Choose `amd64` for x86-64 or `arm64` for ARM64. A stable upstream version `X.Y.Z`
produces `ts-crap_X.Y.Z-1_amd64.deb` or `ts-crap_X.Y.Z-1_arm64.deb`. Prereleases
retain upstream SemVer in asset filenames, for example
`ts-crap_X.Y.Z-rc.1-1_amd64.deb`. The internal Debian Version field uses an
order-preserving encoding: numeric identifiers have an `n` prefix, nonnumeric
identifiers have an `s` prefix with two lowercase letters per ASCII character,
and `~` separates and terminates identifiers. Thus `X.Y.Z-rc.1` becomes
`X.Y.Z~sekdv~n1~-1` internally, preserving SemVer ordering under dpkg, including
numeric versus alphabetic and lexical versus numeric comparisons. Stable
versions remain `X.Y.Z-1`. Build metadata is not accepted by release packaging.

Set `package` to the downloaded filename, with its accompanying checksum in the
same directory, then authenticate and install it:

```sh
package=ts-crap_X.Y.Z-1_amd64.deb  # replace X.Y.Z with a release containing packages
sha256sum --check "$package.sha256"
sudo apt install "./$package"
ts-crap --version
```

Checksums detect corruption; verify provenance before installation when trusting
an artifact's origin. With GitHub CLI installed, set `version` to the upstream
version without `v` and verify both downloaded files:

```sh
version=X.Y.Z
for asset in "$package" "$package.sha256"; do
  gh attestation verify "$asset" --repo RuiCostafrg/typescript-crap \
    --signer-workflow RuiCostafrg/typescript-crap/.github/workflows/release.yml \
    --source-ref "refs/tags/v$version" --deny-self-hosted-runners
done
```

The package owns `/usr/bin/ts-crap`, helper files under
`/usr/share/ts-crap/coverage-helpers`, and notices under `/usr/share/doc/ts-crap`.
It has no maintainer scripts and does not edit shell profiles, collect coverage,
or install language toolchains. Helpers are optional and require Node 24+;
see [their usage guide](COVERAGE_HELPERS.md). They are not needed for CLI scans.

Install a newer downloaded package with the same `apt install ./file.deb` command.
Remove it with `sudo apt remove ts-crap`. Project files and reports are outside
package ownership. Existing shell or Homebrew installations can shadow
`/usr/bin/ts-crap`; check `command -v ts-crap` and use one installation method.
There is no configured APT source, so `apt upgrade` cannot fetch newer releases.

## CI and release gates

[Linux distribution CI](../.github/workflows/linux-distribution.yml) builds both
architectures natively with Rust 1.90 and musl. It rejects ELF interpreters and
shared-library dependencies, checks package architecture, helper versions and
asset checksums, and rebuilds each Debian package twice to compare exact bytes.
Package builds sharing an assets directory hold a Linux advisory directory lock
through package and aggregate-checksum publication, preventing concurrent
architecture builds from losing checksum entries.
Local unit tests also vary the umask and test malformed ELF, checksum binding,
symlink outputs and unexpected helper archive entries. Structural ELF fixtures
are not substitutes for executing actual release binaries.

The runtime matrix uses Ubuntu 22.04, 24.04 and 26.04; Debian 12 and 13 slim;
Alpine 3.22; and Fedora 43. Both native architectures run actual scanners against
known scores, coverage and failing gates without network access or installed
Node/Rust. Debian and Ubuntu additionally install, upgrade, reinstall and remove
the real `.deb`, checking obsolete package files disappear and user files remain.
Because no historical `.deb` exists yet, the upgrade fixture is explicitly a
revision-zero package built from the same upstream binary with an extra obsolete
owned helper file (outside documentation paths that minimal images can exclude).
The package archive must contain the musl notice, and its standard `copyright`
file also includes the notice so minimal images retain it when excluding other
documentation. The fixture proves Debian package upgrade behavior, not compatibility with a
previous upstream binary.

Containers share the runner kernel. Images are pulled before network-isolated
execution; reports record image names, content IDs and checks, with artifacts retained for
14 days. These checks become compatibility evidence only after hosted CI passes.

The release workflow requires Linux distribution CI on its exact main commit,
then assembles both musl archives and packages alongside the existing GNU,
macOS and Windows assets. Candidate verification repeats the native container
matrix before protected publication. All 25 published files are allowlisted;
archive and package checksums and release provenance use the existing hardened
publisher. Musl's copyright notice accompanies the static executables.

After v2.2.1 or a later release containing these assets, run published adoption with
`verify-linux=true`, alongside the normal guide and Homebrew verification:

```sh
gh workflow run verify-published.yml -f version=X.Y.Z \
  -f previous-version=PREVIOUS -f verify-guide=true -f verify-linux=true
```

The Linux jobs download the actual published files, verify each attestation,
then repeat the runtime and package lifecycle tests. Leave `verify-linux=false`
when rechecking v2.1.0 or earlier. Candidate success alone does not establish
published-path success.

To reproduce with Docker and `dpkg-deb` on the matching native architecture:

```sh
python3 scripts/verify-linux-distribution.py --assets /path/to/downloaded-assets \
  --target x86_64-unknown-linux-musl --version X.Y.Z --report linux-report.json
```
