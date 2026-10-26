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
            Err(error) if config.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
                Config {
                    version: 1,
                    include: vec![],
                    exclude: vec![],
                    threshold: default_threshold(),
                    require_coverage: false,
                    fail_on_empty: false,
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
        Ok(Self {
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
