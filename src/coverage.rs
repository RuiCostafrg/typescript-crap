//! Native source-bound statement coverage; validate once per file, not per method.
use anyhow::{Context, Result, bail};
use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    fmt,
    marker::PhantomData,
    path::Path,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    version: u32,
    #[serde(deserialize_with = "deserialize_files")]
    files: BTreeMap<String, FileCoverage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCoverage {
    sha256: String,
    #[serde(deserialize_with = "deserialize_lines")]
    lines: BTreeMap<usize, u64>,
}

// Serde map targets keep only the last entry of a JSON object, so contradictory
// duplicates such as {"1":0,"1":1} must be rejected while entries are streamed.
struct UniqueMap<K, V> {
    label: &'static str,
    key: fn(String) -> Result<K, String>,
    value: PhantomData<fn() -> V>,
}

impl<'de, K: Ord + fmt::Display, V: Deserialize<'de>> Visitor<'de> for UniqueMap<K, V> {
    type Value = BTreeMap<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a coverage {} map with unique keys", self.label)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = BTreeMap::new();
        while let Some(raw) = access.next_key::<String>()? {
            let key = (self.key)(raw).map_err(A::Error::custom)?;
            match map.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(access.next_value()?);
                }
                Entry::Occupied(entry) => {
                    // Stop before deserializing a duplicate's potentially large
                    // value; never replace evidence already read from the map.
                    return Err(A::Error::custom(format_args!(
                        "duplicate coverage {}: {}",
                        self.label,
                        entry.key()
                    )));
                }
            }
        }
        Ok(map)
    }
}

fn deserialize_unique<'de, D, K, V>(
    deserializer: D,
    label: &'static str,
    key: fn(String) -> Result<K, String>,
) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Ord + fmt::Display,
    V: Deserialize<'de>,
{
    deserializer.deserialize_map(UniqueMap {
        label,
        key,
        value: PhantomData,
    })
}

fn deserialize_lines<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<usize, u64>, D::Error> {
    deserialize_unique(deserializer, "line", |line| {
        let number = line.parse::<usize>().map_err(|error| error.to_string())?;
        if number == 0 || number.to_string() != line {
            return Err("coverage line must be a positive canonical integer".into());
        }
        Ok(number)
    })
}

fn deserialize_files<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, FileCoverage>, D::Error> {
    deserialize_unique(deserializer, "file path", Ok)
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
            if !crate::validation::normalized_relative_path(path) {
                bail!("coverage path must be a normalized relative path: {path:?}");
            }
            if !crate::validation::sha256_digest(&file.sha256) {
                bail!("invalid SHA256 for {path}");
            }
        }
        Ok(report)
    }

    pub fn for_source(&self, relative_path: &str, source: &str) -> Result<SourceCoverage<'_>> {
        let file = self.files.get(relative_path);
        let line_count = source.lines().count();
        if let Some(file) = file {
            let actual_hash = hex::encode(Sha256::digest(source.as_bytes()));
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
    pub fn evidence(&self, owned_lines: &BTreeSet<usize>) -> Result<Evidence> {
        let mut complete = self.present();
        let mut covered = 0;
        for &line in owned_lines {
            if line == 0 || line > self.line_count {
                bail!("owned statement line is outside source");
            }
            match self.file.and_then(|file| file.lines.get(&line)) {
                Some(&hits) => covered += usize::from(hits > 0),
                None => complete = false,
            }
        }
        Ok(Evidence {
            complete,
            fraction: if owned_lines.is_empty() {
                0.0
            } else {
                covered as f64 / owned_lines.len() as f64
            },
        })
    }
}

/// Missing reports/files are incomplete even when no statements are owned.
#[derive(Debug, Default)]
pub(crate) struct Evidence {
    pub complete: bool,
    pub fraction: f64,
}

#[cfg(test)]
mod tests;
