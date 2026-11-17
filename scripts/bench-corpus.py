#!/usr/bin/env python3
"""Opt-in real-corpus benchmark: time full scans of a checked-out project.

Each scanner build runs the same command several times, once with the report
destination absent and once with it already present (which adds the output
preflight walk). Every run must produce the same report bytes. The result is a
JSON record of wall time, user and system CPU, peak RSS and the report SHA-256.
This is evidence for humans, not a gate: no budget is applied.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time

MODES = ('absent', 'existing')
# A same-runner baseline build is labelled with this prefix, so the current
# builds keep their names and consumers of results.json keep working.
BASELINE_PREFIX = 'baseline-'
# Git refs are only accepted from a conservative character set and never as an
# option, because the workflow resolves them with git before building.
REF_PATTERN = re.compile(r'[A-Za-z0-9._/-]{1,100}')
# Medians compared between a current build and its baseline counterpart.
RATIO_FIELDS = ('wall_s', 'user_s', 'peak_rss_kib')
# Fields from GNU time's -v output, as (key, label, parser).
TIME_FIELDS = (
    ('user_s', 'User time (seconds)', float),
    ('system_s', 'System time (seconds)', float),
    ('peak_rss_kib', 'Maximum resident set size (kbytes)', int),
    ('exit_status', 'Exit status', int),
)


def labelled(value):
    """Parse LABEL=PATH, keeping the label for reports."""
    label, separator, path = value.partition('=')
    if not separator or not re.fullmatch(r'[a-z0-9][a-z0-9-]*', label) or not path:
        raise argparse.ArgumentTypeError('use LABEL=PATH with a lowercase label')
    return label, Path(path)


def baseline_ref(value):
    """Validate a baseline git ref: plain ref characters and no leading dash."""
    if not REF_PATTERN.fullmatch(value) or value.startswith('-'):
        raise argparse.ArgumentTypeError(
            'use 1-100 characters of letters, digits, ".", "_", "/" or "-", not starting with "-"')
    return value


def parse_time(text):
    """Extract the measured fields from GNU time -v output; every field is required."""
    values = {}
    for key, label, kind in TIME_FIELDS:
        match = re.search(rf'^\s*{re.escape(label)}: (\S+)$', text, re.MULTILINE)
        if not match:
            raise ValueError(f'GNU time output has no "{label}"')
        values[key] = kind(match.group(1))
    return values


def scan_command(scanner, corpus, destination):
    """The measured invocation: the whole corpus, reporting syntax errors, as JSON."""
    return [str(scanner), '--root', str(corpus), '--on-syntax-error', 'report',
            '--format', 'json', '--output', str(destination)]


def measure(scanner, corpus, destination, mode, timeout):
    """Run one scan in a fresh process and return its measurements."""
    if mode == 'absent':
        destination.unlink(missing_ok=True)
    elif not destination.is_file():
        raise ValueError('existing-destination run has no destination')
    with tempfile.NamedTemporaryFile(prefix='time-', suffix='.txt') as metrics:
        started = time.perf_counter()
        # A new session lets a timeout kill the scanner as well as GNU time.
        process = subprocess.Popen(['/usr/bin/time', '-v', '-o', metrics.name,
                                    *scan_command(scanner, corpus, destination)],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                   start_new_session=True)
        try:
            _, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            raise RuntimeError(f'scan exceeded {timeout}s') from None
        wall = time.perf_counter() - started
        values = parse_time(Path(metrics.name).read_text())
    # Exit 1 is the gate failing (scores over the threshold or unparsed files);
    # anything else is an error and the run does not count.
    if process.returncode not in (0, 1) or values['exit_status'] != process.returncode:
        raise RuntimeError(f'scan exited {process.returncode}: {stderr.decode(errors="replace")[-2000:]}')
    if not destination.is_file():
        raise RuntimeError('scan wrote no report')
    values.update(wall_s=wall, report_sha256=hashlib.sha256(destination.read_bytes()).hexdigest())
    return values


def summarize(runs):
    """Medians and ranges over the runs of one scanner and mode."""
    summary = {'runs': len(runs)}
    for key in ('wall_s', 'user_s', 'system_s', 'peak_rss_kib'):
        values = [run[key] for run in runs]
        summary[key] = {'median': statistics.median(values), 'min': min(values), 'max': max(values)}
    return summary


def describe_report(path):
    """Counts that identify the workload, read from one report."""
    report = json.loads(path.read_text(encoding='utf-8'))
    return {'bytes': path.stat().st_size, 'functions': len(report['functions']),
            'files_scanned': report.get('files_scanned'),
            'syntax_errors': len(report.get('syntax_errors', [])), 'failed': report['failed']}


def run_output(command, **kwargs):
    """Metadata helper: a command's trimmed stdout, or None when unavailable."""
    try:
        return subprocess.run(command, check=True, capture_output=True, text=True, **kwargs).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def cpu_model():
    """The first CPU model name, when /proc/cpuinfo has one."""
    try:
        text = Path('/proc/cpuinfo').read_text()
    except OSError:
        return None
    match = re.search(r'^model name\s*:\s*(.+)$', text, re.MULTILINE)
    return match.group(1).strip() if match else None


