#!/usr/bin/env python3
"""Record actual command wall time and exit status without hiding failures."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a command is required after --')
    start = time.monotonic()
    try:
        result = subprocess.run(command, check=False)
        code = result.returncode
    except OSError as error:
        print(f'Cannot run {command[0]}: {error}', file=sys.stderr)
        code = 127
    seconds = time.monotonic() - start
    record = {'label': args.label, 'seconds': seconds, 'exit_code': code}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('a', encoding='utf-8') as output:
        output.write(json.dumps(record) + '\n')
    print(f'{args.label}: {seconds:.3f}s (exit {code})', flush=True)
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with open(summary, 'a', encoding='utf-8') as output:
            output.write(f'- {args.label}: {seconds:.3f}s (exit {code})\n')
    return code if code >= 0 else 128 - code


if __name__ == '__main__':
    sys.exit(main())
