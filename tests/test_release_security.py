"""Security regressions for untrusted release bytes and archive metadata."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import zipfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]

def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

installer = module('installer', ROOT / 'scripts/install-dist.py')
release = module('release', ROOT / 'scripts/prepare-release.py')

class ReleaseSecurityTests(unittest.TestCase):
    def test_tool_download_rejects_tampering_before_writing_executable(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / 'bin'
            with patch.object(installer.urllib.request, 'urlopen', return_value=io.BytesIO(b'untrusted binary')):
                with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                    installer.install('aarch64-unknown-linux-gnu', destination)
            self.assertFalse(destination.exists())

    def test_tool_archive_cannot_extract_other_files(self):
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode='w:xz') as archive:
            for name, contents in [('cargo-dist-test/dist', b'expected executable'), ('../../escaped', b'malicious')]:
                entry = tarfile.TarInfo(name)
                entry.size = len(contents)
                archive.addfile(entry, io.BytesIO(contents))
        raw = data.getvalue()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.dict(installer.ARCHIVES, {'test': ('tar.xz', hashlib.sha256(raw).hexdigest())}):
                with patch.object(installer.urllib.request, 'urlopen', return_value=io.BytesIO(raw)):
                    executable = installer.install('test', root / 'bin')
            self.assertEqual(executable.read_bytes(), b'expected executable')
            self.assertEqual(list((root / 'bin').iterdir()), [executable])
            self.assertFalse((root / 'escaped').exists())

    def test_flat_windows_archive_installs_only_the_verified_executable(self):
        data = io.BytesIO()
        with zipfile.ZipFile(data, mode='w') as archive:
            archive.writestr('dist.exe', b'windows executable')
            archive.writestr('../../unexpected', b'must not extract')
        raw = data.getvalue()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.dict(installer.ARCHIVES, {'test': ('zip', hashlib.sha256(raw).hexdigest())}):
                with patch.object(installer.urllib.request, 'urlopen', return_value=io.BytesIO(raw)):
                    executable = installer.install('test', root / 'bin')
            self.assertEqual(executable.name, 'dist.exe')
            self.assertEqual(executable.read_bytes(), b'windows executable')
            self.assertEqual(list((root / 'bin').iterdir()), [executable])

    def fixture(self, source):
        for target in release.TARGETS:
            name = f"ts-crap-{target}.{'zip' if 'windows' in target else 'tar.xz'}"
            (source / name).write_bytes(b'archive fixture')
            digest = hashlib.sha256(b'archive fixture').hexdigest()
            (source / (name + '.sha256')).write_text(f'{digest} *{name}\n')
        for name in ['ts-crap-installer.sh', 'ts-crap-installer.ps1', 'ts-crap.rb', 'sha256.sum', 'dist-manifest.json']:
            (source / name).write_text('fixture')

    def test_corrupted_archive_and_misbound_checksum_cannot_publish(self):
        for attack in ['corruption', 'filename']:
            with self.subTest(attack=attack), tempfile.TemporaryDirectory() as temporary:
                source = Path(temporary)
                self.fixture(source)
                archive = source / 'ts-crap-aarch64-apple-darwin.tar.xz'
                if attack == 'corruption':
                    archive.write_bytes(b'tampered')
                else:
                    checksum = source / (archive.name + '.sha256')
                    checksum.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + ' *other.tar.xz\n')
                with self.assertRaises(ValueError):
                    release.prepare(source, source / 'out')
                self.assertFalse((source / 'out').exists())

    def test_symlink_asset_cannot_publish(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            self.fixture(source)
            asset = source / 'ts-crap.rb'
            asset.unlink()
            asset.symlink_to(source / 'dist-manifest.json')
            with self.assertRaisesRegex(ValueError, 'non-regular'):
                release.prepare(source, source / 'out')

    def test_only_allowlisted_assets_are_published(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            self.fixture(source)
            (source / 'unrelated-private-file').write_text('must stay local')
            output = source / 'out'
            release.prepare(source, output)
            self.assertEqual(len(list(output.iterdir())), 15)
            self.assertFalse((output / 'unrelated-private-file').exists())

if __name__ == '__main__':
    unittest.main()
