"""Regression tests for performance budgets, observations and fixture validity."""
import hashlib
import importlib.util
import os
import json
import re
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('performance', ROOT / 'scripts/check-performance.py')
performance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(performance)


class PerformanceTests(unittest.TestCase):
    def test_workflow_baseline_handles_missing_zero_and_unavailable_refs(self):
        """Execute the actual workflow guard with valid, missing and hostile-looking refs."""
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        start = workflow.index('          if [[ -z "${BASELINE_REF:-}"')
        end = workflow.index('          if [[ ! -x /usr/bin/time ]]', start)
        script = workflow[start:end] + '\nprintf "%s|%s" "$BASELINE_REF" "${output_policy[*]}"\n'
        allowed = '--allow-output-change'
        for reference, change, expected in [
                ('HEAD', 'false', 'HEAD|'), ('HEAD', 'true', f'HEAD|{allowed}'), ('HEAD', '', 'HEAD|'),
                ('HEAD', 'yes', 'HEAD|'), ('', 'false', f'v2.0.1|{allowed}'),
                ('0' * 40, 'false', f'v2.0.1|{allowed}'),
                ('no-such-baseline-ref', 'false', f'v2.0.1|{allowed}'),
                ('$(exit 99)', 'false', f'v2.0.1|{allowed}'), ('--help', 'false', f'v2.0.1|{allowed}')]:
            with self.subTest(reference=reference, change=change):
                result = subprocess.run(['bash', '-e', '-u', '-c', script], cwd=ROOT,
                                        env=dict(os.environ, BASELINE_REF=reference, OUTPUT_CHANGE=change),
                                        capture_output=True, text=True, check=True)
                self.assertEqual(result.stdout, expected)
        self.assertIn('"$BASELINE_REF" "${output_policy[@]}"', workflow)
        self.assertIn("OUTPUT_CHANGE: ${{ github.event_name != 'pull_request' || "
                      "contains(github.event.pull_request.labels.*.name, 'output-change') }}", workflow)

    def test_unicode_fixture_bytes_do_not_depend_on_locale(self):
        """Generate the real fixture with ASCII locale and Python UTF-8 coercion disabled."""
        code = '''
import importlib.util,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('performance',sys.argv[1])
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
case=module.fixture('unicode-tsx',Path(sys.argv[2])/'fixture')
source=(case['root']/'source-000.tsx').read_bytes().decode('utf-8')
assert chr(0x1f600) in source
assert len(case['locations'])==4000
'''
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run([sys.executable, '-c', code,
                                     str(ROOT / 'scripts/check-performance.py'), temporary],
                                    env=dict(os.environ, LC_ALL='C', PYTHONUTF8='0', PYTHONCOERCECLOCALE='0'),
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_metadata_failures_replace_previous_success_with_failure(self):
        """Missing Git/compiler/runtime/time metadata cannot leave old successful evidence."""
        for failed_tool in ['git', 'rustc', 'node', '/usr/bin/time']:
            with self.subTest(tool=failed_tool), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                report = root / 'results.json'
                report.write_text('{"passed":true,"candidate_sha":"old-run"}')
                def metadata(command, **kwargs):
                    """Fail the selected real orchestration boundary before any build."""
                    if command[0] == failed_tool:
                        raise RuntimeError(f'{failed_tool} unavailable')
                    return 'metadata\n'
                with patch('sys.argv', ['check-performance.py', '--output', str(root)]), \
                        patch.object(performance, 'run', side_effect=metadata):
                    with self.assertRaisesRegex(RuntimeError, 'unavailable'):
                        performance.main()
                result = json.loads(report.read_text())
                self.assertIs(result['passed'], False)
                self.assertIn('unavailable', result['error'])
                self.assertNotEqual(result.get('candidate_sha'), 'old-run')
                self.assertEqual(list(root.glob('.results-*')), [])

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_run_failures_budget_rejection_and_successful_rerun_refresh_evidence(self):
        """Retain failed attempts across execution errors and publish only completed successes."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def exported(reference, destination):
                """Provide a minimal source export without invoking Git or a compiler."""
                (destination / 'scripts').mkdir(parents=True)
                (destination / 'scripts/coverage-lines.mjs').write_text('baseline module')
                return 'baseline-sha'
            def built(source, target):
                """Provide a hashable binary stand-in; actual measurement is tested separately."""
                target.mkdir(parents=True, exist_ok=True)
                binary = target / 'scanner'
                binary.write_bytes(b'scanner')
                return binary
            good = {'assessment': {'wall_ms': {'regressed': False}}}
            bad = {'assessment': {'wall_ms': {'regressed': True}}}
            for phase in ['build', 'timeout', 'malformed', 'budget', 'success']:
                with self.subTest(phase=phase), \
                        patch('sys.argv', ['check-performance.py', '--output', str(root), '--case', 'nested']), \
                        patch.object(performance, 'run', return_value='metadata\n'), \
                        patch.object(performance, 'export_baseline', side_effect=exported), \
                        patch.object(performance, 'build', side_effect=RuntimeError('build failed') if phase == 'build' else built), \
                        patch.object(performance, 'compare_case', return_value=bad if phase == 'budget' else good,
                                     side_effect=RuntimeError('process exceeded 60s') if phase == 'timeout' else
                                     ValueError('invalid observations') if phase == 'malformed' else None):
                    (root / 'results.json').write_text('{"passed":true,"candidate_sha":"old-run"}')
                    if phase in ['build', 'timeout', 'malformed']:
                        with self.assertRaises((RuntimeError, ValueError)):
                            performance.main()
                    elif phase == 'budget':
                        with self.assertRaisesRegex(SystemExit, 'Performance regression detected'):
                            performance.main()
                    else:
                        performance.main()
                    result = json.loads((root / 'results.json').read_text())
                    self.assertEqual(result['passed'], phase == 'success')
                    self.assertNotEqual(result.get('candidate_sha'), 'old-run')
                    self.assertEqual(list(root.glob('.results-*')), [])

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_report_write_failures_preserve_atomicity_and_remove_staging(self):
        """Serialization, sync and replacement failures cannot expose partial JSON."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report = root / 'results.json'
            previous = '{"passed":false,"error":"previous failure"}'
            for target in ['json.dump', 'os.fsync', 'os.replace']:
                report.write_text(previous)
                with patch.object(performance.json if target.startswith('json') else performance.os,
                                  target.split('.')[1], side_effect=OSError('write failed')):
                    with self.assertRaisesRegex(OSError, 'write failed'):
                        performance.publish_results(root, {'passed': True})
                self.assertEqual(report.read_text(), previous)
                self.assertEqual(list(root.glob('.results-*')), [])
            report.write_text('{"passed":true,"candidate_sha":"old-run"}')
            with patch('sys.argv', ['check-performance.py', '--output', str(root)]), \
                    patch.object(performance, 'run', side_effect=RuntimeError('metadata failed')), \
                    patch.object(performance.os, 'replace', side_effect=PermissionError('cannot publish')):
                with self.assertRaisesRegex(PermissionError, 'cannot publish'):
                    performance.main()
            self.assertFalse(report.exists())
            self.assertEqual(list(root.glob('.results-*')), [])

    def test_large_consistent_slowdowns_and_memory_growth_fail(self):
        """Reject substantive time/RSS growth while accepting improvements."""
        for ratio, slack, old, new in [(1.5, 50, 100, 220), (1.3, 16384, 32768, 65536)]:
            with self.subTest(metric_budget=(ratio, slack)):
                self.assertTrue(performance.assess([old] * 5, [new] * 5, ratio, slack)['regressed'])
                self.assertFalse(performance.assess([new] * 5, [old] * 5, ratio, slack)['regressed'])

    def test_outliers_small_absolute_deltas_and_budget_boundaries_pass(self):
        """Timing noise must not masquerade as consistent benchmark regression."""
        for old, new in [([100] * 5, [100, 100, 100, 100, 10000]),
                         ([1] * 5, [10] * 5), ([100] * 5, [150] * 5)]:
            self.assertFalse(performance.assess(old, new, 1.5, 50)['regressed'])
        # Medians alone would flag this; only two pairs exceed both budgets.
        self.assertFalse(performance.assess([100, 100, 100, 1000, 1000],
                                           [100, 210, 210, 1000, 1000], 1.5, 50)['regressed'])

    def test_missing_nonfinite_and_invalid_measurements_fail_closed(self):
        """Incomplete or corrupt measurements cannot produce a passing check."""
        for old, new in [([], []), ([1] * 5, [1] * 3), ([1] * 2, [1] * 2),
                         ([1] * 5, [float('nan')] * 5), ([1] * 5, [float('inf')] * 5),
                         ([1] * 5, [0] * 5), ([True] * 5, [1] * 5)]:
            with self.assertRaises(ValueError):
                performance.assess(old, new, 1.5, 50)
        for ratio, slack in [(1, 50), (0.5, 50), (float('nan'), 50), (1.5, -1)]:
            with self.assertRaises(ValueError):
                performance.assess([100] * 5, [100] * 5, ratio, slack)

    def test_skipping_scan_work_or_changing_scores_is_rejected(self):
        """Performance results require the fixture's known complete scan result."""
        case = {'name': 'covered-scan', 'count': 2, 'complexity': 2, 'coverage': 1.0}
        good = {'failed': False, 'functions': [
            {'name': f'f{i}', 'path': 'source.ts', 'line': i + 1, 'column': 1, 'complexity': 2,
             'coverage': 1.0, 'crap': 2, 'failed': False}
            for i in range(2)]}
        performance.validate(case, good)
        for field, value in [('functions', []), ('failed', True)]:
            with self.assertRaises(ValueError):
                performance.validate(case, good | {field: value})
        for field, value in [('line', 1), ('name', 'f0'), ('coverage', 0.0), ('complexity', 1), ('crap', 1)]:
            bad = json.loads(json.dumps(good)); bad['functions'][1][field] = value
            with self.assertRaises(ValueError):
                performance.validate(case, bad)

    def test_nested_functions_retain_qualified_scope_ownership(self):
        """Accept the public :: names while rejecting flattened or reparented scopes."""
        case = {'name': 'nested', 'count': 3, 'complexity': 1, 'coverage': 0.0}
        good = {'failed': False, 'functions': [
            {'name': name, 'path': 'nested.ts', 'line': i + 1, 'column': 1,
             'complexity': 1, 'coverage': 0.0, 'crap': 2, 'failed': False}
            for i, name in enumerate(['f0', 'f0::f1', 'f0::f1::f2'])]}
        performance.validate(case, good)
        for name in ['f2', 'f0::f2', 'f0.f1.f2']:
            bad = json.loads(json.dumps(good))
            bad['functions'][2]['name'] = name
            with self.assertRaisesRegex(ValueError, 'scope ownership'):
                performance.validate(case, bad)

    def test_unicode_locations_require_exact_byte_and_utf16_columns(self):
        """Reject distinct but wrong Unicode columns and names assigned another location."""
        source = "const marker = '😀'; const f0=()=> '😀'; const f1=()=> 0;"
        locations = performance.unicode_locations(source, 'source-000.tsx')
        self.assertEqual(locations, {'f0': ('source-000.tsx', 1, 33, 31),
                                     'f1': ('source-000.tsx', 1, 55, 51)})
        case = {'name': 'unicode-tsx', 'count': 2, 'complexity': 2, 'coverage': 0.0,
                'locations': locations}
        good = {'failed': False, 'functions': [
            {'name': name, 'path': path, 'line': line, 'column': byte, 'column_utf16': utf16,
             'complexity': 2, 'coverage': 0.0, 'crap': 6, 'failed': False}
            for name, (path, line, byte, utf16) in locations.items()]}
        performance.validate(case, good)
        for field, value in [('column_utf16', 32), ('column', 34), ('line', 2),
                             ('path', 'wrong.tsx'), ('name', 'f1')]:
            bad = json.loads(json.dumps(good))
            bad['functions'][0][field] = value
            if field == 'name':
                bad['functions'][1]['name'] = 'f0'
            with self.assertRaises(ValueError):
                performance.validate(case, bad)

    def test_sarif_findings_require_complete_locations_and_fingerprints(self):
        """Reject faster reports that omit findings or their stable evidence."""
        case = {'name': 'sarif', 'count': 2}
        good = {'runs': [{'results': [
            {'ruleId': 'TSCRAP001', 'level': 'error',
             'message': {'text': f'f{i} has CRAP 6.000 (threshold 0)'},
             'partialFingerprints': {'function/v1': f'{i:064x}'},
             'locations': [{'physicalLocation': {
                 'artifactLocation': {'uri': 'source-000.ts'},
                 'region': {'startLine': i * 4 + 1}}}]}
            for i in range(2)]}]}
        baseline = performance.validate(case, good)
        performance.validate(case, good, baseline)
        bad = json.loads(json.dumps(good))
        bad['runs'][0]['results'][1]['partialFingerprints'] = {'function/v1': 'f' * 64}
        with self.assertRaisesRegex(ValueError, 'identities differ from baseline'):
            performance.validate(case, bad, baseline)
        bad = json.loads(json.dumps(good))
        bad['runs'][0]['results'].pop()
        with self.assertRaises(ValueError):
            performance.validate(case, bad)
        bad = json.loads(json.dumps(good))
        bad['runs'][0]['results'][1]['partialFingerprints'] = {'function/v1': '0' * 64}
        with self.assertRaises(ValueError):
            performance.validate(case, bad)
        bad = json.loads(json.dumps(good))
        bad['runs'][0]['results'][0]['locations'][0]['physicalLocation']['region']['startLine'] = 999
        with self.assertRaises(ValueError):
            performance.validate(case, bad)

    def test_reports_must_match_baseline_unless_a_change_is_allowed(self):
        """Different candidate reports fail; nondeterministic reports fail even when allowed."""
        def rows(*digests):
            return [{'output_sha256': digest} for digest in digests]
        same = {'baseline': rows('a', 'a', 'a'), 'candidate': rows('a', 'a', 'a')}
        changed = {'baseline': rows('a', 'a', 'a'), 'candidate': rows('b', 'b', 'b')}
        self.assertFalse(performance.compare_outputs('case', same, False))
        self.assertFalse(performance.compare_outputs('case', same, True))
        with self.assertRaisesRegex(ValueError, 'case: candidate report differs from the baseline'):
            performance.compare_outputs('case', changed, False)
        with patch('builtins.print') as printed:
            self.assertTrue(performance.compare_outputs('case', changed, True))
        self.assertIn('::notice', printed.call_args.args[0])
        for side in ('baseline', 'candidate'):
            unstable = {'baseline': rows('a', 'a', 'a'), 'candidate': rows('a', 'a', 'a')}
            unstable[side] = rows('a', 'c', 'a')
            for allow in (False, True):
                with self.subTest(side=side, allow=allow), \
                        self.assertRaisesRegex(ValueError, f'{side} reports differ between runs'):
                    performance.compare_outputs('case', unstable, allow)

    def test_every_scanner_case_compares_reports_and_node_case_does_not(self):
        """compare_case wires the policy for every scanner workload, including warmup order."""
        args = type('Args', (), {'pairs': 3, 'time_ratio': 1.5, 'time_slack_ms': 50,
                                 'rss_ratio': 1.3, 'rss_slack_kib': 16384})
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'root'
            (root / 'deep').mkdir(parents=True)
            (root / 'deep/a.ts').write_text('x' * 7)
            (root / '.git').mkdir()
            (root / '.git/ignored.ts').write_text('not a fixture source')
            sides = {'baseline': None, 'candidate': None}
            for name in performance.CASES:
                for allow, candidate in [(False, 'same'), (True, 'same'), (True, 'other'), (False, 'other'),
                                         (False, 'warmup'), (True, 'warmup')]:
                    args.allow_output_change = allow
                    expectations = []
                    def observed(case, scanner, module, directory, label, expected=None):
                        """Return a fixed measurement whose report digest depends on the side."""
                        expectations.append(expected)
                        digest = candidate if label.endswith('candidate') else 'same'
                        if candidate == 'warmup':  # Only the candidate warmup differs.
                            digest = 'other' if label.endswith('warmup-candidate') else 'same'
                        return {'wall_ms': 1.0, 'peak_rss_kib': 1, 'output_sha256': digest,
                                'fingerprints_sha256': 'f'}
                    case = {'name': name, 'count': 1, 'root': root}
                    with self.subTest(name=name, allow=allow, candidate=candidate), \
                            patch.object(performance, 'execute', side_effect=observed), \
                            patch('builtins.print'):
                        if name != 'coverage-ranges' and candidate == 'other' and not allow:
                            with self.assertRaisesRegex(ValueError, 'differs from the baseline'):
                                performance.compare_case(case, sides, sides, root, args)
                            continue
                        if name != 'coverage-ranges' and candidate == 'warmup':
                            # A warmup that disagrees with later runs fails even when allowed.
                            with self.assertRaisesRegex(ValueError, 'candidate reports differ between runs'):
                                performance.compare_case(case, sides, sides, root, args)
                            continue
                        result = performance.compare_case(case, sides, sides, root, args)
                        self.assertEqual(result['source_bytes'], 7)  # Sources below the root count.
                        # SARIF identities are pinned to the baseline unless a change is allowed.
                        pinned = 'f' if name == 'sarif' and not allow else None
                        self.assertEqual(expectations, [None] + [pinned] * (1 + 2 * args.pairs))
                        if name == 'coverage-ranges':
                            self.assertNotIn('output_changed', result)
                        else:
                            self.assertEqual(result['output_changed'], candidate == 'other')
                            self.assertNotEqual(candidate, 'warmup')

    def test_new_workloads_have_the_promised_shape(self):
        """Mixed sizes, deep paths, an existing destination and a matching baseline snapshot."""
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            mixed = performance.fixture('mixed-corpus', workspace / 'mixed-corpus')
            files = performance.sources(mixed['root'])
            sizes = sorted(path.stat().st_size for path in files)
            self.assertGreaterEqual(len(files), 300)
            self.assertEqual(sum(size >= 2 * 1024 * 1024 for size in sizes), len(performance.LARGE_SOURCES))
            self.assertLess(sizes[-len(performance.LARGE_SOURCES) - 1], 4096)
            self.assertTrue(any(path.suffix == '.tsx' for path in files))
            text = ''.join(path.read_text() for path in files)
            self.assertEqual(text.count('function f'), mixed['count'])
            self.assertEqual(text.count('if(flag)'), mixed['count'])  # Complexity 2 everywhere.
            deep = performance.fixture('deep-tree', workspace / 'deep-tree')
            files = performance.sources(deep['root'])
            depths = [len(path.relative_to(deep['root']).parts) - 1 for path in files]
            self.assertEqual(max(depths), performance.DEPTH)
            self.assertEqual(len(files) * performance.FUNCTIONS_PER_FILE, deep['count'])
            existing = performance.fixture('existing-output', workspace / 'existing-output')
            self.assertGreaterEqual(len(performance.sources(existing['root'])), 1000)
            self.assertNotIn(existing['root'], existing['output'].parents)
            self.assertEqual(existing['args'], ['--output', str(existing['output'])])
            baseline = performance.fixture('baseline-gate', workspace / 'baseline-gate')
            self.assertEqual(baseline['threshold'], '0')
            self.assertEqual(baseline['args'][0], '--baseline')
            snapshot = json.loads(Path(baseline['args'][1]).read_text())
            self.assertNotIn(baseline['root'], Path(baseline['args'][1]).parents)
            self.assertEqual(snapshot['threshold'], 0.0)
            expected = set()
            for path in performance.sources(baseline['root']):
                expected |= {(path.name, name) for name in re.findall(r'function (f\d+)', path.read_text())}
            self.assertEqual({(row['path'], row['name']) for row in snapshot['functions']}, expected)
            self.assertEqual(len(snapshot['functions']), baseline['count'])
            self.assertEqual({row['crap'] for row in snapshot['functions']}, {6.0})

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_output_destination_is_reset_and_must_receive_the_report(self):
        """A destination case reads the written file; stale or stdout reports fail."""
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            case = {'name': 'git-blob', 'root': workspace, 'count': 1, 'complexity': 1, 'coverage': 0.0,
                    'args': [], 'locations': {}, 'threshold': None, 'output': workspace / 'report.json'}
            report = json.dumps({'failed': False, 'functions': [
                {'name': 'only', 'path': 'large.ts', 'line': 1, 'column': 1, 'complexity': 1,
                 'coverage': 0.0, 'crap': 2.0, 'failed': False}]})
            def scanner(name, code):
                """Write an executable stand-in scanner."""
                script = workspace / name
                script.write_text(f'#!{sys.executable}\nfrom pathlib import Path\n{code}\n')
                script.chmod(0o755)
                return script
            writes = f'Path({str(case["output"])!r}).write_text({report!r})'
            writer = scanner('writer', writes)
            result = performance.execute(case, writer, None, workspace, 'written')
            self.assertEqual(result['output_sha256'], hashlib.sha256(report.encode()).hexdigest())
            # The destination now holds a valid report; a scanner that writes
            # nothing must not pass by reusing it.
            idle = scanner('idle', 'pass')
            with self.assertRaisesRegex(RuntimeError, 'did not replace the existing output destination'):
                performance.execute(case, idle, None, workspace, 'idle')
            noisy = scanner('noisy', writes + '\nprint("{}")')
            with self.assertRaisesRegex(RuntimeError, 'instead of the output destination'):
                performance.execute(case, noisy, None, workspace, 'stdout')

    def test_scanner_command_uses_case_threshold_and_arguments(self):
        """The baseline gate runs at threshold 0; other JSON cases use the permissive default."""
        commands = []
        def measured(command, output, metrics, **kwargs):
            """Capture the command and leave a report for hashing."""
            commands.append(command)
            output.write_text('{}')
            metrics.write_text('1')
            return {'wall_ms': 1.0, 'peak_rss_kib': 1}
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(performance, 'measure', side_effect=measured), \
                patch.object(performance, 'validate', return_value=None):
            workspace = Path(temporary)
            for name, threshold, args, expected in [
                    ('baseline-gate', '0', ['--baseline', 'snapshot.json'], '0'),
                    ('deep-tree', None, [], '1000000000'), ('sarif', None, [], '0')]:
                case = {'name': name, 'root': workspace, 'args': args, 'threshold': threshold}
                performance.execute(case, Path('scanner'), None, workspace, name)
                command = commands[-1]
                self.assertEqual(command[command.index('--threshold') + 1], expected, name)
                self.assertEqual(command[-len(args):] if args else [], args)

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_fixtures_never_reuse_the_exported_baseline_directory(self):
        """main() keeps case roots apart from workspace/baseline, whatever a case is called."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            roots = []
            def exported(reference, destination):
                """Provide a minimal source export like git archive would."""
                (destination / 'scripts').mkdir(parents=True)
                (destination / 'scripts/coverage-lines.mjs').write_text('baseline module')
                return 'baseline-sha'
            def built(source, target):
                """Provide a hashable binary stand-in."""
                target.mkdir(parents=True, exist_ok=True)
                (target / 'scanner').write_bytes(b'scanner')
                return target / 'scanner'
            def made(name, case_root):
                """Record where each fixture would be created."""
                roots.append(case_root)
                case_root.mkdir()
                return {'name': name}
            arguments = ['check-performance.py', '--output', str(root)]
            for name in performance.CASES:
                arguments += ['--case', name]
            with patch('sys.argv', arguments), \
                    patch.object(performance, 'run', return_value='metadata\n'), \
                    patch.object(performance, 'export_baseline', side_effect=exported), \
                    patch.object(performance, 'build', side_effect=built), \
                    patch.object(performance, 'fixture', side_effect=made), \
                    patch.object(performance, 'compare_case',
                                 return_value={'assessment': {'wall_ms': {'regressed': False}}}):
                performance.main()
            self.assertEqual([path.name for path in roots], list(performance.CASES))
            self.assertEqual({path.parent.name for path in roots}, {'cases'})
            self.assertNotIn('baseline', performance.CASES)

    @unittest.skipUnless(sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists(), 'Linux GNU time required')
    def test_rss_is_per_process_and_command_failures_are_fatal(self):
        """A heavy run must not contaminate subsequent fresh-process RSS samples."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            heavy = performance.measure([sys.executable, '-c', 'x=b"x"*(64*1024*1024);print(len(x))'],
                                        root / 'heavy', root / 'heavy.rss')
            light = performance.measure([sys.executable, '-c', 'print("ok")'], root / 'light', root / 'light.rss')
            self.assertGreater(heavy['peak_rss_kib'] - light['peak_rss_kib'], 32 * 1024)
            self.assertGreater(light['wall_ms'], 0)
            performance.measure([sys.executable, '-c', 'raise SystemExit(1)'],
                                root / 'gate', root / 'gate.rss', expected_status=1)
            with self.assertRaisesRegex(RuntimeError, 'exited 3'):
                performance.measure([sys.executable, '-c', 'raise SystemExit(3)'], root / 'bad', root / 'bad.rss')
            with self.assertRaisesRegex(RuntimeError, 'exceeded'):
                performance.measure([sys.executable, '-c', 'import time;time.sleep(60)'],
                                    root / 'hung', root / 'hung.rss', timeout=0.1)


if __name__ == '__main__':
    unittest.main()
