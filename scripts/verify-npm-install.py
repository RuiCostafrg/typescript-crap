#!/usr/bin/env python3
"""Install typescript-crap with npm, pnpm, Yarn or Bun in a clean project and check what runs.

Packed mode installs the local tarballs before publication, pinning every
platform package to its tarball through the manager's override field.
Registry mode installs the published version. Both install with lifecycle
scripts disabled, compare the installed executable with the release archive,
and run the commands through the package manager.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('npm_packages', ROOT / 'scripts/npm-packages.py')
npm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(npm)
spec = importlib.util.spec_from_file_location('coverage_bundle_check', ROOT / 'scripts/verify-coverage-bundle.py')
bundle_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle_check)

MANAGERS = ('npm', 'pnpm', 'yarn', 'bun')
# Run an installed bin. `npm exec --no` never downloads, so it cannot fall back to the
# unrelated registry package that owns the name `ts-crap`.
RUN = {'npm': ['npm', 'exec', '--no', '--'], 'pnpm': ['pnpm', 'exec'], 'yarn': ['yarn', 'run'], 'bun': ['bun', 'run']}
# Yarn Plug'n'Play resolves packages only inside `yarn node`.
NODE = {'npm': ['node'], 'pnpm': ['node'], 'yarn': ['yarn', 'node'], 'bun': ['node']}
INSTALL = {
    'npm': ['npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund'],
    'pnpm': ['pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile'],
    'yarn': ['yarn', 'install'],  # YARN_ENABLE_SCRIPTS=false below
    'bun': ['bun', 'install', '--ignore-scripts'],
}
ENVIRONMENT = {'npm_config_update_notifier': 'false', 'npm_config_fund': 'false',
               'YARN_ENABLE_SCRIPTS': 'false', 'YARN_ENABLE_IMMUTABLE_INSTALLS': 'false',
               'YARN_ENABLE_TELEMETRY': '0', 'COREPACK_ENABLE_DOWNLOAD_PROMPT': '0'}
PRINT_BINARY = "process.stdout.write(require('typescript-crap').binaryPath())"
PRINT_PACKAGE = "process.stdout.write(require.resolve('typescript-crap/package.json'))"


def host_target(system=None, machine=None):
    system = system or platform.system()
    machine = (machine or platform.machine()).lower()
    cpu = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(machine)
    target = {('Linux', 'x86_64'): 'x86_64-unknown-linux-musl', ('Linux', 'aarch64'): 'aarch64-unknown-linux-musl',
              ('Darwin', 'x86_64'): 'x86_64-apple-darwin', ('Darwin', 'aarch64'): 'aarch64-apple-darwin',
              ('Windows', 'x86_64'): 'x86_64-pc-windows-msvc'}.get((system, cpu))
    if target is None:
        raise ValueError(f'no npm platform package for {system} {machine}')
    return target


def manifest(manager, version, packages):
    """The clean project's package.json: the main package plus, when packed, local platform overrides."""
    fields = {'name': 'typescript-crap-install-check', 'version': '0.0.0', 'private': True}
    if packages is None:
        fields['dependencies'] = {npm.MAIN: version}
        return fields
    tarball = lambda name: 'file:' + (packages / npm.tarball_name(name, version)).resolve().as_posix()
    fields['dependencies'] = {npm.MAIN: tarball(npm.MAIN)}
    pinned = {npm.platform_name(t): tarball(npm.platform_name(t)) for t in npm.TARGETS}
    if manager == 'npm':
        # Root optional dependencies satisfy the main package's exact pins on every
        # npm, keep os/cpu filtering, and avoid npm 9's rejection of file: overrides.
        fields['optionalDependencies'] = pinned
    elif manager == 'pnpm':
        pass  # pnpm 10+ reads overrides from pnpm-workspace.yaml; see workspace_files()
    elif manager == 'yarn':
        fields['resolutions'] = pinned
    else:
        fields['overrides'] = pinned
    return fields


