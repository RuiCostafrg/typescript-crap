"""Regression tests for the GitHub Action's install and scan scripts."""
import hashlib
import io
import json
import re
import os
import shutil
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BASH = shutil.which('bash')
INSTALL = ROOT / 'action/install.sh'
SCAN = ROOT / 'action/scan.sh'
IDENTITY = 'https://github.com/RuiCostafrg/typescript-crap/.github/workflows/release.yml@refs/tags/v2.2.1'

FAKE_GH = r'''#!/usr/bin/env python3
import fnmatch, json, os, shutil, sys
args = sys.argv[1:]
with open(os.environ['GH_CALLS'], 'a') as calls:
    calls.write(json.dumps(args) + '\n')
if args[:2] == ['release', 'download']:
    patterns = [args[i + 1] for i, a in enumerate(args) if a == '--pattern']
    destination = args[args.index('--dir') + 1]
    source = os.path.join(os.environ['GH_RELEASES'], args[2])
    if not os.path.isdir(source):
        sys.exit('release not found')
    for name in os.listdir(source):
        if any(fnmatch.fnmatchcase(name, p) for p in patterns):
            shutil.copy(os.path.join(source, name), destination)
elif args[:2] == ['attestation', 'verify']:
    if os.path.basename(args[2]) in os.environ.get('GH_UNATTESTED', '').split():
        sys.exit('no matching attestation')
else:
    sys.exit('unexpected gh call')
'''

FAKE_TS_CRAP = '''#!/bin/sh
if [ "$1" = --version ]; then
  printf '%s' "${GH_TOKEN:-}${GITHUB_TOKEN:-}" > "$SCAN_CALLS.token"
  echo "ts-crap REPORTED"; exit 0
fi
for argument in "$@"; do printf '%s\\n' "$argument" >> "$SCAN_CALLS"; done
echo --- >> "$SCAN_CALLS"
while [ $# -gt 0 ]; do
  if [ "$1" = --output ]; then
    printf '%s' "$SARIF_REPORT" > "$2"
    exit "${SARIF_STATUS:-0}"
  fi
  shift
done
exit "${TEXT_STATUS:-0}"
'''


SARIF_EMPTY = '{"version":"2.1.0","runs":[{"results":[]}]}'
SARIF_WITH_RESULT = ('{"version":"2.1.0","runs":[{"results":[{"message":{"text":"discount has CRAP 12.000 '
                     '(threshold 5)"},"locations":[{"physicalLocation":{"artifactLocation":'
                     '{"uri":"src/pricing.mts"},"region":{"startLine":1}}}]}]}]}')


def tar_xz(entries):
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode='w:xz') as archive:
        for name, contents in entries:
            entry = tarfile.TarInfo(name)
            entry.size = len(contents)
            entry.mode = 0o755
            archive.addfile(entry, io.BytesIO(contents))
    return data.getvalue()


def executable_zip(entries):
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w') as archive:
        for name, contents in entries:
            entry = zipfile.ZipInfo(name)
            entry.create_system = 3
            entry.external_attr = 0o755 << 16
            archive.writestr(entry, contents)
    return data.getvalue()


def helper_zip():
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w') as archive:
        archive.writestr('ts-crap-coverage-helpers/coverage.mjs', 'export {}\n')
    return data.getvalue()


