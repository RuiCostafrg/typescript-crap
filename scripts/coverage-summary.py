#!/usr/bin/env python3
"""Render LLVM JSON export totals as Markdown. Never evaluate report content."""
import json
import sys


def render(report):
    data = report["data"]
    if not data:
        raise ValueError("coverage report contains no data")
    rows = [
        "## Rust source coverage",
        "",
        "| Metric | Covered | Total | Coverage |",
        "| --- | ---: | ---: | ---: |",
    ]
    for metric in ("lines", "functions", "regions"):
        total = sum(entry["totals"][metric]["count"] for entry in data)
        covered = sum(entry["totals"][metric]["covered"] for entry in data)
        if (
            type(total) is not int
            or type(covered) is not int
            or not 0 <= covered <= total
        ):
            raise ValueError("invalid coverage totals")
        percent = f"{100 * covered / total:.2f}%" if total else "N/A"
        rows.append(f"| {metric.title()} | {covered} | {total} | {percent} |")
    rows.extend([
        "",
        "Scope: production Rust source; test and benchmark source is excluded.",
        "Download the rust-coverage artifact for LCOV, JSON and HTML.",
        "This is Rust test coverage, not the TypeScript coverage consumed by ts-crap.",
    ])
    return "\n".join(rows) + "\n"


def enforce_floor(report, minimum):
    if not 0 <= minimum <= 100:
        raise ValueError("invalid coverage floor")
    # Validate every metric before trusting the line total.
    render(report)
    total = sum(entry["totals"]["lines"]["count"] for entry in report["data"])
    covered = sum(entry["totals"]["lines"]["covered"] for entry in report["data"])
    if total == 0 or covered * 100 < minimum * total:
        raise ValueError(f"production line coverage below {minimum}% floor")


if __name__ == "__main__":
    with open(sys.argv[1], encoding="utf-8") as source:
        report = json.load(source)
    if len(sys.argv) == 4 and sys.argv[2] == "--min-lines":
        enforce_floor(report, float(sys.argv[3]))
    elif len(sys.argv) != 2:
        raise ValueError("Usage: coverage-summary.py REPORT [--min-lines PERCENT]")
    print(render(report), end="")
