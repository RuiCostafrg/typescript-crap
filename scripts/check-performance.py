#!/usr/bin/env python3
"""Paired CLI/helper performance checks against an exact Git baseline on Linux."""
import argparse
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import statistics
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
CASES = ('covered-scan', 'unicode-tsx', 'nested', 'sarif', 'git-batch', 'git-blob', 'coverage-ranges',
         'mixed-corpus', 'existing-output', 'deep-tree', 'baseline-gate')
PLACEHOLDER = '{}\n'  # Existing --output destination content before each run.
# Mixed corpus: a few multi-MiB sources among hundreds of small ones.
LARGE_SOURCES = (2, 3, 4)  # MiB per large file
LARGE_FUNCTIONS = 32
SMALL_SOURCES = 397
# Deep tree: chains of nested directories with sources at every level.
CHAINS, DEPTH, FILES_PER_LEVEL, FUNCTIONS_PER_FILE = 8, 24, 2, 10


def positive(value):
    """Reject nonfinite or nonpositive benchmark policy values."""
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError('must be finite and positive')
    return number


def assess(baseline, candidate, ratio, slack):
    """Require a median regression AND a majority of bad paired observations."""
    if len(baseline) != len(candidate) or len(baseline) < 3:
        raise ValueError('need at least three complete pairs')
    if not all(type(x) in (int, float) and math.isfinite(x) and x > 0
               for x in [*baseline, *candidate, ratio, slack]):
        raise ValueError('invalid measurement or budget')
    if ratio <= 1:
        raise ValueError('ratio must exceed one')
    old, new = statistics.median(baseline), statistics.median(candidate)
    bad_pairs = sum(b > a * ratio and b - a > slack for a, b in zip(baseline, candidate))
    return {'baseline_median': old, 'candidate_median': new, 'ratio': new / old,
            'delta': new - old, 'bad_pairs': bad_pairs,
            'regressed': new > old * ratio and new - old > slack and bad_pairs > len(baseline) // 2}


def run(command, **kwargs):
    """Run an argument vector and fail on command or infrastructure errors."""
    return subprocess.run(command, check=True, text=True, capture_output=True, **kwargs).stdout


def export_baseline(reference, destination):
    """Export tracked baseline sources; never copy the caller's untracked files."""
    sha = run(['git', 'rev-parse', '--verify', '--end-of-options', f'{reference}^{{commit}}'], cwd=ROOT).strip()
    archive = subprocess.check_output(['git', 'archive', '--format=tar', sha], cwd=ROOT)
    with tarfile.open(fileobj=io.BytesIO(archive)) as bundle:
        bundle.extractall(destination, filter='data')
    return sha


def build(source, target):
    """Build isolated, locked release binaries before any timed measurements."""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target))
    subprocess.run(['cargo', 'build', '--locked', '--release', '--bin', 'ts-crap', '-j', '1'],
                   cwd=source, env=environment, check=True)
    return target / 'release/ts-crap'


