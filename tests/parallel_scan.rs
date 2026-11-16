//! Parallel analysis through the public API: every job count gives the same
//! report bytes, and the lowest failing file decides the error.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(dead_code)]
mod common;

use std::{fmt::Write, path::Path};
use ts_crap::{Format, OnSyntaxError, Report, ScanExtensions, ScanOptions, render, scan_with};

/// Enough files to use workers (selections under 32 files never do).
const FILES: usize = 40;

/// `FILES` sources alternating TS and TSX, some nested a few directories deep,
/// some deeply nested expressions. Index order equals path order.
fn project(special: &[(usize, &str)]) -> tempfile::TempDir {
    let mut files = Vec::new();
    for index in 0..FILES {
        let tsx = index % 2 == 1;
        let name = format!("f{index:02}.{}", if tsx { "tsx" } else { "ts" });
        let name = if index % 5 == 0 {
            format!("f{index:02}-dir/a/b/{name}")
        } else {
            name
        };
        let body = match special.iter().find(|(at, _)| *at == index) {
            Some((_, "syntax")) => "export function broken( {".to_owned(),
            Some((_, "utf8")) => String::new(),
            _ => source(index, tsx),
        };
        files.push((name, body));
    }
    let borrowed: Vec<_> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .collect();
    let directory = common::project(&borrowed);
    for (index, _) in special.iter().filter(|(_, kind)| *kind == "utf8") {
        let (name, _) = &files[*index];
        std::fs::write(directory.path().join(name), b"export const x = '\xff';").unwrap();
    }
    directory
}

fn source(index: usize, tsx: bool) -> String {
    let mut text = format!(
        "export function f{index}(a: boolean, b: boolean) {{ if (a && b) return 1; return {index}; }}\n\
         export class C{index} {{ m(x?: number) {{ return x ?? (() => {index})(); }} }}\n"
    );
    if tsx {
        writeln!(
            text,
            "export const V{index} = (p: {{ on: boolean }}) => p.on ? <b>{index}</b> : <i />;"
        )
        .unwrap();
    }
    if index % 7 == 3 {
        let depth = 300;
        text.push_str("export const deep = ");
        text.push_str(&"(x: number) => ".repeat(depth));
        text.push_str(&"(x + ".repeat(depth));
        text.push('1');
        text.push_str(&")".repeat(depth));
        text.push_str(";\n");
    }
    text
}

fn scan(root: &Path, jobs: usize, mode: OnSyntaxError) -> anyhow::Result<Report> {
    let mut extensions = ScanExtensions::default();
    extensions.jobs = jobs;
    extensions.on_syntax_error = mode;
    scan_with(
        &ScanOptions {
            root: root.into(),
            ..Default::default()
        },
        &extensions,
    )
}

fn rendered(report: &Report) -> Vec<String> {
    [
        Format::Json,
        Format::Text,
        Format::Xml,
        Format::Html,
        Format::Sarif,
    ]
    .into_iter()
    .map(|format| render(report, format).unwrap())
    .collect()
}

#[test]
fn every_job_count_gives_the_same_report_bytes() {
    let directory = project(&[]);
    let report = scan(directory.path(), 1, OnSyntaxError::Fail).unwrap();
    assert_eq!(report.files_scanned, FILES);
    let expected = rendered(&report);
    // At most 4 workers: under a looping mutant every worker allocates until the
    // mutation timeout. Larger counts are clamped and unit-tested.
    for jobs in [0, 2, 3, 4] {
        let report = scan(directory.path(), jobs, OnSyntaxError::Fail).unwrap();
        assert_eq!(rendered(&report), expected, "jobs {jobs}");
    }
}

#[test]
fn reported_syntax_errors_keep_file_order_across_workers() {
    let directory = project(&[
        (4, "syntax"),
        (11, "syntax"),
        (12, "syntax"),
        (37, "syntax"),
    ]);
    let expected = scan(directory.path(), 1, OnSyntaxError::Report).unwrap();
    assert_eq!(expected.syntax_errors.len(), 4);
    let expected = rendered(&expected);
    for _ in 0..3 {
        let report = scan(directory.path(), 4, OnSyntaxError::Report).unwrap();
        assert_eq!(rendered(&report), expected);
    }
}

#[test]
fn the_lowest_failing_file_decides_the_error() {
    for (special, expected) in [
        // Analysis errors only.
        (vec![(5, "syntax"), (17, "syntax"), (33, "syntax")], "f05"),
        (vec![(38, "syntax"), (39, "syntax")], "f38"),
        // A read error on the calling thread against analysis errors on workers.
        (vec![(4, "utf8"), (30, "syntax")], "f04"),
        (vec![(1, "syntax"), (4, "utf8")], "f01"),
        (vec![(0, "syntax"), (1, "utf8"), (2, "syntax")], "f00"),
    ] {
        let directory = project(&special);
        let error = |jobs| {
            format!(
                "{:#}",
                scan(directory.path(), jobs, OnSyntaxError::Fail).unwrap_err()
            )
        };
        let sequential = error(1);
        assert!(sequential.contains(expected), "{sequential}");
        for _ in 0..4 {
            assert_eq!(error(4), sequential, "{special:?}");
        }
    }
}
