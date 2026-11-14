"""Tests for the opt-in real-corpus benchmark script and its workflow."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bench_corpus', ROOT / 'scripts/bench-corpus.py')
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)
LINUX_TIME = sys.platform in ('linux', 'android') and Path('/usr/bin/time').exists()

TIME_OUTPUT = '''\tCommand being timed: "ts-crap"
\tUser time (seconds): 12.34
\tSystem time (seconds): 1.50
\tMaximum resident set size (kbytes): 204800
\tExit status: 1
'''


def fake_scanner(directory, name, body, status=1):
    """Write an executable stand-in that writes `body` to its --output path."""
    script = directory / name
    script.write_text(f'#!{sys.executable}\nimport sys\nfrom pathlib import Path\n'
                      'output = Path(sys.argv[sys.argv.index("--output") + 1])\n'
                      f'output.write_text({body!r})\nraise SystemExit({status})\n')
    script.chmod(0o755)
    return script


class CorpusBenchmarkTests(unittest.TestCase):
    def test_time_output_requires_every_measured_field(self):
        """Missing GNU time fields fail rather than record zero."""
        self.assertEqual(bench.parse_time(TIME_OUTPUT), {
            'user_s': 12.34, 'system_s': 1.5, 'peak_rss_kib': 204800, 'exit_status': 1})
        for line in TIME_OUTPUT.splitlines()[1:]:
            with self.subTest(missing=line), self.assertRaisesRegex(ValueError, 'GNU time output has no'):
                bench.parse_time(TIME_OUTPUT.replace(line + '\n', ''))

    def test_scanner_labels_are_validated(self):
        """Labels name builds in the record and must be simple and present."""
        self.assertEqual(bench.labelled('musl=target/ts-crap'), ('musl', Path('target/ts-crap')))
        for value in ['target/ts-crap', '=path', 'Musl=path', 'musl=', '-x=path', 'a b=path']:
            with self.subTest(value=value), self.assertRaises(argparse.ArgumentTypeError):
                bench.labelled(value)

    def test_command_matches_the_documented_invocation(self):
        """The benchmark measures syntax-error reporting with a JSON file output."""
        self.assertEqual(bench.scan_command('ts-crap', 'corpus', 'out.json'),
                         ['ts-crap', '--root', 'corpus', '--on-syntax-error', 'report',
                          '--format', 'json', '--output', 'out.json'])

    def test_summary_reports_median_and_range(self):
        """Each metric keeps its median, minimum and maximum."""
        runs = [{'wall_s': w, 'user_s': 1.0, 'system_s': 0.5, 'peak_rss_kib': r}
                for w, r in [(3.0, 10), (1.0, 30), (2.0, 20)]]
        summary = bench.summarize(runs)
        self.assertEqual(summary['runs'], 3)
        self.assertEqual(summary['wall_s'], {'median': 2.0, 'min': 1.0, 'max': 3.0})
        self.assertEqual(summary['peak_rss_kib'], {'median': 20, 'min': 10, 'max': 30})

    @unittest.skipUnless(LINUX_TIME, 'Linux GNU time required')
    def test_benchmark_requires_identical_reports_and_valid_exits(self):
        """Different report bytes, scanner errors or a missing report fail the run."""
        report = json.dumps({'files_scanned': 2, 'failed': True,
                             'functions': [{'name': 'f'}], 'syntax_errors': [{'path': 'bad.ts'}]})
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            corpus = directory / 'corpus'
            corpus.mkdir()
            work = directory / 'work'
            work.mkdir()
            first = fake_scanner(directory, 'first', report)
            second = fake_scanner(directory, 'second', report, status=0)
            results, described, digests = bench.benchmark(
                [('first', first), ('second', second)], corpus, 3, work, 60)
            self.assertEqual(described, {'bytes': len(report), 'functions': 1, 'files_scanned': 2,
                                         'syntax_errors': 1, 'failed': True})
            self.assertEqual(len(digests), 1)
            self.assertEqual(len(digests[0]), 64)
            for label in ('first', 'second'):
                for mode in bench.MODES:
                    self.assertEqual(len(results[label][mode]), 3)
            other = fake_scanner(directory, 'other', report + ' ')
            _, _, digests = bench.benchmark([('first', first), ('other', other)], corpus, 1, work, 60)
            self.assertEqual(len(digests), 2)
            # Only the first (warmup) run differs; the measured runs agree.
            marker = directory / 'ran-once'
            flaky = directory / 'flaky'
            flaky.write_text(f'#!{sys.executable}\nimport sys\nfrom pathlib import Path\n'
                             f'marker = Path({str(marker)!r})\n'
                             f'body = {report!r} + ("" if marker.exists() else " ")\n'
                             'marker.touch()\n'
                             'Path(sys.argv[sys.argv.index("--output") + 1]).write_text(body)\n'
                             'raise SystemExit(1)\n')
            flaky.chmod(0o755)
            results, _, digests = bench.benchmark([('flaky', flaky)], corpus, 2, work, 60)
            self.assertEqual(len(digests), 2)
            self.assertEqual({run['report_sha256'] for run in results['flaky']['absent']},
                             {run['report_sha256'] for run in results['flaky']['existing']})
            broken = fake_scanner(directory, 'broken', report, status=2)
            with self.assertRaisesRegex(RuntimeError, 'scan exited 2'):
                bench.benchmark([('broken', broken)], corpus, 1, work, 60)
            silent = directory / 'silent'
            silent.write_text(f'#!{sys.executable}\n')
            silent.chmod(0o755)
            with self.assertRaisesRegex(RuntimeError, 'wrote no report'):
                bench.benchmark([('silent', silent)], corpus, 1, work, 60)
            (work / 'report.json').unlink(missing_ok=True)
            with self.assertRaisesRegex(ValueError, 'has no destination'):
                bench.measure(first, corpus, work / 'report.json', 'existing', 60)

    @unittest.skipUnless(LINUX_TIME, 'Linux GNU time required')
    def test_timeout_kills_the_scanner_process_group(self):
        """A hung scanner is killed with GNU time instead of blocking the benchmark."""
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            hung = directory / 'hung'
            hung.write_text(f'#!{sys.executable}\nimport time\ntime.sleep(60)\n')
            hung.chmod(0o755)
            started = time.monotonic()
            with self.assertRaisesRegex(RuntimeError, 'scan exceeded'):
                bench.measure(hung, directory, directory / 'report.json', 'absent', 0.5)
            self.assertLess(time.monotonic() - started, 30)

    @unittest.skipUnless(LINUX_TIME, 'Linux GNU time required')
    def test_record_is_written_before_failing_on_different_reports(self):
        """Evidence survives a report mismatch, and the summary flags it."""
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / 'corpus').mkdir()
            report = json.dumps({'failed': False, 'functions': []})
            first = fake_scanner(directory, 'first', report)
            other = fake_scanner(directory, 'other', report + ' ')
            output, summary = directory / 'record.json', directory / 'summary.md'
            arguments = ['bench-corpus.py', '--corpus', str(directory / 'corpus'), '--runs', '1',
                         '--scanner', f'first={first}', '--scanner', f'other={other}',
                         '--output', str(output), '--summary', str(summary)]
            with patch('sys.argv', arguments), patch('builtins.print'), \
                    self.assertRaisesRegex(SystemExit, 'different reports'):
                bench.main()
            record = json.loads(output.read_text())
            self.assertFalse(record['report_consistent'])
            self.assertEqual(len(record['report_sha256s']), 2)
            self.assertEqual(set(record['results']), {'first', 'other'})
            self.assertEqual(record['results']['first']['summary']['absent']['runs'], 1)
            self.assertIn('Reports differ', summary.read_text())
            consistent = arguments.copy()
            consistent[consistent.index(f'other={other}')] = f'second={first}'
            with patch('sys.argv', consistent), patch('builtins.print'):
                bench.main()
            record = json.loads(output.read_text())
            self.assertTrue(record['report_consistent'])
            self.assertIn('| second | existing |', summary.read_text())

    def test_workflow_pins_the_corpus_and_actions_and_is_opt_in(self):
        """The workflow is manual (or edits to itself), pinned, and builds both Linux targets."""
        workflow = (ROOT / '.github/workflows/corpus-benchmark.yml').read_text(encoding='utf-8')
        self.assertIn('workflow_dispatch:', workflow)
        self.assertNotIn('push:', workflow)
        self.assertNotIn('schedule:', workflow)
        self.assertIn('repository: microsoft/vscode', workflow)
        self.assertIn('ref: f42f0a1102cedb0047aed92bcdf1f4ba422d02b8', workflow)
        for line in workflow.splitlines():
            if line.strip().startswith(('- uses:', 'uses:')):
                reference = line.split('@', 1)[1].split()[0]
                self.assertRegex(reference, '^[0-9a-f]{40}$', line)
        for target in ('x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-musl'):
            self.assertIn(f'--target {target}', workflow)
        self.assertIn('--profile dist', workflow)
        self.assertIn('persist-credentials: false', workflow)
        self.assertIn("github.actor_id == '35194998'", workflow)


if __name__ == '__main__':
    unittest.main()
