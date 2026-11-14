#!/usr/bin/env python3
"""TEMPORARY (D0): time each step of the Action's install path on a hosted runner.

Deleted before the PR merges. Writes one JSON file of raw seconds per step.
Every iteration uses a fresh XDG_CACHE_HOME, so gh starts with a cold TUF
cache, as on a fresh hosted VM, unless the step is explicitly "warm".
"""
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = 'RuiCostafrg/typescript-crap'
VERSION = os.environ.get('MEASURE_VERSION', '2.4.0')
IDENTITY = f'https://github.com/{REPO}/.github/workflows/release.yml@refs/tags/v{VERSION}'
ITERATIONS = int(os.environ.get('MEASURE_ITERATIONS', '5'))
BASH = os.environ['MEASURE_BASH']
CHECKOUT = Path(os.environ['GITHUB_WORKSPACE'])
OUT = Path(sys.argv[1])

system, machine = platform.system(), platform.machine().lower()
TARGET = {
    ('Linux', 'x86_64'): 'x86_64-unknown-linux-musl',
    ('Linux', 'aarch64'): 'aarch64-unknown-linux-musl',
    ('Darwin', 'arm64'): 'aarch64-apple-darwin',
    ('Darwin', 'x86_64'): 'x86_64-apple-darwin',
    ('Windows', 'amd64'): 'x86_64-pc-windows-msvc',
}[(system, machine)]
ARCHIVE = f'ts-crap-{TARGET}.zip' if 'windows' in TARGET else f'ts-crap-{TARGET}.tar.xz'
BINARY = 'ts-crap.exe' if 'windows' in TARGET else 'ts-crap'


def env_with(cache, **extra):
    environment = dict(os.environ)
    environment['XDG_CACHE_HOME'] = str(cache)
    environment.update(extra)
    return environment


def timed(command, environment, cwd=None, stdout=subprocess.DEVNULL, check=True):
    start = time.perf_counter()
    result = subprocess.run(command, env=environment, cwd=cwd, stdout=stdout,
                            stderr=subprocess.PIPE, text=True)
    elapsed = time.perf_counter() - start
    if check and result.returncode != 0:
        raise SystemExit(f'{command[:3]} failed ({result.returncode}): {result.stderr}')
    return elapsed, result


def download_command(directory, sidecar=True):
    patterns = ['--pattern', ARCHIVE] + (['--pattern', f'{ARCHIVE}.sha256'] if sidecar else [])
    return ['gh', 'release', 'download', f'v{VERSION}', '--repo', REPO, *patterns,
            '--dir', str(directory), '--clobber']


def verify_command(asset, root, json_format=False):
    command = ['gh', 'attestation', 'verify', str(asset), '--repo', REPO, '--cert-identity', IDENTITY,
               '--source-ref', f'refs/tags/v{VERSION}', '--deny-self-hosted-runners',
               '--custom-trusted-root', str(root)]
    jq = f'.[].verificationResult.statement.subject[] | select(.name == "{Path(asset).name}") | .digest.sha256'
    return command + (['--format', 'json', '--jq', jq] if json_format else [])


def sha256_tool(path):
    tool = ['sha256sum'] if shutil.which('sha256sum') else ['shasum', '-a', '256']
    with open(path, 'rb') as data:
        return subprocess.run(tool, stdin=data, capture_output=True, text=True, check=True).stdout.split()[0]


