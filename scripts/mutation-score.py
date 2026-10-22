#!/usr/bin/env python3
"""Enforce assertion-killed mutation score; timeouts never count as catches."""
import json
import sys


def check(report, minimum=90):
    if not report.get('end_time'):
        raise ValueError('Incomplete mutation run')
    outcomes = report['outcomes']
    baseline = [o for o in outcomes if o['scenario'] == 'Baseline']
    if len(baseline) != 1 or baseline[0]['summary'] != 'Success':
        raise ValueError('Mutation baseline did not pass')
    counts = {key: 0 for key in ('CaughtMutant', 'MissedMutant', 'Timeout', 'Unviable')}
    for outcome in outcomes:
        if outcome['scenario'] == 'Baseline':
            continue
        summary = outcome['summary']
        if summary not in counts:
            raise ValueError(f'Unexpected mutation outcome: {summary}')
        counts[summary] += 1
    if sum(counts.values()) != report.get('total_mutants'):
        raise ValueError('Mutation results are incomplete')
    tested = counts['CaughtMutant'] + counts['MissedMutant'] + counts['Timeout']
    if not tested:
        raise ValueError('No viable mutants tested')
    score = 100 * counts['CaughtMutant'] / tested
    print(f'Mutation score: {score:.2f}% ({counts}); minimum {minimum}%')
    if score < minimum or counts['Timeout']:
        raise ValueError('Mutation score below floor or unresolved timeouts')
    return score


if __name__ == '__main__':
    with open(sys.argv[1], encoding='utf-8') as source:
        check(json.load(source))
