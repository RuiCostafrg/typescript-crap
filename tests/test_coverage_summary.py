"""Public CLI tests for the GitHub coverage-summary helper."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "coverage-summary.py"


class CoverageSummaryTests(unittest.TestCase):
    def run_report(self, data):
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "summary.json"
            file.write_text(json.dumps(data), encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(SCRIPT), str(file)],
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


if __name__ == "__main__":
    unittest.main()

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
