#!/usr/bin/env python3
"""Enforce assertion-killed mutation score; timeouts never count as catches."""
import argparse
from collections import Counter
import json


def check_all(reports, expected=None, minimum=90):
    if not reports:
        raise ValueError('No mutation reports')
    if expected is not None and not expected:
        raise ValueError('Mutation list is empty')
    counts = {key: 0 for key in ('CaughtMutant', 'MissedMutant', 'Timeout', 'Unviable')}
    names = Counter()
    for report in reports:
        if not report.get('end_time'):
            raise ValueError('Incomplete mutation run')
        outcomes = report['outcomes']
        baseline = [o for o in outcomes if o['scenario'] == 'Baseline']
        if len(baseline) != 1 or baseline[0]['summary'] != 'Success':
            raise ValueError('Mutation baseline did not pass')
        shard = {key: 0 for key in counts}
        for outcome in outcomes:
            if outcome['scenario'] == 'Baseline':
                continue
            summary = outcome['summary']
            if summary not in shard:
                raise ValueError(f'Unexpected mutation outcome: {summary}')
            shard[summary] += 1
            if expected is not None:
                names[outcome['scenario']['Mutant']['name']] += 1
        if sum(shard.values()) != report.get('total_mutants'):
            raise ValueError('Mutation results are incomplete')
        counts = {key: counts[key] + shard[key] for key in counts}
    if expected is not None and names != Counter(m['name'] for m in expected):
        raise ValueError('Mutation results do not match the mutant list')
    tested = counts['CaughtMutant'] + counts['MissedMutant'] + counts['Timeout']
    if not tested:
        raise ValueError('No viable mutants tested')
    score = 100 * counts['CaughtMutant'] / tested
    print(f'Mutation score: {score:.2f}% ({counts}); minimum {minimum}%')
    if score < minimum or counts['Timeout']:
        raise ValueError('Mutation score below floor or unresolved timeouts')
    return score


def check(report, minimum=90):
    return check_all([report], minimum=minimum)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected', help='mutant list from cargo mutants --list --json')
    parser.add_argument('outcomes', nargs='+', help='cargo-mutants outcomes.json file(s)')
    args = parser.parse_args(argv)
    expected = None
    if args.expected:
        with open(args.expected, encoding='utf-8') as source:
            expected = json.load(source)
    reports = []
    for path in args.outcomes:
        with open(path, encoding='utf-8') as source:
            reports.append(json.load(source))
    check_all(reports, expected=expected)


if __name__ == '__main__':
    main()
