#!/usr/bin/env bash
# cargo-mutants 27.1.0 locks reflink 0.1.3, whose ioctl request type does not
# compile on Android. Patch ONLY a local copy to use its supported copy fallback.
# No registry-cache sources, application dependencies, or global configs change.
set -euo pipefail
version=27.1.0
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
install_root=${1:-"$HOME/.local"}
work=$(mktemp -d "${TMPDIR:-/tmp}/cargo-mutants-termux.XXXXXX")
registry_source() {
  local candidate
  for candidate in "$cargo_home"/registry/src/*/"$1"; do
    if [[ -d "$candidate" ]]; then printf '%s\n' "$candidate"; return; fi
  done
  printf 'Missing registry source: %s\n' "$1" >&2
  return 1
}
cargo info "cargo-mutants@$version" >/dev/null
tool_source=$(registry_source "cargo-mutants-$version")
cp -a "$tool_source" "$work/"
chmod -R u+w "$work"
# Download the exact tool's locked dependencies before locating reflink.
cargo fetch --locked --manifest-path "$work/cargo-mutants-$version/Cargo.toml"
cp -a "$(registry_source reflink-0.1.3)" "$work/"
chmod -R u+w "$work/reflink-0.1.3"
python3 - "$work" "$version" <<'PY'
import pathlib, sys
work = pathlib.Path(sys.argv[1])
p = work / 'reflink-0.1.3/src/sys/unix.rs'
source = p.read_text()
linux = '#[cfg(any(target_os = "linux", target_os = "android"))]'
android = '    target_os = "android",\n'
assert source.count(linux) == 1 and source.count(android) == 1
p.write_text(source.replace(linux, '#[cfg(target_os = "linux")]').replace(android, ''))
p = work / ('cargo-mutants-' + sys.argv[2]) / 'Cargo.toml'
p.write_text(p.read_text() + '\n[patch.crates-io]\nreflink = { path = "../reflink-0.1.3" }\n')
PY
cd "$work/cargo-mutants-$version"
cargo update --offline -p reflink@0.1.3
python3 - "$tool_source/Cargo.lock" Cargo.lock <<'PY'
import pathlib, sys
before = pathlib.Path(sys.argv[1]).read_text()
old = 'name = "reflink"\nversion = "0.1.3"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "bc585ec28b565b4c28977ce8363a6636cedc280351ba25a7915f6c9f37f68cbe"\n'
new = 'name = "reflink"\nversion = "0.1.3"\n'
assert before.count(old) == 1
assert pathlib.Path(sys.argv[2]).read_text() == before.replace(old, new), 'Unexpected dependency drift'
PY
CARGO_BUILD_JOBS=1 cargo install --offline --locked --path . -j 1 --root "$install_root"
"$install_root/bin/cargo-mutants" mutants --version
printf 'Tool installed under %s/bin; retained inspectable patched sources at %s\n' "$install_root" "$work"
