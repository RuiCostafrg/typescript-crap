"""Regression tests for performance budgets, observations and fixture validity."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('performance', ROOT / 'scripts/check-performance.py')
performance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(performance)


class PerformanceTests(unittest.TestCase):
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
