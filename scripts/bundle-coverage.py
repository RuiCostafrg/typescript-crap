#!/usr/bin/env python3
"""Build the platform-independent, allowlisted coverage helper release asset."""
import hashlib
import json
from pathlib import Path
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
NAME = 'ts-crap-coverage-helpers.zip'
PREFIX = 'ts-crap-coverage-helpers/'
MODULES = (
    'coverage.mjs', 'coverage-hook.mjs', 'coverage-lines.mjs',
    'coverage-publish.mjs', 'coverage-snapshot.mjs', 'import-coverage.mjs',
    'strict-json.mjs',
)


def build(destination, root=ROOT):
    version = tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
    metadata = {'version': version, 'scannerVersion': version,
                'node': '>=24', 'coverageSchemaVersion': 1}
    contents = {name: (root / 'scripts' / name).read_bytes() for name in MODULES}
    contents['README.md'] = (root / 'docs' / 'COVERAGE_HELPERS.md').read_bytes()
    contents['LICENSE'] = (root / 'LICENSE').read_bytes()
    contents['version.json'] = (json.dumps(metadata, indent=2) + '\n').encode()
    # Resolve/read all inputs before creating output. Never ship arbitrary local files.
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / NAME
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
        for name, data in sorted(contents.items()):
            entry = zipfile.ZipInfo(PREFIX + name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, data)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (destination / (NAME + '.sha256')).write_text(f'{checksum}  {NAME}\n')
    return archive


if __name__ == '__main__':
    print(build(Path(sys.argv[1])))
