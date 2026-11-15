//! Per-file analysis, on the calling thread or on a bounded set of workers.
//!
//! Parallel scans use an ordered producer and pure workers. The calling thread
//! reads each file and, in diff mode, verifies it against its HEAD blob,
//! strictly in file order as a sequential scan does. Workers only parse,
//! collect, bind coverage and score, and never touch the filesystem. Results
//! are merged in file order, and the outcome of the lowest-index failing file
//! is the one reported, so the result never depends on thread timing.
use crate::{FunctionScore, OnSyntaxError, UnparsedFile, analysis, coverage};
use anyhow::{Result, anyhow};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

/// Selections smaller than this are always analyzed on the calling thread.
pub(crate) const MIN_PARALLEL_FILES: usize = 32;
/// Source bytes handed to workers and not yet returned. A larger file runs alone.
/// The calling thread also holds the one file it read and has not handed out.
const MAX_IN_FLIGHT_BYTES: usize = 64 << 20;
/// Files handed out but not yet merged, per worker. Bounds both the queue and
/// the reorder buffer.
const WINDOW_PER_WORKER: usize = 2;

/// How much work may be outstanding at once.
#[derive(Clone, Copy)]
struct Limits {
    /// Files handed out but not yet merged.
    window: usize,
    /// Source bytes handed out but not yet returned by a worker.
    bytes: usize,
}

/// The most work that was outstanding during one parallel run.
#[derive(Debug, Default, PartialEq, Eq)]
struct Peaks {
    handed_out: usize,
    in_flight_bytes: usize,
}
/// Matches the usual main-thread stack rather than the smaller spawned default.
const WORKER_STACK_BYTES: usize = 8 << 20;

/// Settings shared by every file of one scan.
pub(crate) struct FileContext<'a> {
    pub coverage: Option<&'a coverage::Coverage>,
    pub threshold: f64,
    pub on_syntax_error: OnSyntaxError,
}

/// One file reduced to what the report keeps: no tree, source or line sets.
pub(crate) struct FileOutcome {
    functions: Vec<FunctionScore>,
    missing_file: bool,
    functions_missing: usize,
    syntax_error: Option<UnparsedFile>,
}

/// Report fields accumulated in file order.
#[derive(Default)]
pub(crate) struct Totals {
    pub functions: Vec<FunctionScore>,
    pub files_missing_coverage: usize,
    pub functions_missing_coverage: usize,
    pub syntax_errors: Vec<UnparsedFile>,
}

impl Totals {
    fn add(&mut self, mut outcome: FileOutcome) {
        self.files_missing_coverage += usize::from(outcome.missing_file);
        self.functions_missing_coverage += outcome.functions_missing;
        self.functions.append(&mut outcome.functions);
        self.syntax_errors.extend(outcome.syntax_error);
    }
}

/// Analyze, bind coverage and score one file that was already read and verified.
pub(crate) fn analyze_file(
    analyzer: &mut analysis::Analyzer,
    context: &FileContext<'_>,
    relative: &str,
    source: &str,
) -> Result<FileOutcome> {
    #[cfg(test)]
    tests::hook(relative);
    let methods = match analyzer.analyze(relative, source) {
        Ok(methods) => methods,
        Err(error) if context.on_syntax_error == OnSyntaxError::Report => {
            let Some(issue) = error.downcast_ref::<analysis::SyntaxError>() else {
                return Err(error);
            };
            // Reporting syntax errors must not hide stale or invalid coverage.
            let source_coverage = context
                .coverage
                .map(|coverage| coverage.for_source(relative, source))
                .transpose()?;
            return Ok(FileOutcome {
                functions: Vec::new(),
                missing_file: source_coverage.as_ref().is_none_or(|c| !c.present()),
                functions_missing: 0,
                syntax_error: Some(UnparsedFile {
                    path: relative.to_owned(),
                    line: issue.line,
                    column: issue.column,
                    column_utf16: issue.column_utf16,
                    // Saved reports may be shared independently of repository access.
                    message: format!(
                        "invalid TypeScript syntax in {}:{}:{} (UTF-8 byte column)",
                        issue.path, issue.line, issue.column
                    ),
                }),
            });
        }
        Err(error) => return Err(error),
    };
    // Validate coverage even for selected files with no implemented functions.
    let source_coverage = context
        .coverage
        .map(|cov| cov.for_source(relative, source))
        .transpose()?;
    let missing_file = source_coverage.as_ref().is_none_or(|c| !c.present());
    let mut functions = Vec::with_capacity(methods.len());
    let mut functions_missing = 0;
    for method in methods {
        let evidence = source_coverage
            .as_ref()
            .map(|coverage| coverage.evidence(&method.lines))
            .transpose()?
            .unwrap_or_default();
        functions_missing += usize::from(!evidence.complete);
        let cov = evidence.fraction;
        let cc = f64::from(method.complexity);
        let crap = cc * cc * (1.0 - cov).powi(3) + cc;
        functions.push(FunctionScore {
            identity: method.identity,
            path: relative.to_owned(),
            name: method.name,
            line: method.line,
            column: method.column,
            column_utf16: method.column_utf16,
            end_line: method.end_line,
            complexity: method.complexity,
            coverage: cov,
            crap,
            failed: crap > context.threshold,
        });
    }
    Ok(FileOutcome {
        functions,
        missing_file,
        functions_missing,
        syntax_error: None,
    })
}

