//! Oximut mutation-testing evidence, bound to the current source like coverage:
//! the report is validated once at load and once per selected file.
//! See docs/MUTATION_EVIDENCE.md for the decisions recorded here.
use crate::analysis::Method;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Which mutator families the Oximut run generated, as far as its report says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MutatorScope {
    /// Every family the producing Oximut version declares.
    All,
    /// Only `mutator_families`; other families were not run. Partial evidence.
    Subset,
    /// The report does not say. Treated as partial evidence.
    Unknown,
}

/// How much mutation evidence a scored function has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EvidenceKind {
    /// No mutant lies in the function: its line-coverage result stands alone.
    None,
    /// Every mutant in the function was inconclusive (timeout, invalid, ...).
    Inconclusive,
    /// At least one mutant was killed, survived or had no coverage.
    Scored,
}

/// Mutant outcomes for one scored function. `MutationEvidence::functions` has
/// one entry per `Report::functions` entry, in the same order.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct FunctionMutation {
    pub path: String,
    pub name: String,
    pub line: usize,
    /// One-based UTF-8 byte column, like `FunctionScore::column`.
    pub column: usize,
    pub identity: String,
    pub evidence: EvidenceKind,
    /// All mutants the function owns, whatever their outcome.
    pub mutants: usize,
    pub killed: usize,
    /// Tests ran the mutated code and still passed.
    pub survived: usize,
    /// No test reached the mutant.
    pub no_coverage: usize,
    pub timeout: usize,
    pub invalid: usize,
    pub infrastructure_error: usize,
    pub resource_exhausted: usize,
    pub skipped: usize,
    /// `killed / (killed + survived + no_coverage)`, Oximut's own score
    /// definition; `None` unless `evidence` is `Scored`.
    pub kill_ratio: Option<f64>,
    /// `min(coverage, kill_ratio)`: mutation evidence never raises coverage.
    pub effective_coverage: Option<f64>,
    /// CRAP at `effective_coverage`; never below the function's `crap`.
    pub effective_crap: Option<f64>,
    /// At least one mutant survived or had no coverage; independent of CRAP.
    pub failed: bool,
}

/// Mutation evidence attached with `ScanExtensions::mutation_report`.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct MutationEvidence {
    pub tool: &'static str,
    /// Pinned implementation binding, not an upstream stable source-hash field.
    pub source_binding: &'static str,
    pub report_schema_version: u32,
    /// Oximut's `summary.complete`. An incomplete run has inconclusive mutants.
    pub run_complete: bool,
    pub mutators: MutatorScope,
    /// Families named by a `subset` report, sorted; empty otherwise.
    pub mutator_families: Vec<String>,
    pub mutants_in_functions: usize,
    /// Mutants in selected files that no scored function owns.
    pub mutants_outside_functions: usize,
    /// Mutants in files this scan did not select. Not validated or used.
    pub mutants_outside_selection: usize,
    pub files_with_mutants: usize,
    /// Selected files the report has no mutants for: no evidence either way.
    pub files_without_mutants: usize,
    /// Functions whose CRAP score passes and whose mutation gate fails.
    pub functions_failed_by_mutation_only: usize,
    pub functions: Vec<FunctionMutation>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Killed,
    Survived,
    NoCoverage,
    Invalid,
    Timeout,
    InfrastructureError,
    ResourceExhausted,
    Skipped,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Probe {
    schema_version: Option<serde_json::Value>,
}

// Oximut asks consumers to ignore unknown fields, so only consumed fields are
// named. Serde still rejects a duplicate of any named field.
#[derive(Debug, Deserialize)]
struct Document {
    #[serde(rename = "runId")]
    _run_id: String,
    #[serde(rename = "cache")]
    _cache: BTreeMap<String, serde_json::Value>,
    #[serde(rename = "resources")]
    _resources: BTreeMap<String, serde_json::Value>,
    baseline: Baseline,
    mutants: Vec<Entry>,
    diagnostics: Vec<String>,
    summary: Summary,
}

