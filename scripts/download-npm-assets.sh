#!/usr/bin/env bash
# Download the release archives that the npm platform packages repackage, plus
# the helper bundle, and verify each one's build provenance and SHA-256.
# Usage: download-npm-assets.sh VERSION NEW_EMPTY_DIRECTORY
set -euo pipefail
version=$1 directory=$2
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "invalid version: $version" >&2; exit 1; }
repository=${GITHUB_REPOSITORY:-RuiCostafrg/typescript-crap}
# Exact signer identity (release.yml at this tag); see action/install.sh for why
# --cert-identity is used instead of --signer-workflow.
identity="https://github.com/$repository/.github/workflows/release.yml@refs/tags/v$version"
mkdir "$directory"
assets=(ts-crap-x86_64-unknown-linux-musl.tar.xz ts-crap-aarch64-unknown-linux-musl.tar.xz
        ts-crap-x86_64-apple-darwin.tar.xz ts-crap-aarch64-apple-darwin.tar.xz
        ts-crap-x86_64-pc-windows-msvc.zip ts-crap-coverage-helpers.zip)
patterns=()
for asset in "${assets[@]}"; do patterns+=(--pattern "$asset" --pattern "$asset.sha256"); done
gh release download "v$version" --repo "$repository" --dir "$directory" "${patterns[@]}"
cd "$directory"
for asset in "${assets[@]}"; do
  gh attestation verify "$asset" --repo "$repository" --cert-identity "$identity" \
    --source-ref "refs/tags/v$version" --deny-self-hosted-runners >/dev/null
  if command -v sha256sum >/dev/null 2>&1; then sha256sum --check --quiet "$asset.sha256"
  else shasum -a 256 --check --quiet "$asset.sha256"; fi
done
echo "Authenticated ${#assets[@]} v$version release assets in $directory"
