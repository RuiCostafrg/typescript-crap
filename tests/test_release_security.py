"""Security regressions for untrusted release bytes and archive metadata."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
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
    def test_tag_publication_requires_every_exact_source_quality_workflow(self):
        """Reject publication when any exact-source main quality workflow fails."""
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        preflight = workflow.split('          git merge-base --is-ancestor', 1)[1]
        preflight = 'git merge-base --is-ancestor' + preflight.split('      - name:', 1)[0]
        required = ['ci.yml', 'coverage.yml', 'mutation.yml',
                    'installation.yml', 'verification.yml']
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'git').write_text('#!/bin/bash\nexit 0\n')
            (directory / 'gh').write_text(
                '#!/bin/bash\n'
                'echo "$*" >> "$CALLS"\n'
                'if [[ -n "$FAIL_WORKFLOW" && "$*" == *"/$FAIL_WORKFLOW/"* ]]; '
                'then echo false; else echo true; fi\n')
            for name in ['git', 'gh']:
                (directory / name).chmod(0o755)
            for failure in ['', *required]:
                with self.subTest(failed_workflow=failure):
                    calls = directory / 'calls'
                    calls.write_text('')
                    environment = dict(os.environ, PATH=f'{directory}:/usr/bin:/bin',
                                       CALLS=str(calls), FAIL_WORKFLOW=failure,
                                       EVENT='push', REF='refs/tags/v2.0.1',
                                       GITHUB_SHA='exact-source-sha',
                                       GITHUB_REPOSITORY='owner/repo')
                    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', preflight],
                                            env=environment, capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, not failure, result.stderr)
                    if not failure:
                        commands = calls.read_text().splitlines()
                        self.assertEqual(len(commands), len(required))
                        for name, command in zip(required, commands):
                            self.assertIn(f'/{name}/runs?head_sha=exact-source-sha&event=push', command)
                            self.assertIn('head_branch == "main"', command)

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
        names = [f"ts-crap-{target}.{'zip' if 'windows' in target else 'tar.xz'}"
                 for target in release.TARGETS] + ['ts-crap-coverage-helpers.zip']
        for name in names:
            (source / name).write_bytes(b'archive fixture')
            digest = hashlib.sha256(b'archive fixture').hexdigest()
            (source / (name + '.sha256')).write_text(f'{digest} *{name}\n')
        for name in ['ts-crap-installer.sh', 'ts-crap-installer.ps1', 'ts-crap.rb', 'sha256.sum', 'dist-manifest.json']:
            (source / name).write_text('fixture')

    def test_corrupted_archive_and_misbound_checksum_cannot_publish(self):
        for attack, name in [(attack, name)
                             for attack in ['corruption', 'filename']
                             for name in ['ts-crap-aarch64-apple-darwin.tar.xz',
                                          'ts-crap-coverage-helpers.zip']]:
            with self.subTest(attack=attack, name=name), tempfile.TemporaryDirectory() as temporary:
                source = Path(temporary)
                self.fixture(source)
                archive = source / name
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
            self.assertEqual(len(list(output.iterdir())), 17)
            self.assertFalse((output / 'unrelated-private-file').exists())

if __name__ == '__main__':
    unittest.main()