#[derive(Debug, Deserialize)]
struct Baseline {
    outcome: String,
    #[serde(rename = "tests")]
    _tests: Tests,
    #[serde(rename = "errors")]
    _errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Tests {
    #[serde(rename = "passed")]
    _passed: usize,
    #[serde(rename = "failed")]
    _failed: usize,
    #[serde(rename = "skipped")]
    _skipped: usize,
}

#[derive(Debug, Deserialize)]
struct Entry {
    id: String,
    file: String,
    operator: String,
    original: String,
    replacement: String,
    location: Location,
    outcome: Outcome,
    #[serde(rename = "tests")]
    _tests: Tests,
    #[serde(rename = "errors")]
    _errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Location {
    start: Position,
    end: Position,
}

#[derive(Debug, Deserialize)]
struct Position {
    line: usize,
    column: usize,
    /// Zero-based UTF-8 byte offset.
    offset: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    total: usize,
    killed: usize,
    survived: usize,
    no_coverage: usize,
    invalid: usize,
    timeout: usize,
    infrastructure_error: usize,
    resource_exhausted: usize,
    skipped: usize,
    complete: bool,
    score_denominator: usize,
    score: serde_json::Value,
}

#[derive(Debug)]
struct Mutant {
    id: String,
    operator: String,
    original: String,
    replacement: String,
    start: usize,
    end: usize,
    outcome: Outcome,
    start_position: (usize, usize),
    end_position: (usize, usize),
}

/// A loaded report. Mutants are grouped by root-relative source identity.
#[derive(Debug)]
pub(crate) struct Mutation {
    files: BTreeMap<String, Vec<Mutant>>,
    total: usize,
    run_complete: bool,
    mutators: MutatorScope,
    mutator_families: Vec<String>,
}

/// Mutants of one source, validated against its current bytes.
#[derive(Debug)]
pub(crate) struct SourceMutants<'a> {
    mutants: &'a [Mutant],
    /// Zero-based (row, UTF-8 byte column) span per mutant, as Tree-sitter counts.
    spans: Vec<(Point, Point)>,
}

type Point = (usize, usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub killed: usize,
    pub survived: usize,
    pub no_coverage: usize,
    pub invalid: usize,
    pub timeout: usize,
    pub infrastructure_error: usize,
    pub resource_exhausted: usize,
    pub skipped: usize,
}

impl Counts {
    fn add(&mut self, outcome: Outcome) {
        *match outcome {
            Outcome::Killed => &mut self.killed,
            Outcome::Survived => &mut self.survived,
            Outcome::NoCoverage => &mut self.no_coverage,
            Outcome::Invalid => &mut self.invalid,
            Outcome::Timeout => &mut self.timeout,
            Outcome::InfrastructureError => &mut self.infrastructure_error,
            Outcome::ResourceExhausted => &mut self.resource_exhausted,
            Outcome::Skipped => &mut self.skipped,
        } += 1;
    }
    fn scored(&self) -> usize {
        self.killed + self.survived + self.no_coverage
    }
    pub fn total(&self) -> usize {
        self.scored()
            + self.invalid
            + self.timeout
            + self.infrastructure_error
            + self.resource_exhausted
            + self.skipped
    }
    /// Oximut's score definition applied to one function. Inconclusive
    /// outcomes count neither for nor against, as in Oximut's own score.
    pub fn kill_ratio(&self) -> Option<f64> {
        let scored = self.scored();
        (scored > 0).then(|| self.killed as f64 / scored as f64)
    }
}

/// CRAP with the documented formula; shared so both scores stay identical.
pub(crate) fn crap(complexity: u32, coverage: f64) -> f64 {
    let cc = f64::from(complexity);
    cc * cc * (1.0 - coverage).powi(3) + cc
}

impl FunctionMutation {
    pub(crate) fn new(method: &Method, path: &str, counts: Counts, coverage: f64) -> Self {
        let kill_ratio = counts.kill_ratio();
        // A kill is not a line hit: evidence can only lower coverage.
        let effective_coverage = kill_ratio.map(|ratio| coverage.min(ratio));
        let effective_crap = effective_coverage.map(|value| crap(method.complexity, value));
        Self {
            path: path.to_owned(),
            name: method.name.clone(),
            line: method.line,
            column: method.column,
            identity: method.identity.clone(),
            evidence: match (counts.total(), kill_ratio) {
                (0, _) => EvidenceKind::None,
                (_, None) => EvidenceKind::Inconclusive,
                (_, Some(_)) => EvidenceKind::Scored,
            },
            mutants: counts.total(),
            killed: counts.killed,
            survived: counts.survived,
            no_coverage: counts.no_coverage,
            timeout: counts.timeout,
            invalid: counts.invalid,
            infrastructure_error: counts.infrastructure_error,
            resource_exhausted: counts.resource_exhausted,
            skipped: counts.skipped,
            kill_ratio,
            effective_coverage,
            effective_crap,
            failed: counts.survived > 0 || counts.no_coverage > 0,
        }
    }
}

const FAMILIES: &str = "mutator families=";

/// The run's family filter exists only as diagnostic text. Disagreeing or
/// unreadable entries are unknown rather than a guess.
fn mutators(diagnostics: &[String]) -> (MutatorScope, Vec<String>) {
    let mut found: Option<(MutatorScope, Vec<String>)> = None;
    for value in diagnostics.iter().filter_map(|d| d.strip_prefix(FAMILIES)) {
        let list = value.split(';').next().unwrap_or_default().trim();
        let families: BTreeSet<&str> = list.split(',').map(str::trim).collect();
        let current = if list == "all" {
            (MutatorScope::All, Vec::new())
        } else if families.iter().all(|family| {
            !family.is_empty()
                && family
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        }) {
            (
                MutatorScope::Subset,
                families.into_iter().map(str::to_owned).collect(),
            )
        } else {
            return (MutatorScope::Unknown, Vec::new());
        };
        if found.as_ref().is_some_and(|previous| *previous != current) {
            return (MutatorScope::Unknown, Vec::new());
        }
        found = Some(current);
    }
    found.unwrap_or((MutatorScope::Unknown, Vec::new()))
}

fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

/// Converts monotonically increasing byte offsets to Tree-sitter points, which
/// count rows at `\n` and columns in UTF-8 bytes.
#[derive(Clone, Copy, Debug, Default)]
struct Cursor {
    row: usize,
    line_start: usize,
    scanned: usize,
    report_row: usize,
    report_column: usize,
    previous_cr: bool,
}

impl Cursor {
    fn at(&mut self, source: &str, offset: usize) -> Point {
        for (index, ch) in source[self.scanned..offset].char_indices() {
            // Ownership follows Tree-sitter's LF rows and UTF-8 byte columns.
            if ch == '\n' {
                self.row += 1;
                self.line_start = self.scanned + index + 1;
            }
            // Oximut positions follow JavaScript line separators and UTF-16.
            if ch == '\n' && self.previous_cr {
                self.previous_cr = false;
                continue;
            }
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                self.report_row += 1;
                self.report_column = 0;
            } else {
                self.report_column += ch.len_utf16();
            }
            self.previous_cr = ch == '\r';
        }
        self.scanned = offset;
        (self.row, offset - self.line_start)
    }

