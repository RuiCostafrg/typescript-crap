"""Artifact-level regressions for portable Linux package construction."""
import hashlib
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import io
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('packages', ROOT / 'scripts/linux-packages.py')
packages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packages)
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']


def elf(machine=62, category=1, dynamic_tag=None):
    """Make structural ELF fixtures for metadata tests, not runnable fake scanners."""
    data = bytearray(136)
    data[:7] = b'\x7fELF\x02\x01\x01'
    struct.pack_into('<HHI', data, 16, 2, machine, 1)
    struct.pack_into('<Q', data, 32, 64)
    struct.pack_into('<HHH', data, 52, 64, 56, 1)
    struct.pack_into('<I', data, 64, category)
    if dynamic_tag is not None:
        struct.pack_into('<Q', data, 72, 120)
        struct.pack_into('<Q', data, 96, 16)
        struct.pack_into('<qQ', data, 120, dynamic_tag, 0)
    return bytes(data)


def archive(root, target, data, symlink=False):
    """Build a filename-bound archive with optional malicious member type."""
    path = root / f'ts-crap-{target}.tar.xz'
    with tarfile.open(path, 'w:xz') as output:
        member = tarfile.TarInfo('nested/ts-crap')
        if symlink:
            member.type = tarfile.SYMTYPE
            member.linkname = '/etc/passwd'
        else:
            member.size = len(data)
        output.addfile(member, None if symlink else io.BytesIO(data))
    (root / (path.name + '.sha256')).write_text(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.name + '\n')
    return path


