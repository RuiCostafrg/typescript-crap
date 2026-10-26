"""Reject invalid staged release evidence before executing an installer."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('verify_installation', ROOT / 'scripts/verify-installation.py')
installation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installation)


class InstallationEvidenceTests(unittest.TestCase):
    def fixture(self, source, target='x86_64-unknown-linux-gnu', binaries=None):
        binaries = binaries or [('ts-crap', b'expected executable')]
        raw = io.BytesIO()
        if 'windows' in target:
            with zipfile.ZipFile(raw, 'w') as archive:
                for name, data in binaries:
                    archive.writestr(name, data)
            extension = 'zip'
            script = 'ts-crap-installer.ps1'
            text = "$app_version = '1.0.0'\n"
        else:
            with tarfile.open(fileobj=raw, mode='w:xz') as archive:
                for name, data in binaries:
                    entry = tarfile.TarInfo('package/' + name)
                    entry.size = len(data)
                    archive.addfile(entry, io.BytesIO(data))
            extension = 'tar.xz'
            script = 'ts-crap-installer.sh'
            text = 'APP_VERSION="1.0.0"\n'
        name = f'ts-crap-{target}.{extension}'
        (source / name).write_bytes(raw.getvalue())
        (source / (name + '.sha256')).write_text(hashlib.sha256(raw.getvalue()).hexdigest() + ' *' + name + '\n')
        (source / script).write_text(text)
        return source / name

    def test_staging_reads_only_expected_executable_and_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            self.fixture(source, binaries=[('ts-crap', b'expected executable'), ('../../escaped', b'ignored')])
            destination = source / 'staged'
            digest = installation.stage(source, destination, 'x86_64-unknown-linux-gnu', '1.0.0', 'shell')
            self.assertEqual(digest, hashlib.sha256(b'expected executable').hexdigest())
            self.assertFalse((source / 'escaped').exists())
            self.assertEqual(len(list(destination.iterdir())), 2)

    def test_tampering_and_misbound_checksums_do_not_stage(self):
        for attack in ['tamper', 'filename']:
            with self.subTest(attack=attack), tempfile.TemporaryDirectory() as temporary:
                source = Path(temporary)
                archive = self.fixture(source)
                if attack == 'tamper':
                    archive.write_bytes(b'tampered')
                else:
                    archive.with_name(archive.name + '.sha256').write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + ' *other.tar.xz\n')
                with self.assertRaises(ValueError):
                    installation.stage(source, source / 'staged', 'x86_64-unknown-linux-gnu', '1.0.0', 'shell')
                self.assertFalse((source / 'staged').exists())

    def test_installer_version_mismatch_and_duplicate_binaries_do_not_stage(self):
        for attack in ['version', 'duplicate']:
            with self.subTest(attack=attack), tempfile.TemporaryDirectory() as temporary:
                source = Path(temporary)
                self.fixture(source, binaries=[('ts-crap', b'one'), ('nested/ts-crap', b'two')]
                             if attack == 'duplicate' else None)
                with self.assertRaises(ValueError):
                    installation.stage(source, source / 'staged', 'x86_64-unknown-linux-gnu',
                                       '2.0.0' if attack == 'version' else '1.0.0', 'shell')
                self.assertFalse((source / 'staged').exists())

    def test_windows_archive_stages_the_executable(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            self.fixture(source, target='x86_64-pc-windows-msvc', binaries=[('ts-crap.exe', b'windows')])
            digest = installation.stage(source, source / 'staged', 'x86_64-pc-windows-msvc', '1.0.0', 'powershell')
            self.assertEqual(digest, hashlib.sha256(b'windows').hexdigest())

    def test_symlink_asset_and_unsafe_identity_do_not_stage(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            archive = self.fixture(source)
            real = source / 'real'
            archive.rename(real)
            archive.symlink_to(real)
            for target, version in [('x86_64-unknown-linux-gnu', '1.0.0'),
                                    ('../other', '1.0.0'),
                                    ('x86_64-unknown-linux-gnu', '../other')]:
                with self.assertRaises(ValueError):
                    installation.stage(source, source / 'staged', target, version, 'shell')
            self.assertFalse((source / 'staged').exists())
