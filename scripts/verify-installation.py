#!/usr/bin/env python3
"""Exercise real staged release installers in isolated prefixes over loopback HTTP."""
import argparse
from functools import partial
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import threading
import zipfile

TARGETS = {
    'aarch64-apple-darwin', 'x86_64-apple-darwin',
    'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
    'x86_64-pc-windows-msvc',
}


def run(command, *, env=None, expected=0):
    result = subprocess.run([str(item) for item in command], env=env,
                            capture_output=True, text=True, encoding='utf-8',
                            errors='replace', timeout=180)
    if result.returncode != expected:
        raise RuntimeError(f'{command!r}: expected {expected}, got {result.returncode}\n'
                           f'{result.stdout}\n{result.stderr}')
    return result.stdout


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f'Expected regular release asset: {path}')
    return path.read_bytes()


def stage(source, destination, target, version, installer):
    if target not in TARGETS or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?', version):
        raise ValueError('Unsupported target or invalid version')
    name = f'ts-crap-{target}.{"zip" if "windows" in target else "tar.xz"}'
    archive = regular(source / name)
    checksum = regular(source / (name + '.sha256')).decode().split()
    if len(checksum) != 2 or checksum[1].lstrip('*') != name:
        raise ValueError(f'Invalid checksum filename: {name}')
    if hashlib.sha256(archive).hexdigest() != checksum[0]:
        raise ValueError(f'Checksum mismatch: {name}')
    binary = 'ts-crap.exe' if 'windows' in target else 'ts-crap'
    if name.endswith('.zip'):
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            matches = [item for item in bundle.infolist() if Path(item.filename).name == binary and not item.is_dir()]
            if len(matches) != 1:
                raise ValueError('Expected exactly one release executable')
            data = bundle.read(matches[0])
    else:
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:xz') as bundle:
            matches = [item for item in bundle.getmembers() if Path(item.name).name == binary and item.isfile()]
            if len(matches) != 1:
                raise ValueError('Expected exactly one release executable')
            data = bundle.extractfile(matches[0]).read()
    script = {'shell': 'ts-crap-installer.sh', 'powershell': 'ts-crap-installer.ps1',
              'homebrew': 'ts-crap.rb'}[installer]
    contents = regular(source / script)
    pattern = {'shell': rb'APP_VERSION="([^"]+)"',
               'powershell': rb"\$app_version = '([^']+)'",
               'homebrew': rb'  version "([^"]+)"'}[installer]
    match = re.search(pattern, contents)
    if not match or match[1].decode() != version:
        raise ValueError('Installer version does not match the expected release')
    destination.mkdir()
    (destination / name).write_bytes(archive)
    (destination / script).write_bytes(contents)
    return hashlib.sha256(data).hexdigest()


def smoke(binary, version, directory):
    if run([binary, '--version']).strip() != f'ts-crap {version}':
        raise ValueError('Installed version does not match the expected release')
    run([binary, '--help'])
    directory.mkdir(exist_ok=True)
    sources = {'plain.ts': 'export function value() {\n  return 1;\n}\n',
               'View.tsx': 'export function View() {\n  return <span>ready</span>;\n}\n'}
    files = {}
    for name, text in sources.items():
        (directory / name).write_bytes(text.encode())
        files[name] = {'sha256': hashlib.sha256(text.encode()).hexdigest(), 'lines': {'2': 1}}
    args = [binary, '--root', directory, '--format', 'json']
    report = json.loads(run(args))
    if report['files_scanned'] != 2 or len(report['functions']) != 2 or report['failed']:
        raise ValueError('Installed CLI did not scan TypeScript and TSX correctly')
    run([*args, '--threshold', '0'], expected=1)
    coverage = directory / 'coverage.json'
    coverage.write_text(json.dumps({'version': 1, 'files': files}), encoding='utf-8')
    covered = [*args, '--coverage', coverage, '--require-coverage', '--threshold', '1']
    report = json.loads(run(covered))
    if any(function['coverage'] != 1 for function in report['functions']):
        raise ValueError('Installed CLI did not consume source-bound coverage')
    files['plain.ts']['lines']['2'] = 0
    coverage.write_text(json.dumps({'version': 1, 'files': files}), encoding='utf-8')
    run(covered, expected=1)
    (directory / 'plain.ts').write_bytes((sources['plain.ts'] + '// stale\n').encode())
    run(covered, expected=2)


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def verify(args):
    if args.version == args.previous_version:
        raise ValueError('Upgrade verification needs two different release versions')
    if (args.installer == 'powershell') != ('windows' in args.target):
        raise ValueError('Installer does not match target platform')
    with tempfile.TemporaryDirectory(prefix='ts-crap install ü ') as temporary:
        root = Path(temporary).resolve()
        served = root / 'releases'
        served.mkdir()
        current_hash = stage(args.assets, served / 'current', args.target, args.version, args.installer)
        previous_hash = stage(args.previous_assets, served / 'previous', args.target,
                              args.previous_version, args.installer)
        if current_hash == previous_hash:
            raise ValueError('Upgrade archives must contain different executable bytes')
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=str(served)))
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            base = f'http://127.0.0.1:{server.server_port}'
            if args.installer == 'homebrew':
                verify_homebrew(args, root, served, base, current_hash, previous_hash)
            else:
                verify_script(args, root, served, base, current_hash, previous_hash)
        finally:
            server.shutdown()
            server.server_close()
            worker.join()
    print(f'PASS {args.installer}/{args.target}: fresh install, reinstall, '
          f'{args.previous_version} -> {args.version} upgrade, TS/TSX and coverage gates')


