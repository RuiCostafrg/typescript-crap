#!/usr/bin/env python3
"""Build deterministic Debian packages from verified static-musl release archives."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {'x86_64-unknown-linux-musl': ('amd64', 62),
           'aarch64-unknown-linux-musl': ('arm64', 183)}
spec = importlib.util.spec_from_file_location('coverage_bundle', ROOT / 'scripts/bundle-coverage.py')
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


def debian_version(version):
    """Encode SemVer precedence in dpkg ordering; reserve packaging revision one."""
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?', version):
        raise ValueError('invalid release version')
    core, separator, prerelease = version.partition('-')
    if not separator:
        return core + '-1'
    identifiers = []
    for identifier in prerelease.split('.'):
        if identifier.isdigit():
            if len(identifier) > 1 and identifier.startswith('0'):
                raise ValueError('invalid release version: numeric prerelease leading zero')
            identifiers.append('n' + identifier)
        else:
            # dpkg compares numeric runs numerically, whereas SemVer compares
            # nonnumeric identifiers as ASCII. Two lowercase letters per ASCII
            # character preserve lexical ordering without any numeric runs.
            identifiers.append('s' + ''.join(chr(97 + ord(c) // 26) + chr(97 + ord(c) % 26)
                                            for c in identifier))
    # n < s gives numeric identifiers lower precedence. Tilde terminates each
    # identifier below any continuation; a shorter identifier/list sorts first.
    return core + '~' + '~'.join(identifiers) + '~-1'


def package_name(version, architecture):
    """Name a versioned architecture-specific release asset without path components."""
    if architecture not in ('amd64', 'arm64'):
        raise ValueError('unsupported Debian architecture')
    debian_version(version)
    # GitHub sanitizes tildes in release asset names; retain upstream SemVer
    # in filenames independently of the encoded control-file Version field.
    return f'ts-crap_{version}-1_{architecture}.deb'


def verified(path):
    """Read a regular asset only after its filename-bound SHA-256 sidecar matches."""
    checksum = path.with_name(path.name + '.sha256')
    if path.is_symlink() or not path.is_file() or checksum.is_symlink() or not checksum.is_file():
        raise ValueError('expected regular asset and checksum')
    fields = checksum.read_text(encoding='utf-8').split()
    data = path.read_bytes()
    if (len(fields) != 2 or fields[1].lstrip('*') != path.name
            or fields[0] != hashlib.sha256(data).hexdigest()):
        raise ValueError('asset checksum or filename mismatch')
    return data


def static_elf(data, machine):
    """Reject wrong architectures, ELF interpreters and dynamically needed libraries."""
    if len(data) < 64 or data[:7] != b'\x7fELF\x02\x01\x01':
        raise ValueError('expected little-endian ELF64')
    kind, actual_machine = struct.unpack_from('<HH', data, 16)
    offset = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    if kind not in (2, 3) or actual_machine != machine or size != 56 or not count or offset + size * count > len(data):
        raise ValueError('invalid ELF architecture or program headers')
    for index in range(count):
        entry = offset + index * size
        category = struct.unpack_from('<I', data, entry)[0]
        start, length = struct.unpack_from('<Q', data, entry + 8)[0], struct.unpack_from('<Q', data, entry + 32)[0]
        if start + length > len(data) or category == 3:
            raise ValueError('ELF interpreter or invalid segment bounds')
        if category == 2:
            if length % 16:
                raise ValueError('invalid dynamic segment')
            for position in range(start, start + length, 16):
                tag = struct.unpack_from('<q', data, position)[0]
                if tag == 0:
                    break
                if tag == 1:
                    raise ValueError('ELF needs a dynamic library')


def executable(archive, target):
    """Read exactly one regular executable without extracting archive-controlled paths."""
    with tarfile.open(fileobj=io.BytesIO(verified(archive)), mode='r:xz') as source:
        matches = [m for m in source.getmembers() if Path(m.name).name == 'ts-crap']
        if len(matches) != 1 or not matches[0].isfile() or matches[0].size > 128 * 1024 * 1024:
            raise ValueError('expected one bounded regular executable')
        data = source.extractfile(matches[0]).read()
    static_elf(data, TARGETS[target][1])
    return data


def helpers(archive, version):
    """Read only the complete version-matched helper bundle's regular allowlisted files."""
    expected = {*bundle.MODULES, 'README.md', 'LICENSE', 'version.json'}
    with zipfile.ZipFile(io.BytesIO(verified(archive))) as source:
        entries = source.infolist()
        names = {bundle.PREFIX + name for name in expected}
        if len(entries) != len(names) or {e.filename for e in entries} != names:
            raise ValueError('unexpected or duplicate helper bundle entries')
        if any((e.external_attr >> 16) & 0o170000 != 0o100000 for e in entries):
            raise ValueError('non-regular helper bundle entry')
        contents = {name: source.read(bundle.PREFIX + name) for name in expected}
    metadata = json.loads(contents['version.json'])
    if metadata['version'] != version or metadata['scannerVersion'] != version:
        raise ValueError('helper/scanner version mismatch')
    return contents


