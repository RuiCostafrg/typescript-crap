"""Keep the framework version fixtures, CI matrix and supported-versions table in agreement."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests' / 'fixtures' / 'framework-versions'
EXACT = re.compile(r'^\d+\.\d+\.\d+$')


def fixtures():
    return sorted(path for path in FIXTURES.iterdir() if path.is_dir())


class FrameworkVersionTests(unittest.TestCase):
    def test_each_fixture_pins_one_framework_major_matching_its_name(self):
        self.assertEqual([path.name for path in fixtures()],
                         ['jest-29', 'jest-30', 'vitest-3', 'vitest-4', 'vitest-5'])
        for path in fixtures():
            framework, major = path.name.split('-')
            manifest = json.loads((path / 'package.json').read_text())
            dependencies = manifest['devDependencies']
            for name, version in dependencies.items():
                self.assertRegex(version, EXACT, f'{path.name}: {name} must be pinned exactly')
            self.assertEqual(dependencies[framework].split('.')[0], major, path.name)
            other = 'jest' if framework == 'vitest' else 'vitest'
            self.assertNotIn(other, dependencies, f'{path.name} installs one framework')
            if framework == 'vitest':
                for provider in ('@vitest/coverage-istanbul', '@vitest/coverage-v8'):
                    self.assertEqual(dependencies[provider], dependencies['vitest'], path.name)
            else:
                self.assertEqual(dependencies['babel-jest'], dependencies['jest'], path.name)
            self.assertEqual(manifest['workspaces'], ['packages/*'], path.name)

    def test_lockfiles_match_their_manifests(self):
        for path in fixtures():
            manifest = json.loads((path / 'package.json').read_text())
            lock = json.loads((path / 'package-lock.json').read_text())
            packages = lock['packages']
            self.assertEqual(packages['']['devDependencies'], manifest['devDependencies'], path.name)
            for name, version in manifest['devDependencies'].items():
                self.assertEqual(packages[f'node_modules/{name}']['version'], version, f'{path.name}: {name}')

    def test_ci_matrix_and_compatibility_table_list_every_fixture(self):
        workflow = (ROOT / '.github' / 'workflows' / 'ci.yml').read_text()
        match = re.search(r'^\s+versions: \[([^\]]+)\]$', workflow, re.MULTILINE)
        self.assertIsNotNone(match, 'framework-versions matrix not found')
        self.assertEqual(sorted(name.strip() for name in match.group(1).split(',')),
                         [path.name for path in fixtures()])
        table = (ROOT / 'docs' / 'COMPATIBILITY.md').read_text()
        for path in fixtures():
            framework, major = path.name.split('-')
            version = json.loads((path / 'package.json').read_text())['devDependencies'][framework]
            row = f'| {framework.capitalize()} {major} | {version} |'
            self.assertIn(row, table, f'COMPATIBILITY.md must list {path.name} at {version}')


if __name__ == '__main__':
    unittest.main()
