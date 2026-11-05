"""Public CLI tests for the GitHub coverage-summary helper."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "coverage-summary.py"


class CoverageSummaryTests(unittest.TestCase):
    def run_report(self, data, minimum=None):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "summary.json"
            file.write_text(json.dumps(data), encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(SCRIPT), str(file),
                 *([] if minimum is None else ['--min-lines', str(minimum)])],
                capture_output=True, text=True, check=False,
            )

    @staticmethod
    def entry(covered, count):
        return {"totals": {
            key: {"covered": covered, "count": count}
            for key in ("lines", "functions", "regions")
        }}

    def test_aggregates_exported_groups(self):
        result = self.run_report({"data": [self.entry(1, 2), self.entry(2, 2)]})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("| Lines | 3 | 4 | 75.00% |", result.stdout)
        self.assertIn("| Functions | 3 | 4 | 75.00% |", result.stdout)
        self.assertIn("| Regions | 3 | 4 | 75.00% |", result.stdout)
        self.assertIn("not the TypeScript coverage", result.stdout)

    def test_empty_metric_is_not_falsely_reported_as_100_percent(self):
        result = self.run_report({"data": [self.entry(0, 0)]})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("| Lines | 0 | 0 | N/A |", result.stdout)

    def test_rejects_missing_and_impossible_totals(self):
        for data in ({"data": []}, {"data": [self.entry(3, 2)]}, {}):
            result = self.run_report(data)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_invalid_individual_entries_cannot_hide_in_valid_aggregates(self):
        """Reject cancellation, boolean coercion and bad metrics before printing evidence."""
        cases = [
            {'data': [self.entry(20, 10), self.entry(0, 10)]},
            {'data': [self.entry(-5, -5), self.entry(25, 25)]},
            {'data': [self.entry(True, True), self.entry(19, 19)]},
            {'data': [self.entry(1.0, 1.0), self.entry(19, 19)]},
            {'data': [self.entry(float('nan'), 10)]},
        ]
        mixed = {'data': [self.entry(10, 10), self.entry(10, 10)]}
        mixed['data'][1]['totals']['regions']['covered'] = 11
        cases.extend([mixed, {'data': [{'totals': {'lines': {'covered': 1, 'count': 1}}}]}])
        for data in cases:
            for minimum in [None, 95]:
                with self.subTest(data=data, minimum=minimum):
                    result = self.run_report(data, minimum)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, '')

    def test_multi_entry_floor_boundaries_and_zero_lines(self):
        """Aggregate valid groups accurately while keeping the zero-line floor closed."""
        result = self.run_report({'data': [self.entry(1, 1), self.entry(94, 99)]}, 95)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('| Lines | 95 | 100 | 95.00% |', result.stdout)
        for data in [{'data': [self.entry(1, 1), self.entry(93, 99)]},
                     {'data': [self.entry(0, 0), self.entry(0, 0)]}]:
            result = self.run_report(data, 95)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, '')

class CoverageFloorTests(unittest.TestCase):
    def test_floor_boundary_missing_zero_invalid_and_below_floor(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('coverage_summary', SCRIPT)
        helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        self.assertIsNone(helper.enforce_floor({'data':[CoverageSummaryTests.entry(95,100)]},95))
        for covered,total in [(949,1000),(0,0),(101,100)]:
            with self.assertRaises(ValueError): helper.enforce_floor({'data':[CoverageSummaryTests.entry(covered,total)]},95)
        for minimum in [-1,101,float('nan')]:
            with self.assertRaises(ValueError): helper.enforce_floor({'data':[CoverageSummaryTests.entry(95,100)]},minimum)


if __name__ == "__main__":
    unittest.main()
