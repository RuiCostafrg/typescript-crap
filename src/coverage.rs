//! Native source-bound statement coverage; validate once per file, not per method.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, de::Error};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    version: u32,
    files: BTreeMap<String, FileCoverage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCoverage {
    sha256: String,
    #[serde(deserialize_with = "deserialize_lines")]
    lines: BTreeMap<usize, u64>,
}

fn deserialize_lines<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<usize, u64>, D::Error> {
    let raw = BTreeMap::<String, u64>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(line, hits)| {
            let number = line.parse::<usize>().map_err(D::Error::custom)?;
            if number == 0 || number.to_string() != line {
                return Err(D::Error::custom(
                    "coverage line must be a positive canonical integer",
                ));
            }
            Ok((number, hits))
        })
        .collect()
}

/// A view bound to one validated source snapshot. Cannot outlive the loaded report.
#[derive(Debug)]
pub struct SourceCoverage<'a> {
    file: Option<&'a FileCoverage>,
    line_count: usize,
}

impl Coverage {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("reading coverage {}", path.display()))?;
        let report: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid coverage JSON in {}", path.display()))?;
        if report.version != 1 {
            bail!(
                "unsupported coverage version {} (expected 1)",
                report.version
            );
        }
        for (path, file) in &report.files {
            if path.is_empty()
                || path.contains('\\')
                || path.contains(':')
                || path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                bail!("coverage path must be a normalized relative path: {path:?}");
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("invalid SHA256 for {path}");
            }
        }
        Ok(report)
    }

    pub fn for_source(&self, relative_path: &str, source: &str) -> Result<SourceCoverage<'_>> {
        let file = self.files.get(relative_path);
        let line_count = source.lines().count();
        if let Some(file) = file {
            let actual_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
            if !actual_hash.eq_ignore_ascii_case(&file.sha256) {
                bail!(
                    "coverage SHA256 mismatch for {relative_path}: source changed since coverage was collected"
                );
            }
            if let Some((&number, _)) = file.lines.last_key_value()
                && number > line_count
            {
                bail!(
                    "coverage line {number} is outside source {relative_path} ({line_count} lines)"
                );
            }
        }
        Ok(SourceCoverage { file, line_count })
    }
}

impl SourceCoverage<'_> {
    pub fn present(&self) -> bool {
        self.file.is_some()
    }
    pub fn complete(&self, lines: &BTreeSet<usize>) -> bool {
        self.file
            .is_some_and(|file| lines.iter().all(|line| file.lines.contains_key(line)))
    }
    pub fn fraction(&self, owned_lines: &BTreeSet<usize>) -> Result<f64> {
        if owned_lines
            .iter()
            .any(|&line| line == 0 || line > self.line_count)
        {
            bail!("owned statement line is outside source");
        }
        let Some(file) = self.file else {
            return Ok(0.0);
        };
        if owned_lines.is_empty() {
            return Ok(0.0);
        }
        let covered = owned_lines
            .iter()
            .filter(|line| file.lines.get(line).copied().unwrap_or(0) > 0)
            .count();
        Ok(covered as f64 / owned_lines.len() as f64)
    }
}

#[cfg(test)]
mod tests;