def unicode_locations(source, path):
    """Derive fixture arrow locations from independently encoded source prefixes."""
    return {match.group(1): (path, 1, len(source[:match.end()].encode('utf-8')) + 1,
                            len(source[:match.end()].encode('utf-16-le')) // 2 + 1)
            for match in re.finditer(r'const (f\d+)=', source)}


def function_source(index, body_lines=0):
    """One complexity-2 function; optional filler statements make large bodies."""
    if not body_lines:
        return f'function f{index}(flag:boolean){{\nif(flag)return 1;\nreturn 0;\n}}\n'
    return (f'function f{index}(flag:boolean){{\nif(flag)return 1;\nlet total=0;\n'
            + 'total+=1;\n' * body_lines + 'return total;\n}\n')


def mixed_corpus(root):
    """Write hundreds of small sources and a few multi-MiB ones; return the count."""
    count = 0
    for file in range(SMALL_SOURCES):
        functions = file % 37 + 1
        (root / f'small-{file:03}.{"tsx" if file % 3 == 0 else "ts"}').write_text(
            ''.join(function_source(count + i) for i in range(functions)), encoding='utf-8')
        count += functions
    for file, mebibytes in enumerate(LARGE_SOURCES):
        body_lines = mebibytes * 1024 * 1024 // (LARGE_FUNCTIONS * len('total+=1;\n'))
        (root / f'large-{file}.ts').write_text(
            ''.join(function_source(count + i, body_lines) for i in range(LARGE_FUNCTIONS)),
            encoding='utf-8')
        count += LARGE_FUNCTIONS
    return count


def deep_tree(root):
    """Write sources at every level of several deep directory chains."""
    count = 0
    for chain in range(CHAINS):
        directory = root
        for level in range(DEPTH):
            directory = directory / f'chain{chain}-level{level:02}'
            directory.mkdir()
            for file in range(FILES_PER_LEVEL):
                (directory / f'source-{file}.ts').write_text(
                    ''.join(function_source(count + i) for i in range(FUNCTIONS_PER_FILE)),
                    encoding='utf-8')
                count += FUNCTIONS_PER_FILE
    return count


def sources(root):
    """Every fixture source below root, outside Git metadata, in a stable order."""
    return sorted(path for path in root.rglob('*.ts*')
                  if path.is_file() and '.git' not in path.relative_to(root).parts)


def fixture(name, root):
    """Create fixed workloads with known function counts, scores and coverage."""
    root.mkdir()
    coverage, extra, count, complexity, fraction, locations = {}, [], 0, 2, 0.0, {}
    threshold, output = None, None
    if name == 'mixed-corpus':
        count = mixed_corpus(root)
    elif name == 'deep-tree':
        count = deep_tree(root)
    elif name == 'existing-output':
        # Many small files in a few directories: the preflight opens every one.
        for file in range(1500):
            directory = root / f'group-{file // 50:02}'
            directory.mkdir(exist_ok=True)
            (directory / f'source-{file:04}.ts').write_text(
                function_source(count) + function_source(count + 1), encoding='utf-8')
            count += 2
        # An existing destination makes the CLI walk the whole root first; it
        # stays outside the scanned root and is reset before every run.
        output = root.parent / f'{name}-report.json'
        extra += ['--output', str(output)]
    elif name == 'baseline-gate':
        snapshot = []
        for file in range(120):
            path = root / f'source-{file:03}.ts'
            path.write_text(''.join(function_source(count + i) for i in range(25)), encoding='utf-8')
            snapshot += [{'path': path.name, 'name': f'f{count + i}', 'identity': f'{count + i:064x}',
                          'crap': 6.0} for i in range(25)]
            count += 25
        # Every function fails the absolute threshold but matches one entry by
        # path and name with an equal score, so the gate passes.
        threshold = '0'
        path = root.parent / f'{name}-snapshot.json'
        path.write_text(json.dumps({'version': 1, 'threshold': 0.0, 'functions': snapshot}))
        extra += ['--baseline', str(path)]
    elif name == 'git-blob':
        (root / 'large.ts').write_text('/*' + 'x' * (32 * 1024 * 1024) + '*/\nfunction only(){return 1;}\n')
        count, complexity = 1, 1
    elif name == 'nested':
        count, complexity = 192, 1
        (root / 'nested.ts').write_text(''.join(f'function f{i}(){{\n' for i in range(count))
                                          + 'return 1;\n' + '}\n' * count)
    elif name == 'coverage-ranges':
        return {'name': name, 'root': root, 'count': 16000, 'args': []}
    else:
        files, each = (40, 100) if name == 'covered-scan' else ((48, 80) if name == 'git-batch' else (1, 4000))
        fraction = 1.0 if name == 'covered-scan' else 0.0
        count = files * each
        for file in range(files):
            path = root / f'source-{file:03}.{"tsx" if file % 2 or name == "unicode-tsx" else "ts"}'
            source = "const marker = '😀'; " if name == 'unicode-tsx' else ''
            for i in range(each):
                index = file * each + i
                if name == 'unicode-tsx':
                    source += f'const f{index}=(flag:boolean)=>flag?<span>😀</span>:<span/>; '
                else:
                    source += f'function f{index}(flag:boolean){{\nif(flag)return 1;\nreturn 0;\n}}\n'
            path.write_text(source, encoding='utf-8')
            if name == 'unicode-tsx':
                locations.update(unicode_locations(source, path.name))
            if name == 'covered-scan':
                coverage[path.name] = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                                       'lines': {str(n): 1 for n in range(1, len(source.splitlines()) + 1)}}
        if coverage:
            path = root / 'coverage.json'
            path.write_text(json.dumps({'version': 1, 'files': coverage}))
            extra += ['--coverage', str(path), '--require-coverage']
    if name.startswith('git-'):
        run(['git', 'init', '-q'], cwd=root)
        run(['git', '-c', 'user.name=Performance', '-c', 'user.email=performance@example.invalid',
             'commit', '-qm', 'base', '--allow-empty'], cwd=root)
        run(['git', 'add', '--', '.'], cwd=root)
        run(['git', '-c', 'user.name=Performance', '-c', 'user.email=performance@example.invalid',
             '-c', 'core.autocrlf=false', 'commit', '-qm', 'fixture'], cwd=root)
        if name == 'git-batch':
            # Pinned LF blobs, CRLF snapshots: exercise equivalence and batching.
            for path in root.glob('*.ts*'):
                path.write_bytes(path.read_bytes().replace(b'\n', b'\r\n'))
        extra += ['--diff', 'HEAD^']
    return {'name': name, 'root': root, 'count': count, 'complexity': complexity,
            'coverage': fraction, 'args': extra, 'locations': locations,
            'threshold': threshold, 'output': output}


