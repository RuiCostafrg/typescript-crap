//! TypeScript CRAP analysis. Filesystem/Git selection, source-bound coverage, and reports.
//! See the README for the scoring convention and coverage approximation.
mod analysis;
mod coverage;
mod report;
mod selection;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Format {
    Text,
    Json,
    Xml,
    Html,
}

/// Source selection and quality-gate settings. Input paths are root-relative;
/// coverage paths are relative to the caller's current directory.
#[derive(Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub threshold: f64,
    pub coverage: Option<PathBuf>,
    pub diff: Option<String>,
    pub pr: Option<String>,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            paths: Vec::new(),
            threshold: 10.0,
            coverage: None,
            diff: None,
            pr: None,
        }
    }
}

/// Reject non-finite or negative thresholds at both library and CLI seams.
pub fn validate_threshold(value: f64) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        bail!("threshold must be a finite nonnegative number");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct FunctionScore {
    pub path: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub complexity: u32,
    /// Fraction in [0, 1], not percentage
    pub coverage: f64,
    pub crap: f64,
    pub failed: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    #[serde(skip)]
    source_paths: Vec<PathBuf>,
    pub version: u32,
    pub threshold: f64,
    pub files_scanned: usize,
    pub failed: bool,
    pub functions: Vec<FunctionScore>,
}

impl Report {
    /// Whether an absolute, canonical path was scanned (including type-only files).
    /// Report writers must not overwrite these inputs.
    pub fn is_source_path(&self, path: &std::path::Path) -> bool {
        self.source_paths
            .binary_search_by(|candidate| candidate.as_path().cmp(path))
            .is_ok()
    }
}

/// Analyze selected files. No report is published and no repository is modified.
/// An empty selection is a passing report with zero functions.
pub fn scan(args: &ScanOptions) -> Result<Report> {
    validate_threshold(args.threshold)?;
    if args.diff.is_some() && args.pr.is_some() {
        bail!("--diff and --pr are mutually exclusive");
    }
    let root = args
        .root
        .canonicalize()
        .context("cannot resolve project root")?;
    if !root.is_dir() {
        bail!("project root is not a directory");
    }
    let files = selection::files(&root, &args.paths, args.diff.as_deref(), args.pr.as_deref())?;
    let coverage = args
        .coverage
        .as_deref()
        .map(coverage::Coverage::load)
        .transpose()?;
    let mut functions = Vec::new();
    for path in &files {
        let relative = path
            .strip_prefix(&root)?
            .to_str()
            .context("source path is not UTF-8")?
            .replace('\\', "/");
        let source =
            std::fs::read_to_string(path).with_context(|| format!("cannot read {relative}"))?;
        let methods = analysis::analyze(&relative, &source)?;
        // Validate coverage even for selected files with no implemented functions.
        let source_coverage = coverage
            .as_ref()
            .map(|cov| cov.for_source(&relative, &source))
            .transpose()?;
        for method in methods {
            let cov = match &source_coverage {
                Some(c) => c.fraction(&method.lines)?,
                None => 0.0,
            };
            let cc = f64::from(method.complexity);
            let crap = cc * cc * (1.0 - cov).powi(3) + cc;
            functions.push(FunctionScore {
                path: relative.clone(),
                name: method.name,
                line: method.line,
                column: method.column,
                end_line: method.end_line,
                complexity: method.complexity,
                coverage: cov,
                crap,
                failed: crap > args.threshold,
            });
        }
    }
    functions
        .sort_unstable_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
    let report = Report {
        version: 1,
        source_paths: files.clone(),
        threshold: args.threshold,
        files_scanned: files.len(),
        failed: functions.iter().any(|f| f.failed),
        functions,
    };
    Ok(report)
}

/// Render a report without changing files or writing to stdout.
pub fn render(report: &Report, format: Format) -> Result<String> {
    report::render(report, format)
}