def workspace_files(manager, version, packages):
    """Extra project files: pnpm's overrides (JSON is valid YAML) and Yarn Berry's project-root marker."""
    if manager == 'yarn':
        return {'yarn.lock': ''}
    if manager == 'pnpm' and packages is not None:
        pinned = {npm.platform_name(t): 'file:' + (packages / npm.tarball_name(npm.platform_name(t), version)).resolve().as_posix()
                  for t in npm.TARGETS}
        return {'pnpm-workspace.yaml': json.dumps({'overrides': pinned}, indent=2) + '\n'}
    return {}


def run(command, cwd, expected=0, timeout=600):
    executable = shutil.which(command[0])
    if executable is None:
        raise RuntimeError(f'{command[0]} is not installed')
    env = dict(os.environ, **ENVIRONMENT)
    # Audited: argument vectors built in this script from fixed package-manager
    # commands and local paths, never a shell or untrusted command text.
    # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
    result = subprocess.run([executable, *command[1:]], cwd=cwd, capture_output=True, text=True,
                            timeout=timeout, env=env)
    if result.returncode != expected:
        raise RuntimeError(f'{command}: exit {result.returncode}, expected {expected}\n'
                           f'{result.stdout[-8000:]}\n{result.stderr[-8000:]}')
    return result


def fake_vitest_project(root):
    """A project with a stand-in Vitest that writes a real Istanbul report, as verify-coverage-bundle uses."""
    project = root / 'vitest-project'
    (project / 'node_modules' / 'vitest').mkdir(parents=True)
    (project / 'a.mts').write_text('export function f() {\n  return 1;\n}\n')
    (project / 'vitest.config.mjs').write_text('export default {};\n')
    (project / 'node_modules' / 'vitest' / 'package.json').write_text(
        json.dumps({'name': 'vitest', 'version': '0.0.0', 'bin': {'vitest': './vitest.mjs'}}))
    (project / 'node_modules' / 'vitest' / 'vitest.mjs').write_text(
        "import fs from 'node:fs'; import path from 'node:path';\n"
        "const dir = process.argv.find(a => a.startsWith('--coverage.reportsDirectory=')).split('=').slice(1).join('=');\n"
        "fs.mkdirSync(dir, { recursive: true });\n"
        "fs.writeFileSync(path.join(dir, 'coverage-final.json'), JSON.stringify({ 'a.mts': { path: 'a.mts', "
        "statementMap: { 0: { start: { line: 2, column: 2 } } }, s: { 0: 1 } } }));\n")
    (project / 'node_modules' / '@vitest' / 'coverage-istanbul').mkdir(parents=True)
    (project / 'node_modules' / '@vitest' / 'coverage-istanbul' / 'package.json').write_text('{}')
    return project


def node_major(cwd):
    return int(run(['node', '-p', 'process.versions.node'], cwd).stdout.strip().split('.')[0])


def other_platform_paths(project, target):
    """Paths in the project named after a platform package for another platform."""
    others = [npm.platform_name(t) for t in npm.TARGETS if t != target]
    found = []
    for path in project.rglob('*'):
        if any(path.name == o or path.name.startswith((o + '@', o + '-npm-')) for o in others):
            found.append(path)
    return found


