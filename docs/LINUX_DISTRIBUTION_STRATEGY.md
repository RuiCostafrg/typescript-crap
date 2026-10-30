# Linux distribution strategy

Drafted 2026-10-30 for discussion. Nothing here is implemented yet. See
[ROADMAP_V2_3.md](ROADMAP_V2_3.md) for the feature roadmap and
[LINUX_DISTRIBUTION.md](LINUX_DISTRIBUTION.md) for what ships today.

## Who installs ts-crap, and how

ts-crap users are TypeScript developers and their CI pipelines. They already
have Node and a package manager (npm, pnpm, yarn or bun). They expect three
things:

1. **Pin a version in the project.** The tool version should be recorded in
   `package.json` and the lockfile, so local runs and CI use exactly the same
   tool.
2. **Zero extra setup in CI.** `npm ci` (or equivalent) should be enough.
3. **One-off use.** `npx <tool>` should work without a global install.

System package managers (APT, DNF, pacman) serve a different audience:
machine-wide tools at one version for every project. A distro package doesn't
help a TypeScript developer pin the tool per project.

## What exists today (v2.2.1)

| Channel | Linux status | Notes |
| --- | --- | --- |
| Shell installer from the release tag | ✅ x86-64, ARM64 | cargo-dist installer. Each archive's SHA-256 is embedded. It uses the GNU build when glibc ≥ 2.43 and otherwise falls back to the **static musl** build, so it also works on older distros and Alpine. Installs to `~/.local/bin`. |
| Manual archives (`.tar.xz`) | ✅ GNU and static musl | Each has a `.sha256` and GitHub build provenance (`gh attestation verify`). |
| Debian packages (`.deb`) | ✅ amd64, arm64 | Release assets installed with `apt install ./file.deb`. There is no APT repository, so there are no `apt upgrade` updates. |
| Homebrew tap | ✅ Linux x86-64 | Owner-approved tap PRs; provenance is checked before every update. |
| npm | ❌ | The name `ts-crap` belongs to an unrelated package. |
| crates.io / cargo-binstall | ❌ | Planned in the roadmap (item 2.6). |
| APT/RPM repo, AUR, Nix, container image, GitHub Action | ❌ | — |

The GNU build needs glibc 2.43 (Ubuntu 26.04). Most current CI and LTS
distributions are older, so in practice the **static musl binary is the
portable Linux artifact**. It has no runtime library dependencies.

## Evaluation criteria

- **Reach:** does the channel serve TypeScript developers where they already
  work?
- **Pinning:** is the version recorded per project, so CI is reproducible?
- **Security fit:** does it work with the current trust model? That model allows
  no long-lived publishing secrets, requires owner approval before publishing,
  attaches provenance to every artifact, and treats artifacts as data.
- **Maintenance:** how much ongoing work for one maintainer?
- **Verification:** can CI install the published artifact in clean containers
  before and after release?

## Options

### A. npm package with a prebuilt binary — recommended primary channel

Use the established pattern from esbuild, Biome and oxlint. A small main package
declares one `optionalDependencies` entry per platform package, and each
platform package carries a single prebuilt binary:

```text
<name>                        main package: launcher + coverage helpers
<name>-linux-x64              static musl binary (works on glibc and musl)
<name>-linux-arm64            static musl binary
<name>-darwin-x64 / -arm64    macOS binaries
<name>-win32-x64              Windows binary
```

npm installs only the matching platform package, using its `os` and `cpu`
fields.

- **No `postinstall` download.** The binary comes from the npm registry like any
  other dependency. This works offline from a cache and with `--ignore-scripts`.
  It also works with pnpm, which blocks install scripts by default, and behind
  corporate registry mirrors.
- **The static musl build removes the glibc/musl split on Linux.** There is one
  package per CPU and no `libc` field. It covers Ubuntu, Debian, RHEL and
  Alpine, including old LTS releases.
- **The main package bundles the Node coverage helpers.** The scanner and
  helpers must be the exact same version (`docs/COVERAGE_HELPERS.md:24`). One
  npm package guarantees that. It also gives the Vitest/Jest one-step command
  (roadmap 1.1) a natural home:
  `npx <name> coverage --framework vitest`.
- **Security fit:**
  - Publishing uses npm trusted publishing (OIDC) from the existing release
    workflow, with provenance and no npm token stored anywhere.
  - Publishing happens behind the same owner-approved `release` environment.
  - Every platform binary is byte-identical to the attested GitHub release
    archive, and CI checks this before publishing.
  - Trade-off: npm packages can't be made immutable like GitHub releases. A
    published version can't be republished, but it can be deprecated, and
    within 72 hours unpublished.
- **Name:** `ts-crap` is taken.
  - Free options found on 2026-10-30: `typescript-crap` (matches the
    repository), `@ruicostafrg/ts-crap` (needs an npm user or organization
    named `ruicostafrg`), `ts-crap-cli` and `tscrap`.
  - The installed command stays `ts-crap` in every case.