def atomic_write(path, data):
    """Publish a complete sibling file without following a planted destination symlink."""
    staging = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.package-', delete=False) as output:
            staging = Path(output.name)
            output.write(data)
            output.flush()
            os.fchmod(output.fileno(), 0o644)
            os.fsync(output.fileno())
        os.replace(staging, path)
    finally:
        if staging is not None:
            staging.unlink(missing_ok=True)


def build(assets, version, epoch, targets=TARGETS):
    """Serialize shared-directory builds, including aggregate checksum publication (Linux only)."""
    import fcntl
    directory = os.open(assets, os.O_RDONLY | os.O_DIRECTORY)
    try:
        fcntl.flock(directory, fcntl.LOCK_EX)
        return _build(assets, version, epoch, targets)
    finally:
        os.close(directory)


def _build(assets, version, epoch, targets):
    """Build script-free packages with root ownership, stable timestamps and no runtime deps."""
    debian_version(version)
    if type(epoch) is not int or epoch < 0:
        raise ValueError('invalid source timestamp')
    contents = helpers(assets / bundle.NAME, version)
    outputs = []
    for target in targets:
        architecture, _ = TARGETS[target]
        binary = executable(assets / f'ts-crap-{target}.tar.xz', target)
        name = package_name(version, architecture)
        with tempfile.TemporaryDirectory(prefix='linux-package-') as temporary:
            stage = Path(temporary) / 'root'
            files = {'usr/bin/ts-crap': (binary, 0o755),
                     'usr/share/doc/ts-crap/copyright': ((ROOT / 'LICENSE').read_bytes() + b'\n\n' +
                         (ROOT / 'packaging/licenses/musl-COPYRIGHT').read_bytes(), 0o644),
                     'usr/share/doc/ts-crap/musl-COPYRIGHT': ((ROOT / 'packaging/licenses/musl-COPYRIGHT').read_bytes(), 0o644)}
            files.update({f'usr/share/ts-crap/coverage-helpers/{n}': (data, 0o644) for n, data in contents.items()})
            installed_size = sum((len(data) + 1023) // 1024 for data, _ in files.values())
            control = (f'Package: ts-crap\nVersion: {debian_version(version)}\nArchitecture: {architecture}\n'
                       'Maintainer: RuiCostafrg <35194998+RuiCostafrg@users.noreply.github.com>\n'
                       'Section: devel\nPriority: optional\n'
                       'Suggests: git, nodejs (>= 24)\n'
                       f'Installed-Size: {installed_size}\n'
                       'Homepage: https://github.com/RuiCostafrg/typescript-crap\n'
                       'Description: Source-aware CRAP scores for TypeScript functions\n'
                       ' Standalone scanner with optional Node 24+ coverage helpers.\n')
            files['DEBIAN/control'] = (control.encode(), 0o644)
            for relative, (data, mode) in files.items():
                path = stage / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                path.chmod(mode)
            for path in sorted(stage.rglob('*'), reverse=True):
                if path.is_dir():
                    path.chmod(0o755)
                os.utime(path, (epoch, epoch))
            stage.chmod(0o755)
            os.utime(stage, (epoch, epoch))
            output = Path(temporary) / name
            subprocess.run(['dpkg-deb', '--build', '--root-owner-group', '--uniform-compression',
                            '--threads-max=1', '-Zgzip', str(stage), str(output)],
                           check=True, stdout=subprocess.DEVNULL,
                           env=dict(os.environ, SOURCE_DATE_EPOCH=str(epoch)))
            destination = assets / name
            atomic_write(destination, output.read_bytes())
            digest = hashlib.sha256(destination.read_bytes()).hexdigest()
            atomic_write(assets / (name + '.sha256'), f'{digest}  {name}\n'.encode())
            outputs.append(destination)
    combined = assets / 'sha256.sum'
    if combined.exists() or combined.is_symlink():
        if combined.is_symlink() or not combined.is_file():
            raise ValueError('expected regular combined checksum file')
        names = {p.name for p in outputs}
        lines = []
        for line in combined.read_text(encoding='utf-8').splitlines():
            # cargo-dist terminates its aggregate with a blank line; drop only empty lines.
            if not line.strip():
                continue
            fields = line.split()
            if len(fields) != 2 or not re.fullmatch('[0-9a-f]{64}', fields[0]):
                raise ValueError('invalid combined checksum entry')
            if fields[1].lstrip('*') not in names:
                lines.append(line)
        lines.extend((p.with_name(p.name + '.sha256')).read_text().strip() for p in outputs)
        atomic_write(combined, ('\n'.join(lines) + '\n').encode())
    return outputs


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--epoch', type=int, required=True)
    parser.add_argument('--target', choices=TARGETS, action='append')
    args = parser.parse_args()
    for output in build(args.assets, args.version, args.epoch, args.target or TARGETS):
        print(output)
