use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default = "default_threshold")]
    threshold: f64,
    #[serde(default)]
    require_coverage: bool,
    #[serde(default)]
    fail_on_empty: bool,
    /// Directory names skipped anywhere under the root; replaces the default list.
    #[serde(default)]
    excluded_directories: Option<Vec<String>>,
}

/// Directory names skipped during source selection unless configuration replaces them.
pub const DEFAULT_EXCLUDED_DIRECTORIES: [&str; 6] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "coverage",
];

/// Each entry names one directory (a single path component), at most once.
fn excluded_directories(names: Option<Vec<String>>) -> Result<Vec<String>> {
    let Some(names) = names else {
        return Ok(DEFAULT_EXCLUDED_DIRECTORIES.map(String::from).to_vec());
    };
    let mut seen = std::collections::BTreeSet::new();
    for name in &names {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
            || name.contains('\0')
        {
            bail!("excluded_directories entries must be single directory names: {name:?}");
        }
        if !seen.insert(name.as_str()) {
            bail!("duplicate excluded_directories entry: {name:?}");
        }
    }
    Ok(names)
}

fn default_threshold() -> f64 {
    10.0
}

#[derive(Debug)]
pub struct Rules {
    include: GlobSet,
    exclude: GlobSet,
    pub threshold: f64,
    pub require_coverage: bool,
    pub fail_on_empty: bool,
    pub excluded_directories: Vec<String>,
}
fn compile(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.is_empty()
            || pattern.contains('\\')
            || pattern.starts_with('/')
            || pattern.split('/').any(|part| part == "..")
        {
            bail!("selection patterns must be nonempty root-relative forward-slash globs");
        }
        builder.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
    }
    Ok(builder.build()?)
}
impl Rules {
    pub fn load(
        root: &Path,
        config: Option<&PathBuf>,
        include: &[String],
        exclude: &[String],
    ) -> Result<Self> {
        let path = config.cloned().unwrap_or_else(|| root.join("ts-crap.json"));
        let mut settings = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<Config>(&bytes).context("invalid project configuration")?
            }
            // A dangling policy link is a present but unreadable configuration,
            // not an absent default file that may safely use built-in settings.
            Err(error)
                if config.is_none()
                    && error.kind() == std::io::ErrorKind::NotFound
                    && path
                        .symlink_metadata()
                        .is_err_and(|entry| entry.kind() == std::io::ErrorKind::NotFound) =>
            {
                Config {
                    version: 1,
                    include: vec![],
                    exclude: vec![],
                    threshold: default_threshold(),
                    require_coverage: false,
                    fail_on_empty: false,
                    excluded_directories: None,
                }
            }
            Err(error) => return Err(error).context("cannot read project configuration"),
        };
        if settings.version != 1 {
            bail!("unsupported project configuration version");
        }
        crate::validate_threshold(settings.threshold).context("invalid project configuration")?;
        // A nonempty CLI list replaces that configuration list; exclusions always win.
        if !include.is_empty() {
            settings.include = include.to_vec();
        }
        if !exclude.is_empty() {
            settings.exclude = exclude.to_vec();
        }
        let excluded_directories = excluded_directories(settings.excluded_directories)
            .context("invalid project configuration")?;
        Ok(Self {
            excluded_directories,
            include: compile(&settings.include)?,
            exclude: compile(&settings.exclude)?,
            threshold: settings.threshold,
            require_coverage: settings.require_coverage,
            fail_on_empty: settings.fail_on_empty,
        })
    }
    pub fn matches(&self, path: &str) -> bool {
        (self.include.is_empty() || self.include.is_match(path)) && !self.exclude.is_match(path)
    }
}
