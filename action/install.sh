#!/usr/bin/env bash
# Installs a published ts-crap release for the GitHub Action (see action.yml).
# Downloaded release files are data: nothing downloaded runs before its build
# provenance and SHA-256 checksum verify, and no remote script is executed.
set -euo pipefail

# The release source is fixed. It is deliberately not an input, so a workflow
# cannot redirect the action to another repository's releases or signer.
readonly REPOSITORY=RuiCostafrg/typescript-crap
# Exact signing-certificate identity: release.yml at the version tag. An exact
# --cert-identity match is used instead of --signer-workflow, which gh 2.101.0
# and older match only as a prefix (GHSA-wjmr-j3rp-mh2g), and it is
# case-sensitive, unlike their --source-ref (GHSA-4mq3-hpgx-9cx8). It works on
# older gh too, such as 2.98.0 on the macos-26 Intel image.
readonly SIGNER_URL="https://github.com/$REPOSITORY/.github/workflows/release.yml"

fail() {
  echo "::error title=ts-crap setup::$*" >&2
  exit 1
}

version=${TS_CRAP_ACTION_VERSION:-}
version=${version#v}
if [[ ! "$version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-[0-9A-Za-z.-]+)?$ ]]; then
  fail "version must be a published release such as 2.2.1 (got '${TS_CRAP_ACTION_VERSION:-}'); 'latest' is not accepted"
fi
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]} prerelease=${BASH_REMATCH[4]}
# 2.2.1 is the first release with static musl Linux archives on every architecture.
if (( 10#$major < 2 || (10#$major == 2 && (10#$minor < 2 || (10#$minor == 2 && 10#$patch < 1))) )) \
  || [[ "$major.$minor.$patch" == 2.2.1 && -n "$prerelease" ]]; then
  fail "version $version is not supported; the action requires 2.2.1 or newer"
fi

for pair in helpers:HELPERS scan:SCAN sarif:SARIF upload-sarif:UPLOAD; do
  variable=TS_CRAP_ACTION_${pair#*:}
  # Unset means the default; an explicitly empty input is invalid.
  value=${!variable-false}
  [[ "$value" == true || "$value" == false ]] || fail "${pair%%:*} must be true or false"
done
helpers=${TS_CRAP_ACTION_HELPERS:-false}
# Fail before installing rather than silently skipping a requested report.
if [[ "${TS_CRAP_ACTION_UPLOAD:-false}" == true && "${TS_CRAP_ACTION_SARIF:-false}" != true ]]; then
  fail "upload-sarif requires sarif: true"
fi
if [[ "${TS_CRAP_ACTION_SARIF:-false}" == true && "${TS_CRAP_ACTION_SCAN:-false}" != true ]]; then
  fail "sarif requires scan: true"
fi

case "${RUNNER_OS:-}/${RUNNER_ARCH:-}" in
  Linux/X64) target=x86_64-unknown-linux-musl ;;
  Linux/ARM64) target=aarch64-unknown-linux-musl ;;
  macOS/X64) target=x86_64-apple-darwin ;;
  macOS/ARM64) target=aarch64-apple-darwin ;;
  Windows/X64) target=x86_64-pc-windows-msvc ;;
  *) fail "no ts-crap release build for runner ${RUNNER_OS:-unknown}/${RUNNER_ARCH:-unknown}" ;;
esac
if [[ "$target" == *windows* ]]; then
  archive="ts-crap-$target.zip"
  binary=ts-crap.exe
else
  archive="ts-crap-$target.tar.xz"
  binary=ts-crap
fi

native_path() {
  if [[ "${RUNNER_OS:-}" == Windows ]]; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

# Hash from stdin: with a file argument, GNU sha256sum prefixes the digest
# with a backslash when the path contains one (Windows RUNNER_TEMP).
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum < "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 < "$1" | cut -d ' ' -f 1
  fi
}

