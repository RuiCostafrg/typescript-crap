#!/usr/bin/env python3
"""Exercise installed helper bytes with a matching scanner outside the checkout."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile


def run(command, cwd, expected=0):
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=60)
    if result.returncode != expected:
        raise RuntimeError(f'{command}: exit {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout


def exercise(helpers, scanner, root):
    """Run the helper modules in HELPERS against SCANNER, with a project created under ROOT."""
    metadata = json.loads((helpers / 'version.json').read_text())
    if run([str(scanner), '--version'], root).strip() != 'ts-crap ' + metadata['scannerVersion']:
        raise ValueError('Helper/scanner version mismatch')
    project = root / 'project'
    project.mkdir()
    source = project / 'a.mts'
    source.write_text('export function f() {\n  return 1;\n}\n')
    (project / 'test.mjs').write_text("import { f } from './a.mts'; if (f() !== 1) throw Error('bad result');\n")
    native = root / 'native.json'
    run(['node', str(helpers / 'coverage.mjs'), '--root', str(project),
         '--output', str(native), '--', 'node', 'test.mjs'], root)
    scan = [str(scanner), '--root', str(project), '--require-coverage', '--format', 'json']
    report = json.loads(run(scan + ['--coverage', str(native)], root))
    if len(report['functions']) != 1 or report['functions'][0]['coverage'] != 1:
        raise ValueError('Native bundle collection did not produce complete coverage')
    for format_name, raw in [
        ('istanbul', json.dumps({'a.mts': {'path': 'a.mts', 'statementMap': {
            '0': {'start': {'line': 2, 'column': 2}}}, 's': {'0': 1}}})),
        ('lcov', 'SF:a.mts\nDA:2,1\nend_of_record\n'),
    ]:
        output = root / (format_name + '.json')
        raw_path = project / (format_name + '.raw')
        command = "require('node:fs').writeFileSync(" + json.dumps(str(raw_path)) + ',' + json.dumps(raw) + ')'
        run(['node', str(helpers / 'import-coverage.mjs'), '--root', str(project),
             '--format', format_name, '--input', str(raw_path), '--output', str(output),
             '--', 'node', '-e', command], root)
        imported = json.loads(run(scan + ['--coverage', str(output)], root))
        if imported['functions'][0]['coverage'] != 1:
            raise ValueError('Imported bundle coverage was incomplete')
    # The installed one-step command checks version.json against the scanner.
    framework = project / 'node_modules' / 'vitest'
    framework.mkdir(parents=True)
    (framework / 'package.json').write_text(json.dumps({'name': 'vitest', 'version': '0.0.0', 'bin': {'vitest': './vitest.mjs'}}))
    (framework / 'vitest.mjs').write_text(
        "import fs from 'node:fs'; import path from 'node:path';\n"
        "const dir = process.argv.find(a => a.startsWith('--coverage.reportsDirectory=')).split('=').slice(1).join('=');\n"
        "fs.mkdirSync(dir, { recursive: true });\n"
        "fs.writeFileSync(path.join(dir, 'coverage-final.json'), JSON.stringify({ 'a.mts': { path: 'a.mts', "
        "statementMap: { 0: { start: { line: 2, column: 2 } } }, s: { 0: 1 } } }));\n")
    (project / 'vitest.config.mjs').write_text('export default {};\n')
    provider = project / 'node_modules' / '@vitest' / 'coverage-istanbul'
    provider.mkdir(parents=True)
    (provider / 'package.json').write_text('{}')
    output = run(['node', str(helpers / 'coverage-run.mjs'), '--root', str(project), '--scan', '--scanner', str(scanner)], root)
    if 'a.mts' not in output:
        raise ValueError('Installed one-step command did not scan the project')
    source.write_text('export function f() {\n  return 2;\n}\n')
    run(scan + ['--coverage', str(native)], root, expected=2)
    # Prove the distributed loader hook retains its source-integrity guard.
    (project / 'mutate.mjs').write_text(
        "import fs from 'node:fs'; fs.appendFileSync('a.mts', '// changed'); await import('./a.mts');\n")
    rejected = root / 'rejected.json'
    run(['node', str(helpers / 'coverage.mjs'), '--root', str(project),
         '--output', str(rejected), '--', 'node', 'mutate.mjs'], root, expected=1)
    if rejected.exists():
        raise ValueError('Changed source unexpectedly published coverage')


def verify_installed(helpers, scanner):
    """Exercise an already installed helper directory, such as the npm package's coverage/."""
    with tempfile.TemporaryDirectory(prefix='ts-crap-installed helpers-') as temporary:
        exercise(helpers.resolve(), scanner.resolve(), Path(temporary))
    print('Installed helper directory: native, Istanbul, LCOV, one-step run, stale-source and loader integrity checks passed')


def verify(archive, scanner):
    archive, scanner = archive.resolve(), scanner.resolve()
    fields = Path(str(archive) + '.sha256').read_text().split()
    if (len(fields) != 2 or fields[1].lstrip('*') != archive.name
            or fields[0] != hashlib.sha256(archive.read_bytes()).hexdigest()):
        raise ValueError('Helper bundle checksum mismatch')
    with tempfile.TemporaryDirectory(prefix='ts-crap-installed helpers-') as temporary:
        root = Path(temporary)
        with zipfile.ZipFile(archive) as bundle:
            for entry in bundle.infolist():
                # Validate names before extracting even these locally built artifacts.
                parts = entry.filename.split('/')
                if (len(parts) != 2 or parts[0] != 'ts-crap-coverage-helpers'
                        or parts[1] in ('', '.', '..') or '\\' in entry.filename
                        or ':' in entry.filename or (entry.external_attr >> 16) & 0o170000 != 0o100000):
                    raise ValueError('Invalid helper bundle entry')
            bundle.extractall(root)
        exercise(root / 'ts-crap-coverage-helpers', scanner, root)
    print('Installed helper bundle: native, Istanbul, LCOV, one-step run, stale-source and loader integrity checks passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--archive', type=Path, help='checksummed helper bundle ZIP')
    source.add_argument('--helpers', type=Path, help='installed helper directory containing version.json')
    parser.add_argument('--scanner', required=True, type=Path)
    args = parser.parse_args()
    if args.archive:
        verify(args.archive, args.scanner)
    else:
        verify_installed(args.helpers, args.scanner)