    fn reported(&self) -> Point {
        (self.report_row + 1, self.report_column + 1)
    }
}

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

impl Mutation {
    /// `root` is the canonical project root. Oximut records canonical absolute
    /// paths, so both tools must have run on this checkout with this root.
    pub fn load(path: &Path, root: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading mutation report {}", path.display()))?;
        let invalid = || format!("invalid Oximut report JSON in {}", path.display());
        // Check the version before interpreting anything else, as Oximut requires.
        let probe: Probe = serde_json::from_slice(&bytes).with_context(invalid)?;
        match probe.schema_version {
            Some(version) if version.as_u64() == Some(1) => {}
            Some(version) => {
                bail!("unsupported Oximut report schemaVersion {version} (expected 1)")
            }
            None => bail!(
                "{} is not an Oximut report: schemaVersion is missing",
                path.display()
            ),
        }
        let document: Document = serde_json::from_slice(&bytes).with_context(invalid)?;
        ensure!(
            document.baseline.outcome == "passed",
            "Oximut baseline outcome is {:?}: mutant results are evidence only when the unmutated tests passed",
            document.baseline.outcome
        );
        let mut tally = Counts::default();
        let mut ids = BTreeSet::new();
        let mut files = BTreeMap::<String, Vec<Mutant>>::new();
        let total = document.mutants.len();
        for entry in document.mutants {
            tally.add(entry.outcome);
            let relative = Path::new(&entry.file)
                .strip_prefix(root)
                .ok()
                .and_then(|relative| crate::validation::source_path(relative).ok())
                .with_context(|| {
                    format!(
                        "mutant file {:?} is not inside the project root {}: run Oximut and ts-crap on the same checkout with the same root",
                        entry.file,
                        root.display()
                    )
                })?;
            ensure!(
                entry.location.start.offset <= entry.location.end.offset,
                "mutant {} in {relative} has an inverted location",
                short(&entry.id)
            );
            ensure!(
                entry.location.start.line > 0
                    && entry.location.start.column > 0
                    && entry.location.end.line > 0
                    && entry.location.end.column > 0,
                "Oximut mutant {} in {relative} must have one-based line and column",
                short(&entry.id)
            );
            ensure!(
                ids.insert(entry.id.clone()),
                "duplicate mutant id {} in Oximut report",
                short(&entry.id)
            );
            ensure!(
                crate::validation::sha256_digest(&entry.id),
                "invalid Oximut mutant id"
            );
            files.entry(relative).or_default().push(Mutant {
                start_position: (entry.location.start.line, entry.location.start.column),
                end_position: (entry.location.end.line, entry.location.end.column),
                id: entry.id,
                operator: entry.operator,
                original: entry.original,
                replacement: entry.replacement,
                start: entry.location.start.offset,
                end: entry.location.end.offset,
                outcome: entry.outcome,
            });
        }
        let s = &document.summary;
        let expected = Counts {
            killed: s.killed,
            survived: s.survived,
            no_coverage: s.no_coverage,
            invalid: s.invalid,
            timeout: s.timeout,
            infrastructure_error: s.infrastructure_error,
            resource_exhausted: s.resource_exhausted,
            skipped: s.skipped,
        };
        // A truncated or edited mutant list must not pass as the run's evidence.
        ensure!(
            s.total == total && expected == tally,
            "Oximut report summary does not match its mutant entries"
        );
        ensure!(
            s.score_denominator == tally.scored(),
            "Oximut score denominator does not match its mutants"
        );
        let score_valid = match tally.kill_ratio() {
            Some(ratio) => s
                .score
                .as_f64()
                .is_some_and(|score| (score - 100.0 * ratio).abs() < 1e-9),
            None => s.score.is_null(),
        };
        ensure!(score_valid, "Oximut score does not match its mutants");
        ensure!(
            !s.complete || tally.total() == tally.scored(),
            "Oximut complete summary contains inconclusive mutants"
        );
        // Report order varies with scheduling; ownership needs source order.
        for mutants in files.values_mut() {
            mutants.sort_by(|a, b| (a.start, a.end, &a.id).cmp(&(b.start, b.end, &b.id)));
        }
        let (mutators, mutator_families) = mutators(&document.diagnostics);
        Ok(Self {
            files,
            total,
            run_complete: s.complete,
            mutators,
            mutator_families,
        })
    }

