#!/usr/bin/env python3
"""Run portable binaries and real Debian lifecycle tests in network-isolated native containers."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('linux_packages', ROOT / 'scripts/linux-packages.py')
packages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packages)
IMAGES = ('ubuntu:22.04', 'ubuntu:24.04', 'ubuntu:26.04', 'debian:12-slim',
          'debian:13-slim', 'alpine:3.22', 'fedora:43')
DEBIAN_IMAGES = IMAGES[:5]
NO_TOOLCHAINS_SHELL = '''
for tool in node rustc cargo; do
    if command -v "$tool" >/dev/null 2>&1; then
        printf 'unexpected runtime dependency: %s\n' "$tool" >&2
        exit 1
    fi
done
'''


def run(command, **kwargs):
    """Run only argument vectors, with bounded time and captured diagnostics."""
    arguments = [str(x) for x in command]
    if not arguments or arguments[0] not in ('docker', 'dpkg-deb'):
        raise ValueError('verification may execute only docker or dpkg-deb')
    # Audited: fixed executable allowlist, argument vectors, no shell expansion.
    # Dynamic paths and versions remain single arguments, never command text.
    # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
    result = subprocess.run(arguments, shell=False, capture_output=True, text=True,
                            timeout=kwargs.pop('timeout', 180), **kwargs)
    if result.returncode:
        raise RuntimeError(f'{command!r}: {result.returncode}\n{result.stdout[-12000:]}\n{result.stderr[-12000:]}')
    return result.stdout


def verify(assets, target, version, report, images=IMAGES):
    """Exercise real artifacts without compilers, Node, network or cross-architecture emulation."""
    architecture = packages.TARGETS[target][0]
    results = {'target': target, 'version': version, 'passed': False, 'images': []}
    try:
        binary = packages.executable(assets / f'ts-crap-{target}.tar.xz', target)
        deb = assets / packages.package_name(version, architecture)
        packages.verified(deb)
        with tempfile.TemporaryDirectory(prefix='linux-distribution-') as temporary:
            work = Path(temporary)
            scanner = work / 'ts-crap'
            scanner.write_bytes(binary)
            scanner.chmod(0o755)
            project = work / 'project'
            project.mkdir()
            source = 'export function choose(flag:boolean){\nif(flag)return 1;\nreturn 0;\n}\n'
            (project / 'a.ts').write_text(source, encoding='utf-8')
            coverage = {'version': 1, 'files': {'a.ts': {
                'sha256': hashlib.sha256(source.encode()).hexdigest(), 'lines': {'2': 1, '3': 1}}}}
            (project / 'coverage.json').write_text(json.dumps(coverage))
            # There is no historical .deb before this feature. An explicitly
            # synthetic revision-zero package tests a genuine dpkg upgrade,
            # including removal of an obsolete owned file, without relabelling
            # the binary as a different upstream release.
            old_root = work / 'previous-root'
            run(['dpkg-deb', '--raw-extract', deb, old_root])
            if not (old_root / 'usr/share/doc/ts-crap/musl-COPYRIGHT').is_file():
                raise ValueError('package is missing the musl license notice')
            control = old_root / 'DEBIAN/control'
            previous_version = packages.debian_version(version).removesuffix('-1') + '-0'
            control.write_text(control.read_text().replace(
                f'Version: {packages.debian_version(version)}\n', f'Version: {previous_version}\n'))
            (old_root / 'usr/share/ts-crap/coverage-helpers/obsolete-upgrade-probe').write_text('old revision\n')
            previous = work / 'previous.deb'
            run(['dpkg-deb', '--build', '--root-owner-group', old_root, previous])
            for image in images:
                run(['docker', 'pull', image], timeout=300)
                image_id = run(['docker', 'image', 'inspect', '--format', '{{.Id}}', image]).strip()
                common = ['docker', 'run', '--rm', '--network', 'none',
                          '--mount', f'type=bind,src={project},dst=/project,readonly']
                command = [*common, '--read-only', '--tmpfs', '/tmp',
                           '--mount', f'type=bind,src={scanner},dst=/usr/local/bin/ts-crap,readonly',
                           image, '/bin/sh', '-ec', NO_TOOLCHAINS_SHELL + '''
ts-crap --version
ts-crap --root /project --format json --threshold 100
ts-crap --root /project --coverage /project/coverage.json --require-coverage --threshold 2 >/dev/null
set +e
ts-crap --root /project --threshold 0 >/dev/null
status=$?
set -e
test "$status" -eq 1
''']
                output = run(command)
                first, rest = output.split('\n', 1)
                if first != f'ts-crap {version}':
                    raise ValueError(f'wrong installed scanner version: {first!r}')
                scanned = json.loads(rest)
                if (len(scanned['functions']) != 1 or scanned['failed']
                        or scanned['functions'][0]['complexity'] != 2 or scanned['functions'][0]['crap'] != 6):
                    raise ValueError('portable scanner changed fixture results')
                lifecycle = image in DEBIAN_IMAGES
                if lifecycle:
                    run([*common,
                         '--mount', f'type=bind,src={deb.resolve()},dst=/candidate.deb,readonly',
                         '--mount', f'type=bind,src={previous},dst=/previous.deb,readonly',
                         '-e', f'EXPECTED_VERSION={packages.debian_version(version)}',
                         '-e', f'PREVIOUS_VERSION={previous_version}', '-e', f'UPSTREAM_VERSION={version}',
                         image, '/bin/sh', '-exc', '''
mkdir -p /root/user-project
printf 'keep\n' > /root/user-project/keep
dpkg -i /previous.deb
test "$(dpkg-query -W -f='${Version}' ts-crap)" = "$PREVIOUS_VERSION"
test -f /usr/share/ts-crap/coverage-helpers/obsolete-upgrade-probe
dpkg -i /candidate.deb
test "$(dpkg-query -W -f='${Version}' ts-crap)" = "$EXPECTED_VERSION"
test ! -e /usr/share/ts-crap/coverage-helpers/obsolete-upgrade-probe
test "$(ts-crap --version)" = "ts-crap $UPSTREAM_VERSION"
ts-crap --root /project --coverage /project/coverage.json --require-coverage --threshold 2 >/dev/null
test -f /usr/share/ts-crap/coverage-helpers/version.json
grep -q 'musl as a whole' /usr/share/doc/ts-crap/copyright
''' + NO_TOOLCHAINS_SHELL + '''
dpkg -i /candidate.deb
dpkg --remove ts-crap
test ! -e /usr/bin/ts-crap
test ! -e /usr/share/ts-crap/coverage-helpers/version.json
test "$(cat /root/user-project/keep)" = keep
'''])
                results['images'].append({'image': image, 'image_id': image_id, 'portable_scan': True, 'deb_lifecycle': lifecycle})
                print(f'{target}: {image}: portable scan passed; Debian lifecycle={lifecycle}', flush=True)
        results['passed'] = True
    except BaseException as error:
        results['error'] = str(error)
        raise
    finally:
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--target', choices=packages.TARGETS, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    verify(args.assets.resolve(), args.target, args.version, args.report)
