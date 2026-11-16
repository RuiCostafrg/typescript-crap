#!/usr/bin/env bash
# Installs a published ts-crap release for the GitHub Action (see action.yml).
# Downloaded release files are data: nothing downloaded runs or is extracted
# before its build provenance verifies and its SHA-256 matches the digest that
# the verified attestation records for it by name. No remote script is executed.
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

# On any exit, give a background trusted-root fetch up to 30 seconds to finish
# rather than leave it running or cut it off mid-write, which could leave a
# half-written TUF cache for later gh calls in the job. A fetch that is still
# stalled after that is stopped. The script's exit status is kept.
root_pid=
finish_root_fetch() {
  [[ -n "$root_pid" ]] || return 0
  local tenths=0
  while kill -0 "$root_pid" 2>/dev/null && (( tenths < 300 )); do
    sleep 0.1
    tenths=$(( tenths + 1 ))
  done
  kill "$root_pid" 2>/dev/null || true
  wait "$root_pid" 2>/dev/null || true
}
trap finish_root_fetch EXIT

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

# One release attestation lists every release asset as a named subject. gh
# only checks that some subject has the file's digest, so also require every
# subject named like this asset to carry exactly its SHA-256: another attested
# asset (say, a different platform's archive) cannot stand in for it. This
# replaces the unsigned .sha256 sidecar, which is still published for manual use.
verify_attested_digest() {
  local file=$1 record=$2 name actual digest found=
  name=$(basename "$file")
  actual=$(sha256 "$file")
  while IFS= read -r digest || [[ -n "$digest" ]]; do
    digest=${digest%$'\r'}
    [[ -z "$digest" ]] && continue
    [[ "$digest" == "$actual" ]] || fail "SHA-256 of $name does not match the digest its attestation records"
    found=1
  done < "$record"
  [[ -n "$found" ]] || fail "the verified attestation records no digest for $name"
}

command -v gh >/dev/null 2>&1 \
  || fail "the GitHub CLI (gh) is required; it is preinstalled on GitHub-hosted runners"
work=$(mktemp -d "${RUNNER_TEMP:?RUNNER_TEMP is not set}/ts-crap-action.XXXXXX")
download="$work/download"
mkdir "$download" "$work/tool"
patterns=(--pattern "$archive")
assets=("$archive")
if [[ "$helpers" == true ]]; then
  patterns+=(--pattern ts-crap-coverage-helpers.zip)
  assets+=(ts-crap-coverage-helpers.zip)
fi

# gh refreshes the Sigstore trusted root over TUF on every verification, which
# is most of its runtime. Fetch it once per install, through the same TUF
# client, in the background while the assets download. Its errors are printed
# after the download finishes, so the two outputs never interleave.
trusted_root="$work/trusted-root.jsonl"
gh attestation trusted-root > "$trusted_root" 2> "$work/trusted-root.err" &
root_pid=$!

# GitHub occasionally answers release downloads with a transient HTTP 5xx or a
# network error, so try up to three times. "release not found" is retried too:
# gh reports it when its published and draft lookups both fail, even if the
# published lookup failed transiently. A missing asset in a found release, and
# HTTP 4xx other than 408 and 429, repeat identically and stop at once.
attempt=1
until gh release download "v$version" --repo "$REPOSITORY" "${patterns[@]}" --dir "$download" \
  --clobber 2> "$work/download.err"; do
  cat "$work/download.err" >&2
  if (( attempt >= 3 )) || grep -q -e 'no assets match' "$work/download.err" \
    || { grep -q -E 'HTTP 4[0-9]{2}' "$work/download.err" && ! grep -q -E 'HTTP (408|429)' "$work/download.err"; }; then
    fail "could not download v$version assets for $target from $REPOSITORY"
  fi
  sleep $(( attempt * 2 ))
  attempt=$(( attempt + 1 ))
done

# Check every asset first, so a missing one can't leave a verifier running.
for asset in "${assets[@]}"; do
  [[ -f "$download/$asset" ]] || fail "release v$version has no $asset"
done
root_status=0
wait "$root_pid" || root_status=$?
root_pid=
cat "$work/trusted-root.err" >&2
(( root_status == 0 )) || fail "could not fetch the Sigstore trusted root"
[[ -s "$trusted_root" ]] || fail "the Sigstore trusted root is empty"

pids=()
for asset in "${assets[@]}"; do
  # Asset names come from the fixed list above (no quotes or backslashes), so
  # they are safe inside the jq string literal.
  gh attestation verify "$download/$asset" --repo "$REPOSITORY" \
    --cert-identity "$SIGNER_URL@refs/tags/v$version" --source-ref "refs/tags/v$version" \
    --deny-self-hosted-runners --custom-trusted-root "$trusted_root" --format json \
    --jq ".[].verificationResult.statement.subject[] | select(.name == \"$asset\") | .digest.sha256" \
    > "$work/$asset.attested" &
  pids+=("$!")
done
# Wait for every verification before failing, so none outlives this script.
unverified=
for index in "${!pids[@]}"; do
  wait "${pids[$index]}" || unverified="$unverified ${assets[$index]}"
done
[[ -z "$unverified" ]] || fail "build provenance verification failed for$unverified"
for asset in "${assets[@]}"; do
  verify_attested_digest "$download/$asset" "$work/$asset.attested"
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