class LinuxPackagesTests(unittest.TestCase):
    def test_version_mapping_rejects_injection_and_orders_prerelease(self):
        """Debian prereleases sort before stable without accepting path/control injection."""
        self.assertEqual(packages.debian_version('2.2.0-rc.1'), '2.2.0~rc.1-1')
        self.assertEqual(packages.package_name('2.2.0', 'arm64'), 'ts-crap_2.2.0-1_arm64.deb')
        for version in ['../2.2.0', '2.2.0\nDepends: bad', '2.2', '-1.2.3', '01.2.3', '2.2.0+meta']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                packages.debian_version(version)
        if shutil.which('dpkg'):
            subprocess.run(['dpkg', '--compare-versions', packages.debian_version('2.2.0-rc.1'),
                            'lt', packages.debian_version('2.2.0')], check=True)

    def test_elf_rejects_dynamic_wrong_architecture_and_truncated_headers(self):
        """Check architecture and both dynamic-linking mechanisms independently."""
        packages.static_elf(elf(), 62)
        packages.static_elf(elf(machine=183), 183)
        packages.static_elf(elf(category=2, dynamic_tag=0), 62)
        for data in [b'#!/bin/sh\n', elf(machine=183), elf(category=3),
                     elf(category=2, dynamic_tag=1), elf()[:100]]:
            with self.subTest(data=data[:20]), self.assertRaises(ValueError):
                packages.static_elf(data, 62)

    def test_checksum_binding_and_symlink_executable_reject_before_packaging(self):
        """Untrusted archive content never becomes a package through filename/type confusion."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = 'x86_64-unknown-linux-musl'
            path = archive(root, target, elf())
            self.assertEqual(packages.executable(path, target), elf())
            path.write_bytes(b'tampered')
            with self.assertRaisesRegex(ValueError, 'checksum'):
                packages.executable(path, target)
            path = archive(root, target, elf(), symlink=True)
            with self.assertRaisesRegex(ValueError, 'regular executable'):
                packages.executable(path, target)
            path = archive(root, target, elf())
            (root / (path.name + '.sha256')).write_text(hashlib.sha256(path.read_bytes()).hexdigest() + '  other.tar.xz\n')
            with self.assertRaisesRegex(ValueError, 'filename mismatch'):
                packages.executable(path, target)

    @unittest.skipUnless(shutil.which('dpkg-deb'), 'dpkg-deb required')
    def test_real_deb_is_reproducible_root_owned_script_free_and_version_bound(self):
        """Use dpkg itself to inspect packages; real runnable artifacts are exercised by CI."""
        for target, architecture, machine in [
                ('x86_64-unknown-linux-musl', 'amd64', 62),
                ('aarch64-unknown-linux-musl', 'arm64', 183)]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                packages.bundle.build(root)
                archive(root, target, elf(machine=machine))
                combined = root / 'sha256.sum'
                combined.write_text((root / f'ts-crap-{target}.tar.xz.sha256').read_text())
                output, = packages.build(root, VERSION, 1700000000, [target])
                self.assertEqual(output.name, f'ts-crap_{packages.debian_version(VERSION)}_{architecture}.deb')
                first = output.read_bytes()
                previous_umask = os.umask(0o077)
                try:
                    packages.build(root, VERSION, 1700000000, [target])
                finally:
                    os.umask(previous_umask)
                self.assertEqual(output.read_bytes(), first)
                for field, expected in [('Package', 'ts-crap'), ('Architecture', architecture),
                                        ('Version', packages.debian_version(VERSION)), ('Depends', '')]:
                    actual = subprocess.check_output(['dpkg-deb', '--field', str(output), field], text=True).strip()
                    self.assertEqual(actual, expected)
                contents = subprocess.check_output(['dpkg-deb', '--fsys-tarfile', str(output)])
                with tarfile.open(fileobj=io.BytesIO(contents)) as data:
                    members = data.getmembers()
                    self.assertTrue(all(m.uid == m.gid == 0 for m in members))
                    names = {m.name.removeprefix('./') for m in members}
                    self.assertIn('usr/bin/ts-crap', names)
                    self.assertIn('usr/share/ts-crap/coverage-helpers/version.json', names)
                    self.assertIn('usr/share/doc/ts-crap/musl-COPYRIGHT', names)
                    binary = next(m for m in members if m.name.endswith('/bin/ts-crap'))
                    self.assertEqual(binary.mode, 0o755)
                    self.assertEqual(data.extractfile(binary).read(), elf(machine=machine))
                controls = subprocess.check_output(['dpkg-deb', '--ctrl-tarfile', str(output)])
                with tarfile.open(fileobj=io.BytesIO(controls)) as control:
                    self.assertEqual({m.name.removeprefix('./') for m in control if m.isfile()}, {'control'})
                self.assertEqual(packages.verified(output), first)
                checksums = combined.read_text().splitlines()
                self.assertEqual(len(checksums), 2)
                self.assertEqual(checksums[-1].split()[1], output.name)
                victim = root / 'victim.txt'
                victim.write_text('preserve me')
                checksum = output.with_name(output.name + '.sha256')
                checksum.unlink()
                checksum.symlink_to(victim)
                packages.build(root, VERSION, 1700000000, [target])
                self.assertEqual(victim.read_text(), 'preserve me')
                self.assertFalse(checksum.is_symlink())
                self.assertEqual(packages.verified(output), first)

    @unittest.skipUnless(shutil.which('dpkg-deb'), 'dpkg-deb required')
    def test_concurrent_architecture_builds_keep_all_checksums(self):
        """Concurrent real package builds must not lose the other architecture's manifest entry."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages.bundle.build(root)
            combined = root / 'sha256.sum'
            combined.write_text('')
            targets = [('x86_64-unknown-linux-musl', 62), ('aarch64-unknown-linux-musl', 183)]
            for target, machine in targets:
                archive(root, target, elf(machine=machine))
            with ThreadPoolExecutor(max_workers=2) as executor:
                builds = [executor.submit(packages.build, root, VERSION, 1700000000, [target])
                          for target, _ in targets]
                outputs = [task.result(timeout=30)[0] for task in builds]
            fields = [line.split() for line in combined.read_text().splitlines()]
            self.assertEqual(len(fields), 2)
            self.assertEqual({name for _, name in fields}, {p.name for p in outputs})
            for digest, name in fields:
                self.assertEqual(digest, hashlib.sha256((root / name).read_bytes()).hexdigest())

    @unittest.skipUnless(shutil.which('dpkg') and shutil.which('dpkg-deb'), 'dpkg required')
    def test_minimal_image_doc_filter_keeps_owned_helpers(self):
        """A real dpkg install can exclude notices while retaining the upgrade probe's directory."""
        architecture = subprocess.check_output(['dpkg', '--print-architecture'], text=True).strip()
        matching = [(t, m) for t, (a, m) in packages.TARGETS.items() if a == architecture]
        if not matching:
            self.skipTest('unsupported native Debian architecture')
        target, machine = matching[0]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages.bundle.build(root)
            archive(root, target, elf(machine=machine))
            output, = packages.build(root, VERSION, 1700000000, [target])
            installed = root / 'installed'
            installed.mkdir()
            command = ['dpkg', '--root=' + str(installed), '--force-not-root', '--force-bad-path',
                       '--path-exclude=/usr/share/doc/*', '--install', str(output)]
            subprocess.run(command, check=True, capture_output=True, text=True)
            self.assertTrue((installed / 'usr/share/ts-crap/coverage-helpers/version.json').is_file())
            self.assertFalse((installed / 'usr/share/doc/ts-crap/musl-COPYRIGHT').exists())
            # The notice is present in the package, independent of installation policy.
            with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(
                    ['dpkg-deb', '--fsys-tarfile', str(output)]))) as data:
                self.assertTrue(any(m.name.endswith('/musl-COPYRIGHT') for m in data))

    def test_bundle_rejects_version_mismatch_and_unlisted_files(self):
        """Optional installed helpers must still match the scanner and the strict bundle allowlist."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = packages.bundle.build(root)
            with self.assertRaisesRegex(ValueError, 'version mismatch'):
                packages.helpers(path, '0.0.0')
            with zipfile.ZipFile(path, 'a') as source:
                source.writestr('../../escape', 'bad')
            path.with_name(path.name + '.sha256').write_text(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.name + '\n')
            with self.assertRaisesRegex(ValueError, 'bundle entries'):
                packages.helpers(path, VERSION)


if __name__ == '__main__':
    unittest.main()
