use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use ts_crap::{Format, RenderOptions, ScanOptions, Sort};

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
    /// Fail when any score is strictly greater than this value
    #[arg(long, default_value = "10", value_parser = threshold)]
    threshold: f64,
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
    /// Fail if selected files or function statements lack coverage evidence
    #[arg(long)]
    require_coverage: bool,
    /// Fail if the selection contains no implemented functions
    #[arg(long)]
    fail_on_empty: bool,
    /// Fail only for new or worsened violations relative to this snapshot
    #[arg(long, conflicts_with = "write_baseline")]
    baseline: Option<PathBuf>,
    /// Write an absolute-score baseline (gate exit code still applies)
    #[arg(long)]
    write_baseline: Option<PathBuf>,
    /// Explicit file move mapping for changed functions, OLD=NEW
    #[arg(long, requires = "baseline")]
    baseline_rename: Vec<String>,
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
    if !value.is_finite() || value < 0.0 {
        return Err("threshold must be a finite nonnegative number".into());
    }
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
    if let Some(path) = &args.output {
        crate::output::validate(&args.root, path, args.coverage.as_deref())?;
    }
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| args.root.join("ts-crap.json"));
    if config_path.exists() {
        for destination in [&args.output, &args.write_baseline].into_iter().flatten() {
            crate::output::validate(&args.root, destination, Some(&config_path))?;
        }
    }
    let mut report = ts_crap::scan(&ScanOptions {
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
    })?;
    if let Some(path) = &args.baseline {
        if let Some(output) = &args.output {
            crate::output::validate(&args.root, output, Some(path))?;
        }
        let snapshot = ts_crap::baseline::Baseline::load(&std::fs::read(path)?)?;
        snapshot.apply(&mut report, &args.baseline_rename)?;
    }
    if let Some(path) = &args.write_baseline {
        if let Some(output) = &args.output
            && output == path
        {
            anyhow::bail!("baseline and report destinations must differ");
        }
        crate::output::write_report(
            &args.root,
            path,
            args.coverage.as_deref(),
            &ts_crap::baseline::Baseline::capture(&report).render()?,
        )?;
    }
    if let (Some(path), Some(output)) = (&args.write_baseline, &args.output) {
        crate::output::validate(&args.root, output, Some(path))?;
    }
    let rendered = ts_crap::render_with_options(&report, args.format, &presentation)?;
    if let Some(path) = args.output {
        crate::output::write_report(&args.root, &path, args.coverage.as_deref(), &rendered)?;
    } else {
        use std::io::Write;
        std::io::stdout().lock().write_all(rendered.as_bytes())?;
    }
    Ok(report.failed)
}
