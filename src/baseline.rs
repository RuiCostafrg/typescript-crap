//! Versioned regression snapshots. Ambiguous matches are treated as new findings.
use crate::{FunctionScore, Report, validate_threshold};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{HashMap, HashSet};

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

// Borrowed mirrors of `Baseline` and `Entry`: same field names and order, so
// they serialize to the same bytes without copying the report's strings.
#[derive(Serialize)]
struct BaselineView<'a> {
    version: u32,
    threshold: f64,
    functions: FunctionsView<'a>,
}
struct FunctionsView<'a>(&'a [FunctionScore]);
#[derive(Serialize)]
struct EntryView<'a> {
    path: &'a str,
    name: &'a str,
    identity: &'a str,
    crap: f64,
}
impl Serialize for FunctionsView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|f| EntryView {
            path: &f.path,
            name: &f.name,
            identity: &f.identity,
            crap: f.crap,
        }))
    }
}

impl Baseline {
    /// Snapshot the scored functions, including from an incomplete report. Callers
    /// requiring every selected source to parse must check `report.syntax_errors`;
    /// the CLI does so before `--write-baseline`. Applying any snapshot preserves
    /// the report's completeness failure.
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
        let mut baseline: Self = serde_json::from_slice(bytes)?;
        if baseline.version != 1 {
            bail!("unsupported baseline version");
        }
        validate_threshold(baseline.threshold)?;
        for f in &mut baseline.functions {
            validate_threshold(f.crap)?;
            if !crate::validation::normalized_relative_path(&f.path)
                || !crate::validation::sha256_digest(&f.identity)
            {
                bail!("invalid baseline function identity or path");
            }
            f.identity.make_ascii_lowercase();
        }
        Ok(baseline)
    }
    pub fn render(&self) -> Result<String> {
        let mut bytes = Vec::new();
        self.render_to(&mut bytes)?;
        Ok(String::from_utf8(bytes)?)
    }
    /// Stream a snapshot; use a staged writer when partial output is unacceptable.
    pub fn render_to(&self, mut writer: impl std::io::Write) -> Result<()> {
        serde_json::to_writer_pretty(&mut writer, self)?;
        writer.write_all(b"\n")?;
        Ok(())
    }
    /// Stream the snapshot of `report`: the same bytes as
    /// `Baseline::capture(report).render_to(writer)`, without copying every
    /// function's path, name and identity first. Use a staged writer when
    /// partial output is unacceptable.
    pub fn render_report_to(report: &Report, mut writer: impl std::io::Write) -> Result<()> {
        let view = BaselineView {
            version: 1,
            threshold: report.threshold,
            functions: FunctionsView(&report.functions),
        };
        serde_json::to_writer_pretty(&mut writer, &view)?;
        writer.write_all(b"\n")?;
        Ok(())
    }
    /// Compare absolute scores anew; earlier comparisons do not compose.
    pub fn apply(&self, report: &mut Report, renames: &[String]) -> Result<()> {
        if self.threshold != report.threshold {
            bail!("baseline threshold differs from scan threshold");
        }
        // Lookup-only maps: their iteration order is never observed.
        let mut paths = HashMap::new();
        let mut destinations = HashSet::new();
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
        let mut named = HashMap::<(&str, &str), Vec<usize>>::new();
        let mut fingerprints = HashMap::<&str, Vec<usize>>::new();
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
        let mut current_names = HashMap::new();
        let mut current_ids = HashMap::new();
        for f in &report.functions {
            *current_names
                .entry((f.path.as_str(), f.name.as_str()))
                .or_insert(0) += 1;
            *current_ids.entry(f.identity.as_str()).or_insert(0) += 1;
        }
        let mut used = HashSet::new();
        let mut failures = Vec::with_capacity(report.functions.len());
        for f in &report.functions {
            let absolute_failed = f.crap > report.threshold;
            let mut failed = absolute_failed;
            let by_name = named
                .get(&(f.path.as_str(), f.name.as_str()))
                .filter(|entries| {
                    entries.len() == 1 && current_names[&(f.path.as_str(), f.name.as_str())] == 1
                });
            let candidates = by_name.or_else(|| {
                fingerprints.get(f.identity.as_str()).filter(|entries| {
                    entries.len() == 1
                        && current_ids[f.identity.as_str()] == 1
                        && stable_name(&self.functions[entries[0]].name) == stable_name(&f.name)
                })
            });
            if let Some(entries) = candidates {
                let index = entries[0];
                if used.insert(index) {
                    failed = absolute_failed && f.crap > self.functions[index].crap;
                }
            }
            failures.push(failed);
        }
        // Snapshots hold CRAP scores only and cannot suppress mutation findings.
        let mutation: Vec<_> = report.mutation.as_ref().map_or_else(
            || vec![false; report.functions.len()],
            |m| m.functions.iter().map(|f| f.failed).collect(),
        );
        for ((function, failed), mutation) in
            report.functions.iter_mut().zip(failures).zip(mutation)
        {
            function.failed = failed || mutation;
        }
        report.failed = report.completeness_failed || report.functions.iter().any(|f| f.failed);
        Ok(())
    }
}

#[cfg(test)]
mod tests;

// Anonymous locations are presentation labels, including inside qualified scopes.
// Keep named scope components so renaming a containing function stays conservative.
fn stable_name(name: &str) -> String {
    // Quoted/computed/assignment names can contain text resembling our labels.
    // Preserve them verbatim rather than normalize source spelling as metadata.
    if name.contains(['\"', '\'', '`', '[', ']']) {
        return name.to_owned();
    }
    name.split("::")
        .map(|part| {
            let anonymous = part
                .strip_prefix("<anonymous@")
                .and_then(|label| label.strip_suffix('>'))
                .and_then(|position| position.split_once(':'))
                .is_some_and(|(line, column)| {
                    !line.is_empty()
                        && !column.is_empty()
                        && line.bytes().all(|b| b.is_ascii_digit())
                        && column.bytes().all(|b| b.is_ascii_digit())
                });
            if anonymous { "<anonymous>" } else { part }
        })
        .collect::<Vec<_>>()
        .join("::")
}