- **Verification:**
  - Install from the registry with npm, pnpm, yarn and bun on clean
    Ubuntu/Debian/Alpine/Fedora, macOS and Windows runners.
  - Check that the binary matches the release and that `npx` works.
  - Run the published-release check after publishing, as Homebrew does today.
- **Effort:** M–L. Most of the work is the publish job and its verification;
  the packages themselves are small.

### B. Shell installer from the release tag — keep and harden

This already exists and works on any distro. Improvements:

- Document version-pinned installs for CI:
  `…/releases/download/v2.2.1/ts-crap-installer.sh`. Today's documented `latest`
  URL changes under users.
- Document a verify-before-run flow: download the installer, check it with
  `gh attestation verify`, then run it. Piping a URL into `sh` remains the quick
  path.
- Consider making Linux always prefer the static musl build. That gives one
  tested Linux binary and removes the glibc-2.43 GNU path.
- **Effort:** S.

### C. GitHub Action — recommended for CI

`uses: RuiCostafrg/typescript-crap@v2` with `version:` input.

- It downloads the release archive, verifies its checksum and attestation, and
  adds the binary to `PATH`.
- It can run the scan with SARIF upload as an option.
- Projects already on npm (option A) don't need it, but non-Node pipelines and
  security-sensitive users benefit.
- **Effort:** S–M.

### D. crates.io and cargo-binstall

This is roadmap item 2.6. It serves Rust users and source builds.
`cargo binstall ts-crap` can fetch the attested release archives. Publishing
would use crates.io trusted publishing, with no token.

- **Effort:** S.

### E. Debian/Ubuntu APT repository — defer

This gives `apt install ts-crap` and `apt upgrade`.

- It needs a repository signing key, which is a long-lived secret. That
  conflicts with the current "no persistent secrets" model unless the key stays
  offline and the owner signs each release by hand.
- Hosting is also needed: GitHub Pages, or a service such as Cloudsmith or
  packagecloud.
- Direct `.deb` downloads already cover one-off installs.
- **Revisit when** users ask for managed upgrades.
- **Effort:** M to set up, plus ongoing key management.

### F. RPM / Fedora COPR — defer

`.rpm` assets could be built the same way as the `.deb` packages. A COPR
repository would build from source on Fedora's infrastructure.

- The static musl tarball and the npm package already work on Fedora and RHEL.
- **Revisit on demand.**
- **Effort:** S–M for assets, M for COPR.

### G. AUR and Nix — community channels

- An AUR `ts-crap-bin` PKGBUILD or a nixpkgs entry pointing at the release
  archives is cheap. These are usually maintained by community packagers, and
  they need no secret in this repository.
- Welcome them, and link to them from the README once they exist.
- **Effort:** S each.

### H. Container image (GHCR) — optional

- `ghcr.io/ruicostafrg/ts-crap:2.2.1` built `FROM scratch` with the static musl
  binary.
- Publishing would use the workflow `GITHUB_TOKEN` and attach provenance.
- It is useful for container-native CI. Coverage helpers would need a Node
  variant.
- **Effort:** S–M.

### I. Snap / Flatpak — not recommended

They're designed for desktop apps. Sandboxing gets in the way of a CLI that
reads project files and runs Git and Node, and they add a store account and
review for little benefit.

## Recommendation

| Priority | Channel | Why |
| --- | --- | --- |
| 1 | **npm package** (A) | Fits how TypeScript users install and pin dev tools, and works in CI with no extra setup. It also keeps the helpers and scanner on the same version and is the home for the Vitest/Jest one-step command. |
| 2 | **Shell installer hardening** (B) | Cheap. Version-pinned and verifiable installs on any distro. |
| 3 | **GitHub Action** (C) | One-line, verified CI setup for non-Node pipelines. |
| 4 | **crates.io** (D) | Already on the roadmap; cheap. |
| Later | GHCR image (H), AUR/Nix (G), RPM (F), APT repository (E) | Add when users ask for them. |
| No | Snap/Flatpak (I) | Poor fit for a CLI. |

Release suggestion: ship npm (A) and installer hardening (B) in **v2.3.0**,
next to the Vitest/Jest work. The one-step coverage command then reaches
users as `npx typescript-crap coverage --framework vitest`. Add C and D in
v2.4.0.

## Decisions needed from the owner

1. **npm package name:**
   - `typescript-crap` (recommended: free and matches the repository)
   - `@ruicostafrg/ts-crap` (needs an npm user or organization `ruicostafrg`)
   - another name
2. **npm account setup.** Create or confirm the npm account or organization,
   enable 2FA, and configure trusted publishing for this repository's
   `release.yml` and the `release` environment. Only the owner can do this.
3. **Linux default binary.** Should every channel use the static musl build on
   Linux (recommended), or keep GNU where glibc ≥ 2.43?
4. **Whether to pursue an APT repository** despite the signing-key requirement,
   or keep `.deb` assets as direct downloads only (recommended for now).