def verify(manager, version, assets, packages, scan_root, work, target):
    expected = npm.executable(assets, target)
    project = work / f'{manager}-project'
    project.mkdir()
    (project / 'package.json').write_text(json.dumps(manifest(manager, version, packages), indent=2) + '\n')
    for name, text in workspace_files(manager, version, packages).items():
        (project / name).write_text(text)
    run(INSTALL[manager], project)
    runner = RUN[manager]
    for command in ('ts-crap', 'typescript-crap'):
        result = run([*runner, command, '--version'], project)
        if result.stdout.strip() != f'ts-crap {version}':
            raise ValueError(f'{manager} {command} --version printed {result.stdout!r}')
    run([*runner, 'ts-crap', '--root', str(scan_root), '--threshold', '20'], project)
    # Exit statuses pass through the launcher and the package manager unchanged.
    run([*runner, 'ts-crap', '--root', str(scan_root), '--threshold', '1'], project, expected=1)
    run([*runner, 'ts-crap', '--no-such-option'], project, expected=2)
    binary = Path(run([*NODE[manager], '-e', PRINT_BINARY], project).stdout)
    if binary.read_bytes() != expected:
        raise ValueError(f'{manager}: installed {binary} differs from {npm.archive_name(target)}')
    if os.name != 'nt' and not os.access(binary, os.X_OK):
        raise ValueError(f'{manager}: installed {binary} is not executable')
    # Yarn may keep other platforms' archives in its cache, and Bun installs every
    # override-pinned local tarball; registry installs check Bun's os/cpu filtering.
    if manager != 'yarn' and not (manager == 'bun' and packages is not None):
        stray = other_platform_paths(project, target)
        if stray:
            raise ValueError(f'{manager} installed other platform packages: {stray}')
    helpers = Path(run([*NODE[manager], '-e', PRINT_PACKAGE], project).stdout).parent / 'coverage'
    fake = fake_vitest_project(work / manager)
    if node_major(project) >= 24:
        bundle_check.verify_installed(helpers, binary)
        result = run([*runner, 'typescript-crap', 'coverage', '--root', str(fake), '--scan'], project)
        if 'a.mts' not in result.stdout:
            raise ValueError(f'{manager}: typescript-crap coverage --scan did not scan the project')
        printed = run([*runner, 'typescript-crap', 'coverage', '--root', str(fake)], project).stdout.strip()
        if not printed.startswith(f'npx {npm.MAIN}@{version} --root '):
            raise ValueError(f'{manager}: unexpected follow-up command {printed!r}')
    else:
        result = run([*runner, 'typescript-crap', 'coverage', '--root', str(fake)], project, expected=2)
        if 'Node.js 24' not in result.stderr:
            raise ValueError(f'{manager}: missing Node.js 24 requirement message: {result.stderr!r}')
    return binary


ONE_OFF = {
    'npm': lambda spec: ['npx', '--yes', spec],
    'pnpm': lambda spec: ['pnpm', 'dlx', spec],
    'yarn': lambda spec: ['yarn', 'dlx', '-q', spec],
    'bun': lambda spec: ['bunx', spec],
}


def verify_one_off(manager, version, work):
    """`npx typescript-crap@VERSION` and its equivalents run the published scanner without a project."""
    directory = work / f'{manager}-one-off'
    directory.mkdir()
    result = run([*ONE_OFF[manager](f'{npm.MAIN}@{version}'), '--version'], directory)
    if result.stdout.strip().splitlines()[-1] != f'ts-crap {version}':
        raise ValueError(f'{manager} one-off run printed {result.stdout!r}')


def verify_npx_packed(version, packages, work, target):
    """`npx` with the local tarballs (before publication) runs the scanner from a temporary install."""
    directory = work / 'npx-packed'
    directory.mkdir()
    specs = [packages / npm.tarball_name(npm.MAIN, version), packages / npm.tarball_name(npm.platform_name(target), version)]
    command = ['npx', '--yes', *[f'--package={p.resolve()}' for p in specs], 'typescript-crap', '--version']
    result = run(command, directory)
    if result.stdout.strip() != f'ts-crap {version}':
        raise ValueError(f'packed npx printed {result.stdout!r}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manager', choices=MANAGERS, action='append', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--assets', type=Path, required=True, help='release archives with .sha256 sidecars')
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--packages', type=Path, help='local tarballs from scripts/npm-packages.py')
    source.add_argument('--registry', action='store_true', help='install the published version')
    parser.add_argument('--scan-root', type=Path, default=ROOT / 'examples/shop')
    parser.add_argument('--target', choices=npm.TARGETS, help='release target of this host (default: detected)')
    args = parser.parse_args()
    target = args.target or host_target()
    with tempfile.TemporaryDirectory(prefix='typescript-crap-npm-') as temporary:
        work = Path(temporary).resolve()
        for name in args.manager:
            installed = verify(name, args.version, args.assets.resolve(), args.packages, args.scan_root.resolve(), work, target)
            if args.registry:
                verify_one_off(name, args.version, work)
            elif name == 'npm':
                verify_npx_packed(args.version, args.packages, work, target)
            print(f'{name}: typescript-crap {args.version} installed without scripts; {installed} matches the release archive')
