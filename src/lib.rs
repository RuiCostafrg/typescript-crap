//! TypeScript CRAP analysis. Filesystem/Git selection, source-bound coverage, and reports.
//! See the README for the scoring convention and coverage approximation.
mod analysis;
pub mod baseline;
mod config;
mod coverage;
mod report;
pub use report::terminal as escape_terminal;
mod selection;
mod validation;

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
    Sarif,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    #[default]
    Path,
    Crap,
}

/// Presentation only: the full report always determines the gate and baseline.
#[derive(Debug, Default)]
pub struct RenderOptions {
    pub only_failing: bool,
    pub sort: Sort,
    pub top: Option<usize>,
}

impl RenderOptions {
    pub fn validate(&self, format: Format) -> Result<()> {
        if self.top == Some(0) {
            bail!("--top must be greater than zero");
        }
        if (self.only_failing || self.sort != Sort::Path || self.top.is_some())
            && !matches!(format, Format::Text | Format::Html)
        {
            bail!("report navigation options require text or html format");
        }
        Ok(())
    }
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
    pub config: Option<PathBuf>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub require_coverage: bool,
    pub fail_on_empty: bool,
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
            config: None,
            include: Vec::new(),
            exclude: Vec::new(),
            require_coverage: false,
            fail_on_empty: false,
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
    pub identity: String,
    pub path: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    /// One-based UTF-16 column for SARIF consumers (column remains byte based).
    pub column_utf16: usize,
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
    pub files_missing_coverage: usize,
    pub functions_missing_coverage: usize,
    pub completeness_failed: bool,
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
    let rules = config::Rules::load(&root, args.config.as_ref(), &args.include, &args.exclude)?;
    let mut files = selection::files(&root, &args.paths, args.diff.as_deref(), args.pr.as_deref())?;
    files.retain(|path| {
        rules.matches(
            &path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
        )
    });
    let coverage = args
        .coverage
        .as_deref()
        .map(coverage::Coverage::load)
        .transpose()?;
    let mut functions = Vec::new();
    let mut files_missing_coverage = 0;
    let mut functions_missing_coverage = 0;
    let mut analyzer = analysis::Analyzer::default();
    for path in &files {
        let relative = path
            .strip_prefix(&root)?
            .to_str()
            .context("source path is not UTF-8")?
            .replace('\\', "/");
        let source =
            std::fs::read_to_string(path).with_context(|| format!("cannot read {relative}"))?;
        let methods = analyzer.analyze(&relative, &source)?;
        // Validate coverage even for selected files with no implemented functions.
        let source_coverage = coverage
            .as_ref()
            .map(|cov| cov.for_source(&relative, &source))
            .transpose()?;
        let missing_file = source_coverage.as_ref().is_none_or(|c| !c.present());
        files_missing_coverage += usize::from(missing_file);
        for method in methods {
            let evidence = source_coverage
                .as_ref()
                .map(|coverage| coverage.evidence(&method.lines))
                .transpose()?
                .unwrap_or_default();
            functions_missing_coverage += usize::from(!evidence.complete);
            let cov = evidence.fraction;
            let cc = f64::from(method.complexity);
            let crap = cc * cc * (1.0 - cov).powi(3) + cc;
            functions.push(FunctionScore {
                identity: method.identity,
                path: relative.clone(),
                name: method.name,
                line: method.line,
                column: method.column,
                column_utf16: method.column_utf16,
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
    let completeness_failed = (args.fail_on_empty && functions.is_empty())
        || (args.require_coverage
            && (files_missing_coverage > 0 || functions_missing_coverage > 0));
    let report = Report {
        version: 1,
        source_paths: files.clone(),
        threshold: args.threshold,
        files_scanned: files.len(),
        files_missing_coverage,
        functions_missing_coverage,
        completeness_failed,
        failed: completeness_failed || functions.iter().any(|f| f.failed),
        functions,
    };
    Ok(report)
}

/// Render a report without changing files or writing to stdout.
pub fn render(report: &Report, format: Format) -> Result<String> {
    render_with_options(report, format, &RenderOptions::default())
}

/// Filter/order human-readable rows without changing gate decisions or totals.
pub fn render_with_options(
    report: &Report,
    format: Format,
    options: &RenderOptions,
) -> Result<String> {
    options.validate(format)?;
    report::render(report, format, options)
}
