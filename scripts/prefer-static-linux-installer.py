#!/usr/bin/env python3
"""Make the generated shell installer always select the static musl build on Linux.

cargo-dist 0.33.1 installs the GNU archive when the system glibc is 2.43 or
newer and falls back to the static musl archive otherwise. This rewrites only
those two Linux GNU selection blocks so every Linux system gets the same
static binary. The musl archives' embedded SHA-256 values, all other platforms,
and the GNU release archives themselves are unchanged.

The rewrite is an exact text replacement. If the generated template differs,
for example after a cargo-dist upgrade, it fails rather than guessing.
"""
import sys
from pathlib import Path

ARCHITECTURES = ('aarch64', 'x86_64')


def gnu_block(arch):
    return f'''        "{arch}-unknown-linux-gnu")
            _archive="ts-crap-{arch}-unknown-linux-gnu.tar.xz"
            if ! check_glibc "2" "43"; then
                _archive=""
            fi
            if [ -n "$_archive" ]; then
                echo "$_archive"
                return 0
            fi
            _archive="ts-crap-{arch}-unknown-linux-musl.tar.xz"
'''


def static_block(arch):
    return f'''        "{arch}-unknown-linux-gnu")
            # ts-crap: every Linux system installs the static musl build.
            _archive="ts-crap-{arch}-unknown-linux-musl.tar.xz"
'''


def patch(text):
    for arch in ARCHITECTURES:
        before, after = gnu_block(arch), static_block(arch)
        if text.count(before) != 1:
            raise ValueError(f'unexpected installer template for {arch}-unknown-linux-gnu; '
                             'review the musl preference after changing cargo-dist')
        text = text.replace(before, after)
    if 'check_glibc "2"' in text.split('check_glibc() {', 1)[0]:
        raise ValueError('installer still selects a GNU archive by glibc version')
    return text


def main():
    if len(sys.argv) != 2:
        raise SystemExit('usage: prefer-static-linux-installer.py INSTALLER')
    path = Path(sys.argv[1])
    if path.is_symlink() or not path.is_file():
        raise SystemExit(f'expected a regular installer file: {path}')
    path.write_text(patch(path.read_text(encoding='utf-8')), encoding='utf-8')


if __name__ == '__main__':
    main()
