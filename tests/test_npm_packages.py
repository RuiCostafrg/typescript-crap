"""npm packages carry exactly the verified release bytes, with no install-time code."""
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest
import unittest.mock
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


npm = module('npm_packages', ROOT / 'scripts/npm-packages.py')
verifier = module('verify_npm_packages', ROOT / 'scripts/verify-npm-packages.py')
installer = module('verify_npm_install', ROOT / 'scripts/verify-npm-install.py')
bundle = module('bundle', ROOT / 'scripts/bundle-coverage.py')
linux_tests = module('linux_tests', ROOT / 'tests/test_linux_packages.py')
VERSION = npm.linux.bundle.tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']


def macho(cpu):
    return b'\xcf\xfa\xed\xfe' + struct.pack('<I', cpu) + b'\0' * 56


def pe(machine=0x8664):
    data = bytearray(0x100)
    data[:2] = b'MZ'
    struct.pack_into('<I', data, 0x3C, 0x80)
    data[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<H', data, 0x84, machine)
    return bytes(data)


EXECUTABLES = {
    'x86_64-unknown-linux-musl': linux_tests.elf(62),
    'aarch64-unknown-linux-musl': linux_tests.elf(183),
    'x86_64-apple-darwin': macho(0x01000007),
    'aarch64-apple-darwin': macho(0x0100000C),
    'x86_64-pc-windows-msvc': pe(),
}


def checksum(path):
    (path.parent / (path.name + '.sha256')).write_text(hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + path.name + '\n')


def write_archive(assets, target, data, extra=()):
    """An archive in the cargo-dist layout: a nested executable plus notices."""
    path = assets / npm.archive_name(target)
    if 'windows' in target:
        with zipfile.ZipFile(path, 'w') as output:
            output.writestr('LICENSE', 'MIT\n')
            output.writestr('ts-crap.exe', data)
            for name, contents in extra:
                output.writestr(name, contents)
    else:
        with tarfile.open(path, 'w:xz') as output:
            for name, contents in [('LICENSE', b'MIT\n'), ('ts-crap', data), *extra]:
                entry = tarfile.TarInfo(f'ts-crap-{target}/{name}')
                entry.size, entry.mode = len(contents), 0o755
                output.addfile(entry, io.BytesIO(contents))
    checksum(path)
    return path


def release_assets(root):
    assets = root / 'assets'
    assets.mkdir()
    for target, data in EXECUTABLES.items():
        write_archive(assets, target, data)
    bundle.build(assets)
    return assets


def read(path):
    return verifier.read_tarball(path)


def rewrite(path, change):
    """Repack a tarball after CHANGE edits its {name: (bytes, mode)} mapping."""
    files = read(path)
    change(files)
    path.write_bytes(npm.tarball(files))


class NpmPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = release_assets(self.root)

    def build(self, name='packages'):
        return npm.build(self.assets, VERSION, self.root / name)

    def test_builds_platform_packages_first_and_main_last_reproducibly(self):
        first = self.build('first')
        second = self.build('second')
        self.assertEqual([p.name for p in first], [npm.tarball_name(npm.platform_name(t), VERSION) for t in npm.TARGETS]
                         + [npm.tarball_name('typescript-crap', VERSION)])
        self.assertEqual([p.read_bytes() for p in first], [p.read_bytes() for p in second])
        with gzip.open(first[0]) as stream, tarfile.open(fileobj=io.BytesIO(stream.read())) as archive:
            for member in archive.getmembers():
                self.assertEqual((member.mtime, member.uid, member.gid, member.uname), (npm.MTIME, 0, 0, ''))
        verifier.verify(self.assets, self.root / 'first', VERSION)

    def test_platform_packages_hold_the_archive_executable_and_select_by_os_and_cpu(self):
        self.build()
        expected = {'x86_64-unknown-linux-musl': ('linux', 'x64'), 'aarch64-unknown-linux-musl': ('linux', 'arm64'),
                    'x86_64-apple-darwin': ('darwin', 'x64'), 'aarch64-apple-darwin': ('darwin', 'arm64'),
                    'x86_64-pc-windows-msvc': ('win32', 'x64')}
        for target, (os_name, cpu) in expected.items():
            with self.subTest(target=target):
                name = npm.platform_name(target)
                files = read(self.root / 'packages' / npm.tarball_name(name, VERSION))
                fields = json.loads(files['package.json'][0])
                self.assertEqual((fields['name'], fields['version'], fields['os'], fields['cpu']),
                                 (f'typescript-crap-{os_name}-{cpu}', VERSION, [os_name], [cpu]))
                self.assertTrue(fields['preferUnplugged'])
                self.assertNotIn('libc', fields)
                self.assertEqual(fields['repository'], {'type': 'git', 'url': 'git+https://github.com/RuiCostafrg/typescript-crap.git'})
                for key in ('scripts', 'bin', 'dependencies', 'optionalDependencies'):
                    self.assertNotIn(key, fields)
                binary = 'bin/ts-crap.exe' if os_name == 'win32' else 'bin/ts-crap'
                self.assertEqual(files[binary], (EXECUTABLES[target], 0o755))
                self.assertEqual('musl-COPYRIGHT' in files, os_name == 'linux')
                self.assertEqual(files['LICENSE'][0], (ROOT / 'LICENSE').read_bytes())

    def test_main_package_pins_every_platform_and_ships_the_helper_bundle_bytes(self):
        self.build()
        files = read(self.root / 'packages' / npm.tarball_name('typescript-crap', VERSION))
        fields = json.loads(files['package.json'][0])
        self.assertEqual(fields['optionalDependencies'], {
            'typescript-crap-linux-x64': VERSION, 'typescript-crap-linux-arm64': VERSION,
            'typescript-crap-darwin-x64': VERSION, 'typescript-crap-darwin-arm64': VERSION,
            'typescript-crap-win32-x64': VERSION})
        self.assertEqual(fields['bin'], {'ts-crap': 'bin/ts-crap.js', 'typescript-crap': 'bin/typescript-crap.js'})
        self.assertEqual((fields['main'], fields['type'], fields['engines']), ('lib/index.js', 'commonjs', {'node': '>=18'}))
        self.assertTrue(fields['preferUnplugged'])
        for key in verifier.FORBIDDEN:
            self.assertNotIn(key, fields)
        with zipfile.ZipFile(self.assets / bundle.NAME) as helpers:
            expected = {e.filename.removeprefix(bundle.PREFIX): helpers.read(e) for e in helpers.infolist()}
        self.assertEqual({k.removeprefix('coverage/'): v[0] for k, v in files.items() if k.startswith('coverage/')}, expected)
        self.assertIn('coverage/coverage-run.mjs', files)
        for relative, mode in npm.LAUNCHER.items():
            self.assertEqual(files[relative], ((ROOT / 'packaging/npm' / relative).read_bytes(), mode))
        self.assertEqual(files['README.md'][0], (ROOT / 'packaging/npm/README.md').read_bytes())

    def test_rejects_tampered_or_mismatched_inputs_before_writing(self):
        cases = {
            'checksum': lambda: (self.assets / npm.archive_name('x86_64-apple-darwin')).write_bytes(b'tampered'),
            'darwin architecture': lambda: write_archive(self.assets, 'x86_64-apple-darwin', EXECUTABLES['aarch64-apple-darwin']),
            'windows architecture': lambda: write_archive(self.assets, 'x86_64-pc-windows-msvc', pe(0x014C)),
            'not PE': lambda: write_archive(self.assets, 'x86_64-pc-windows-msvc', b'MZ' + b'\0' * 70),
            'linux architecture': lambda: write_archive(self.assets, 'x86_64-unknown-linux-musl', linux_tests.elf(183)),
            'dynamic ELF': lambda: write_archive(self.assets, 'aarch64-unknown-linux-musl', linux_tests.elf(183, category=2, dynamic_tag=1)),
            'duplicate executable': lambda: write_archive(self.assets, 'aarch64-apple-darwin', EXECUTABLES['aarch64-apple-darwin'],
                                                          extra=[('nested/ts-crap', EXECUTABLES['aarch64-apple-darwin'])]),
            'duplicate windows executable': lambda: write_archive(self.assets, 'x86_64-pc-windows-msvc', pe(),
                                                                  extra=[('sub/ts-crap.exe', pe())]),
            'helper version': lambda: self.rebuild_helpers('9.9.9'),
        }
        for name, damage in cases.items():
            with self.subTest(case=name):
                self.assets = release_assets(Path(tempfile.mkdtemp(dir=self.root)))
                damage()
                output = self.root / f'out-{len(list(self.root.iterdir()))}'
                with self.assertRaises(ValueError):
                    npm.build(self.assets, VERSION, output)
                self.assertFalse(output.exists())

    def rebuild_helpers(self, version):
        source = self.root / f'source-{version}'
        (source / 'scripts').mkdir(parents=True, exist_ok=True)
        (source / 'docs').mkdir(exist_ok=True)
        (source / 'Cargo.toml').write_text(f'[package]\nversion = "{version}"\n')
        for name in bundle.MODULES:
            (source / 'scripts' / name).write_bytes((ROOT / 'scripts' / name).read_bytes())
        (source / 'docs/COVERAGE_HELPERS.md').write_text('x')
        (source / 'LICENSE').write_text('x')
        bundle.build(self.assets, source)

    def test_symlink_executable_is_rejected(self):
        path = self.assets / npm.archive_name('x86_64-apple-darwin')
        with tarfile.open(path, 'w:xz') as output:
            entry = tarfile.TarInfo('ts-crap-x86_64-apple-darwin/ts-crap')
            entry.type, entry.linkname = tarfile.SYMTYPE, '/bin/sh'
            output.addfile(entry)
        checksum(path)
        with self.assertRaisesRegex(ValueError, 'one bounded regular executable'):
            npm.executable(self.assets, 'x86_64-apple-darwin')

    def test_rejects_non_semver_versions_and_existing_output(self):
        for version in ['../1.0.0', '1.0', '1.0.0\n', '1.0.0+build']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                npm.packages(self.assets, version)
        (self.root / 'exists').mkdir()
        with self.assertRaises(FileExistsError):
            npm.build(self.assets, VERSION, self.root / 'exists')

    @unittest.skipIf(os.name == 'nt', 'POSIX modes')
    def test_output_directory_gets_the_normal_umask_mode(self):
        previous = os.umask(0o027)
        try:
            output = self.build()[0].parent
        finally:
            os.umask(previous)
        self.assertEqual(output.stat().st_mode & 0o777, 0o750)

    def test_failed_write_leaves_no_partial_output(self):
        output = self.root / 'packages'
        original = Path.write_bytes
        calls = []

        def failing(path, data):
            calls.append(path)
            if len(calls) == 3:
                raise OSError('disk full')
            return original(path, data)
        with unittest.mock.patch.object(Path, 'write_bytes', failing), self.assertRaisesRegex(OSError, 'disk full'):
            npm.build(self.assets, VERSION, output)
        self.assertFalse(output.exists())
        self.assertEqual([p.name for p in self.root.iterdir() if p.name.startswith('.npm-packages-')], [])
        npm.build(self.assets, VERSION, output)
        verifier.verify(self.assets, output, VERSION)


class VerifyNpmPackagesTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = release_assets(self.root)
        self.packages = self.root / 'packages'
        npm.build(self.assets, VERSION, self.packages)

    def tarball(self, name):
        return self.packages / npm.tarball_name(name, VERSION)

    def assertRejected(self, pattern):
        with self.assertRaisesRegex(ValueError, pattern):
            verifier.verify(self.assets, self.packages, VERSION)

    def test_accepts_the_built_packages(self):
        verifier.verify(self.assets, self.packages, VERSION)

    def test_rejects_a_substituted_executable(self):
        rewrite(self.tarball('typescript-crap-linux-x64'),
                lambda f: f.update({'bin/ts-crap': (linux_tests.elf(62) + b'\0', 0o755)}))
        self.assertRejected('differs from ts-crap-x86_64-unknown-linux-musl.tar.xz')

    def test_rejects_a_non_executable_binary(self):
        rewrite(self.tarball('typescript-crap-darwin-arm64'),
                lambda f: f.update({'bin/ts-crap': (f['bin/ts-crap'][0], 0o644)}))
        self.assertRejected('not executable')

    def test_rejects_install_scripts_and_dependencies(self):
        for key, value in [('scripts', {'postinstall': 'node x.js'}), ('dependencies', {'left-pad': '1'}),
                           ('bundleDependencies', ['x'])]:
            with self.subTest(key=key):
                original = self.tarball('typescript-crap').read_bytes()

                def add(files):
                    fields = json.loads(files['package.json'][0])
                    fields[key] = value
                    files['package.json'] = (json.dumps(fields).encode(), 0o644)
                rewrite(self.tarball('typescript-crap'), add)
                self.assertRejected(f'must not declare {key}')
                self.tarball('typescript-crap').write_bytes(original)

    def test_rejects_wrong_platform_metadata_and_loose_pins(self):
        for name, key, value in [('typescript-crap-win32-x64', 'cpu', ['arm64']),
                                 ('typescript-crap-linux-arm64', 'os', ['darwin']),
                                 ('typescript-crap-linux-arm64', 'version', '0.0.1'),
                                 ('typescript-crap', 'optionalDependencies', {'typescript-crap-linux-x64': f'^{VERSION}'}),
                                 ('typescript-crap', 'bin', {'ts-crap': 'evil.js'})]:
            with self.subTest(name=name, key=key):
                original = self.tarball(name).read_bytes()

                def change(files):
                    fields = json.loads(files['package.json'][0])
                    fields[key] = value
                    files['package.json'] = (json.dumps(fields).encode(), 0o644)
                rewrite(self.tarball(name), change)
                self.assertRejected(f'package.json {key} is')
                self.tarball(name).write_bytes(original)

    def test_rejects_changed_or_extra_helper_files(self):
        original = self.tarball('typescript-crap').read_bytes()
        rewrite(self.tarball('typescript-crap'),
                lambda f: f.update({'coverage/coverage-run.mjs': (b'// changed\n', 0o644)}))
        self.assertRejected('coverage/ differs')
        self.tarball('typescript-crap').write_bytes(original)
        rewrite(self.tarball('typescript-crap'), lambda f: f.update({'lib/extra.js': (b'', 0o644)}))
        self.assertRejected('unexpected files')

    def test_rejects_changed_launcher_notices_and_manifest_fields(self):
        cases = [('typescript-crap', 'lib/index.js', b'// changed\n', 0o644),
                 ('typescript-crap', 'bin/ts-crap.js', b'#!/usr/bin/env node\n', 0o755),
                 ('typescript-crap', 'README.md', b'changed', 0o644),
                 ('typescript-crap-linux-x64', 'musl-COPYRIGHT', b'changed', 0o644),
                 ('typescript-crap-win32-x64', 'LICENSE', b'changed', 0o644)]
        for name, relative, data, mode in cases:
            with self.subTest(name=name, file=relative):
                original = self.tarball(name).read_bytes()
                rewrite(self.tarball(name), lambda f: f.update({relative: (data, mode)}))
                self.assertRejected(rf"files differ from the generated package: \['{relative}'\]")
                self.tarball(name).write_bytes(original)
        for name, key in [('typescript-crap', 'preferUnplugged'), ('typescript-crap', 'engines'),
                          ('typescript-crap-darwin-x64', 'preferUnplugged')]:
            with self.subTest(name=name, removed=key):
                original = self.tarball(name).read_bytes()

                def remove(files):
                    fields = json.loads(files['package.json'][0])
                    del fields[key]
                    files['package.json'] = (npm.manifest(fields), 0o644)
                rewrite(self.tarball(name), remove)
                self.assertRejected(r"files differ from the generated package: \['package.json'\]")
                self.tarball(name).write_bytes(original)

    def test_rejects_extra_files_in_platform_packages(self):
        rewrite(self.tarball('typescript-crap-darwin-x64'), lambda f: f.update({'install.js': (b'', 0o644)}))
        self.assertRejected('unexpected files')

    def test_rejects_bin_in_platform_packages(self):
        def add(files):
            fields = json.loads(files['package.json'][0])
            fields['bin'] = {'ts-crap': 'bin/ts-crap'}
            files['package.json'] = (json.dumps(fields).encode(), 0o644)
        rewrite(self.tarball('typescript-crap-linux-x64'), add)
        self.assertRejected('must not declare bin')

    def test_rejects_missing_or_unexpected_tarballs(self):
        (self.packages / 'other-1.0.0.tgz').write_bytes(b'')
        self.assertRejected('expected exactly')
        (self.packages / 'other-1.0.0.tgz').unlink()
        self.tarball('typescript-crap-win32-x64').unlink()
        self.assertRejected('expected exactly')

    def test_rejects_unsafe_tarball_entries(self):
        for name, kind in [('package/../escape', tarfile.REGTYPE), ('other/file', tarfile.REGTYPE),
                           ('package/link', tarfile.SYMTYPE), ('package/dir', tarfile.DIRTYPE)]:
            with self.subTest(entry=name):
                raw = io.BytesIO()
                with tarfile.open(fileobj=raw, mode='w:gz') as output:
                    entry = tarfile.TarInfo(name)
                    entry.type = kind
                    output.addfile(entry, io.BytesIO(b'') if kind == tarfile.REGTYPE else None)
                path = self.root / 'unsafe.tgz'
                path.write_bytes(raw.getvalue())
                with self.assertRaisesRegex(ValueError, 'unexpected tarball entry'):
                    verifier.read_tarball(path)

    def test_rejects_duplicate_tarball_entries(self):
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode='w:gz') as output:
            for _ in range(2):
                output.addfile(tarfile.TarInfo('package/a'), io.BytesIO(b''))
        path = self.root / 'duplicate.tgz'
        path.write_bytes(raw.getvalue())
        with self.assertRaisesRegex(ValueError, 'duplicate entry a'):
            verifier.read_tarball(path)


