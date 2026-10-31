"""The release installer selects the static musl build on every Linux system."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'tests/fixtures/installer/cargo-dist-0.33.1-select-archive.sh'
spec = importlib.util.spec_from_file_location('prefer_static', ROOT / 'scripts/prefer-static-linux-installer.py')
prefer_static = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prefer_static)


def select(installer_text, arch, glibc):
    """Run the installer's own select_archive_for_arch with a fake ldd."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)
        (directory / 'ldd').write_text(f'#!/bin/sh\necho "ldd (GNU libc) {glibc}"\n')
        (directory / 'ldd').chmod(0o755)
        script = directory / 'select.sh'
        script.write_text('say() { echo "$1" >&2; }\nerr() { echo "$1" >&2; exit 1; }\n'
                          + installer_text + f'\nselect_archive_for_arch "{arch}"\n')
        result = subprocess.run(['sh', str(script)], capture_output=True, text=True,
                                env={**os.environ, 'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}'})
    if result.returncode:
        raise AssertionError(result.stderr)
    return result.stdout.strip()


class StaticLinuxInstallerTests(unittest.TestCase):
    def setUp(self):
        self.original = FIXTURE.read_text()
        self.patched = prefer_static.patch(self.original)

    def test_generated_installer_prefers_gnu_only_on_new_glibc(self):
        for arch in ['aarch64', 'x86_64']:
            with self.subTest(arch=arch):
                self.assertEqual(select(self.original, f'{arch}-unknown-linux-gnu', '2.43'),
                                 f'ts-crap-{arch}-unknown-linux-gnu.tar.xz')
                self.assertEqual(select(self.original, f'{arch}-unknown-linux-gnu', '2.39'),
                                 f'ts-crap-{arch}-unknown-linux-musl.tar.xz')

    def test_patched_installer_selects_static_musl_on_every_linux(self):
        for arch in ['aarch64', 'x86_64']:
            for detected in ['gnu', 'musl-dynamic', 'musl-static']:
                for glibc in ['2.17', '2.39', '2.43', '2.99']:
                    with self.subTest(arch=arch, detected=detected, glibc=glibc):
                        self.assertEqual(select(self.patched, f'{arch}-unknown-linux-{detected}', glibc),
                                         f'ts-crap-{arch}-unknown-linux-musl.tar.xz')

    def test_other_platforms_are_unchanged(self):
        for arch, archive in [('aarch64-apple-darwin', 'ts-crap-aarch64-apple-darwin.tar.xz'),
                              ('x86_64-apple-darwin', 'ts-crap-x86_64-apple-darwin.tar.xz'),
                              ('x86_64-pc-windows-msvc', 'ts-crap-x86_64-pc-windows-msvc.zip'),
                              ('aarch64-pc-windows-msvc', 'ts-crap-x86_64-pc-windows-msvc.zip')]:
            with self.subTest(arch=arch):
                self.assertEqual(select(self.patched, arch, '2.43'), archive)
        removed = [line for line in self.original.splitlines() if line not in self.patched.splitlines()]
        self.assertTrue(all('gnu' in line or 'check_glibc "2"' in line or line.strip() in
                            {'_archive=""', 'fi', 'if [ -n "$_archive" ]; then', 'echo "$_archive"',
                             'return 0'} for line in removed), removed)

    def test_rejects_unexpected_or_already_patched_templates(self):
        for text in [self.patched, self.original.replace('"2" "43"', '"2" "44"'),
                     self.original.replace('aarch64-unknown-linux-gnu")', 'aarch64-unknown-linux-gnu" )'),
                     self.original + self.original, '']:
            with self.assertRaisesRegex(ValueError, 'unexpected installer template'):
                prefer_static.patch(text)

    def test_rejects_any_remaining_glibc_selection(self):
        extra = '\n'.join(['        "riscv64-unknown-linux-gnu")',
                           '            if ! check_glibc "2" "40"; then', '                _archive=""',
                           '            fi', '            ;;'])
        text = self.original.replace('    case "$_true_arch" in \n', '    case "$_true_arch" in \n' + extra + '\n', 1)
        self.assertNotEqual(text, self.original)
        with self.assertRaisesRegex(ValueError, 'still selects a GNU archive'):
            prefer_static.patch(text)

    def test_command_line_rewrites_only_regular_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            installer = Path(temporary) / 'ts-crap-installer.sh'
            installer.write_text(self.original)
            script = ROOT / 'scripts/prefer-static-linux-installer.py'
            subprocess.run(['python3', '-I', str(script), str(installer)], check=True)
            self.assertEqual(installer.read_text(), self.patched)
            link = Path(temporary) / 'link.sh'
            link.symlink_to(installer)
            result = subprocess.run(['python3', '-I', str(script), str(link)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('regular installer file', result.stderr)


if __name__ == '__main__':
    unittest.main()