# Accept only "<64 hex> [*]<expected name>" so a checksum file cannot vouch for
# a different file than the one being installed.
verify_checksum() {
  local file=$1 line expected name actual
  [[ -f "$file.sha256" ]] || fail "missing checksum file for $(basename "$file")"
  line=$(tr -d '\r' < "$file.sha256" | sed '/^$/d')
  [[ "$line" =~ ^([0-9a-fA-F]{64})\ \ ?\*?(.+)$ ]] || fail "malformed checksum file for $(basename "$file")"
  expected=$(printf '%s' "${BASH_REMATCH[1]}" | tr 'A-F' 'a-f')
  name=${BASH_REMATCH[2]}
  [[ "$name" == "$(basename "$file")" ]] || fail "checksum file names '$name', not $(basename "$file")"
  actual=$(sha256 "$file")
  [[ "$actual" == "$expected" ]] || fail "SHA-256 mismatch for $(basename "$file")"
}

command -v gh >/dev/null 2>&1 \
  || fail "the GitHub CLI (gh) is required; it is preinstalled on GitHub-hosted runners"
work=$(mktemp -d "${RUNNER_TEMP:?RUNNER_TEMP is not set}/ts-crap-action.XXXXXX")
download="$work/download"
mkdir "$download" "$work/tool"
patterns=(--pattern "$archive" --pattern "$archive.sha256")
assets=("$archive")
if [[ "$helpers" == true ]]; then
  patterns+=(--pattern ts-crap-coverage-helpers.zip --pattern ts-crap-coverage-helpers.zip.sha256)
  assets+=(ts-crap-coverage-helpers.zip)
fi
gh release download "v$version" --repo "$REPOSITORY" "${patterns[@]}" --dir "$download" \
  || fail "could not download v$version assets for $target from $REPOSITORY"

for asset in "${assets[@]}"; do
  [[ -f "$download/$asset" ]] || fail "release v$version has no $asset"
  gh attestation verify "$download/$asset" --repo "$REPOSITORY" \
    --cert-identity "$SIGNER_URL@refs/tags/v$version" --source-ref "refs/tags/v$version" \
    --deny-self-hosted-runners >/dev/null \
    || fail "build provenance verification failed for $asset"
  verify_checksum "$download/$asset"
done

if [[ "$archive" == *.zip ]]; then
  unzip -q "$download/$archive" -d "$work/tool"
else
  tar -xf "$download/$archive" -C "$work/tool"
fi
find "$work/tool" -type f -name "$binary" > "$work/executables"
count=$(wc -l < "$work/executables" | tr -d ' ')
[[ "$count" == 1 ]] || fail "expected exactly one $binary in $archive, found $count"
executable=$(cat "$work/executables")
# The token is only for gh; the downloaded binary never sees it.
reported=$(unset GH_TOKEN GITHUB_TOKEN; "$executable" --version | tr -d '\r')
[[ "$reported" == "ts-crap $version" ]] || fail "installed binary reports '$reported', expected 'ts-crap $version'"

bin_dir=$(dirname "$executable")
native_path "$bin_dir" >> "$GITHUB_PATH"
{
  echo "version=$version"
  echo "target=$target"
  echo "bin-dir=$(native_path "$bin_dir")"
  echo "executable=$(native_path "$executable")"
} >> "$GITHUB_OUTPUT"

if [[ "$helpers" == true ]]; then
  unzip -q "$download/ts-crap-coverage-helpers.zip" -d "$work/helpers"
  [[ -f "$work/helpers/ts-crap-coverage-helpers/coverage.mjs" ]] \
    || fail "coverage helper bundle has an unexpected layout"
  helpers_dir=$(native_path "$work/helpers/ts-crap-coverage-helpers")
  echo "TS_CRAP_HELPERS=$helpers_dir" >> "$GITHUB_ENV"
  echo "helpers-dir=$helpers_dir" >> "$GITHUB_OUTPUT"
elif [[ "${TS_CRAP_HELPERS:-}" == *ts-crap-action.*[/\\]helpers[/\\]ts-crap-coverage-helpers ]]; then
  # An earlier run of this action exported helpers for a different install.
  echo "TS_CRAP_HELPERS=" >> "$GITHUB_ENV"
fi
echo "Installed $reported ($target) from attested $REPOSITORY release v$version"