class VerifyNpmInstallTests(unittest.TestCase):
    def test_host_targets_use_static_musl_on_linux(self):
        for system, machine, target in [('Linux', 'x86_64', 'x86_64-unknown-linux-musl'),
                                        ('Linux', 'aarch64', 'aarch64-unknown-linux-musl'),
                                        ('Linux', 'arm64', 'aarch64-unknown-linux-musl'),
                                        ('Darwin', 'arm64', 'aarch64-apple-darwin'),
                                        ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
                                        ('Windows', 'AMD64', 'x86_64-pc-windows-msvc')]:
            self.assertEqual(installer.host_target(system, machine), target)
        for system, machine in [('Windows', 'ARM64'), ('FreeBSD', 'amd64'), ('Linux', 'riscv64')]:
            with self.assertRaises(ValueError):
                installer.host_target(system, machine)

    def test_registry_manifest_pins_the_exact_published_version(self):
        for manager in installer.MANAGERS:
            fields = installer.manifest(manager, '2.3.0', None)
            self.assertEqual(fields['dependencies'], {'typescript-crap': '2.3.0'})
            self.assertEqual(set(fields) - {'name', 'version', 'private', 'dependencies'}, set())

    def test_packed_manifest_pins_every_platform_through_each_managers_override_field(self):
        packages = Path('/packed')
        field = {'npm': 'optionalDependencies', 'bun': 'overrides', 'yarn': 'resolutions'}
        for manager in installer.MANAGERS:
            with self.subTest(manager=manager):
                fields = installer.manifest(manager, '2.3.0', packages)
                if manager == 'pnpm':
                    self.assertNotIn('pnpm', fields)
                    pinned = json.loads(installer.workspace_files(manager, '2.3.0', packages)['pnpm-workspace.yaml'])['overrides']
                else:
                    pinned = fields[field[manager]]
                self.assertEqual(fields['dependencies']['typescript-crap'],
                                 'file:' + (packages / 'typescript-crap-2.3.0.tgz').resolve().as_posix())
                self.assertEqual(sorted(pinned), sorted(npm.platform_name(t) for t in npm.TARGETS))
                for name, spec in pinned.items():
                    self.assertEqual(spec, 'file:' + (packages / f'{name}-2.3.0.tgz').resolve().as_posix())

    def test_workspace_files_mark_yarn_projects_and_pin_pnpm_only_when_packed(self):
        self.assertEqual(installer.workspace_files('yarn', '2.3.0', None), {'yarn.lock': ''})
        self.assertEqual(installer.workspace_files('pnpm', '2.3.0', None), {})
        self.assertEqual(installer.workspace_files('npm', '2.3.0', Path('/p')), {})
        self.assertEqual(installer.workspace_files('bun', '2.3.0', Path('/p')), {})

    def test_runs_never_download_the_unrelated_ts_crap_package(self):
        self.assertEqual(installer.RUN['npm'], ['npm', 'exec', '--no', '--'])
        self.assertIn('--ignore-scripts', installer.INSTALL['npm'])
        self.assertIn('--ignore-scripts', installer.INSTALL['pnpm'])
        self.assertIn('--ignore-scripts', installer.INSTALL['bun'])
        self.assertEqual(installer.ENVIRONMENT['YARN_ENABLE_SCRIPTS'], 'false')
        for manager in installer.MANAGERS:
            self.assertIn('typescript-crap@2.3.0', installer.ONE_OFF[manager]('typescript-crap@2.3.0'))

    def test_provenance_count_reads_npm_audit_signatures_output(self):
        self.assertEqual(installer.attested_count(
            'audited 2 packages in 1s\n\n2 packages have verified registry signatures\n\n2 packages have verified attestations\n'), 2)
        self.assertEqual(installer.attested_count('1 package has a verified attestation\n'), 1)
        self.assertEqual(installer.attested_count('2 packages have verified registry signatures\n'), 0)

    def test_other_platform_packages_are_detected_in_every_layout(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = Path(temporary)
            (project / 'node_modules/typescript-crap-linux-x64').mkdir(parents=True)
            (project / 'node_modules/typescript-crap').mkdir()
            self.assertEqual(installer.other_platform_paths(project, 'x86_64-unknown-linux-musl'), [])
            for stray in ['node_modules/typescript-crap-darwin-arm64',
                          'node_modules/.pnpm/typescript-crap-win32-x64@2.3.0',
                          '.yarn/unplugged/typescript-crap-linux-arm64-npm-2.3.0-abc']:
                with self.subTest(stray=stray):
                    (project / stray).mkdir(parents=True)
                    self.assertEqual(installer.other_platform_paths(project, 'x86_64-unknown-linux-musl'), [project / stray])
                    (project / stray).rmdir()


if __name__ == '__main__':
    unittest.main()
