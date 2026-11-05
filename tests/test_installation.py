"""Reject invalid staged release evidence before executing an installer."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import sys
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('verify_installation', ROOT / 'scripts/verify-installation.py')
installation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installation)


class InstallationEvidenceTests(unittest.TestCase):
    def test_installer_archive_selection_stays_on_the_same_platform(self):
        self.assertEqual(installation.archive_target('x86_64-unknown-linux-gnu', None),
                         'x86_64-unknown-linux-gnu')
        for arch in ['aarch64', 'x86_64']:
            self.assertEqual(installation.archive_target(f'{arch}-unknown-linux-gnu', f'{arch}-unknown-linux-musl'),
                             f'{arch}-unknown-linux-musl')
        for target, selected in [('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-musl'),
                                 ('aarch64-apple-darwin', 'aarch64-unknown-linux-musl'),
                                 ('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-musl'),
                                 ('aarch64-unknown-linux-gnu', 'aarch64-apple-darwin'),
                                 ('x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-gnux')]:
            with self.subTest(target=target, selected=selected), self.assertRaises(ValueError):
                installation.archive_target(target, selected)

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


class InstallationCommandTests(unittest.TestCase):
    def test_slow_command_timeout_keeps_partial_stdout_and_stderr(self):
        command = [sys.executable, '-c',
                   "import sys,time; print('output-'+str(17*13),flush=True); "
                   "print('error-'+str(13*19),file=sys.stderr,flush=True); time.sleep(10)"]
        with self.assertRaises(RuntimeError) as failure:
            installation.run(command, timeout=1)
        message = str(failure.exception)
        self.assertIn('timed out after 1 seconds', message)
        self.assertIn('output-221', message.splitlines())
        self.assertIn('error-247', message.splitlines())

    def test_command_override_keeps_real_exit_failures_and_output(self):
        command = [sys.executable, '-c', "print('complete'); raise SystemExit(7)"]
        self.assertEqual(installation.run(command, expected=7, timeout=10), 'complete\n')
        with self.assertRaisesRegex(RuntimeError, 'expected 0, got 7'):
            installation.run(command, timeout=10)


class LiveHomebrewIsolationTests(unittest.TestCase):
    def test_escaped_prefix_cellar_or_cache_is_rejected_before_package_operations(self):
        from contextlib import redirect_stdout
        from unittest import mock
        live_spec = importlib.util.spec_from_file_location('verify_live_homebrew', ROOT / 'scripts/verify-live-homebrew.py')
        live = importlib.util.module_from_spec(live_spec)
        live_spec.loader.exec_module(live)
        for escaped in ['--prefix', '--cellar', '--cache']:
            with self.subTest(escaped=escaped), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                prefix = root/'homebrew'
                commands = []
                def command(args, **_kwargs):
                    commands.append(args)
                    if args == ['/system/brew', '--repository']:
                        return str(root/'seed')
                    if str(args[0]) == str(prefix/'bin/brew'):
                        if args[1] == escaped:
                            return '/system/unsafe'
                        return str({'--prefix':prefix, '--cellar':prefix/'Cellar', '--cache':root/'cache'}[args[1]])
                    return ''
                with mock.patch.object(live.shutil, 'which', return_value='/system/brew'), \
                     mock.patch.object(live.tempfile, 'TemporaryDirectory') as directory, \
                     mock.patch.object(live.installation, 'run', side_effect=command), \
                     mock.patch.object(live.installation, 'stage') as stage, \
                     redirect_stdout(io.StringIO()):
                    directory.return_value.__enter__.return_value = str(root)
                    with self.assertRaisesRegex(ValueError, 'escaped the disposable'):
                        live.verify(object())
                stage.assert_not_called()
                package_operations = {'install','uninstall','tap','update','upgrade','reinstall'}
                self.assertFalse(any(len(cmd)>1 and cmd[1] in package_operations for cmd in commands))


    def test_disposable_homebrew_is_outside_runtime_writable_trees(self):
        from contextlib import redirect_stdout
        from unittest import mock
        live_spec = importlib.util.spec_from_file_location('verify_live_homebrew_layout', ROOT / 'scripts/verify-live-homebrew.py')
        live = importlib.util.module_from_spec(live_spec)
        live_spec.loader.exec_module(live)
        class BootstrapComplete(Exception):
            pass
        with tempfile.TemporaryDirectory() as temporary:
            user_directory = Path(temporary).resolve()/'runner-account'
            user_directory.mkdir()
            prefix = None
            runtime = {}
            def command(args, **kwargs):
                nonlocal prefix
                runtime.update(kwargs['env'])
                if args == ['/system/brew', '--repository']:
                    return str(user_directory/'system-brew')
                if args[:2] == ['git', 'clone']:
                    prefix = Path(args[-1])
                if prefix and str(args[0]) == str(prefix/'bin/brew'):
                    if args[1] == 'list':
                        raise BootstrapComplete()
                    return str({'--prefix':prefix, '--cellar':prefix/'Cellar', '--cache':prefix.parent/'cache'}[args[1]])
                return ''
            with mock.patch.object(live.shutil, 'which', return_value='/system/brew'), \
                 mock.patch.object(live.Path, 'home', return_value=user_directory), \
                 mock.patch.object(live.installation, 'run', side_effect=command), \
                 redirect_stdout(io.StringIO()):
                with self.assertRaises(BootstrapComplete):
                    live.verify(object())
            self.assertTrue(prefix.is_relative_to(user_directory))
            for key in ['HOMEBREW_TEMP','HOMEBREW_CACHE','HOMEBREW_LOGS','XDG_CONFIG_HOME']:
                self.assertFalse(prefix.is_relative_to(Path(runtime[key])))
            self.assertFalse(prefix.parent.exists(), 'Disposable Homebrew was not removed')
