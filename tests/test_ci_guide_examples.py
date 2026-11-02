"""Keep the copyable CI workflows and the executed CI-guide verifier in agreement."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


class CiGuideExampleTests(unittest.TestCase):
    def test_jest_example_runs_the_one_step_command_the_verifier_executes(self):
        example = (ROOT / 'examples' / 'ci' / 'github-actions-jest.yml').read_text()
        verifier = (ROOT / 'scripts' / 'verify-ci-guide.py').read_text()
        command = re.search(r'coverage-run\.mjs" (--framework jest[^\n\\]*)\\\n\s+([^\n]*)', example)
        self.assertIsNotNone(command, 'Jest example must call coverage-run.mjs')
        flags = (command.group(1) + ' ' + command.group(2)).split()
        for flag in ('--framework', 'jest', '--provider', 'babel', '--format', 'json', '--root', '--output', '--', '--runInBand'):
            self.assertIn(flag, flags)
        for flag in ("'--framework','jest'", "'--provider','babel'", "'--format','json'", "'--','--runInBand'"):
            self.assertIn(flag, verifier, f'verify-ci-guide.py must execute {flag}')

    def test_both_examples_share_the_reviewed_gate_steps(self):
        vitest = (ROOT / 'examples' / 'ci' / 'github-actions.yml').read_text()
        jest = (ROOT / 'examples' / 'ci' / 'github-actions-jest.yml').read_text()
        start = 'set +e'
        self.assertEqual(vitest[vitest.index(start):], jest[jest.index(start):],
                         'the scan, baseline and artifact steps must stay identical')
        self.assertRegex(jest, r"TS_CRAP_VERSION: '2\.(?:[3-9]|\d{2,})\.\d+'",
                         'the Jest example needs a release whose helper bundle has coverage-run.mjs')


if __name__ == '__main__':
    unittest.main()
