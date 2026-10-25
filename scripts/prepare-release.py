#!/usr/bin/env python3
"""Validate exact release contents and hashes before privileged publication."""
import hashlib
from pathlib import Path
import shutil
import sys
import importlib.util

spec = importlib.util.spec_from_file_location("install_dist", Path(__file__).with_name("install-dist.py"))
install_dist = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install_dist)
TARGETS = install_dist.ARCHIVES

def prepare(source, destination):
    archives = [f"ts-crap-{t}.{'zip' if 'windows' in t else 'tar.xz'}" for t in TARGETS]
    archives.append('ts-crap-coverage-helpers.zip')
    names = archives + [a + ".sha256" for a in archives] + [
        "ts-crap-installer.sh", "ts-crap-installer.ps1", "ts-crap.rb", "sha256.sum", "dist-manifest.json"]
    for name in names:
        path = source / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Missing or non-regular release asset: {name}")
    for name in archives:
        expected = (source / (name + ".sha256")).read_text().split()
        if len(expected) != 2 or expected[1].lstrip("*") != name:
            raise ValueError(f"Invalid checksum filename: {name}")
        if hashlib.sha256((source / name).read_bytes()).hexdigest() != expected[0]:
            raise ValueError(f"Checksum mismatch: {name}")
    destination.mkdir()
    for name in names:
        shutil.copyfile(source / name, destination / name)
    return names

if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]))