def validate(case, output, expected_fingerprints=None):
    """Check independent fixture invariants so skipped work cannot look faster."""
    if case['name'] == 'coverage-ranges':
        if output['mode'] != 'dense-ranges' or output['size'] != case['count']:
            raise ValueError('coverage benchmark did not execute the expected workload')
        return
    if case['name'] == 'sarif':
        results = output['runs'][0]['results']
        if len(results) != case['count']:
            raise ValueError('SARIF dropped results')
        fingerprints = []
        for index, row in enumerate(results):
            fingerprint = row['partialFingerprints']['function/v1']
            location = row['locations'][0]['physicalLocation']
            if (row['ruleId'] != 'TSCRAP001' or row['level'] != 'error'
                    or row['message']['text'] != f'f{index} has CRAP 6.000 (threshold 0)'
                    or not re.fullmatch('[0-9a-f]{64}', fingerprint)
                    or location['artifactLocation']['uri'] != 'source-000.ts'
                    or location['region']['startLine'] != index * 4 + 1):
                raise ValueError('SARIF changed fixture evidence')
            fingerprints.append(fingerprint)
        if len(set(fingerprints)) != len(results):
            raise ValueError('SARIF duplicated fixture fingerprints')
        digest = hashlib.sha256(json.dumps(fingerprints).encode()).hexdigest()
        if expected_fingerprints is not None and digest != expected_fingerprints:
            raise ValueError('SARIF function identities differ from baseline')
        return digest
    functions = output['functions']
    expected_names = (['only'] if case['name'] == 'git-blob' else
                      ['::'.join(f'f{i}' for i in range(n + 1)) for n in range(case['count'])]
                      if case['name'] == 'nested' else [f'f{i}' for i in range(case['count'])])
    if {f['name'] for f in functions} != set(expected_names):
        raise ValueError('scan changed function names or scope ownership')
    if len(functions) != case['count'] or output['failed']:
        raise ValueError('scan skipped functions or changed the gate')
    cc, cov = case['complexity'], case['coverage']
    score = cc * cc * (1 - cov) ** 3 + cc
    if len({(row['path'], row['line'], row['column']) for row in functions}) != len(functions):
        raise ValueError('duplicate function location')
    for row in functions:
        if case['name'] == 'unicode-tsx' and (
                row['path'], row['line'], row['column'], row['column_utf16']) != case['locations'][row['name']]:
            raise ValueError('Unicode fixture location or UTF-16 column changed')
        if (row['complexity'], row['coverage'], row['crap'], row['failed']) != (cc, cov, score, False):
            raise ValueError('fixture score or coverage changed')