    /// Reject evidence that does not describe exactly these source bytes.
    pub fn for_source(&self, relative_path: &str, source: &str) -> Result<SourceMutants<'_>> {
        let mutants = self
            .files
            .get(relative_path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        // Hash the path and whole source once; each mutant extends a clone.
        let mut prefix = Sha256::new();
        if !mutants.is_empty() {
            field(&mut prefix, relative_path.as_bytes());
            field(&mut prefix, source.as_bytes());
        }
        let mut spans = Vec::with_capacity(mutants.len());
        let mut cursor = Cursor::default();
        for mutant in mutants {
            // Documented contract: each mutant's byte range and original text.
            if source.get(mutant.start..mutant.end) != Some(mutant.original.as_str()) {
                bail!(
                    "mutation report is stale for {relative_path}: mutant {} expects {:?} at bytes {}..{}; the source changed since the mutation run",
                    short(&mutant.id),
                    mutant.original,
                    mutant.start,
                    mutant.end
                );
            }
            // Oximut 1.0.0-rc.1 derives each id from the root-relative path, the
            // whole source and the edit, which binds every other byte of the file.
            let mut hash = prefix.clone();
            field(&mut hash, mutant.start.to_string().as_bytes());
            field(&mut hash, mutant.end.to_string().as_bytes());
            field(&mut hash, mutant.operator.as_bytes());
            field(&mut hash, mutant.replacement.as_bytes());
            if !hex::encode(hash.finalize()).eq_ignore_ascii_case(&mutant.id) {
                bail!(
                    "mutation report does not match {relative_path}: mutant {} was not generated from the current source. The source changed since the mutation run, Oximut ran with a different --root, or this Oximut version derives mutant ids differently from 1.0.0-rc.1",
                    short(&mutant.id)
                );
            }
            let start = cursor.at(source, mutant.start);
            // Later mutants may start inside this one; resume from its start.
            let mut end_cursor = cursor;
            let end = end_cursor.at(source, mutant.end);
            for (expected, reported) in [
                (cursor.reported(), mutant.start_position),
                (end_cursor.reported(), mutant.end_position),
            ] {
                ensure!(
                    reported == expected,
                    "Oximut location disagrees with byte offsets in {relative_path}: mutant {} reports {reported:?}, expected {expected:?} (line, UTF-16 column)",
                    short(&mutant.id)
                );
            }
            spans.push((start, end));
        }
        Ok(SourceMutants { mutants, spans })
    }

