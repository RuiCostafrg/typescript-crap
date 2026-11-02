#!/usr/bin/env python3
"""Build deterministic npm package tarballs from verified release archives (artifacts are data)."""
import argparse
import gzip
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('linux_packages', ROOT / 'scripts/linux-packages.py')
linux = importlib.util.module_from_spec(spec)
spec.loader.exec_module(linux)

MAIN = 'typescript-crap'
# Release target -> (npm platform suffix, os, cpu). Linux uses the static musl
# build, so there is one package per CPU and no libc field.
TARGETS = {
    'x86_64-unknown-linux-musl': ('linux-x64', 'linux', 'x64'),
    'aarch64-unknown-linux-musl': ('linux-arm64', 'linux', 'arm64'),
    'x86_64-apple-darwin': ('darwin-x64', 'darwin', 'x64'),
    'aarch64-apple-darwin': ('darwin-arm64', 'darwin', 'arm64'),
    'x86_64-pc-windows-msvc': ('win32-x64', 'win32', 'x64'),
}
REPOSITORY = {'type': 'git', 'url': 'git+https://github.com/RuiCostafrg/typescript-crap.git'}
HOMEPAGE = 'https://github.com/RuiCostafrg/typescript-crap#readme'
BUGS = {'url': 'https://github.com/RuiCostafrg/typescript-crap/issues'}
LAUNCHER = {'bin/ts-crap.js': 0o755, 'bin/typescript-crap.js': 0o755, 'lib/index.js': 0o644}
BIN = {'ts-crap': 'bin/ts-crap.js', 'typescript-crap': 'bin/typescript-crap.js'}
# npm pack's fixed timestamp (1985-10-26T08:15:00Z) keeps tarballs reproducible.
MTIME = 499162500
MAX_BINARY = 128 * 1024 * 1024
MACHO = {'x86_64-apple-darwin': 0x01000007, 'aarch64-apple-darwin': 0x0100000C}


def platform_name(target):
    return f'{MAIN}-{TARGETS[target][0]}'


def archive_name(target):
    return f"ts-crap-{target}.{'zip' if 'windows' in target else 'tar.xz'}"


def binary_name(target):
    return 'ts-crap.exe' if 'windows' in target else 'ts-crap'


def tarball_name(name, version):
    return f'{name}-{version}.tgz'


def check_executable(data, target):
    """Reject an executable built for another platform or architecture."""
    if 'linux' in target:
        linux.static_elf(data, linux.TARGETS[target][1])
    elif 'darwin' in target:
        if len(data) < 8 or data[:4] != b'\xcf\xfa\xed\xfe' or struct.unpack_from('<I', data, 4)[0] != MACHO[target]:
            raise ValueError(f'expected a 64-bit Mach-O executable for {target}')
    else:
        offset = struct.unpack_from('<I', data, 0x3C)[0] if len(data) >= 0x40 and data[:2] == b'MZ' else None
        if (offset is None or offset + 6 > len(data) or data[offset:offset + 4] != b'PE\0\0'
                or struct.unpack_from('<H', data, offset + 4)[0] != 0x8664):
            raise ValueError(f'expected an x86-64 PE executable for {target}')


def executable(assets, target):
    """Read exactly one bounded regular executable from a checksum-verified archive, without extracting."""
    data = linux.verified(assets / archive_name(target))
    wanted = binary_name(target)
    if 'windows' in target:
        with zipfile.ZipFile(io.BytesIO(data)) as source:
            matches = [e for e in source.infolist() if e.filename.replace('\\', '/').split('/')[-1] == wanted]
            if (len(matches) != 1 or matches[0].is_dir() or matches[0].file_size > MAX_BINARY
                    or (matches[0].external_attr >> 16) & 0o170000 not in (0, 0o100000)):
                raise ValueError('expected one bounded regular executable')
            binary = source.read(matches[0])
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:xz') as source:
            matches = [m for m in source.getmembers() if Path(m.name).name == wanted]
            if len(matches) != 1 or not matches[0].isfile() or matches[0].size > MAX_BINARY:
                raise ValueError('expected one bounded regular executable')
            binary = source.extractfile(matches[0]).read()
    check_executable(binary, target)
    return binary


def manifest(fields):
    return (json.dumps(fields, indent=2) + '\n').encode()


def common(name, version, description):
    return {'name': name, 'version': version, 'description': description, 'license': 'MIT',
            'homepage': HOMEPAGE, 'bugs': BUGS, 'repository': REPOSITORY}


