#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{
    FileContext, INLINE_SOURCE_BYTES, Limits, MAX_IN_FLIGHT_BYTES, MIN_PARALLEL_FILES, Peaks,
    Totals, must_wait, parallel, workers,
};
use crate::{Format, OnSyntaxError, ScanExtensions, ScanOptions, render, scan_with};
use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
};

const PANIC_MARKER: &str = "panic-hook.ts";
const PANIC_MESSAGE: &str = "pipeline test hook panic";

const PROBE_MARKER: &str = "thread-probe";
/// Names of the threads that analyzed files whose path contains `PROBE_MARKER`.
static PROBED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Test-only hooks: analyzing a file named `*panic-hook.ts` panics, and files
/// under a `thread-probe` directory record the analyzing thread's name.
pub(super) fn hook(relative: &str) {
    if relative.ends_with(PANIC_MARKER) {
        std::panic::panic_any(PANIC_MESSAGE);
    }
    if relative.contains(PROBE_MARKER) {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        PROBED.lock().unwrap().push(name);
    }
}

/// Parallel-scan tests take turns, so at most one test's workers run at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    // A test that panicked while holding the lock leaves nothing to clean up.
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const FILES: usize = 40;

/// `FILES` sources alternating TS and TSX, some nested a few directories deep.
/// `special` replaces the source (or name suffix) of chosen indices.
fn project(special: &[(usize, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..FILES {
        let special = special
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, kind)| *kind);
        let extension = if index % 2 == 0 { "ts" } else { "tsx" };
        let name = match special {
            Some("panic") => format!("f{index:02}-{PANIC_MARKER}"),
            _ => format!("f{index:02}.{extension}"),
        };
        let path = if index % 5 == 0 {
            directory.path().join(format!("f{index:02}-dir/a/b/{name}"))
        } else {
            directory.path().join(name)
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match special {
            Some("syntax") => fs::write(&path, "export function broken( {").unwrap(),
            Some("utf8") => fs::write(&path, b"export const x = '\xff';").unwrap(),
            _ => fs::write(&path, source(index, extension == "tsx")).unwrap(),
        }
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
        // Deep nesting, on several workers at once.
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

fn scan(root: &Path, jobs: usize, mode: OnSyntaxError) -> anyhow::Result<crate::Report> {
    let extensions = ScanExtensions {
        on_syntax_error: mode,
        jobs,
    };
    scan_with(
        &ScanOptions {
            root: root.into(),
            ..Default::default()
        },
        &extensions,
    )
}

fn rendered(report: &crate::Report) -> Vec<String> {
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
fn bounds_wait_for_the_window_and_for_in_flight_bytes() {
    let limits = Limits {
        window: 8,
        bytes: MAX_IN_FLIGHT_BYTES,
        inline: usize::MAX,
    };
    assert!(!must_wait(limits, 0, 0, 0));
    assert!(!must_wait(limits, 7, 0, 10));
    assert!(must_wait(limits, 8, 0, 10));
    assert!(must_wait(limits, 9, 0, 10));
    // Bytes: fill up to the limit, never past it.
    assert!(!must_wait(limits, 1, MAX_IN_FLIGHT_BYTES - 10, 10));
    assert!(must_wait(limits, 1, MAX_IN_FLIGHT_BYTES - 10, 11));
    // A file larger than the limit waits until nothing is in flight, then runs alone.
    assert!(must_wait(limits, 1, 1, MAX_IN_FLIGHT_BYTES + 1));
    assert!(!must_wait(limits, 0, 0, MAX_IN_FLIGHT_BYTES + 1));
    assert!(must_wait(limits, 1, MAX_IN_FLIGHT_BYTES + 1, 0));
    assert!(must_wait(limits, 1, 1, usize::MAX));
}

/// Runs `parallel` directly over in-memory sources with small limits. Indices
/// in `broken` hold a syntax error.
fn run_parallel(
    sizes: &[usize],
    broken: &[usize],
    jobs: usize,
    limits: Limits,
) -> (anyhow::Result<Peaks>, Totals) {
    let paths: Vec<PathBuf> = (0..sizes.len())
        .map(|index| PathBuf::from(format!("f{index:03}.ts")))
        .collect();
    let mut load = |path: &PathBuf| -> anyhow::Result<(String, String)> {
        let index: usize = path.to_str().unwrap()[1..4].parse().unwrap();
        let body = if broken.contains(&index) {
            "export function broken( {\n".to_owned()
        } else {
            format!("export function f{index}() {{ return {index}; }}\n")
        };
        // Pad with a comment to the requested size.
        let padding = sizes[index].saturating_sub(body.len() + 3);
        Ok((
            path.to_str().unwrap().to_owned(),
            format!("{body}//{}\n", "x".repeat(padding)),
        ))
    };
    let context = FileContext {
        coverage: None,
        threshold: 10.0,
        on_syntax_error: OnSyntaxError::Fail,
    };
    let mut totals = Totals::default();
    let peaks = parallel(&paths, jobs, limits, &mut load, &context, &mut totals);
    (peaks, totals)
}

fn peaks(sizes: &[usize], jobs: usize, limits: Limits) -> (Peaks, Totals) {
    let (peaks, totals) = run_parallel(sizes, &[], jobs, limits);
    (peaks.unwrap(), totals)
}

fn names(totals: &Totals) -> Vec<&str> {
    totals.functions.iter().map(|f| f.path.as_str()).collect()
}

fn expected_names(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("f{index:03}.ts")).collect()
}

#[test]
fn parallel_runs_never_exceed_the_window_or_the_byte_limit() {
    let _serial = serial();
    let limits = Limits {
        window: 3,
        bytes: 1_000,
        inline: usize::MAX,
    };
    for _ in 0..10 {
        let (seen, totals) = peaks(&[300; 40], 4, limits);
        assert!(seen.handed_out <= 3, "{seen:?}");
        assert!(seen.in_flight_bytes <= 1_000, "{seen:?}");
        assert_eq!(seen.inline, 0);
        // Three 300-byte files fit; work did overlap.
        assert!(
            seen.handed_out >= 1 && seen.in_flight_bytes >= 300,
            "{seen:?}"
        );
        assert_eq!(names(&totals), expected_names(40));
    }
    // A file over the byte limit runs alone: nothing else is in flight with it.
    let mut sizes = vec![100; 40];
    sizes[7] = 5_000;
    sizes[20] = 2_000;
    for _ in 0..10 {
        let (seen, totals) = peaks(&sizes, 4, limits);
        assert!(seen.in_flight_bytes <= 5_000, "{seen:?}");
        assert_eq!(names(&totals), expected_names(40));
    }
}

#[test]
fn large_files_run_on_the_calling_thread_in_file_order() {
    let _serial = serial();
    let limits = Limits {
        window: 4,
        bytes: 1_000,
        inline: 500,
    };
    let mut sizes = vec![100; 40];
    for index in [0, 7, 8, 20, 39] {
        sizes[index] = 600;
    }
    // Over the byte limit too: it runs alone, on the calling thread.
    sizes[30] = 5_000;
    for _ in 0..5 {
        let (seen, totals) = peaks(&sizes, 3, limits);
        assert_eq!(seen.inline, 6, "{seen:?}");
        // Inline sources never count as handed out to workers.
        assert!(seen.in_flight_bytes <= 400, "{seen:?}");
        assert_eq!(names(&totals), expected_names(40));
    }
    // The lowest failing file wins whether it ran inline or on a worker.
    for (broken, expected) in [
        (vec![7, 3], "f003"),
        (vec![7, 12], "f007"),
        (vec![25, 30], "f025"),
        (vec![30, 35], "f030"),
    ] {
        for _ in 0..5 {
            let (result, _) = run_parallel(&sizes, &broken, 3, limits);
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains(expected), "{broken:?}: {error}");
        }
    }
}

#[test]
fn small_selections_and_single_jobs_stay_on_the_calling_thread() {
    assert_eq!(MIN_PARALLEL_FILES, 32);
    assert_eq!(INLINE_SOURCE_BYTES, 128 << 10);
    assert_eq!(workers(4, MIN_PARALLEL_FILES - 1), 1);
    assert_eq!(workers(4, MIN_PARALLEL_FILES), 4);
    assert_eq!(workers(0, 100), 1);
    assert_eq!(workers(1, 100), 1);
    assert_eq!(workers(64, 40), 40);
}

#[test]
fn every_job_count_gives_the_same_report_bytes() {
    let _serial = serial();
    let directory = project(&[]);
    let report = scan(directory.path(), 1, OnSyntaxError::Fail).unwrap();
    assert_eq!(report.files_scanned, FILES);
    let expected = rendered(&report);
    // Few workers on purpose: under a looping mutant every worker allocates until
    // the mutation timeout, so many workers can exhaust a CI runner's memory.
    // Counts above the file count are covered by `workers` itself.
    for jobs in [0, 2, 3, 4] {
        for _ in 0..2 {
            let report = scan(directory.path(), jobs, OnSyntaxError::Fail).unwrap();
            assert_eq!(rendered(&report), expected, "jobs {jobs}");
        }
    }
}

#[test]
fn reported_syntax_errors_keep_file_order_across_workers() {
    let _serial = serial();
    let directory = project(&[
        (4, "syntax"),
        (11, "syntax"),
        (12, "syntax"),
        (37, "syntax"),
    ]);
    let expected = scan(directory.path(), 1, OnSyntaxError::Report).unwrap();
    assert_eq!(expected.syntax_errors.len(), 4);
    let expected = rendered(&expected);
    for _ in 0..5 {
        let report = scan(directory.path(), 4, OnSyntaxError::Report).unwrap();
        assert_eq!(rendered(&report), expected);
    }
}

fn error(directory: &Path, jobs: usize) -> String {
    format!(
        "{:#}",
        scan(directory, jobs, OnSyntaxError::Fail).unwrap_err()
    )
}

#[test]
fn the_lowest_failing_file_decides_the_error() {
    let _serial = serial();
    for (special, expected) in [
        // Analysis errors only.
        (vec![(5, "syntax"), (17, "syntax"), (33, "syntax")], "f05"),
        (vec![(38, "syntax"), (39, "syntax")], "f38"),
        // A read error on the calling thread against analysis errors on workers.
        (vec![(4, "utf8"), (30, "syntax")], "f04"),
        (vec![(1, "syntax"), (4, "utf8")], "f01"),
        (vec![(0, "syntax"), (1, "utf8"), (2, "syntax")], "f00"),
        // A lower error wins over a later panic.
        (vec![(2, "syntax"), (20, "panic")], "f02"),
        (vec![(2, "utf8"), (3, "panic")], "f02"),
    ] {
        let directory = project(&special);
        let sequential = error(directory.path(), 1);
        assert!(sequential.contains(expected), "{sequential}");
        for _ in 0..10 {
            assert_eq!(error(directory.path(), 4), sequential, "{special:?}");
        }
    }
}

#[test]
fn a_lower_panic_resumes_and_wins_over_later_errors() {
    let _serial = serial();
    for special in [
        vec![(3, "panic"), (10, "syntax")],
        vec![(3, "panic"), (4, "utf8")],
        vec![(3, "panic"), (36, "panic")],
        vec![(39, "panic")],
    ] {
        let directory = project(&special);
        for jobs in [1, 4] {
            for _ in 0..if jobs == 1 { 1 } else { 6 } {
                let payload =
                    std::panic::catch_unwind(|| scan(directory.path(), jobs, OnSyntaxError::Fail))
                        .expect_err("the panic must reach the caller");
                assert_eq!(
                    payload.downcast_ref::<&str>(),
                    Some(&PANIC_MESSAGE),
                    "{special:?}, jobs {jobs}"
                );
            }
        }
    }
}

#[test]
fn large_selections_really_run_on_workers_and_small_ones_do_not() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    let probe = directory.path().join(PROBE_MARKER);
    fs::create_dir(&probe).unwrap();
    for index in 0..40 {
        fs::write(probe.join(format!("f{index:02}.ts")), source(index, false)).unwrap();
    }
    let names = |paths: &[&str], jobs: usize| -> Vec<String> {
        PROBED.lock().unwrap().clear();
        let extensions = ScanExtensions {
            on_syntax_error: OnSyntaxError::Fail,
            jobs,
        };
        scan_with(
            &ScanOptions {
                root: directory.path().into(),
                paths: paths.iter().map(Into::into).collect(),
                ..Default::default()
            },
            &extensions,
        )
        .unwrap();
        std::mem::take(&mut *PROBED.lock().unwrap())
    };
    let parallel = names(&[], 4);
    assert_eq!(parallel.len(), 40);
    assert!(
        parallel
            .iter()
            .all(|name| name.starts_with("ts-crap-analysis-")),
        "{parallel:?}"
    );
    // Selections under 32 files and single jobs stay on the calling thread.
    let few: Vec<String> = (0..31)
        .map(|index| format!("{PROBE_MARKER}/f{index:02}.ts"))
        .collect();
    let few: Vec<&str> = few.iter().map(String::as_str).collect();
    for (paths, jobs) in [(few.as_slice(), 4), (&[][..], 1)] {
        let calling = names(paths, jobs);
        assert!(!calling.is_empty());
        assert!(
            calling
                .iter()
                .all(|name| !name.starts_with("ts-crap-analysis-")),
            "{calling:?}"
        );
    }
}
