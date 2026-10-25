"""Distribution contract: complete unchanged modules, version, and no local data."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import tempfile
import tomllib
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bundle', ROOT / 'scripts/bundle-coverage.py')
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class CoverageBundleTests(unittest.TestCase):
    def test_archive_has_complete_unchanged_modules_and_compatibility_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = bundle.build(Path(temporary))
            with zipfile.ZipFile(archive) as packaged:
                names = set(packaged.namelist())
                for entry in ['coverage.mjs', 'import-coverage.mjs', 'coverage-hook.mjs']:
                    self.assertIn(bundle.PREFIX + entry, names)
                for name in names:
                    if not name.endswith('.mjs'):
                        continue
                    text = packaged.read(name).decode()
                    self.assertEqual(packaged.read(name), (ROOT / 'scripts' / Path(name).name).read_bytes())
                    for dependency in re.findall(r"from ['\"]\./([^'\"]+)['\"]", text):
                        self.assertIn(bundle.PREFIX + dependency, names)
                metadata = json.loads(packaged.read(bundle.PREFIX + 'version.json'))
                version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
                self.assertEqual(metadata['version'], version)
                self.assertEqual(metadata['scannerVersion'], version)
                self.assertEqual(metadata['node'], '>=24')
                self.assertEqual(metadata['coverageSchemaVersion'], 1)
                self.assertIn(bundle.PREFIX + 'README.md', names)
                self.assertIn(bundle.PREFIX + 'LICENSE', names)
            checksum = Path(str(archive) + '.sha256').read_text().split()
            self.assertEqual(checksum, [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name])

    def test_bundle_is_reproducible_and_excludes_unlisted_local_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'source'
            root.mkdir()
            shutil.copyfile(ROOT / 'Cargo.toml', root / 'Cargo.toml')
            shutil.copyfile(ROOT / 'LICENSE', root / 'LICENSE')
            (root / 'scripts').mkdir()
            (root / 'docs').mkdir()
            shutil.copyfile(ROOT / 'docs/COVERAGE_HELPERS.md', root / 'docs/COVERAGE_HELPERS.md')
            for name in bundle.MODULES:
                shutil.copyfile(ROOT / 'scripts' / name, root / 'scripts' / name)
            first = bundle.build(Path(temporary) / 'first', root).read_bytes()
            (root / 'scripts/private.mjs').write_text('private local data')
            (root / 'docs/local-report.md').write_text('private report')
            second = bundle.build(Path(temporary) / 'second', root).read_bytes()
            self.assertEqual(first, second)
            with zipfile.ZipFile(Path(temporary) / 'second' / bundle.NAME) as archive:
                self.assertEqual(len(archive.namelist()), 10)
            (root / 'scripts/coverage-hook.mjs').unlink()
            with self.assertRaises(FileNotFoundError):
                bundle.build(Path(temporary) / 'missing', root)
            self.assertFalse((Path(temporary) / 'missing').exists())


if __name__ == '__main__':
    unittest.main()