/// How many workers analyze a selection of `files`; 1 means the calling thread.
pub(crate) fn workers(jobs: usize, files: usize) -> usize {
    if files < MIN_PARALLEL_FILES {
        1
    } else {
        jobs.clamp(1, files)
    }
}

/// Read, verify and analyze `paths` in order. `load` reads and verifies one file
/// and returns its report path and source; it always runs on the calling thread.
pub(crate) fn run(
    paths: &[PathBuf],
    jobs: usize,
    load: &mut dyn FnMut(&PathBuf) -> Result<(String, String)>,
    context: &FileContext<'_>,
    totals: &mut Totals,
) -> Result<()> {
    let jobs = workers(jobs, paths.len());
    if jobs > 1 {
        let limits = Limits {
            window: jobs.saturating_mul(WINDOW_PER_WORKER),
            bytes: MAX_IN_FLIGHT_BYTES,
        };
        parallel(paths, jobs, limits, load, context, totals).map(drop)
    } else {
        sequential(paths, load, context, totals)
    }
}

fn sequential(
    paths: &[PathBuf],
    load: &mut dyn FnMut(&PathBuf) -> Result<(String, String)>,
    context: &FileContext<'_>,
    totals: &mut Totals,
) -> Result<()> {
    let mut analyzer = analysis::Analyzer::default();
    for path in paths {
        let (relative, source) = load(path)?;
        totals.add(analyze_file(&mut analyzer, context, &relative, &source)?);
    }
    Ok(())
}

struct Job {
    index: usize,
    relative: String,
    source: String,
}

struct Done {
    index: usize,
    bytes: usize,
    outcome: thread::Result<Result<FileOutcome>>,
}

fn worker(jobs: &Mutex<Receiver<Job>>, results: &Sender<Done>, context: &FileContext<'_>) {
    let mut analyzer = analysis::Analyzer::default();
    loop {
        // Nothing panics while the lock is held, so it cannot be poisoned.
        let Ok(job) = jobs.lock().map(|jobs| jobs.recv()) else {
            return;
        };
        let Ok(Job {
            index,
            relative,
            source,
        }) = job
        else {
            return;
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            analyze_file(&mut analyzer, context, &relative, &source)
        }));
        if outcome.is_err() {
            // Never reuse a parser that was interrupted mid-parse.
            analyzer = analysis::Analyzer::default();
        }
        let bytes = source.len();
        drop(source);
        if results
            .send(Done {
                index,
                bytes,
                outcome,
            })
            .is_err()
        {
            return;
        }
    }
}

/// Whether the next file must wait for results before it is handed out.
fn must_wait(limits: Limits, handed_out: usize, in_flight_bytes: usize, size: usize) -> bool {
    handed_out >= limits.window
        || (in_flight_bytes > 0 && in_flight_bytes.saturating_add(size) > limits.bytes)
}