class Digests(list):
    """Every observed report digest, with the warmup digests per scanner label."""

    def __init__(self, values, warmups):
        super().__init__(values)
        self.warmups = warmups


def benchmark(scanners, corpus, runs, workspace, timeout):
    """Alternate scanners within each round so drift affects all of them alike."""
    results = {label: {mode: [] for mode in MODES} for label, _ in scanners}
    warmup_digests = []
    warmups = {}
    destination = workspace / 'report.json'
    for mode in MODES:
        for label, scanner in scanners:  # One unmeasured warmup per scanner and mode.
            if mode == 'existing':
                destination.write_text('{}\n')
            digest = measure(scanner, corpus, destination, mode, timeout)['report_sha256']
            warmup_digests.append(digest)
            warmups.setdefault(label, set()).add(digest)
        for round_index in range(runs):
            order = scanners if round_index % 2 == 0 else scanners[::-1]
            for label, scanner in order:
                if mode == 'existing':
                    destination.write_text('{}\n')
                results[label][mode].append(measure(scanner, corpus, destination, mode, timeout))
                print(f'{label} {mode} {round_index + 1}/{runs}: '
                      f'{results[label][mode][-1]["wall_s"]:.2f}s', flush=True)
    # The caller records every observation before failing on different reports.
    # Warmup reports must agree too; only their timings are excluded.
    digests = sorted({run['report_sha256'] for modes in results.values() for rows in modes.values() for run in rows}
                     | set(warmup_digests))
    return results, describe_report(destination), Digests(digests, warmups)


def per_label_digests(results, digests):
    """Every report digest of each scanner label, warmups included."""
    observed = {label: {run['report_sha256'] for rows in modes.values() for run in rows}
                for label, modes in results.items()}
    for label, values in getattr(digests, 'warmups', {}).items():
        observed.setdefault(label, set()).update(values)
    return observed


def report_failure(record, allow_report_change=False):
    """The message for report evidence that must fail a run, or None.

    Runs of one build must always agree byte for byte. A baseline may change the
    report (an older version can legitimately emit different output); that only
    passes when the caller accepts the change, and it is recorded either way.
    """
    if not record['report_consistent']:
        return f'scanners or runs produced different reports: {record["report_sha256s"]}'
    baseline = record.get('baseline')
    if baseline and not baseline['report_matches'] and not allow_report_change:
        return (f'baseline {baseline["ref"]} at {baseline["commit"]} produced a different report: '
                f'{baseline["report_sha256s"]} not in {record["report_sha256s"]}; '
                'pass --allow-report-change to record it')
    return None


def baseline_pairs(results):
    """Current build labels paired with their baseline build, in record order."""
    return [(label, BASELINE_PREFIX + label) for label in results
            if not label.startswith(BASELINE_PREFIX) and BASELINE_PREFIX + label in results]


