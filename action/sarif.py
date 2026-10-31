"""Post-process the action's SARIF report (see action/scan.sh).

ts-crap writes result paths relative to its --root. Code scanning resolves
them against the repository checkout, so when the scan root is a
subdirectory, the paths are prefixed with that directory. The script then
prints one summary line per result. Results without a location, such as
coverage-completeness findings, print their message alone.

Usage: python3 -I sarif.py REPORT SCAN_ROOT WORKSPACE
Exit status 3 means SCAN_ROOT is outside WORKSPACE and paths were left as-is.
"""
import json
import os
import sys
from pathlib import PurePosixPath
from urllib.parse import quote, unquote


def root_prefix(scan_root, workspace):
    """Scan root relative to the checkout, or None when it is outside it."""
    try:
        relative = os.path.relpath(os.path.realpath(scan_root), os.path.realpath(workspace))
    except ValueError:  # different Windows drives
        return None
    if relative == os.pardir or relative.startswith(os.pardir + os.sep) or os.path.isabs(relative):
        return None
    return '' if relative == os.curdir else PurePosixPath(*relative.split(os.sep)).as_posix()


def rewrite(report, prefix):
    # ts-crap percent-encodes result URIs, so encode the directory prefix the
    # same way (keeping `/`) and leave the existing URI as it is.
    prefix = quote(prefix, safe='/')
    for run in report.get('runs', []):
        for result in run.get('results', []):
            for location in result.get('locations', []):
                artifact = location.get('physicalLocation', {}).get('artifactLocation', {})
                uri = artifact.get('uri')
                if prefix and isinstance(uri, str) and '://' not in uri and not uri.startswith('/'):
                    artifact['uri'] = f'{prefix}/{uri}'


def printable(text):
    """Keep the text on one line and free of terminal control characters.

    PR-controlled file and function names reach the log, so every character
    that is not printable (newlines, tabs, escape sequences) is shown as an
    escape code. Ordinary spaces are kept, so paths print exactly.
    """
    return ''.join(char if char == ' ' or char.isprintable()
                   else char.encode('unicode_escape').decode('ascii') for char in text)


def summary(report):
    lines = []
    results = [result for run in report.get('runs', []) for result in run.get('results', [])]
    for result in results:
        message = str(result.get('message', {}).get('text', '(no message)'))
        message = printable(message)
        location = (result.get('locations') or [{}])[0].get('physicalLocation', {})
        uri = location.get('artifactLocation', {}).get('uri')
        if isinstance(uri, str):
            uri = printable(unquote(uri))
        line = location.get('region', {}).get('startLine')
        if uri is None:
            lines.append(message)
        else:
            lines.append(f'{uri}:{line}: {message}' if line is not None else f'{uri}: {message}')
    lines.append(f'{len(results)} SARIF result(s)')
    return lines


def main():
    path, scan_root, workspace = sys.argv[1:4]
    with open(path, encoding='utf-8') as source:
        report = json.load(source)
    prefix = root_prefix(scan_root, workspace)
    if prefix:
        rewrite(report, prefix)
        with open(path, 'w', encoding='utf-8') as target:
            json.dump(report, target, indent=2)
    for line in summary(report):
        print(line)
    # 3 tells scan.sh to warn (outside its stop-commands block) that the paths
    # could not be made relative to the checkout.
    return 3 if prefix is None else 0


if __name__ == '__main__':
    sys.exit(main())