/// Results received from workers, merged into `totals` in file order.
struct Merge<'t> {
    totals: &'t mut Totals,
    pending: BTreeMap<usize, thread::Result<Result<FileOutcome>>>,
    /// The next file index to merge; every lower index is merged.
    merged: usize,
    in_flight_bytes: usize,
    received: usize,
    failed: bool,
}

impl Merge<'_> {
    fn receive(&mut self, results: &Receiver<Done>) -> Result<()> {
        let done = results
            .recv()
            .map_err(|_| anyhow!("analysis workers stopped unexpectedly"))?;
        self.received += 1;
        self.in_flight_bytes = self.in_flight_bytes.saturating_sub(done.bytes);
        self.failed |= !matches!(done.outcome, Ok(Ok(_)));
        self.pending.insert(done.index, done.outcome);
        // Stop at the first failure: it decides the scan unless a lower index fails.
        while let Some(entry) = self.pending.first_entry() {
            if *entry.key() != self.merged || !matches!(entry.get(), Ok(Ok(_))) {
                break;
            }
            if let Ok(Ok(outcome)) = entry.remove() {
                self.totals.add(outcome);
            }
            self.merged += 1;
        }
        Ok(())
    }
}

fn parallel(
    paths: &[PathBuf],
    jobs: usize,
    limits: Limits,
    load: &mut dyn FnMut(&PathBuf) -> Result<(String, String)>,
    context: &FileContext<'_>,
    totals: &mut Totals,
) -> Result<Peaks> {
    let (work, queue) = mpsc::sync_channel::<Job>(limits.window);
    let queue = Mutex::new(queue);
    let (sender, results) = mpsc::channel::<Done>();
    thread::scope(|scope| {
        let mut spawned: usize = 0;
        for index in 0..jobs {
            let (queue, sender) = (&queue, sender.clone());
            let started = thread::Builder::new()
                .name(format!("ts-crap-analysis-{index}"))
                .stack_size(WORKER_STACK_BYTES)
                .spawn_scoped(scope, move || worker(queue, &sender, context));
            // `Scope::spawn` would panic here; run with the workers that started.
            if started.is_err() {
                break;
            }
            spawned += 1;
        }
        drop(sender);
        // Size the window by the workers that actually started.
        let limits = Limits {
            window: limits.window.min(spawned.saturating_mul(WINDOW_PER_WORKER)),
            ..limits
        };
        if spawned == 0 {
            drop(work);
            return sequential(paths, load, context, totals).map(|()| Peaks::default());
        }
        let mut merge = Merge {
            totals,
            pending: BTreeMap::new(),
            merged: 0,
            in_flight_bytes: 0,
            received: 0,
            failed: false,
        };
        let mut sent = 0;
        let mut stopped = None;
        let mut peaks = Peaks::default();
        for (index, path) in paths.iter().enumerate() {
            if merge.failed {
                break;
            }
            // Read and verification errors keep their sequential order.
            let (relative, source) = match load(path) {
                Ok(loaded) => loaded,
                Err(error) => {
                    stopped = Some(error);
                    break;
                }
            };
            let size = source.len();
            while !merge.failed
                && must_wait(limits, sent - merge.merged, merge.in_flight_bytes, size)
            {
                merge.receive(&results)?;
            }
            if merge.failed {
                break;
            }
            work.send(Job {
                index,
                relative,
                source,
            })
            .map_err(|_| anyhow!("analysis workers stopped unexpectedly"))?;
            sent += 1;
            merge.in_flight_bytes += size;
            peaks.handed_out = peaks.handed_out.max(sent - merge.merged);
            peaks.in_flight_bytes = peaks.in_flight_bytes.max(merge.in_flight_bytes);
        }
        drop(work);
        while merge.received < sent {
            merge.receive(&results)?;
        }
        // Every file below `merged` succeeded; a pending entry there failed first.
        if let Some(outcome) = merge.pending.remove(&merge.merged) {
            match outcome {
                Ok(result) => result.map(drop)?,
                Err(payload) => resume_unwind(payload),
            }
        }
        stopped.map_or(Ok(peaks), Err)
    })
}

#[cfg(test)]
#[path = "pipeline/tests.rs"]
mod tests;