    pub fn summary(
        &self,
        functions: Vec<FunctionMutation>,
        mutants_in_selection: usize,
        files_with_mutants: usize,
        files_without_mutants: usize,
        functions_failed_by_mutation_only: usize,
    ) -> MutationEvidence {
        let mutants_in_functions = functions.iter().map(|f| f.mutants).sum::<usize>();
        MutationEvidence {
            tool: "oximut",
            source_binding: "oximut-1.0.0-rc.1-mutant-id",
            report_schema_version: 1,
            run_complete: self.run_complete,
            mutators: self.mutators,
            mutator_families: self.mutator_families.clone(),
            mutants_in_functions,
            mutants_outside_functions: mutants_in_selection - mutants_in_functions,
            mutants_outside_selection: self.total - mutants_in_selection,
            files_with_mutants,
            files_without_mutants,
            functions_failed_by_mutation_only,
            functions,
        }
    }
}

impl SourceMutants<'_> {
    pub fn len(&self) -> usize {
        self.mutants.len()
    }

    /// Each mutant belongs to the innermost function whose node contains its
    /// whole edit, matching CRAP ownership: nested functions are scored
    /// separately. `methods` must be in the analyzer's preorder.
    pub fn assign(&self, methods: &[Method]) -> Vec<Counts> {
        let mut counts = vec![Counts::default(); methods.len()];
        let mut open: Vec<usize> = Vec::new();
        let mut next = 0;
        for (mutant, &(start, end)) in self.mutants.iter().zip(&self.spans) {
            while next < methods.len() && methods[next].start <= start {
                while open
                    .last()
                    .is_some_and(|&index| methods[index].end <= methods[next].start)
                {
                    open.pop();
                }
                open.push(next);
                next += 1;
            }
            while open
                .last()
                .is_some_and(|&index| methods[index].end <= start)
            {
                open.pop();
            }
            if let Some(&owner) = open.iter().rev().find(|&&index| end <= methods[index].end) {
                counts[owner].add(mutant.outcome);
            }
        }
        counts
    }
}

#[cfg(test)]
mod tests;
