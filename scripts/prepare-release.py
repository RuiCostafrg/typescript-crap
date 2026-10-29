#!/usr/bin/env python3
"""Validate exact release contents and hashes before privileged publication."""
import hashlib
from pathlib import Path
import shutil
import sys
import importlib.util
import tomllib

spec = importlib.util.spec_from_file_location("install_dist", Path(__file__).with_name("install-dist.py"))
install_dist = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install_dist)
spec = importlib.util.spec_from_file_location("linux_packages", Path(__file__).with_name("linux-packages.py"))
linux_packages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(linux_packages)
# Host cargo-dist downloads and application release targets are distinct lists.
TARGETS = (*install_dist.ARCHIVES, *linux_packages.TARGETS)

def checksummed_assets(version=None):
    """Require every native archive, helper bundle and versioned Debian package."""
    if version is None:
        version = tomllib.loads((Path(__file__).resolve().parents[1] / 'Cargo.toml').read_text())['package']['version']
    return ([f"ts-crap-{t}.{'zip' if 'windows' in t else 'tar.xz'}" for t in TARGETS]
            + ['ts-crap-coverage-helpers.zip']
            + [linux_packages.package_name(version, arch) for arch in ('amd64', 'arm64')])

def prepare(source, destination):
    archives = checksummed_assets()
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