def platform_files(target, version, binary):
    suffix, os_name, cpu = TARGETS[target]
    name = platform_name(target)
    fields = common(name, version, f'The ts-crap scanner executable for {suffix}. Install {MAIN} instead.')
    # preferUnplugged lets Yarn Plug'n'Play run the executable from disk.
    fields.update({'os': [os_name], 'cpu': [cpu], 'preferUnplugged': True})
    readme = (f'# {name}\n\nThe prebuilt `ts-crap` {version} executable for {suffix}, copied byte for byte\n'
              f'from the attested `{archive_name(target)}` GitHub release asset.\n\n'
              f'Do not depend on this package directly: install [`{MAIN}`](https://www.npmjs.com/package/{MAIN}),\n'
              'which selects the right platform package and provides the `ts-crap` command.\n')
    files = {'package.json': (manifest(fields), 0o644), 'README.md': (readme.encode(), 0o644),
             'LICENSE': ((ROOT / 'LICENSE').read_bytes(), 0o644),
             f'bin/{binary_name(target)}': (binary, 0o755)}
    if 'linux' in target:
        files['musl-COPYRIGHT'] = ((ROOT / 'packaging/licenses/musl-COPYRIGHT').read_bytes(), 0o644)
    return name, files


def main_fields(version):
    fields = common(MAIN, version, 'Source-aware CRAP scores for TypeScript functions: '
                    'the ts-crap scanner and its Vitest/Jest coverage helpers')
    fields.update({
        'keywords': ['typescript', 'crap', 'complexity', 'coverage', 'quality-gate', 'vitest', 'jest'],
        'type': 'commonjs', 'main': 'lib/index.js', 'bin': BIN,
        # The launcher runs on any maintained Node.js; the coverage helpers check for 24+.
        'engines': {'node': '>=18'},
        # Yarn Plug'n'Play must unpack the helpers: Node runs them, and their children, from disk.
        'preferUnplugged': True,
        'optionalDependencies': {platform_name(t): version for t in TARGETS},
    })
    return fields


def main_files(version, helpers):
    files = {'package.json': (manifest(main_fields(version)), 0o644),
             'README.md': ((ROOT / 'packaging/npm/README.md').read_bytes(), 0o644),
             'LICENSE': ((ROOT / 'LICENSE').read_bytes(), 0o644)}
    for relative, mode in LAUNCHER.items():
        files[relative] = ((ROOT / 'packaging/npm' / relative).read_bytes(), mode)
    # Helpers are the release bundle's bytes, so scanner and helpers share one version.
    files.update({f'coverage/{name}': (data, 0o644) for name, data in helpers.items()})
    return files


def tarball(files):
    """Serialize files under package/ with fixed metadata: identical inputs give identical bytes."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w', format=tarfile.USTAR_FORMAT) as output:
        for relative in sorted(files):
            data, mode = files[relative]
            entry = tarfile.TarInfo('package/' + relative)
            entry.size, entry.mode, entry.mtime = len(data), mode, MTIME
            entry.uid = entry.gid = 0
            entry.uname = entry.gname = ''
            output.addfile(entry, io.BytesIO(data))
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode='wb', mtime=0, compresslevel=9) as stream:
        stream.write(raw.getvalue())
    return compressed.getvalue()


def packages(assets, version):
    """Return {tarball filename: bytes}, platform packages first and the main package last."""
    linux.debian_version(version)  # SemVer only: the version names files and registry entries.
    helpers = linux.helpers(assets / linux.bundle.NAME, version)
    result = {}
    for target in TARGETS:
        name, files = platform_files(target, version, executable(assets, target))
        result[tarball_name(name, version)] = tarball(files)
    result[tarball_name(MAIN, version)] = tarball(main_files(version, helpers))
    return result


def build(assets, version, output):
    """Write the tarballs to a new OUTPUT directory, all or nothing."""
    built = packages(assets, version)
    if output.exists() or output.is_symlink():
        raise FileExistsError(f'output already exists: {output}')
    staging = Path(tempfile.mkdtemp(prefix='.npm-packages-', dir=output.parent))
    try:
        # mkdtemp is owner-only; give the result the mode mkdir() would have.
        mask = os.umask(0)
        os.umask(mask)
        staging.chmod(0o777 & ~mask)
        for name, data in built.items():
            (staging / name).write_bytes(data)
        staging.rename(output)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return [output / name for name in built]


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True, help='new directory for the tarballs')
    args = parser.parse_args()
    for path in build(args.assets, args.version, args.output):
        print(path)