def verify_script(args, root, served, base, current_hash, previous_hash):
    for scenario in ['fresh', 'upgrade']:
        prefix = root / scenario / 'bin with spaces'
        config = root / scenario / 'config'
        env = {**os.environ, 'TS_CRAP_INSTALL_DIR': str(prefix), 'XDG_CONFIG_HOME': str(config),
               'TS_CRAP_NO_MODIFY_PATH': '1',
               'NO_PROXY': os.environ.get('NO_PROXY', '') + ',127.0.0.1,localhost'}
        env.pop('TS_CRAP_UNMANAGED_INSTALL', None)
        env.pop('TS_CRAP_DISABLE_UPDATE', None)
        binary = prefix / ('ts-crap.exe' if 'windows' in args.target else 'ts-crap')

        def install(release, version, digest):
            env['TS_CRAP_DOWNLOAD_URL'] = f'{base}/{release}'
            script = served / release / ('ts-crap-installer.ps1' if args.installer == 'powershell' else 'ts-crap-installer.sh')
            command = ([args.powershell, '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script]
                       if args.installer == 'powershell' else ['sh', script])
            run(command, env=env)
            if hashlib.sha256(regular(binary)).hexdigest() != digest:
                raise ValueError('Installed executable differs from the verified archive')
            receipt = json.loads(regular(config / 'ts-crap' / 'ts-crap-receipt.json'))
            if receipt['version'] != version or Path(receipt['install_prefix']).resolve() != prefix.resolve():
                raise ValueError('Installer receipt has incorrect version or prefix')
            smoke(binary, version, root / 'project')

        if scenario == 'upgrade':
            install('previous', args.previous_version, previous_hash)
        install('current', args.version, current_hash)
        install('current', args.version, current_hash)


def verify_homebrew(args, root, served, base, current_hash, previous_hash):
    brew = shutil.which('brew')
    if not brew:
        raise ValueError('Homebrew must be installed before running formula verification')
    env = {**os.environ, 'HOMEBREW_NO_AUTO_UPDATE': '1', 'HOMEBREW_NO_ANALYTICS': '1',
           'HOMEBREW_NO_INSTALL_CLEANUP': '1', 'HOMEBREW_NO_ENV_HINTS': '1',
           # tap-new commits its template; clean runners need a local test identity.
           'GIT_AUTHOR_NAME': 'ts-crap installation verification',
           'GIT_AUTHOR_EMAIL': 'ts-crap-installation@example.invalid',
           'GIT_COMMITTER_NAME': 'ts-crap installation verification',
           'GIT_COMMITTER_EMAIL': 'ts-crap-installation@example.invalid'}
    existing = run([brew, 'list', '--formula', '--versions'], env=env)
    if any(line.split()[0] == 'ts-crap' for line in existing.splitlines() if line.strip()):
        raise ValueError('Refusing to replace an existing Homebrew ts-crap installation')
    tap = f'ts-crap-test/run-{os.getpid()}'
    formula = f'{tap}/ts-crap'
    run([brew, 'tap-new', tap], env=env)
    tap_root = Path(run([brew, '--repository', tap], env=env).strip())
    try:
        def install(release, version, digest, operation):
            text = (served / release / 'ts-crap.rb').read_text(encoding='utf-8')
            # Only redirect the generated artifact URLs to the staged assets.
            # Keep the formula's actual version, checksums, and install logic.
            original = f'https://github.com/RuiCostafrg/typescript-crap/releases/download/v{version}'
            if original not in text:
                raise ValueError('Generated formula has an unexpected release URL')
            (tap_root / 'Formula' / 'ts-crap.rb').write_text(text.replace(original, f'{base}/{release}'), encoding='utf-8')
            run([brew, operation, '--formula', formula], env=env)
            prefix = Path(run([brew, '--prefix', formula], env=env).strip())
            binary = prefix / 'bin' / 'ts-crap'
            if hashlib.sha256(regular(binary)).hexdigest() != digest:
                raise ValueError('Homebrew executable differs from the verified archive')
            smoke(binary, version, root / 'project')

        install('current', args.version, current_hash, 'install')
        install('current', args.version, current_hash, 'reinstall')
        run([brew, 'uninstall', '--force', formula], env=env)
        install('previous', args.previous_version, previous_hash, 'install')
        install('current', args.version, current_hash, 'upgrade')
    finally:
        # The guard above ensures this formula belongs to this test run.
        try:
            remaining = run([brew, 'list', '--formula', '--versions'], env=env)
            if any(line.split()[0] == 'ts-crap' for line in remaining.splitlines() if line.strip()):
                run([brew, 'uninstall', '--force', formula], env=env)
        finally:
            run([brew, 'untap', tap], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--previous-assets', type=Path, required=True)
    parser.add_argument('--previous-version', required=True)
    parser.add_argument('--target', choices=sorted(TARGETS), required=True)
    parser.add_argument('--installer', choices=['shell', 'powershell', 'homebrew'], required=True)
    parser.add_argument('--powershell', default='powershell')
    verify(parser.parse_args())


if __name__ == '__main__':
    main()