def measure(command, output, metrics, timeout=60, expected_status=0):
    """Measure a fresh process, retaining per-run RSS and killing its group on timeout."""
    with output.open('wb') as stdout:
        started = time.perf_counter()
        process = subprocess.Popen(['/usr/bin/time', '-q', '-f', '%M', '-o', str(metrics), *command],
                                   stdout=stdout, stderr=subprocess.PIPE, start_new_session=True)
        try:
            _, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            raise RuntimeError(f'performance process exceeded {timeout}s') from None
        elapsed = (time.perf_counter() - started) * 1000
    if process.returncode != expected_status:
        raise RuntimeError(f'performance process exited {process.returncode}: {stderr.decode(errors="replace")}')
    rss = int(metrics.read_text().strip())
    if rss <= 0:
        raise ValueError('missing or invalid per-process RSS')
    return {'wall_ms': elapsed, 'peak_rss_kib': rss}


def execute(case, scanner, module, directory, label, expected_fingerprints=None):
    """Execute and validate one fresh-process observation outside compiler activity."""
    output, metrics = directory / f'{label}.json', directory / f'{label}.rss'
    if case['name'] == 'coverage-ranges':
        command = ['node', str(ROOT / 'scripts/bench-coverage-lines.mjs'), str(case['count']), str(module)]
    else:
        threshold = case.get('threshold') or ('0' if case['name'] == 'sarif' else '1000000000')
        command = [str(scanner), '--root', str(case['root']), '--threshold', threshold,
                   '--format', 'sarif' if case['name'] == 'sarif' else 'json', *case['args']]
    # A report written with --output replaces the destination; stdout stays empty.
    report = case.get('output') or output
    try:
        if case.get('output'):
            # Reset the existing destination so a run that writes nothing cannot
            # pass with an earlier run's report.
            report.write_text(PLACEHOLDER)
        result = measure(command, output, metrics, expected_status=1 if case['name'] == 'sarif' else 0)
        if case.get('output') and output.stat().st_size:
            raise ValueError('report written to stdout instead of the output destination')
        if case.get('output') and report.read_text() == PLACEHOLDER:
            raise ValueError('scanner did not replace the existing output destination')
        fingerprints = validate(case, json.loads(report.read_text()), expected_fingerprints)
    except Exception as error:
        raise RuntimeError(f'{label}: {error}') from error
    result['output_sha256'] = hashlib.sha256(report.read_bytes()).hexdigest()
    if case['name'] == 'sarif':
        result['fingerprints_sha256'] = fingerprints
    output.unlink()  # Avoid retaining large repeated reports in the artifact.
    metrics.unlink()
    return result


def compare_outputs(name, observations, allow_change):
    """Require identical scanner reports on each side and, unless allowed, across sides."""
    digests = {side: {row['output_sha256'] for row in rows} for side, rows in observations.items()}
    for side, values in digests.items():
        if len(values) != 1:
            raise ValueError(f'{name}: {side} reports differ between runs')
    changed = digests['baseline'] != digests['candidate']
    if changed and not allow_change:
        raise ValueError(f'{name}: candidate report differs from the baseline report '
                         '(label the pull request output-change if this is intended)')
    if changed:
        print(f'::notice title=Report changed::{name}: candidate report differs from the baseline '
              '(allowed by --allow-output-change)', flush=True)
    return changed


def compare_case(case, binaries, modules, workspace, args):
    """Collect alternating complete pairs and assess both performance metrics."""
    observations = {'baseline': [], 'candidate': []}
    warmups = {}
    name = case['name']
    expected_fingerprints = None
    for side in observations:
        warmup = warmups[side] = execute(case, binaries[side], modules[side], workspace,
                                         f'{name}-warmup-{side}', expected_fingerprints)
        # An allowed output change may also change function identities.
        if name == 'sarif' and side == 'baseline' and not args.allow_output_change:
            expected_fingerprints = warmup['fingerprints_sha256']
    for pair in range(args.pairs):
        order = ('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')
        for side in order:
            observations[side].append(execute(
                case, binaries[side], modules[side], workspace, f'{name}-{pair}-{side}', expected_fingerprints))
    assessments = {}
    for metric, ratio, slack in [('wall_ms', args.time_ratio, args.time_slack_ms),
                                 ('peak_rss_kib', args.rss_ratio, args.rss_slack_kib)]:
        assessments[metric] = assess(
            *[[r[metric] for r in observations[side]] for side in ('baseline', 'candidate')],
            ratio, slack)
    print(name + ': ' + json.dumps(assessments), flush=True)
    result = {'name': name, 'size': case['count'],
              'source_bytes': sum(p.stat().st_size for p in sources(case['root'])),
              'observations': observations, 'assessment': assessments}
    if name != 'coverage-ranges':  # The Node benchmark output contains timings.
        # Warmup reports must agree too; only their timings are excluded.
        reports = {side: [warmups[side], *rows] for side, rows in observations.items()}
        result['output_changed'] = compare_outputs(name, reports, args.allow_output_change)
    return result


