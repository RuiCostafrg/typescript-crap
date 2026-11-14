use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use ts_crap::{Format, OnSyntaxError, RenderOptions, ScanExtensions, ScanOptions, Sort};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Measure TypeScript function CRAP scores and enforce a quality gate"
)]
struct Args {
    /// Files/directories relative to root; omitted means the whole project
    paths: Vec<PathBuf>,
    /// Project root (all report paths are relative to this)
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Fail when any score is strictly greater than this value (config or 10)
    #[arg(long, value_parser = threshold)]
    threshold: Option<f64>,
    /// Source-hashed line coverage JSON; missing coverage is 0%
    #[arg(long)]
    coverage: Option<PathBuf>,
    /// Scan changed files from the merge-base of this ref to HEAD (committed changes)
    #[arg(long, conflicts_with = "pr")]
    diff: Option<String>,
    /// Current GitHub PR (or number/URL); requires gh and a checkout of its HEAD
    #[arg(long, num_args = 0..=1, default_missing_value = "", conflicts_with = "diff")]
    pr: Option<String>,
    /// Explicit config file (default: ROOT/ts-crap.json)
    #[arg(long)]
    config: Option<PathBuf>,
    /// Root-relative include globs; repeated flags replace configured includes
    #[arg(long)]
    include: Vec<String>,
    /// Root-relative exclude globs; repeated flags replace configured excludes
    #[arg(long)]
    exclude: Vec<String>,
    /// Require coverage evidence; use =false to override project policy
    #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true, action = clap::ArgAction::Set)]
    require_coverage: Option<bool>,
    /// Fail on an empty selection; use =false to override project policy
    #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true, action = clap::ArgAction::Set)]
    fail_on_empty: Option<bool>,
    /// Fail only for new or worsened violations relative to this snapshot
    #[arg(long, conflicts_with = "write_baseline")]
    baseline: Option<PathBuf>,
    /// Write an absolute-score baseline (gate exit code still applies)
    #[arg(long)]
    write_baseline: Option<PathBuf>,
    /// Explicit file move mapping for changed functions, OLD=NEW
    #[arg(long, requires = "baseline")]
    baseline_rename: Vec<String>,
    /// Oximut mutation report (schemaVersion 1) for the same sources and root;
    /// surviving or uncovered mutants fail a separate mutation gate
    #[arg(long)]
    mutation_report: Option<PathBuf>,
    /// Abort on a source that does not parse (fail), or score the rest, list it and
    /// fail the completeness gate (report)
    #[arg(long, value_enum, default_value = "fail")]
    on_syntax_error: OnSyntaxError,
    /// Report format; defaults to a readable table
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
    /// Show only failing functions (text/html); the gate still uses all functions
    #[arg(long)]
    only_failing: bool,
    /// Order text/html rows by location or descending CRAP score
    #[arg(long, value_enum, default_value = "path")]
    sort: Sort,
    /// Show at most N text/html rows, after filtering and sorting
    #[arg(long)]
    top: Option<usize>,
    /// Write report to this file rather than stdout (also written on gate failure)
    #[arg(long, short)]
    output: Option<PathBuf>,
}

fn threshold(raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .parse()
        .map_err(|_| "threshold must be a finite nonnegative number")?;
    ts_crap::validate_threshold(value).map_err(|error| error.to_string())?;
    Ok(value)
}

pub fn execute() -> Result<bool> {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    let presentation = RenderOptions {
        only_failing: args.only_failing,
        sort: args.sort,
        top: args.top,
    };
    presentation.validate(args.format)?;
    let default_config = args.root.join("ts-crap.json");
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| default_config.clone());
    let destinations = [&args.write_baseline, &args.output]
        .into_iter()
        .flatten()
        .map(PathBuf::as_path)
        .collect();
    let mut protected: Vec<_> = [&args.coverage, &args.baseline, &args.mutation_report]
        .into_iter()
        .flatten()
        .map(PathBuf::as_path)
        .collect();
    // Reserve both config locations even when absent: a successful publication
    // must not create an invalid config for a later default or explicit scan.
    protected.extend([default_config.as_path(), config_path.as_path()]);
    let publication = crate::output::DestinationPlan::new(&args.root, destinations, protected)?;
    let mut extensions = ScanExtensions::default();
    extensions.on_syntax_error = args.on_syntax_error;
    extensions.mutation_report = args.mutation_report.clone();
    let mut report = ts_crap::scan_with(
        &ScanOptions {
            root: args.root.clone(),
            paths: args.paths,
            threshold: args.threshold,
            coverage: args.coverage.clone(),
            diff: args.diff,
            pr: args.pr,
            config: args.config,
            include: args.include,
            exclude: args.exclude,
            require_coverage: args.require_coverage,
            fail_on_empty: args.fail_on_empty,
        },
        &extensions,
    )?;
    if let Some(path) = &args.baseline {
        let snapshot = ts_crap::baseline::Baseline::load(&std::fs::read(path)?)?;
        snapshot.apply(&mut report, &args.baseline_rename)?;
    }
    // A baseline must record every selected function; unparsable files have none.
    if args.write_baseline.is_some() && !report.syntax_errors.is_empty() {
        anyhow::bail!(
            "cannot write a baseline while {} selected files have syntax errors",
            report.syntax_errors.len()
        );
    }
    let write_baseline = args.write_baseline.is_some();
    // Stage stdout before publishing any baseline, preserving all-or-error rendering.
    let stdout_report = if args.output.is_none() {
        Some(StagedReport::render(&report, args.format, &presentation)?)
    } else {
        None
    };
    let count = usize::from(write_baseline) + usize::from(args.output.is_some());
    publication.publish_with(count, |index, writer| {
        if index == 0 && write_baseline {
            ts_crap::baseline::Baseline::render_report_to(&report, writer)
        } else {
            ts_crap::render_to_with_options(&report, args.format, &presentation, writer)
        }
    })?;
    if let Some(staged) = stdout_report {
        staged.copy_to_stdout()?;
    }
    Ok(report.failed)
}

/// A fully rendered stdout report, held in an anonymous temporary file so large
/// reports do not sit in memory. If the temporary file cannot be created or
/// written (say, a full temporary directory), the report is rendered in memory
/// instead; either way the bytes are the same, and a real rendering error still
/// fails because rendering is deterministic.
enum StagedReport {
    File(std::fs::File),
    Memory(String),
}

impl StagedReport {
    fn render(report: &ts_crap::Report, format: Format, options: &RenderOptions) -> Result<Self> {
        match Self::stage(report, format, options) {
            Ok(file) => Ok(Self::File(file)),
            Err(_) => Ok(Self::Memory(ts_crap::render_with_options(
                report, format, options,
            )?)),
        }
    }

    fn stage(
        report: &ts_crap::Report,
        format: Format,
        options: &RenderOptions,
    ) -> Result<std::fs::File> {
        use std::io::{Seek, SeekFrom};
        let mut writer = std::io::BufWriter::new(tempfile::tempfile()?);
        ts_crap::render_to_with_options(report, format, options, &mut writer)?;
        let mut file = writer.into_inner().map_err(|error| error.into_error())?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    fn copy_to_stdout(self) -> Result<()> {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        match self {
            Self::File(mut file) => {
                std::io::copy(&mut file, &mut stdout)?;
            }
            Self::Memory(rendered) => stdout.write_all(rendered.as_bytes())?,
        }
        stdout.flush()?;
        Ok(())
    }
}
