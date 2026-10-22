"""Timing telemetry must preserve command failures and append actual measurements."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/time-command.py'


class TimingTests(unittest.TestCase):
    def test_append_timings_propagate_status_and_write_job_summary(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'nested' / 'timings.jsonl'
            summary = Path(temporary) / 'summary.md'
            for code in [0, 7]:
                result = subprocess.run([sys.executable, str(SCRIPT), '--label', 'test',
                    '--output', str(output), '--', sys.executable, '-c', f'raise SystemExit({code})'],
                    env={**os.environ, 'GITHUB_STEP_SUMMARY': str(summary)}, capture_output=True, text=True)
                self.assertEqual(result.returncode, code, result.stderr)
            records = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual([r['exit_code'] for r in records], [0, 7])
            self.assertTrue(all(r['seconds'] >= 0 and r['label'] == 'test' for r in records))
            self.assertEqual(len(summary.read_text().splitlines()), 2)

    def test_missing_command_and_launch_error_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'timings.jsonl'
            args = [sys.executable, str(SCRIPT), '--label', 'missing', '--output', str(output)]
            result = subprocess.run(args, capture_output=True)
            self.assertEqual(result.returncode, 2)
            self.assertFalse(output.exists())
            result = subprocess.run(args + ['--', str(Path(temporary) / 'no-such-command')],
                                    capture_output=True)
            self.assertEqual(result.returncode, 127)
            self.assertEqual(json.loads(output.read_text())['exit_code'], 127)
