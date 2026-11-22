# Stage definitions for the shared verified-release-kit runner.
LOCAL_CI_NAME="typescript-crap"
LOCAL_CI_STAGES=(tests lint docs coverage security)
LOCAL_CI_DEFAULT_STAGES=(tests lint docs)

prepare() { node_min 24; }

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
  local package_files
  package_files="$(cargo package --list)"
  if grep -E '^(reports|coverage|target)/|(^|/)__pycache__/|\.py[co]$' <<<"$package_files"; then
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
    _fail "cargo-audit missing — cargo install cargo-audit --version 0.23.0 --locked"
  fi
  cargo audit --deny warnings
}