def publish_results(destination, results):
    """Replace evidence atomically after writing and syncing a complete sibling file."""
    staging = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=destination,
                                         prefix='.results-', suffix='.tmp', delete=False) as output:
            staging = Path(output.name)
            json.dump(results, output, indent=2)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(staging, destination / 'results.json')
    finally:
        if staging is not None:
            staging.unlink(missing_ok=True)


def main():
    """Compare alternating baseline/candidate pairs and fail substantive regressions."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', default='v2.0.1')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/performance')
    parser.add_argument('--case', action='append', choices=CASES)
    parser.add_argument('--pairs', type=int, default=5)
    parser.add_argument('--time-ratio', type=positive, default=1.5)
    parser.add_argument('--time-slack-ms', type=positive, default=50)
    parser.add_argument('--rss-ratio', type=positive, default=1.3)
    parser.add_argument('--rss-slack-kib', type=positive, default=16384)
    parser.add_argument('--allow-output-change', action='store_true',
                        help='record, rather than fail, scanner reports that differ from the baseline')
    args = parser.parse_args()
    destination = args.output.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    # Remove a prior success before anything that can fail during run setup.
    (destination / 'results.json').unlink(missing_ok=True)
    results = {'version': 1, 'passed': False, 'baseline_ref': args.baseline,
               'budgets': vars(args) | {'output': str(destination)}, 'cases': []}
    try:
        if sys.platform not in ('linux', 'android') or not Path('/usr/bin/time').is_file():
            parser.error('requires Linux and GNU /usr/bin/time (install the time package)')
        if args.pairs < 3 or args.pairs % 2 == 0 or args.time_ratio <= 1 or args.rss_ratio <= 1:
            parser.error('use an odd pair count >=3 and ratios >1')
        results.update({
            'candidate_sha': run(['git', 'rev-parse', 'HEAD'], cwd=ROOT).strip(),
            'rustc': run(['rustc', '--version']).strip(), 'node': run(['node', '--version']).strip(),
            'platform': platform.platform(),
            'working_tree_dirty': bool(run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT).strip()),
            'harness_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'node_harness_sha256': hashlib.sha256((ROOT / 'scripts/bench-coverage-lines.mjs').read_bytes()).hexdigest(),
            'time_tool': run(['/usr/bin/time', '--version']).splitlines()[0],
        })
        with tempfile.TemporaryDirectory(prefix='performance-', dir=destination) as temporary:
            workspace = Path(temporary)
            baseline = workspace / 'baseline'
            results['baseline_sha'] = export_baseline(args.baseline, baseline)
            binaries = {'baseline': build(baseline, destination / 'baseline-build'),
                        'candidate': build(ROOT, destination / 'candidate-build')}
            results['binary_sha256'] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()}
            modules = {'baseline': baseline / 'scripts/coverage-lines.mjs', 'candidate': ROOT / 'scripts/coverage-lines.mjs'}
            results['module_sha256'] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in modules.items()}
            # Fixtures live apart from the exported baseline sources.
            cases = workspace / 'cases'
            cases.mkdir()
            for name in args.case or CASES:
                case = fixture(name, cases / name)
                results['cases'].append(compare_case(case, binaries, modules, workspace, args))
        results['passed'] = not any(a['regressed'] for c in results['cases'] for a in c['assessment'].values())
    except BaseException as error:
        results.update(passed=False, error=str(error))
        raise
    finally:
        publish_results(destination, results)
    if not results['passed']:
        raise SystemExit('Performance regression detected; inspect target/performance/results.json')


if __name__ == '__main__':
    main()