def one_iteration(work):
    row = {}
    cache = work / 'cache'
    download = work / 'download'
    download.mkdir(parents=True)
    environment = env_with(cache)

    # Current serial flow.
    row['download_archive_and_sidecar'], _ = timed(download_command(download), environment)
    root = work / 'trusted-root.jsonl'
    with open(root, 'w') as handle:
        row['trusted_root_cold'], _ = timed(['gh', 'attestation', 'trusted-root'], environment, stdout=handle)
    row['tuf_cache_populated'] = (cache / 'gh' / '.sigstore' / 'root').is_dir()
    row['verify_archive'], _ = timed(verify_command(download / ARCHIVE, root), environment)
    start = time.perf_counter()
    expected = (download / f'{ARCHIVE}.sha256').read_text().split()[0].lower()
    actual = sha256_tool(download / ARCHIVE)
    row['sidecar_checksum'] = time.perf_counter() - start
    assert actual == expected
    tool = work / 'tool'
    tool.mkdir()
    if ARCHIVE.endswith('.zip'):
        row['extract'], _ = timed(['unzip', '-q', str(download / ARCHIVE), '-d', str(tool)], environment)
    else:
        row['extract'], _ = timed(['tar', '-xf', str(download / ARCHIVE), '-C', str(tool)], environment)
    executable = next(tool.rglob(BINARY))
    row['first_version'], result = timed([str(executable), '--version'], environment, stdout=subprocess.PIPE)
    assert result.stdout.strip() == f'ts-crap {VERSION}', result.stdout

    # Variants that decide D1, D3 and D4.
    archive_only = work / 'archive-only'
    archive_only.mkdir()
    row['download_archive_only'], _ = timed(download_command(archive_only, sidecar=False), environment)
    with open(work / 'warm-root.jsonl', 'w') as handle:
        row['trusted_root_warm'], _ = timed(['gh', 'attestation', 'trusted-root'], environment, stdout=handle)
    row['verify_archive_json'], result = timed(verify_command(download / ARCHIVE, root, True), environment,
                                               stdout=subprocess.PIPE)
    assert result.stdout.split() == [actual], result.stdout
    row['attested_subject_digest_matches'] = True

    overlap = work / 'overlap'
    overlap.mkdir()
    overlap_env = env_with(work / 'overlap-cache')
    start = time.perf_counter()
    with open(work / 'overlap-root.jsonl', 'w') as handle:
        root_process = subprocess.Popen(['gh', 'attestation', 'trusted-root'], env=overlap_env,
                                        stdout=handle, stderr=subprocess.PIPE)
        download_process = subprocess.Popen(download_command(overlap, sidecar=False), env=overlap_env,
                                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        download_process.wait()
        row['overlap_download_done'] = time.perf_counter() - start
        root_process.wait()
    row['overlap_download_and_root_wall'] = time.perf_counter() - start
    assert root_process.returncode == 0 and download_process.returncode == 0

    # End to end: the Action's install script from this checkout, cold TUF cache.
    runner_temp = work / 'runner-temp'
    runner_temp.mkdir()
    for name in ['path', 'output', 'env']:
        (work / name).write_text('')
    script_env = env_with(work / 'script-cache', RUNNER_TEMP=str(runner_temp),
                          GITHUB_PATH=str(work / 'path'), GITHUB_OUTPUT=str(work / 'output'),
                          GITHUB_ENV=str(work / 'env'), TS_CRAP_ACTION_VERSION=VERSION)
    row['install_sh_total'], result = timed([BASH, str(CHECKOUT / 'action' / 'install.sh')], script_env,
                                            stdout=subprocess.PIPE)
    row['install_sh_stdout'] = result.stdout.strip().splitlines()[-1:]
    script_env['TS_CRAP_ACTION_HELPERS'] = 'true'
    for name in ['path', 'output', 'env']:
        (work / name).write_text('')
    script_env['XDG_CACHE_HOME'] = str(work / 'script-cache-helpers')
    row['install_sh_total_with_helpers'], _ = timed([BASH, str(CHECKOUT / 'action' / 'install.sh')],
                                                    script_env)

    # D5: other install paths, cold caches.
    npm = shutil.which('npm')
    if npm:
        project = work / 'npm-project'
        project.mkdir()
        (project / 'package.json').write_text('{"private":true}\n')
        npm_env = env_with(cache, npm_config_cache=str(work / 'npm-cache'))
        row['npm_install'], _ = timed([npm, 'install', '--no-audit', '--no-fund', '--save-exact',
                                       f'typescript-crap@{VERSION}'], npm_env, cwd=project)
        row['npm_first_version'], result = timed(
            [shutil.which('node'), str(project / 'node_modules' / 'typescript-crap' / 'bin' / 'ts-crap.js'),
             '--version'], npm_env, stdout=subprocess.PIPE)
        assert result.stdout.strip() == f'ts-crap {VERSION}', result.stdout
    if system != 'Windows':
        install_dir = work / 'shell-installer'
        installer_env = env_with(cache, TS_CRAP_UNMANAGED_INSTALL=str(install_dir))
        url = f'https://github.com/{REPO}/releases/download/v{VERSION}/ts-crap-installer.sh'
        row['shell_installer'], _ = timed(
            [BASH, '-c', f"curl --proto '=https' --tlsv1.2 -LsSf {url} | sh"], installer_env)
        row['shell_installer_first_version'], _ = timed([str(next(install_dir.rglob('ts-crap'))), '--version'],
                                                        installer_env)
    return row


def main():
    rows = []
    for index in range(ITERATIONS):
        with tempfile.TemporaryDirectory(dir=os.environ.get('RUNNER_TEMP')) as directory:
            row = one_iteration(Path(directory))
            row['iteration'] = index + 1
            rows.append(row)
            print(json.dumps(row), flush=True)
    gh_version = subprocess.run(['gh', '--version'], capture_output=True, text=True).stdout.splitlines()[0]
    OUT.write_text(json.dumps({
        'runner': os.environ.get('MEASURE_RUNNER'), 'image': os.environ.get('ImageOS'),
        'image_version': os.environ.get('ImageVersion'), 'target': TARGET, 'gh': gh_version,
        'version': VERSION, 'checkout_sha': os.environ.get('GITHUB_SHA'), 'rows': rows,
    }, indent=1) + '\n')


main()
