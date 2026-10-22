//! Versioned regression snapshots. Ambiguous matches are treated as new findings.
use crate::{Report, validate_threshold};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    version: u32,
    threshold: f64,
    functions: Vec<Entry>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    name: String,
    identity: String,
    crap: f64,
}
impl Baseline {
    pub fn capture(report: &Report) -> Self {
        Self {
            version: 1,
            threshold: report.threshold,
            functions: report
                .functions
                .iter()
                .map(|f| Entry {
                    path: f.path.clone(),
                    name: f.name.clone(),
                    identity: f.identity.clone(),
                    crap: f.crap,
                })
                .collect(),
        }
    }
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let baseline: Self = serde_json::from_slice(bytes)?;
        if baseline.version != 1 {
            bail!("unsupported baseline version");
        }
        validate_threshold(baseline.threshold)?;
        for f in &baseline.functions {
            validate_threshold(f.crap)?;
            if f.path.is_empty()
                || f.path.contains('\\')
                || f.path.contains(':')
                || f.path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                || f.identity.len() != 64
                || !f.identity.bytes().all(|b| b.is_ascii_hexdigit())
            {
                bail!("invalid baseline function identity or path");
            }
        }
        Ok(baseline)
    }
    pub fn render(&self) -> Result<String> {
        Ok(format!("{}\n", serde_json::to_string_pretty(self)?))
    }
    pub fn apply(&self, report: &mut Report, renames: &[String]) -> Result<()> {
        if self.threshold != report.threshold {
            bail!("baseline threshold differs from scan threshold");
        }
        let mut paths = BTreeMap::new();
        let mut destinations = BTreeSet::new();
        for rename in renames {
            let (old, new) = rename
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("baseline rename must be OLD=NEW"))?;
            if old.is_empty()
                || new.is_empty()
                || paths.insert(old, new).is_some()
                || !destinations.insert(new)
            {
                bail!("duplicate or empty baseline rename");
            }
        }
        let mut named = BTreeMap::<(&str, &str), Vec<usize>>::new();
        let mut fingerprints = BTreeMap::<&str, Vec<usize>>::new();
        for (index, f) in self.functions.iter().enumerate() {
            named
                .entry((
                    paths.get(f.path.as_str()).copied().unwrap_or(&f.path),
                    &f.name,
                ))
                .or_default()
                .push(index);
            fingerprints.entry(&f.identity).or_default().push(index);
        }
        let mut current_names = BTreeMap::new();
        let mut current_ids = BTreeMap::new();
        for f in &report.functions {
            *current_names
                .entry((f.path.clone(), f.name.clone()))
                .or_insert(0) += 1;
            *current_ids.entry(f.identity.clone()).or_insert(0) += 1;
        }
        let mut used = BTreeSet::new();
        for f in &mut report.functions {
            let by_name = named
                .get(&(f.path.as_str(), f.name.as_str()))
                .filter(|entries| {
                    entries.len() == 1 && current_names[&(f.path.clone(), f.name.clone())] == 1
                });
            let candidates = by_name.or_else(|| {
                fingerprints
                    .get(f.identity.as_str())
                    .filter(|entries| entries.len() == 1 && current_ids[&f.identity] == 1)
            });
            if let Some(entries) = candidates {
                let index = entries[0];
                if used.insert(index) {
                    f.failed = f.failed && f.crap > self.functions[index].crap;
                }
            }
        }
        report.failed = report.completeness_failed || report.functions.iter().any(|f| f.failed);
        Ok(())
    }
}