class ActionScripts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.bin = self.root / 'fake-bin'
        self.bin.mkdir()
        (self.bin / 'gh').write_text(FAKE_GH)
        (self.bin / 'gh').chmod(0o755)
        self.releases = self.root / 'releases'
        self.runner_temp = self.root / 'runner temp'
        self.runner_temp.mkdir()
        for name in ['path', 'output', 'env', 'calls', 'scan-calls']:
            (self.root / name).write_text('')

    def tearDown(self):
        self.temporary.cleanup()

    def publish(self, version='2.2.1', target='x86_64-unknown-linux-musl', reported=None,
                archive=None, checksum=None, helpers=True):
        release = self.releases / f'v{version}'
        release.mkdir(parents=True, exist_ok=True)
        name = f'ts-crap-{target}.tar.xz'
        executable = FAKE_TS_CRAP.replace('REPORTED', reported or version).encode()
        raw = archive if archive is not None else tar_xz(
            [(f'ts-crap-{target}/ts-crap', executable), (f'ts-crap-{target}/LICENSE', b'MIT')])
        (release / name).write_bytes(raw)
        digest = hashlib.sha256(raw).hexdigest()
        (release / f'{name}.sha256').write_text(
            checksum if checksum is not None else f'{digest} *{name}\n')
        if helpers:
            bundle = helper_zip()
            (release / 'ts-crap-coverage-helpers.zip').write_bytes(bundle)
            (release / 'ts-crap-coverage-helpers.zip.sha256').write_text(
                f'{hashlib.sha256(bundle).hexdigest()}  ts-crap-coverage-helpers.zip\n')
        return name

    def run_script(self, script, **variables):
        environment = {
            'PATH': f'{self.bin}{os.pathsep}{os.environ["PATH"]}',
            'HOME': str(self.root),
            'GH_CALLS': str(self.root / 'calls'),
            'GH_RELEASES': str(self.releases),
            'SCAN_CALLS': str(self.root / 'scan-calls'),
            'RUNNER_OS': 'Linux', 'RUNNER_ARCH': 'X64',
            'RUNNER_TEMP': str(self.runner_temp),
            'GITHUB_PATH': str(self.root / 'path'),
            'GITHUB_OUTPUT': str(self.root / 'output'),
            'GITHUB_ENV': str(self.root / 'env'),
            'SARIF_REPORT': SARIF_EMPTY,
        }
        environment.update(variables)
        cwd = environment.pop('WORKDIR', self.root)
        return subprocess.run([BASH, str(script)], env=environment,
                              capture_output=True, text=True, cwd=cwd)

    def install(self, version='2.2.1', **variables):
        return self.run_script(INSTALL, TS_CRAP_ACTION_VERSION=version, **variables)

    def outputs(self):
        lines = (self.root / 'output').read_text().splitlines()
        return dict(line.split('=', 1) for line in lines)

    def assert_rejected(self, result, message):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertEqual((self.root / 'path').read_text(), '')
        self.assertEqual((self.root / 'output').read_text(), '')

    def test_installs_attested_static_linux_build_on_path(self):
        for arch, target in [('X64', 'x86_64-unknown-linux-musl'),
                             ('ARM64', 'aarch64-unknown-linux-musl')]:
            with self.subTest(arch=arch):
                (self.root / 'path').write_text('')
                (self.root / 'output').write_text('')
                (self.root / 'calls').write_text('')
                name = self.publish(target=target)
                result = self.install('v2.2.1', RUNNER_ARCH=arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                outputs = self.outputs()
                self.assertEqual(outputs['version'], '2.2.1')
                self.assertEqual(outputs['target'], target)
                executable = Path(outputs['executable'])
                self.assertEqual(executable.parent, Path(outputs['bin-dir']))
                self.assertTrue(executable.is_relative_to(self.runner_temp))
                self.assertEqual((self.root / 'path').read_text(), outputs['bin-dir'] + '\n')
                self.assertEqual((self.root / 'env').read_text(), '')
                calls = self.gh_calls()
                self.assertEqual(calls, [
                    ['release', 'download', 'v2.2.1', '--repo', 'RuiCostafrg/typescript-crap',
                     '--pattern', name, '--pattern', f'{name}.sha256',
                     '--dir', str(executable.parents[2] / 'download')],
                    ['attestation', 'verify', str(executable.parents[2] / 'download' / name),
                     '--repo', 'RuiCostafrg/typescript-crap', '--cert-identity', IDENTITY,
                     '--source-ref', 'refs/tags/v2.2.1', '--deny-self-hosted-runners']])

    def gh_calls(self):
        return [json.loads(line) for line in (self.root / 'calls').read_text().splitlines()]

    def test_platform_selection(self):
        cases = [('macOS', 'ARM64', 'aarch64-apple-darwin'), ('macOS', 'X64', 'x86_64-apple-darwin')]
        for runner_os, arch, target in cases:
            with self.subTest(runner_os=runner_os, arch=arch):
                (self.root / 'output').write_text('')
                self.publish(target=target)
                result = self.install(RUNNER_OS=runner_os, RUNNER_ARCH=arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.outputs()['target'], target)
        for runner_os, arch in [('Windows', 'ARM64'), ('Linux', 'ARM'), ('Linux', 'X86'), ('', '')]:
            with self.subTest(unsupported=f'{runner_os}/{arch}'):
                (self.root / 'path').write_text('')
                (self.root / 'output').write_text('')
                self.assert_rejected(self.install(RUNNER_OS=runner_os, RUNNER_ARCH=arch),
                                     'no ts-crap release build')

    def test_checksums_paths_containing_backslashes(self):
        # Windows RUNNER_TEMP is D:\\a\\_temp; GNU sha256sum escapes such paths.
        temp = self.root / 'a\\_temp'
        temp.mkdir()
        self.publish()
        result = self.install(RUNNER_TEMP=str(temp))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(Path(self.outputs()['executable']).is_relative_to(temp))

    def test_windows_installs_the_zip_and_reports_native_paths(self):
        (self.bin / 'cygpath').write_text('#!/bin/sh\n[ "$1" = -w ] && printf "W:%s\\n" "$2"\n')
        (self.bin / 'cygpath').chmod(0o755)
        target = 'x86_64-pc-windows-msvc'
        name = f'ts-crap-{target}.zip'
        executable = FAKE_TS_CRAP.replace('REPORTED', '2.2.1').encode()
        self.publish(target=target, archive=executable_zip(
            [('ts-crap.exe', executable), ('LICENSE', b'MIT')]))
        release = self.releases / 'v2.2.1'
        (release / f'ts-crap-{target}.tar.xz').rename(release / name)
        (release / f'ts-crap-{target}.tar.xz.sha256').unlink()
        raw = (release / name).read_bytes()
        (release / f'{name}.sha256').write_text(f'{hashlib.sha256(raw).hexdigest()} *{name}\n')
        result = self.install(RUNNER_OS='Windows', TS_CRAP_ACTION_HELPERS='true')
        self.assertEqual(result.returncode, 0, result.stderr)
        outputs = self.outputs()
        self.assertEqual(outputs['target'], target)
        self.assertTrue(outputs['executable'].startswith('W:'))
        self.assertTrue(outputs['executable'].endswith('/tool/ts-crap.exe'))
        self.assertEqual((self.root / 'path').read_text(), outputs['bin-dir'] + '\n')
        self.assertTrue(outputs['bin-dir'].startswith('W:'))
        self.assertEqual((self.root / 'env').read_text(), f"TS_CRAP_HELPERS={outputs['helpers-dir']}\n")
        self.assertTrue(outputs['helpers-dir'].startswith('W:'))

    def test_requires_the_github_cli(self):
        empty = self.root / 'no-tools'
        empty.mkdir()
        self.assert_rejected(self.install(PATH=str(empty)), 'GitHub CLI (gh) is required')

    def test_installs_matching_helpers_when_requested(self):
        self.publish()
        result = self.install(TS_CRAP_ACTION_HELPERS='true')
        self.assertEqual(result.returncode, 0, result.stderr)
        helpers = self.outputs()['helpers-dir']
        self.assertTrue((Path(helpers) / 'coverage.mjs').is_file())
        self.assertEqual((self.root / 'env').read_text(), f'TS_CRAP_HELPERS={helpers}\n')
        attested = [call[2] for call in self.gh_calls() if call[0] == 'attestation']
        self.assertEqual([Path(path).name for path in attested],
                         ['ts-crap-x86_64-unknown-linux-musl.tar.xz', 'ts-crap-coverage-helpers.zip'])

    def test_rejects_unpinned_malformed_and_unsupported_versions(self):
        for version in ['', 'latest', '2.2', 'v', '2.2.1 ', '2.2.1;id', '$(id)', '2.2.1+build',
                        '2.2.0', '2.1.9', '1.9.9', '2.2.1-rc.1', '0.10.0']:
            with self.subTest(version=version):
                self.assert_rejected(self.install(version), 'version')
        self.assertEqual((self.root / 'calls').read_text(), '')

    def test_accepts_newer_versions_and_prereleases(self):
        for version in ['2.2.2', '2.3.0-rc.1', '2.10.0', '3.0.0', '10.0.0']:
            with self.subTest(version=version):
                (self.root / 'output').write_text('')
                self.publish(version=version, helpers=False)
                result = self.install(version)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.outputs()['version'], version)

    def test_rejects_unattested_assets(self):
        self.publish()
        for asset in ['ts-crap-x86_64-unknown-linux-musl.tar.xz', 'ts-crap-coverage-helpers.zip']:
            with self.subTest(asset=asset):
                self.assert_rejected(self.install(TS_CRAP_ACTION_HELPERS='true', GH_UNATTESTED=asset),
                                     f'build provenance verification failed for {asset}')

    def test_rejects_checksum_mismatch_and_misbound_checksum(self):
        name = 'ts-crap-x86_64-unknown-linux-musl.tar.xz'
        cases = [
            ('0' * 64 + f' *{name}\n', 'SHA-256 mismatch'),
            (hashlib.sha256(b'x').hexdigest() + '  other.tar.xz\n', "checksum file names 'other.tar.xz'"),
            ('not a checksum\n', 'malformed checksum'),
            ('', 'malformed checksum'),
        ]
        for checksum, message in cases:
            with self.subTest(message=message):
                self.publish(checksum=checksum)
                self.assert_rejected(self.install(), message)

    def test_rejects_missing_assets_and_releases(self):
        self.assert_rejected(self.install('2.9.9'), 'could not download v2.9.9')
        self.publish(target='aarch64-apple-darwin')
        self.assert_rejected(self.install(), 'has no ts-crap-x86_64-unknown-linux-musl.tar.xz')

    def test_rejects_wrong_or_ambiguous_executables(self):
        self.publish(reported='2.2.0')
        self.assert_rejected(self.install(), "reports 'ts-crap 2.2.0'")
        executable = FAKE_TS_CRAP.replace('REPORTED', '2.2.1').encode()
        self.publish(archive=tar_xz([('a/ts-crap', executable), ('b/ts-crap', executable)]))
        self.assert_rejected(self.install(), 'found 2')
        self.publish(archive=tar_xz([('a/README.md', b'no binary')]))
        self.assert_rejected(self.install(), 'found 0')

    def scan(self, args='', sarif='false', **variables):
        self.publish()
        installed = self.install()
        self.assertEqual(installed.returncode, 0, installed.stderr)
        executable = self.outputs()['executable']
        (self.root / 'output').write_text('')
        return self.run_script(SCAN, TS_CRAP_ACTION_EXECUTABLE=executable,
                               TS_CRAP_ACTION_ARGS=args, TS_CRAP_ACTION_SARIF=sarif, **variables)

    def scan_calls(self):
        text = (self.root / 'scan-calls').read_text()
        return [call.splitlines() for call in text.split('---\n') if call]

    def test_scan_passes_one_argument_per_line_without_shell_evaluation(self):
        result = self.scan('--root\r\nsrc dir/$(id)\n\n  \n--threshold\n15\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.scan_calls(), [['--root', 'src dir/$(id)', '--threshold', '15']])
        self.assertEqual(self.outputs(), {'exit-code': '0'})

    def test_scan_without_arguments_scans_the_project(self):
        result = self.scan()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'scan-calls').read_text(), '---\n')

    def test_sarif_scan_runs_once_and_records_gate_status(self):
        for status, expected in [('0', '0'), ('1', '1'), ('2', '2'), ('3', '2')]:
            with self.subTest(status=status):
                (self.root / 'scan-calls').write_text('')
                result = self.scan('--threshold\n5', 'true', SARIF_STATUS=status)
                self.assertEqual(result.returncode, 0, result.stderr)
                outputs = self.outputs()
                self.assertEqual(outputs['exit-code'], expected)
                report = Path(outputs['sarif-file'])
                self.assertTrue(report.is_file())
                self.assertTrue(report.is_relative_to(self.runner_temp))
                self.assertEqual(self.scan_calls(), [
                    ['--format', 'sarif', '--output', str(report), '--threshold', '5']])

    def test_sarif_scan_prints_a_readable_summary(self):
        result = self.scan('--threshold\n5', 'true', SARIF_STATUS='1', SARIF_REPORT=SARIF_WITH_RESULT)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('src/pricing.mts:1: discount has CRAP 12.000 (threshold 5)\n1 SARIF result(s)', result.stdout)

    def test_sarif_options_go_before_user_arguments_and_respect_the_separator(self):
        result = self.scan('--threshold\n5\n--\n--output\n-o dir', 'true')
        self.assertEqual(result.returncode, 0, result.stderr)
        report = self.outputs()['sarif-file']
        self.assertEqual(self.scan_calls(), [
            ['--format', 'sarif', '--output', report, '--threshold', '5', '--', '--output', '-o dir']])

    def test_paths_containing_option_words_are_single_arguments(self):
        result = self.scan('src/my --output dir', 'true')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.scan_calls()[0][-1], 'src/my --output dir')

    def test_scan_rejects_conflicting_report_and_display_arguments(self):
        for argument, message in [('--format', 'do not pass'), ('--format=json', 'do not pass'),
                                  ('--output', 'do not pass'), ('--output=x', 'do not pass'),
                                  ('-o', 'do not pass'), ('-ox', 'do not pass'),
                                  ('--only-failing', 'only applies to text and html'),
                                  ('--sort=crap', 'only applies to text and html'),
                                  ('--sort', 'only applies to text and html'),
                                  ('--top', 'only applies to text and html')]:
            with self.subTest(argument=argument):
                (self.root / 'scan-calls').write_text('')
                result = self.scan(f'{argument}\nvalue', 'true')
                self.assertEqual(result.returncode, 2)
                self.assertIn(message, result.stderr)
                self.assertEqual(self.scan_calls(), [])
        for argument in ['--format\njson', '--only-failing\n--top\n3']:
            with self.subTest(text_mode=argument):
                result = self.scan(argument, 'false')
                self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.scan(sarif='yes').returncode, 2)

    def test_rejects_invalid_or_inconsistent_report_inputs_before_installing(self):
        self.publish()
        for variables, message in [
                ({'TS_CRAP_ACTION_HELPERS': 'yes'}, 'helpers must be true or false'),
                ({'TS_CRAP_ACTION_SCAN': 'True'}, 'scan must be true or false'),
                ({'TS_CRAP_ACTION_SARIF': '1'}, 'sarif must be true or false'),
                ({'TS_CRAP_ACTION_UPLOAD': 'on'}, 'upload-sarif must be true or false'),
                ({'TS_CRAP_ACTION_UPLOAD': 'true', 'TS_CRAP_ACTION_SCAN': 'true'},
                 'upload-sarif requires sarif: true'),
                ({'TS_CRAP_ACTION_SARIF': 'true'}, 'sarif requires scan: true'),
                ({'TS_CRAP_ACTION_UPLOAD': 'true', 'TS_CRAP_ACTION_SARIF': 'true'},
                 'sarif requires scan: true')]:
            with self.subTest(variables=variables):
                self.assert_rejected(self.install(**variables), message)
        self.assertEqual(self.gh_calls(), [])
        result = self.install(TS_CRAP_ACTION_SCAN='true', TS_CRAP_ACTION_SARIF='true',
                              TS_CRAP_ACTION_UPLOAD='true')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_downloaded_binary_never_sees_the_token(self):
        self.publish()
        result = self.install(GH_TOKEN='secret-token', GITHUB_TOKEN='other-token')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'scan-calls.token').read_text(), '')

    def test_clears_helpers_exported_by_an_earlier_run_only(self):
        self.publish()
        earlier = str(self.runner_temp / 'ts-crap-action.AbC123/helpers/ts-crap-coverage-helpers')
        for current, cleared in [(earlier, True), ('/opt/my-helpers', False), ('', False)]:
            with self.subTest(current=current):
                (self.root / 'env').write_text('')
                (self.root / 'output').write_text('')
                result = self.install(TS_CRAP_HELPERS=current)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((self.root / 'env').read_text(), 'TS_CRAP_HELPERS=\n' if cleared else '')

    def test_scan_output_cannot_issue_workflow_commands(self):
        for sarif in ['false', 'true']:
            with self.subTest(sarif=sarif):
                result = self.scan('--threshold\n5', sarif)
                self.assertEqual(result.returncode, 0, result.stderr)
                lines = result.stdout.splitlines()
                start = re.fullmatch(r'::stop-commands::([0-9a-f]{32})', lines[0])
                self.assertIsNotNone(start, lines)
                self.assertIn(f'::{start[1]}::', lines)
                self.assertNotEqual(start[1], '0' * 32)
        first = re.match(r'::stop-commands::(\w+)', self.scan('', 'true').stdout)[1]
        second = re.match(r'::stop-commands::(\w+)', self.scan('', 'true').stdout)[1]
        self.assertNotEqual(first, second)

    def test_validation_errors_are_reported_before_commands_stop(self):
        result = self.scan('--top\n3', 'true')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(self.outputs(), {'exit-code': '2'})
        self.assertNotIn('stop-commands', result.stdout + result.stderr)
        self.assertIn('::error title=ts-crap scan::', result.stderr)

    def test_sarif_paths_are_made_relative_to_the_checkout(self):
        workspace = self.root / 'checkout'
        (workspace / 'packages' / 'shop').mkdir(parents=True)
        for args, prefix in [('--root\npackages/shop', 'packages/shop/'),
                             ('--root=packages/shop', 'packages/shop/'),
                             ('--threshold\n5', ''),
                             ('--root\npackages/shop\n--\n--root\nignored', 'packages/shop/')]:
            with self.subTest(args=args):
                result = self.scan(args, 'true', SARIF_REPORT=SARIF_WITH_RESULT,
                                   GITHUB_WORKSPACE=str(workspace), WORKDIR=str(workspace))
                self.assertEqual(result.returncode, 0, result.stderr)
                report = json.loads(Path(self.outputs()['sarif-file']).read_text())
                location = report['runs'][0]['results'][0]['locations'][0]['physicalLocation']
                self.assertEqual(location['artifactLocation']['uri'], f'{prefix}src/pricing.mts')
                self.assertIn(f'{prefix}src/pricing.mts:1: discount has CRAP', result.stdout)
                self.assertNotIn('::warning', result.stdout)

    def test_sarif_prefix_is_percent_encoded_like_the_reported_uri(self):
        workspace = self.root / 'checkout'
        (workspace / 'packages' / 'shop#1 ü').mkdir(parents=True)
        report = SARIF_WITH_RESULT.replace('"src/pricing.mts"', '"sub%20dir/a%20b%23c.mts"')
        result = self.scan('--root\npackages/shop#1 ü', 'true', SARIF_REPORT=report,
                           GITHUB_WORKSPACE=str(workspace), WORKDIR=str(workspace))
        self.assertEqual(result.returncode, 0, result.stderr)
        rewritten = json.loads(Path(self.outputs()['sarif-file']).read_text())
        self.assertEqual(rewritten['runs'][0]['results'][0]['locations'][0]['physicalLocation']
                         ['artifactLocation']['uri'], 'packages/shop%231%20%C3%BC/sub%20dir/a%20b%23c.mts')
        self.assertIn('packages/shop#1 ü/sub dir/a b#c.mts:1: discount has CRAP', result.stdout)

    def test_sarif_paths_outside_the_checkout_are_left_with_a_warning(self):
        workspace = self.root / 'checkout'
        workspace.mkdir()
        result = self.scan('--root\n..', 'true', SARIF_REPORT=SARIF_WITH_RESULT,
                           GITHUB_WORKSPACE=str(workspace), WORKDIR=str(workspace))
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(Path(self.outputs()['sarif-file']).read_text())
        self.assertEqual(report['runs'][0]['results'][0]['locations'][0]['physicalLocation']
                         ['artifactLocation']['uri'], 'src/pricing.mts')
        token = re.match(r'::stop-commands::(\w+)', result.stdout)[1]
        after = result.stdout.split(f'::{token}::', 1)[1]
        self.assertIn('::warning title=ts-crap scan::scan root .. is outside the checkout', after)

    def test_summary_handles_results_without_locations_and_multiline_messages(self):
        report = ('{"version":"2.1.0","runs":[{"results":['
                  '{"ruleId":"TSCRAP002","message":{"text":"coverage is missing\\nfor 3 functions"}},'
                  '{"message":{"text":"no region"},"locations":[{"physicalLocation":'
                  '{"artifactLocation":{"uri":"a.ts"}}}]}]}]}')
        result = self.scan('', 'true', SARIF_STATUS='1', SARIF_REPORT=report)
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(lines[1:4], ['coverage is missing\\nfor 3 functions', 'a.ts: no region', '2 SARIF result(s)'])
        self.assertEqual(self.outputs()['exit-code'], '1')
        self.assertNotIn('Traceback', result.stdout + result.stderr)

    def test_summary_escapes_control_characters_and_keeps_spaces(self):
        report = SARIF_WITH_RESULT.replace('"src/pricing.mts"', '"my%20%20dir/a%09b%1B%5B31m.ts"').replace(
            'discount has CRAP', 'disc\\u001b]0;x\\u0007ount  has\\rCRAP')
        result = self.scan('', 'true', SARIF_REPORT=report)
        self.assertEqual(result.returncode, 0, result.stderr)
        line = result.stdout.splitlines()[1]
        self.assertEqual(line, 'my  dir/a\\tb\\x1b[31m.ts:1: disc\\x1b]0;x\\x07ount  has\\rCRAP 12.000 (threshold 5)')
        self.assertNotIn('\x1b', result.stdout)

    def test_summary_prints_non_ascii_names_in_utf8_mode(self):
        self.assertIn('python3 -I -X utf8 "$helper"', SCAN.read_text())
        report = SARIF_WITH_RESULT.replace('"src/pricing.mts"', '"src/%E6%B1%89.ts"')
        result = self.scan('', 'true', SARIF_REPORT=report)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('src/汉.ts:1: discount has CRAP', result.stdout)
        self.assertNotIn('::warning', result.stdout)

    def test_rejects_explicitly_empty_boolean_inputs(self):
        for name, variable in [('helpers', 'TS_CRAP_ACTION_HELPERS'), ('scan', 'TS_CRAP_ACTION_SCAN'),
                               ('sarif', 'TS_CRAP_ACTION_SARIF'), ('upload-sarif', 'TS_CRAP_ACTION_UPLOAD')]:
            with self.subTest(name=name):
                self.assert_rejected(self.install(**{variable: ''}), f'{name} must be true or false')

    def test_action_metadata_pins_actions_and_never_interpolates_inputs_into_scripts(self):
        metadata = (ROOT / 'action.yml').read_text()
        for line in metadata.splitlines():
            if 'uses:' in line and not line.lstrip().startswith('#'):
                self.assertRegex(line, r'uses: [\w./-]+@[0-9a-f]{40} # v')
        for block in metadata.split('      run: ')[1:]:
            script = block.split('\n    - ', 1)[0]
            self.assertNotIn('${{', script)


if __name__ == '__main__':
    unittest.main()
