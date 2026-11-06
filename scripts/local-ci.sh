#!/usr/bin/env bash
# typescript-crap local CI — Linux only, powered by basecamp/gh-signoff.
# Usage:
#   scripts/local-ci.sh               stages: tests lint docs
#   scripts/local-ci.sh tests         selected stages only
#   scripts/local-ci.sh --list        list stages (incl. optional coverage security)
#   scripts/local-ci.sh --no-signoff  run checks, don't post signoff statuses
#   scripts/local-ci.sh --help        this text
set -euo pipefail

AVAILABLE_STAGES=(tests lint docs coverage security)
DEFAULT_STAGES=(tests lint docs)

_fail()   { printf '✗ %s\n' "$*" >&2; exit 1; }
_log()    { printf '▸ %s\n' "$*"; }
_ok()     { printf '✓ %s\n' "$*"; }
require() { command -v "$1" >/dev/null 2>&1 || _fail "missing '$1'${2:+ — $2}"; }

node_min() {
  require node ""
  local major; major="$(node --version | sed 's/^v//' | cut -d. -f1)"
  [ "$major" -ge "$1" ] || _fail "Node.js >= $1 required (found v$major)"
}

case "$(uname -s)" in
  Linux) ;;
  *) _fail "Linux-only local CI; refusing to run on $(uname -s) (macOS/Windows stay in GitHub Actions)." ;;
esac

NO_SIGNOFF=0
SELECTED=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-signoff) NO_SIGNOFF=1 ;;
    --list) printf '%s\n' "stages: ${AVAILABLE_STAGES[*]}"; exit 0 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    -*) _fail "unknown option: $1 (try --help)" ;;
    *) SELECTED+=("$1") ;;
  esac
  shift
done
[ ${#SELECTED[@]} -gt 0 ] || SELECTED=("${DEFAULT_STAGES[@]}")

if [ "$NO_SIGNOFF" = 0 ]; then
  require gh ""
  gh extension list 2>/dev/null | grep -qi "gh-signoff" \
    || _fail "gh-signoff extension missing — run: gh extension install basecamp/gh-signoff (or pass --no-signoff)"
fi

node_min 24

# ── stages ────────────────────────────────────────────────────────────────────

stage_tests() {
  require cargo "Rust toolchain (https://rustup.rs) — CI matrix uses 1.90 and stable"
  require python3 ""
  python3 -B -m unittest discover -s tests -p 'test_*.py'
  cargo test --locked
  TS_CRAP_SCANNER="$PWD/target/debug/ts-crap" node --test tests/*.test.mjs
  cargo test --locked --release
}

stage_lint() {
  require cargo ""
  cargo fmt --check
  # Publishable package contents: no generated/private artifacts.
  if cargo package --list | grep -E '^(reports|coverage|target)/|(^|/)__pycache__/|\.py[co]$'; then
    _fail "generated/private artifacts must not be packaged"
  fi
  cargo clippy --locked --all-targets -- -D warnings
}

stage_docs() {
  require cargo ""
  RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
  for f in packaging/npm/bin/*.js packaging/npm/lib/*.js; do node --check "$f"; done
}

stage_coverage() {
  # Optional; needs cargo-llvm-cov.
  require cargo ""
  if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    _fail "cargo-llvm-cov missing — cargo install cargo-llvm-cov --locked"
  fi
  require python3 ""
  cargo llvm-cov clean --workspace
  cargo llvm-cov --locked --lib --bins --tests --no-report
  python3 scripts/coverage-summary.py coverage/summary.json --min-lines 95
}

stage_security() {
  # Mirrors advisories.yml; needs cargo-audit (pinned, as in CI).
  require cargo ""
  if ! command -v cargo-audit >/dev/null 2>&1; then
    _fail "cargo-audit missing — cargo install cargo-audit --version 0.22.2 --locked"
  fi
  cargo audit --deny warnings
}

# ── runner ────────────────────────────────────────────────────────────────────

# Validate all selected stage names before running anything.
for s in "${SELECTED[@]}"; do
  declare -F "stage_$s" >/dev/null || _fail "unknown stage '$s' (stages: ${AVAILABLE_STAGES[*]})"
done

if git status --porcelain | grep -q .; then
  # gh-signoff refuses to attest a dirty tree, so fail fast BEFORE the long
  # stages instead of finishing a full run with nothing posted.
  if [ "$NO_SIGNOFF" = 0 ]; then
    _fail "working tree has uncommitted changes — commit or stash them, or pass --no-signoff"
  fi
  _log "warning: working tree has uncommitted changes — results may not match HEAD"
fi

for s in "${SELECTED[@]}"; do
  _log "stage: $s"
  # Run the stage in a subshell with errexit, OUTSIDE any if-condition context:
  # inside `if ! (...)` bash suppresses errexit, so a failing command mid-stage
  # could be masked and the stage wrongly reported green (bot review P0).
  set +e
  ( set -e; "stage_$s" )
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    [ "$NO_SIGNOFF" = 0 ] && gh signoff fail "$s" --description "$s failed locally" >/dev/null 2>&1 || true
    _fail "stage '$s' failed"
  fi
  _ok "$s passed"
  if [ "$NO_SIGNOFF" = 0 ]; then
    gh signoff "$s" >/dev/null 2>&1 \
      || printf '⚠ could not post signoff status for %s — is HEAD pushed? gh authorized?\n' "$s" >&2
  fi
done

if [ "$NO_SIGNOFF" = 1 ]; then
  _log "all done — no signoff statuses posted (was --no-signoff)."
else
  _log "all done."
fi
