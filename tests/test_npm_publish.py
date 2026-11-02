"""The npm publish job: least privilege, attested inputs only, platform packages first."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = (ROOT / '.github/workflows/release.yml').read_text()


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


packages_tests = module('packages_tests', ROOT / 'tests/test_npm_packages.py')
npm = packages_tests.npm
VERSION = packages_tests.VERSION
TOOLS = all(shutil.which(t) for t in ('bash', 'jq', 'unzip', 'tar', 'openssl', 'cmp', 'diff', 'base64'))


def job(name):
    """The text of one job in release.yml."""
    body = WORKFLOW.split(f'\n  {name}:\n', 1)[1]
    following = [body.find(f'\n  {other}:\n') for other in ('plan', 'build', 'verify', 'verify-npm', 'assemble', 'publish', 'publish-npm')]
    ends = [i for i in following if i >= 0]
    return body[:min(ends)] if ends else body


def step(job_text, name):
    """The run script of the step called NAME, dedented."""
    block = job_text.split(f'- name: {name}\n', 1)[1]
    # GitHub's `shell: bash` is `bash -eo pipefail`, which run_step reproduces.
    assert block.split('        run: |\n', 1)[0].count('        shell: bash\n') == 1, name
    script = block.split('        run: |\n', 1)[1]
    lines = []
    for line in script.splitlines():
        if line and not line.startswith('          '):
            break
        lines.append(line[10:])
    return '\n'.join(lines) + '\n'


class PublishJobShapeTests(unittest.TestCase):
    def test_publish_npm_has_only_oidc_and_read_permissions_after_the_github_release(self):
        text = job('publish-npm')
        self.assertIn('    needs: [plan, publish]\n', text)
        self.assertIn("    if: needs.plan.outputs.publish == 'true'\n", text)
        self.assertIn('    environment: release\n', text)
        self.assertIn('    runs-on: ubuntu-26.04\n', text)
        self.assertIn('    concurrency:\n      group: npm-publish\n      cancel-in-progress: false\n', text)
        permissions = text.split('    permissions:\n', 1)[1].split('    steps:\n', 1)[0]
        self.assertEqual(sorted(line.strip() for line in permissions.splitlines() if line.strip()),
                         ['attestations: read', 'contents: read', 'id-token: write'])
        for forbidden in ('actions/checkout', 'NPM_TOKEN', 'NODE_AUTH_TOKEN', 'secrets.', 'registry-url',
                          'contents: write', 'python3', 'scripts/'):
            self.assertNotIn(forbidden, text)

    def test_github_publication_waits_for_npm_verification_and_attests_the_npm_tarballs(self):
        text = job('publish')
        self.assertIn('    needs: [plan, assemble, verify, verify-npm]\n', text)
        self.assertNotIn('npm publish', text)
        self.assertIn('          subject-path: |\n            release-assets/*\n            npm-packages/*\n', text)
        # Attested, but only the release assets are attached to the GitHub release.
        self.assertIn('--generate-notes "${flags[@]}" release-assets/*\n', text)
        self.assertIn('retention-days: 30', job('assemble'))
        self.assertIn('uses: ./.github/workflows/npm-install.yml', job('verify-npm'))
        assemble = job('assemble')
        self.assertIn('scripts/npm-packages.py --assets release-assets', assemble)
        self.assertIn('scripts/verify-npm-packages.py --assets release-assets --packages npm-packages', assemble)

    def test_npm_cli_is_pinned_by_sha512_and_installed_without_scripts(self):
        script = step(job('publish-npm'), 'Install the pinned npm CLI (trusted publishing needs npm 11.5.1 or later)')
        self.assertIn('https://registry.npmjs.org/npm/-/npm-12.2.1.tgz', script)
        self.assertRegex(script, r'echo "[0-9a-f]{128}  \$RUNNER_TEMP/npm-12\.2\.1\.tgz" \| sha512sum --check')
        self.assertIn('npm install -g --ignore-scripts', script)
        self.assertIn('[[ "$(npm --version)" == 12.2.1 ]]', script)


@unittest.skipUnless(TOOLS and os.name != 'nt', 'needs bash, jq, unzip, tar, openssl, cmp and diff')
class PublishJobScriptTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.assets = packages_tests.release_assets(self.root)
        self.work = self.root / 'job'
        self.work.mkdir()
        npm.build(self.assets, VERSION, self.work / 'npm-packages')
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.calls = self.root / 'calls'
        self.calls.write_text('')
        # gh serves the release assets from disk; attestation can be made to fail per asset.
        (self.bin / 'gh').write_text(
            '#!/bin/bash\necho "gh $*" >> "$CALLS"\n'
            'if [[ "$1 $2" == "release download" ]]; then\n'
            '  shift 2; dir=""; names=()\n'
            '  while (($#)); do case "$1" in --dir) dir=$2; shift 2;; --pattern) names+=("$2"); shift 2;; *) shift;; esac; done\n'
            '  mkdir -p "$dir"; for n in "${names[@]}"; do cp "$ASSETS/$n" "$dir/"; done; exit 0\n'
            'fi\n'
            'if [[ "$1 $2" == "attestation verify" && -n "$UNATTESTED" && "$3" == *"$UNATTESTED" ]]; then exit 1; fi\n')
        self.published = self.root / 'published'
        self.published.mkdir()
        (self.bin / 'npm').write_text(
            '#!/bin/bash\necho "npm $*" >> "$CALLS"\n'
            'if [[ "$1 $3" == "view dist-tags.latest" ]]; then [[ -f "$PUBLISHED/latest" ]] && cat "$PUBLISHED/latest"; exit 0; fi\n'
            'if [[ "$1" == view ]]; then f="$PUBLISHED/${2%@*}"; [[ -f "$f" ]] || exit 0\n'
            '  if [[ "$3" == version ]]; then echo "${2##*@}"; else cat "$f"; fi; exit 0; fi\n'
            'if [[ "$1" == publish ]]; then n=$(basename "$2"); n=${n%-$VERSION.tgz}\n'
            '  echo "sha512-$(openssl dgst -sha512 -binary "$2" | base64 -w0)" > "$PUBLISHED/$n"; fi\n')
        (self.bin / 'sleep').write_text('#!/bin/sh\nexit 0\n')
        for name in ('gh', 'npm', 'sleep'):
            (self.bin / name).chmod(0o755)

    def run_step(self, name, tag=None, **environment):
        script = step(job('publish-npm'), name)
        env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}', CALLS=str(self.calls), ASSETS=str(self.assets),
                   PUBLISHED=str(self.published), TAG=tag or f'v{VERSION}', VERSION=(tag or f'v{VERSION}')[1:], GITHUB_REPOSITORY='RuiCostafrg/typescript-crap',
                   **{'UNATTESTED': '', **environment})
        return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', script], cwd=self.work, env=env,
                              capture_output=True, text=True, timeout=120)

    def verify(self, **environment):
        return self.run_step('Compare every npm executable and helper with the attested release', **environment)

    def tarball(self, name):
        return self.work / 'npm-packages' / npm.tarball_name(name, VERSION)

    def test_verification_accepts_matching_packages_and_authenticates_every_asset(self):
        result = self.verify()
        self.assertEqual(result.returncode, 0, result.stderr)
        verified = [line for line in self.calls.read_text().splitlines() if line.startswith('gh attestation verify')]
        self.assertEqual(len(verified), 12)
        for name in [npm.tarball_name(npm.platform_name(t), VERSION) for t in npm.TARGETS] + [npm.tarball_name('typescript-crap', VERSION)]:
            self.assertTrue(any(f'npm-packages/{name} ' in line for line in verified), name)
        for line in verified:
            self.assertIn('--cert-identity https://github.com/RuiCostafrg/typescript-crap/.github/workflows/release.yml@refs/tags/v'
                          + VERSION, line)
            self.assertIn('--deny-self-hosted-runners', line)

    def test_verification_rejects_an_unattested_asset(self):
        for asset in ('ts-crap-x86_64-pc-windows-msvc.zip', 'ts-crap-coverage-helpers.zip',
                      npm.tarball_name('typescript-crap', VERSION), npm.tarball_name('typescript-crap-linux-x64', VERSION)):
            with self.subTest(asset=asset):
                self.assertNotEqual(self.verify(UNATTESTED=asset).returncode, 0)

    def test_verification_rejects_substituted_executables_helpers_and_metadata(self):
        def manifest_change(key, value):
            def change(files):
                fields = json.loads(files['package.json'][0])
                fields[key] = value
                files['package.json'] = (npm.manifest(fields), 0o644)
            return change
        cases = [
            ('typescript-crap-win32-x64', lambda f: f.update({'bin/ts-crap.exe': (packages_tests.pe() + b'x', 0o755)})),
            ('typescript-crap-linux-arm64', lambda f: f.update({'bin/ts-crap': (packages_tests.linux_tests.elf(183) + b'x', 0o755)})),
            ('typescript-crap', lambda f: f.update({'coverage/coverage-run.mjs': (b'// changed\n', 0o644)})),
            ('typescript-crap', lambda f: f.update({'coverage/extra.mjs': (b'', 0o644)})),
            ('typescript-crap', manifest_change('scripts', {'postinstall': 'node x'})),
            ('typescript-crap', manifest_change('optionalDependencies', {npm.platform_name(t): f'^{VERSION}' for t in npm.TARGETS})),
            ('typescript-crap-darwin-x64', manifest_change('scripts', {'install': 'node x'})),
            ('typescript-crap-darwin-x64', manifest_change('version', '0.0.1')),
        ]
        for name, change in cases:
            with self.subTest(name=name):
                original = self.tarball(name).read_bytes()
                packages_tests.rewrite(self.tarball(name), change)
                self.assertNotEqual(self.verify().returncode, 0)
                self.tarball(name).write_bytes(original)

    def test_verification_rejects_entries_outside_package_or_with_dot_segments(self):
        for entry in ('package/../escape', 'package/./x', 'other/file', 'package/.hidden'):
            with self.subTest(entry=entry):
                path = self.tarball('typescript-crap-darwin-x64')
                original = path.read_bytes()
                files = packages_tests.read(path)
                raw = npm.tarball(files)
                import gzip, io, tarfile
                with tarfile.open(fileobj=io.BytesIO(gzip.decompress(raw))) as source:
                    members = [(m, source.extractfile(m).read()) for m in source.getmembers()]
                out = io.BytesIO()
                with tarfile.open(fileobj=out, mode='w:gz') as output:
                    for member, data in members:
                        output.addfile(member, io.BytesIO(data))
                    extra = tarfile.TarInfo(entry)
                    output.addfile(extra, io.BytesIO(b''))
                path.write_bytes(out.getvalue())
                self.assertNotEqual(self.verify().returncode, 0)
                path.write_bytes(original)

    def test_verification_rejects_missing_or_extra_tarballs(self):
        extra = self.work / 'npm-packages' / 'other-1.0.0.tgz'
        extra.write_bytes(b'')
        self.assertNotEqual(self.verify().returncode, 0)
        extra.unlink()
        self.tarball('typescript-crap-linux-x64').unlink()
        self.assertNotEqual(self.verify().returncode, 0)

    def publish(self, tag=None):
        return self.run_step('Publish platform packages, then the main package, with provenance', tag=tag)

    def published_calls(self):
        return [line for line in self.calls.read_text().splitlines() if line.startswith('npm publish')]

    def test_publishes_platform_packages_first_and_the_main_package_last_with_provenance(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = [npm.platform_name(t) for t in npm.TARGETS] + ['typescript-crap']
        self.assertEqual(self.published_calls(), [
            f'npm publish npm-packages/{name}-{VERSION}.tgz --access public --provenance --tag latest' for name in expected])

    def test_prereleases_use_the_next_dist_tag(self):
        for path in (self.work / 'npm-packages').iterdir():
            path.rename(path.with_name(path.name.replace(f'-{VERSION}.tgz', f'-{VERSION}-rc.1.tgz')))
        result = self.publish(tag=f'v{VERSION}-rc.1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all(call.endswith('--tag next') for call in self.published_calls()))
        self.assertEqual(len(self.published_calls()), 6)

    def test_an_older_stable_release_does_not_move_latest(self):
        # Numeric, not lexical: 10 > 9 in every position.
        major, minor, patch = (int(p) for p in VERSION.split('-')[0].split('.'))
        for older in ('0.0.0', f'{major}.{minor}.{patch}', f'{major}.{minor}.{patch - 1}' if patch else '0.0.0',
                      f'{major - 1}.99.99' if major else '0.0.0'):
            with self.subTest(latest=older):
                (self.published / 'latest').write_text(older + '\n')
                self.assertTrue(self.dist_tag_is('latest'))
        for newer in (f'{major}.{minor}.{patch + 10}', f'{major}.{minor + 10}.0', f'{major + 1}.0.0'):
            with self.subTest(latest=newer):
                (self.published / 'latest').write_text(newer + '\n')
                self.assertTrue(self.dist_tag_is('backport'))

    def dist_tag_is(self, tag):
        for name in [npm.platform_name(t) for t in npm.TARGETS] + ['typescript-crap']:
            (self.published / name).unlink(missing_ok=True)
        self.calls.write_text('')
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.published_calls()
        return len(calls) == 6 and all(call.endswith(f'--tag {tag}') for call in calls)

    def test_main_package_waits_until_the_registry_serves_every_platform_package(self):
        # A registry that never serves linux-arm64 stops before the main package.
        npm_stub = (self.bin / 'npm').read_text().replace(
            'if [[ "$3" == version ]]', 'if [[ "$3" == version && "$2" != typescript-crap-linux-arm64@* ]]')
        npm_stub = npm_stub.replace('then echo "${2##*@}"; else cat "$f"; fi',
                                    'then echo "${2##*@}"; elif [[ "$3" != version ]]; then cat "$f"; fi')
        (self.bin / 'npm').write_text(npm_stub)
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f'not served yet: typescript-crap-linux-arm64@{VERSION}', result.stdout)
        self.assertEqual(sum('version' == line.split()[-1] and 'linux-arm64' in line
                             for line in self.calls.read_text().splitlines()), 30)
        self.assertEqual(len(self.published_calls()), 5)
        self.assertNotIn('npm-packages/typescript-crap-' + VERSION, ' '.join(self.published_calls()))

    def test_registry_lookup_errors_stop_before_publishing(self):
        stub = (self.bin / 'npm').read_text()
        for failing in ('dist-tags.latest', 'dist.integrity'):
            with self.subTest(lookup=failing):
                (self.bin / 'npm').write_text(stub.replace(
                    'echo "npm $*" >> "$CALLS"\n',
                    f'echo "npm $*" >> "$CALLS"\nif [[ "$1 $3" == "view {failing}" ]]; then echo E500 >&2; exit 1; fi\n'))
                self.calls.write_text('')
                self.assertNotEqual(self.publish().returncode, 0)
                self.assertEqual(self.published_calls(), [])
        (self.bin / 'npm').write_text(stub)

    def test_rerun_skips_identical_published_versions_and_stops_on_different_ones(self):
        name = 'typescript-crap-linux-x64'
        digest = base64.b64encode(hashlib.sha512(self.tarball(name).read_bytes()).digest()).decode()
        (self.published / name).write_text(f'sha512-{digest}\n')
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'{name}@{VERSION} is already published with identical contents', result.stdout)
        self.assertEqual(len(self.published_calls()), 5)
        self.calls.write_text('')
        (self.published / 'typescript-crap-darwin-x64').write_text('sha512-different\n')
        result = self.publish()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('already published with different contents', result.stdout)
        # The first run's packages are now identical and skipped; publishing stops at
        # the conflict, so nothing after it, and never the main package, is published.
        self.assertEqual(self.published_calls(), [])


if __name__ == '__main__':
    unittest.main()
