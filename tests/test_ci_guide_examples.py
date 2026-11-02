"""Keep the copyable CI workflows and the executed CI-guide verifier in agreement."""
from pathlib import Path
import re
import shlex
import unittest

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / 'examples' / 'ci'
# The exact one-step command the Jest workflow runs and verify-ci-guide.py executes.
JEST_COMMAND = ['node', '$TS_CRAP_HELPERS/coverage-run.mjs', '--framework', 'jest', '--provider', 'babel',
                '--format', 'json', '--root', '.', '--output', '$run_dir/native.json', '--', '--runInBand']


def collection_block(text):
    """Lines after the reviewed-baseline retrieval up to `set +e` (comments included)."""
    marker = '"$run_dir/reviewed-baseline.json"\n'
    start = text.index(marker) + len(marker)
    return text[start:text.index('          set +e', start)]


def without_documented_differences(text):
    """The workflow minus its header comments, name, version pin and collection block."""
    text = text.replace(collection_block(text), '<collection>\n')
    lines = text.splitlines()
    kept = [line for line in lines if not line.startswith('#') and not line.startswith('name:')
            and 'TS_CRAP_VERSION:' not in line and not re.match(r'\s+# .*(helper bundle|coverage-run)', line)]
    return '\n'.join(kept)


class CiGuideExampleTests(unittest.TestCase):
    def test_jest_example_runs_exactly_the_one_step_command_the_verifier_executes(self):
        example = (EXAMPLES / 'github-actions-jest.yml').read_text()
        block = [line for line in collection_block(example).splitlines() if not line.strip().startswith('#')]
        tokens = shlex.split(' '.join(line.rstrip('\\').strip() for line in block))
        self.assertEqual(tokens, JEST_COMMAND)
        verifier = (ROOT / 'scripts' / 'verify-ci-guide.py').read_text()
        expected = ("'--framework','jest','--provider','babel',\n"
                    "                             '--format','json','--root',project,'--output',native,'--','--runInBand'")
        self.assertIn(expected, verifier, 'verify-ci-guide.py must execute the same arguments in the same order')

    def test_examples_differ_only_in_header_version_and_collection(self):
        vitest = (EXAMPLES / 'github-actions.yml').read_text()
        jest = (EXAMPLES / 'github-actions-jest.yml').read_text()
        # Checkout, install, reviewed policy/baseline retrieval, gate and artifacts must match.
        self.assertEqual(without_documented_differences(vitest), without_documented_differences(jest))
        for text in (vitest, jest):
            self.assertIn('git show "$BASE_SHA:crap-baseline.json"', text)
            self.assertIn('git show "$BASE_SHA:ts-crap.json"', text)
        self.assertRegex(jest, r"TS_CRAP_VERSION: '2\.(?:[3-9]|\d{2,})\.\d+'",
                         'the Jest example needs a release whose helper bundle has coverage-run.mjs')

    def test_the_comparison_notices_a_changed_baseline_step(self):
        vitest = (EXAMPLES / 'github-actions.yml').read_text()
        tampered = vitest.replace('git show "$BASE_SHA:crap-baseline.json"', 'git show "HEAD:crap-baseline.json"')
        self.assertNotEqual(without_documented_differences(vitest), without_documented_differences(tampered))


if __name__ == '__main__':
    unittest.main()
