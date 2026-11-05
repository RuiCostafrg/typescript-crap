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

/// What a scan does with a selected source that does not parse as TypeScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
#[non_exhaustive]
pub enum OnSyntaxError {
    /// Abort the whole scan with an error (exit 2). The default.
    #[default]
    Fail,
    /// Score every other file, list unparsable files in the report, and fail the
    /// completeness gate.
    Report,
}

/// Opt-in scan settings added after 2.0. Construct with `ScanExtensions::default()`
/// and set fields, so later additions stay compatible.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ScanExtensions {
    pub on_syntax_error: OnSyntaxError,
}

/// A selected source that could not be parsed, at its first syntax error.
#[derive(Debug, Serialize)]
pub struct UnparsedFile {
    pub path: String,
    pub line: usize,
    /// One-based UTF-8 byte column, like `FunctionScore::column`.
    pub column: usize,
    /// One-based UTF-16 column for SARIF consumers.
    pub column_utf16: usize,
    pub message: String,
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
    /// Explicit override; None inherits project configuration, then 10.
    pub threshold: Option<f64>,
    pub coverage: Option<PathBuf>,
    pub diff: Option<String>,
    pub pr: Option<String>,
    pub config: Option<PathBuf>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Explicit override; None inherits project configuration, then false.
    pub require_coverage: Option<bool>,
    /// Explicit override; None inherits project configuration, then false.
    pub fail_on_empty: Option<bool>,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            paths: Vec::new(),
            threshold: None,
            coverage: None,
            diff: None,
            pr: None,
            config: None,
            include: Vec::new(),
            exclude: Vec::new(),
            require_coverage: None,
            fail_on_empty: None,
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
    /// Unparsable selected sources (only with `OnSyntaxError::Report`). Each one
    /// fails the completeness gate. Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub syntax_errors: Vec<UnparsedFile>,
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
/// Empty selections pass unless the effective fail_on_empty policy is enabled.
pub fn scan(args: &ScanOptions) -> Result<Report> {
    scan_with(args, &ScanExtensions::default())
}

/// `scan` with opt-in settings; `scan_with(args, &ScanExtensions::default())` is `scan(args)`.
pub fn scan_with(args: &ScanOptions, extensions: &ScanExtensions) -> Result<Report> {
    if let Some(threshold) = args.threshold {
        validate_threshold(threshold)?;
    }
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
    let threshold = args.threshold.unwrap_or(rules.threshold);
    let require_coverage = args.require_coverage.unwrap_or(rules.require_coverage);
    let fail_on_empty = args.fail_on_empty.unwrap_or(rules.fail_on_empty);
    let selection = selection::files(
        &root,
        &args.paths,
        args.diff.as_deref(),
        args.pr.as_deref(),
        &rules.excluded_directories,
    )?;
    let mut files = selection.paths;
    let mut identities = std::collections::BTreeSet::new();
    let mut selected = Vec::new();
    for path in files {
        let relative = validation::source_path(path.strip_prefix(&root)?)?;
        if rules.matches(&relative) {
            if !identities.insert(relative) {
                bail!("duplicate source path identity");
            }
            selected.push(path);
        }
    }
    files = selected;
    let coverage = args
        .coverage
        .as_deref()
        .map(coverage::Coverage::load)
        .transpose()?;
    let mut functions = Vec::new();
    let mut files_missing_coverage = 0;
    let mut functions_missing_coverage = 0;
    let mut syntax_errors = Vec::new();
    let mut analyzer = analysis::Analyzer::default();
    let mut committed = if files.is_empty() || selection.committed.is_empty() {
        None
    } else {
        Some(selection::BlobReader::new(&root)?)
    };
    for path in &files {
        let relative = validation::source_path(path.strip_prefix(&root)?)?;
        let source =
            std::fs::read_to_string(path).with_context(|| format!("cannot read {relative}"))?;
        if let Some(reader) = &mut committed {
            let oid = selection
                .committed
                .get(path)
                .context("selected source has no committed identity")?;
            reader.verify(oid, path, &source)?;
        }
        let methods = match analyzer.analyze(&relative, &source) {
            Ok(methods) => methods,
            Err(error) if extensions.on_syntax_error == OnSyntaxError::Report => {
                let Some(issue) = error.downcast_ref::<analysis::SyntaxError>() else {
                    return Err(error);
                };
                // Reporting syntax errors must not hide stale or invalid coverage.
                let source_coverage = coverage
                    .as_ref()
                    .map(|coverage| coverage.for_source(&relative, &source))
                    .transpose()?;
                files_missing_coverage +=
                    usize::from(source_coverage.as_ref().is_none_or(|c| !c.present()));
                syntax_errors.push(UnparsedFile {
                    path: relative.clone(),
                    line: issue.line,
                    column: issue.column,
                    column_utf16: issue.column_utf16,
                    // Saved reports may be shared independently of repository access.
                    message: format!(
                        "invalid TypeScript syntax in {}:{}:{} (UTF-8 byte column)",
                        issue.path, issue.line, issue.column
                    ),
                });
                continue;
            }
            Err(error) => return Err(error),
        };
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
                failed: crap > threshold,
            });
        }
    }
    if let Some(reader) = &mut committed {
        reader.finish()?;
    }
    functions
        .sort_unstable_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
    // An unparsable file is unscored evidence: the scan is incomplete under any policy.
    let completeness_failed = (fail_on_empty && functions.is_empty())
        || (require_coverage && (files_missing_coverage > 0 || functions_missing_coverage > 0))
        || !syntax_errors.is_empty();
    let report = Report {
        version: 1,
        source_paths: files.clone(),
        threshold,
        files_scanned: files.len(),
        files_missing_coverage,
        functions_missing_coverage,
        completeness_failed,
        failed: completeness_failed || functions.iter().any(|f| f.failed),
        functions,
        syntax_errors,
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
    let mut bytes = Vec::new();
    render_to_with_options(report, format, options, &mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}

/// Stream a report. Writer failures may leave partial bytes in the supplied writer.
/// Analysis completes before rendering; use a staging file for atomic publication.
pub fn render_to<W: std::io::Write>(report: &Report, format: Format, writer: W) -> Result<()> {
    render_to_with_options(report, format, &RenderOptions::default(), writer)
}

/// Stream with presentation options, preserving the full report's gate and totals.
pub fn render_to_with_options<W: std::io::Write>(
    report: &Report,
    format: Format,
    options: &RenderOptions,
    writer: W,
) -> Result<()> {
    options.validate(format)?;
    report::render_to(report, format, options, writer)
}
