#!/usr/bin/env python3
"""Verify published tap URLs, fresh install, reinstall and a real version upgrade."""
import argparse
import hashlib
import importlib.util
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location('installation', Path(__file__).with_name('verify-installation.py'))
installation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installation)


def verify(args):
    brew = shutil.which('brew')
    if not brew:
        raise ValueError('Homebrew is required')
    env = {**os.environ, 'HOMEBREW_NO_AUTO_UPDATE':'1', 'HOMEBREW_NO_ANALYTICS':'1',
           'HOMEBREW_NO_INSTALL_CLEANUP':'1', 'HOMEBREW_NO_ENV_HINTS':'1'}
    def run(command, *, timeout=180):
        return installation.run(command, env=env, timeout=timeout)
    if any(line.split()[0] == 'ts-crap' for line in run([brew, 'list', '--formula', '--versions']).splitlines() if line.strip()):
        raise ValueError('Use a clean runner without ts-crap installed')
    tap = 'ruicostafrg/tap'
    if tap in run([brew, 'tap']).lower().splitlines():
        raise ValueError('Use a clean runner without the live tap already configured')
    with tempfile.TemporaryDirectory(prefix='ts-crap live tap ') as temporary:
        root = Path(temporary)
        current = installation.stage(args.assets, root/'current', args.target, args.version, 'homebrew')
        previous = installation.stage(args.previous_assets, root/'previous', args.target, args.previous_version, 'homebrew')
        try:
            run([brew, 'tap', 'RuiCostafrg/tap'])
            repository = Path(run([brew, '--repository', 'RuiCostafrg/tap']).strip())
            if (repository/'Formula/ts-crap.rb').read_bytes() != (args.assets/'ts-crap.rb').read_bytes():
                raise ValueError('Live formula does not equal the published release formula')
            head = run(['git', '-C', repository, 'rev-parse', 'HEAD']).strip()
            if run(['git', '-C', repository, 'rev-parse', '--is-shallow-repository']).strip() == 'true':
                run(['git', '-C', repository, 'fetch', '--unshallow', 'origin'])
            # Locate the exact published old formula in the live tap's history.
            # Neither formula URLs nor checksum fields are rewritten.
            previous_revision = None
            for revision in run(['git', '-C', repository, 'rev-list', 'HEAD', '--', 'Formula/ts-crap.rb']).splitlines():
                candidate = subprocess.run(['git', '-C', repository, 'show', f'{revision}:Formula/ts-crap.rb'], capture_output=True, check=True).stdout
                if candidate == (args.previous_assets/'ts-crap.rb').read_bytes():
                    previous_revision = revision
                    break
            if previous_revision is None:
                raise ValueError('Published previous formula is absent from live tap history')
            def check(version, digest, name):
                binary = Path(run([brew, '--prefix', 'RuiCostafrg/tap/ts-crap']).strip())/'bin/ts-crap'
                if hashlib.sha256(binary.read_bytes()).hexdigest() != digest:
                    raise ValueError('Live installation differs from checksum-verified release')
                installation.smoke(binary, version, root/name)
            run(['git', '-C', repository, 'checkout', '--detach', previous_revision])
            run([brew, 'install', 'RuiCostafrg/tap/ts-crap'])
            check(args.previous_version, previous, 'previous-project')
            run(['git', '-C', repository, 'checkout', 'main'])
            if run(['git', '-C', repository, 'rev-parse', 'HEAD']).strip() != head:
                raise ValueError('Unexpected live tap branch')
            # A cold Intel macOS runner may spend several minutes refreshing
            # Homebrew and its preinstalled taps. Keep the other command limits.
            run([brew, 'update'], timeout=600)
            if (repository/'Formula/ts-crap.rb').read_bytes() != (args.assets/'ts-crap.rb').read_bytes():
                raise ValueError('Live tap changed during verification')
            run([brew, 'upgrade', 'RuiCostafrg/tap/ts-crap'])
            check(args.version, current, 'upgraded-project')
            run([brew, 'reinstall', 'RuiCostafrg/tap/ts-crap'])
            check(args.version, current, 'reinstalled-project')
            run([brew, 'uninstall', 'RuiCostafrg/tap/ts-crap'])
            run([brew, 'install', 'RuiCostafrg/tap/ts-crap'])
            check(args.version, current, 'fresh-project')
        finally:
            subprocess.run([brew, 'uninstall', '--force', 'RuiCostafrg/tap/ts-crap'], env=env, capture_output=True, timeout=180)
            subprocess.run([brew, 'untap', 'RuiCostafrg/tap'], env=env, capture_output=True, timeout=180)
    print(f'PASS live Homebrew: exact published formula, {args.previous_version} -> {args.version}, reinstall, fresh install, TS/TSX and coverage gates')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--previous-assets', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--previous-version', required=True)
    parser.add_argument('--target', required=True)
    verify(parser.parse_args())
