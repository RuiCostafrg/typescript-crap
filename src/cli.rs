use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use ts_crap::{Format, ScanOptions};

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
    /// Report format; defaults to a readable table
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
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
    let args = Args::parse();
    if let Some(path) = &args.output {
        crate::output::validate(&args.root, path, args.coverage.as_deref())?;
    }
    let report = ts_crap::scan(&ScanOptions {
        root: args.root.clone(),
        paths: args.paths,
        threshold: args.threshold,
        coverage: args.coverage.clone(),
        diff: args.diff,
        pr: args.pr,
    })?;
    let rendered = ts_crap::render(&report, args.format)?;
    if let Some(path) = args.output {
        crate::output::write_report(&args.root, &path, args.coverage.as_deref(), &rendered)?;
    } else {
        use std::io::Write;
        std::io::stdout().lock().write_all(rendered.as_bytes())?;
    }
    Ok(report.failed)
}