def baseline_markdown(record):
    """An extra summary table comparing each current build with its baseline."""
    baseline = record.get('baseline')
    if not baseline:
        return []
    lines = ['', f'### Baseline comparison: `{baseline["ref"]}` at `{baseline["commit"]}`', '',
             'Reports matched the current builds.' if baseline['report_matches']
             else '**Reports differ from the current builds.**', '',
             '| Build | Destination | Wall (s) | User (s) | Peak RSS (MiB) | Wall ratio | User ratio | RSS ratio |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    for label, partner_label in baseline_pairs(record['results']):
        summaries = {label: record['results'][label]['summary'],
                     partner_label: record['results'][partner_label]['summary']}
        for name, modes in summaries.items():
            for mode in MODES:
                summary = modes.get(mode)
                if not summary:
                    continue
                ratios = ['—'] * len(RATIO_FIELDS)
                if name == label and mode in summaries[partner_label]:
                    old = summaries[partner_label][mode]
                    ratios = [f'{summary[key]["median"] / old[key]["median"]:.2f}'
                              if old[key]['median'] else '—' for key in RATIO_FIELDS]
                lines.append(f'| {name} | {mode} | {summary["wall_s"]["median"]:.2f} | '
                             f'{summary["user_s"]["median"]:.2f} | '
                             f'{summary["peak_rss_kib"]["median"] / 1024:.1f} | '
                             + ' | '.join(ratios) + ' |')
    return lines


def markdown(record):
    """A job-summary table of the medians, plus the baseline comparison."""
    digests = ', '.join(f'`{digest}`' for digest in record['report_sha256s'])
    lines = [f'### Corpus benchmark: {record["corpus"]["name"]} at `{record["corpus"]["commit"]}`', '',
             f'{record["report"]["functions"]:,} functions, report {record["report"]["bytes"]:,} bytes, '
             f'SHA-256 {digests}', '',
             *([] if record['report_consistent'] else ['**Reports differ between builds or runs.**', '']),
             '| Build | Destination | Wall (s) | User (s) | System (s) | Peak RSS (MiB) |',
             '| --- | --- | ---: | ---: | ---: | ---: |']
    for label, modes in record['results'].items():
        for mode, summary in modes['summary'].items():
            lines.append(f'| {label} | {mode} | {summary["wall_s"]["median"]:.2f} | '
                         f'{summary["user_s"]["median"]:.2f} | {summary["system_s"]["median"]:.2f} | '
                         f'{summary["peak_rss_kib"]["median"] / 1024:.1f} |')
    lines += ['', f'Medians of {record["runs"]} runs after one warmup per build and destination mode.']
    lines += baseline_markdown(record)
    return '\n'.join(lines) + '\n'


def main():
    """Benchmark labelled scanner builds on one corpus and write a JSON record."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True, help='checked-out project to scan')
    parser.add_argument('--corpus-name', default=None, help='label for the corpus (default: directory name)')
    parser.add_argument('--scanner', type=labelled, action='append', required=True,
                        help='LABEL=PATH of a ts-crap binary; repeat to compare builds')
    parser.add_argument('--baseline-scanner', type=labelled, action='append', default=[],
                        help='LABEL=PATH of a baseline binary; the label must start with "baseline-"')
    parser.add_argument('--baseline-ref', type=baseline_ref, default=None,
                        help='git ref of the baseline measured in the same run')
    parser.add_argument('--baseline-commit', default=None,
                        help='resolved commit SHA of --baseline-ref, recorded in results.json')
    parser.add_argument('--allow-report-change', action='store_true',
                        help='record, but do not fail on, a baseline report that differs')
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=1800)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--summary', type=Path, help='append a Markdown table (e.g. $GITHUB_STEP_SUMMARY)')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'android') or not Path('/usr/bin/time').is_file():
        parser.error('requires Linux and GNU /usr/bin/time')
    if args.runs < 1:
        parser.error('--runs must be at least 1')
    current = list(args.scanner)
    baseline = list(args.baseline_scanner)
    labels = [label for label, _ in current + baseline]
    if len(set(labels)) != len(labels):
        parser.error('scanner labels must be unique')
    for label, _ in current:
        if label.startswith(BASELINE_PREFIX):
            parser.error(f'current build labels must not start with "{BASELINE_PREFIX}"')
    for label, _ in baseline:
        if not label.startswith(BASELINE_PREFIX):
            parser.error(f'baseline build labels must start with "{BASELINE_PREFIX}"')
    if baseline and not (args.baseline_ref and args.baseline_commit):
        parser.error('--baseline-scanner requires --baseline-ref and --baseline-commit')
    if args.baseline_ref and not baseline:
        parser.error('--baseline-ref requires at least one --baseline-scanner')
    corpus = args.corpus.resolve(strict=True)
    scanners = [(label, path.resolve(strict=True)) for label, path in current + baseline]
    with tempfile.TemporaryDirectory(prefix='corpus-benchmark-') as temporary:
        results, report, digests = benchmark(scanners, corpus, args.runs, Path(temporary), args.timeout)
    observed = per_label_digests(results, digests)
    current_labels = [label for label, _ in current]
    baseline_labels = [label for label, _ in baseline]
    current_digests = set().union(*(observed[label] for label in current_labels))
    baseline_digests = set().union(*(observed[label] for label in baseline_labels)) if baseline_labels else set()
    record = {
        'version': 1,
        'command': scan_command('ts-crap', '<corpus>', '<report.json>'),
        'corpus': {'name': args.corpus_name or corpus.name,
                   'commit': run_output(['git', '-C', str(corpus), 'rev-parse', 'HEAD'])},
        'candidate_commit': run_output(['git', 'rev-parse', 'HEAD'], cwd=Path(__file__).resolve().parents[1]),
        'runs': args.runs,
        'report': report,
        'report_consistent': len(current_digests) <= 1 and len(baseline_digests) <= 1,
        'report_sha256s': sorted(digests),
        'baseline': {'ref': args.baseline_ref, 'commit': args.baseline_commit,
                     'report_matches': current_digests == baseline_digests,
                     'report_sha256s': sorted(baseline_digests)} if baseline_labels else None,
        'machine': {'platform': platform.platform(), 'cpu': cpu_model(), 'cpus': os.cpu_count(),
                    'runner': os.environ.get('RUNNER_NAME'), 'image': os.environ.get('ImageOS'),
                    'image_version': os.environ.get('ImageVersion'),
                    'time_tool': (run_output(['/usr/bin/time', '--version']) or '').splitlines()[:1]},
        'results': {label: {'binary_sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                            'version': run_output([str(path), '--version']),
                            'linkage': run_output(['file', '-b', str(path)]),
                            'summary': {mode: summarize(results[label][mode]) for mode in MODES},
                            'observations': results[label]}
                    for label, path in scanners},
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    table = markdown(record)
    print(table)
    if args.summary:
        with args.summary.open('a', encoding='utf-8') as summary:
            summary.write(table)
    # Every observation and the record are written first; only then fail.
    failure = report_failure(record, args.allow_report_change)
    if failure:
        raise SystemExit(failure)


if __name__ == '__main__':
    main()
