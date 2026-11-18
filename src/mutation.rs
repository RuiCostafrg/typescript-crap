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
    /// The function has mutants but none is scoreable: every one was
    /// inconclusive (timeout, invalid, ...) or an acknowledged equivalent
    /// mutant, which is excluded from the score.
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
    /// Survivors acknowledged as equivalent by the project configuration. They
    /// are not in `survived`, do not fail the gate and are outside `kill_ratio`.
    pub equivalent: usize,
    /// No test reached the mutant.
    pub no_coverage: usize,
    pub timeout: usize,
    pub invalid: usize,
    pub infrastructure_error: usize,
    pub resource_exhausted: usize,
    pub skipped: usize,
    /// `killed / (killed + survived + no_coverage)`, Oximut's own score
    /// definition (acknowledged equivalent mutants excluded); `None` unless
    /// `evidence` is `Scored`.
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
    /// How selected files were bound to the report: `oximut-source-sha256`
    /// when the report lists per-file hashes in `sources`, else the pinned
    /// `oximut-1.0.0-rc.1-mutant-id` reproduction.
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
    /// Selected files missing from the report's `sources` list: Oximut did not
    /// run on them. `None` when the report has no `sources` (Oximut 1.0.0-rc.1).
    pub files_not_in_report: Option<usize>,
    /// Functions whose CRAP score passes and whose mutation gate fails.
    pub functions_failed_by_mutation_only: usize,
    /// Mutants acknowledged as equivalent by configuration, over all functions.
    pub equivalent_acknowledged: usize,
    /// Configured equivalent entries that matched no mutant in this scan, in
    /// configuration order. They do not fail the scan: a report limited with
    /// `--mutators` legitimately lacks them.
    pub unmatched_equivalents: Vec<UnmatchedEquivalent>,
    pub functions: Vec<FunctionMutation>,
}

/// One reviewed acknowledgment from `ts-crap.json`: a surviving mutant that
/// cannot change behaviour. Matched by function identity and exact edit.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EquivalentEntry {
    pub path: String,
    pub identity: String,
    pub operator: String,
    pub original: String,
    pub replacement: String,
    pub reason: String,
}

/// An acknowledgment that matched nothing, as reported.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct UnmatchedEquivalent {
    pub path: String,
    pub identity: String,
    pub operator: String,
    pub original: String,
    pub replacement: String,
}

impl EquivalentEntry {
    /// Config-time validation; `identity` is normalized to lowercase.
    pub(crate) fn validate(&mut self) -> Result<()> {
        ensure!(
            crate::validation::normalized_relative_path(&self.path),
            "mutation.equivalent path must be a normalized root-relative path: {:?}",
            self.path
        );
        ensure!(
            crate::validation::sha256_digest(&self.identity),
            "mutation.equivalent identity must be 64 hexadecimal characters: {:?}",
            self.identity
        );
        ensure!(
            !self.reason.trim().is_empty(),
            "mutation.equivalent reason must not be blank (path {:?})",
            self.path
        );
        self.identity.make_ascii_lowercase();
        Ok(())
    }

    fn key(&self) -> [&str; 5] {
        [
            &self.path,
            &self.identity,
            &self.operator,
            &self.original,
            &self.replacement,
        ]
    }

    fn matches(&self, method: &Method, mutant: &Mutant) -> bool {
        self.identity == method.identity
            && self.operator == mutant.operator
            && self.original == mutant.original
            && self.replacement == mutant.replacement
    }
}

/// Validate a configured list: each entry, then duplicates.
pub(crate) fn validate_equivalents(entries: &mut [EquivalentEntry]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        entry.validate()?;
        ensure!(
            seen.insert(entry.key().map(str::to_owned)),
            "duplicate mutation.equivalent entry for {:?} {:?} {:?}",
            entry.path,
            entry.operator,
            entry.original
        );
    }
    Ok(())
}

/// The configured entries with a per-path lookup, built once per scan.
#[derive(Debug, Default)]
pub(crate) struct Equivalents {
    entries: Vec<EquivalentEntry>,
    by_path: BTreeMap<String, Vec<usize>>,
}

impl Equivalents {
    pub fn new(entries: Vec<EquivalentEntry>) -> Self {
        let mut by_path = BTreeMap::<String, Vec<usize>>::new();
        for (index, entry) in entries.iter().enumerate() {
            by_path.entry(entry.path.clone()).or_default().push(index);
        }
        Self { entries, by_path }
    }

    /// `(index, entry)` pairs for one file, in configuration order.
    pub fn for_path(&self, path: &str) -> Vec<(usize, &EquivalentEntry)> {
        self.by_path.get(path).map_or_else(Vec::new, |indexes| {
            indexes.iter().map(|&i| (i, &self.entries[i])).collect()
        })
    }

    pub fn unmatched(&self, matched: &BTreeSet<usize>) -> Vec<UnmatchedEquivalent> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !matched.contains(index))
            .map(|(_, e)| UnmatchedEquivalent {
                path: e.path.clone(),
                identity: e.identity.clone(),
                operator: e.operator.clone(),
                original: e.original.clone(),
                replacement: e.replacement.clone(),
            })
            .collect()
    }
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
    /// Per-file source hashes; absent from Oximut 1.0.0-rc.1 reports.
    #[serde(default)]
    sources: Option<Vec<SourceEntry>>,
    /// Machine-readable mutator selection; absent from 1.0.0-rc.1 reports.
    #[serde(default)]
    mutators: Option<Mutators>,
}

#[derive(Debug, Deserialize)]
struct SourceEntry {
    file: String,
    sha256: String,
    mutants: usize,
}

#[derive(Debug, Deserialize)]
struct Mutators {
    selection: Selection,
    families: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Selection {
    All,
    Subset,
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

/// Selected-file counts for `Mutation::summary`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectedFiles {
    pub total: usize,
    pub with_mutants: usize,
    /// Named by the report's `sources` list.
    pub listed: usize,
}

/// A loaded report. Mutants are grouped by root-relative source identity.
#[derive(Debug)]
pub(crate) struct Mutation {
    files: BTreeMap<String, Vec<Mutant>>,
    /// Lowercase SHA-256 per root-relative source the run read, when reported.
    hashes: Option<BTreeMap<String, String>>,
    total: usize,
    run_complete: bool,
    mutators: MutatorScope,
    mutator_families: Vec<String>,
}

/// Mutants of one source, validated against its current bytes.
#[derive(Debug)]
pub(crate) struct SourceMutants<'a> {
    mutants: &'a [Mutant],
    /// The report's `sources` lists this file (always false without `sources`).
    pub listed: bool,
    /// Zero-based (row, UTF-8 byte column) span per mutant, as Tree-sitter counts.
    spans: Vec<(Point, Point)>,
}

type Point = (usize, usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub killed: usize,
    pub survived: usize,
    /// Survivors acknowledged as equivalent; never also in `survived`.
    pub equivalent: usize,
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
    /// Scoreable mutants: acknowledged equivalent mutants are excluded.
    fn scored(&self) -> usize {
        self.killed + self.survived + self.no_coverage
    }
    pub fn total(&self) -> usize {
        self.scored()
            + self.equivalent
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
            equivalent: counts.equivalent,
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
        } else if families.iter().all(|family| valid_family(family)) {
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

fn valid_family(family: &str) -> bool {
    !family.is_empty()
        && family
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}

/// Oximut records canonical absolute paths; both tools must share the root.
fn relative_source(file: &str, root: &Path) -> Result<String> {
    Path::new(file)
        .strip_prefix(root)
        .ok()
        .and_then(|relative| crate::validation::source_path(relative).ok())
        .with_context(|| {
            format!(
                "mutant file {file:?} is not inside the project root {}: run Oximut and ts-crap on the same checkout with the same root",
                root.display()
            )
        })
}

/// Validates `sources` against the grouped mutants: every mutant's file is
/// listed once, with exactly as many mutants as the list claims.
fn source_hashes(
    sources: Vec<SourceEntry>,
    files: &BTreeMap<String, Vec<Mutant>>,
    root: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for source in sources {
        let relative = relative_source(&source.file, root)?;
        ensure!(
            crate::validation::sha256_digest(&source.sha256),
            "invalid Oximut source SHA256 for {relative}"
        );
        let found = files.get(&relative).map_or(0, Vec::len);
        ensure!(
            found == source.mutants,
            "Oximut sources entry for {relative} claims {} mutants, the report has {found}",
            source.mutants
        );
        ensure!(
            hashes
                .insert(relative.clone(), source.sha256.to_ascii_lowercase())
                .is_none(),
            "duplicate Oximut sources entry for {relative}"
        );
    }
    if let Some(file) = files.keys().find(|file| !hashes.contains_key(*file)) {
        bail!("Oximut report has mutants for {file} but no sources entry for it");
    }
    Ok(hashes)
}

fn rc1_id_matches(prefix: &Sha256, mutant: &Mutant) -> bool {
    let mut hash = prefix.clone();
    field(&mut hash, mutant.start.to_string().as_bytes());
    field(&mut hash, mutant.end.to_string().as_bytes());
    field(&mut hash, mutant.operator.as_bytes());
    field(&mut hash, mutant.replacement.as_bytes());
    hex::encode(hash.finalize()).eq_ignore_ascii_case(&mutant.id)
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
            let relative = relative_source(&entry.file, root)?;
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
            equivalent: 0,
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
        let hashes = document
            .sources
            .map(|sources| source_hashes(sources, &files, root))
            .transpose()?;
        let (mut mutators, mut mutator_families) = mutators(&document.diagnostics);
        if let Some(declared) = document.mutators {
            let families: BTreeSet<String> = declared.families.into_iter().collect();
            ensure!(
                families.iter().all(|family| valid_family(family)),
                "Oximut mutators field names an invalid family"
            );
            let declared = match declared.selection {
                Selection::All => (MutatorScope::All, Vec::new()),
                Selection::Subset => {
                    ensure!(
                        !families.is_empty(),
                        "Oximut mutators subset names no families"
                    );
                    (MutatorScope::Subset, families.into_iter().collect())
                }
            };
            // The field is authoritative; a contradicting diagnostic is corrupt.
            ensure!(
                mutators == MutatorScope::Unknown
                    || (mutators, &mutator_families) == (declared.0, &declared.1),
                "Oximut mutators field disagrees with its mutator families diagnostic"
            );
            (mutators, mutator_families) = declared;
        }
        Ok(Self {
            files,
            hashes,
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
        let expected = self
            .hashes
            .as_ref()
            .and_then(|hashes| hashes.get(relative_path));
        if let Some(expected) = expected {
            // Binds every byte, including files whose run produced no mutants.
            let actual = hex::encode(Sha256::digest(source.as_bytes()));
            ensure!(
                actual == *expected,
                "mutation report is stale for {relative_path}: Oximut source SHA256 mismatch; the source changed since the mutation run"
            );
        }
        // Without reported hashes, reproduce rc1's ids, which hash the whole
        // source. With them, ids stay opaque, as Oximut documents.
        // Hash the path and whole source once; each mutant extends a clone.
        let prefix = (self.hashes.is_none() && !mutants.is_empty()).then(|| {
            let mut prefix = Sha256::new();
            field(&mut prefix, relative_path.as_bytes());
            field(&mut prefix, source.as_bytes());
            prefix
        });
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
            if let Some(prefix) = &prefix
                && !rc1_id_matches(prefix, mutant)
            {
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
        Ok(SourceMutants {
            mutants,
            listed: expected.is_some(),
            spans,
        })
    }

    pub fn summary(
        &self,
        functions: Vec<FunctionMutation>,
        mutants_in_selection: usize,
        files: SelectedFiles,
        functions_failed_by_mutation_only: usize,
        unmatched_equivalents: Vec<UnmatchedEquivalent>,
    ) -> MutationEvidence {
        let SelectedFiles {
            total,
            with_mutants: files_with_mutants,
            listed,
        } = files;
        let mutants_in_functions = functions.iter().map(|f| f.mutants).sum::<usize>();
        MutationEvidence {
            tool: "oximut",
            source_binding: if self.hashes.is_some() {
                "oximut-source-sha256"
            } else {
                "oximut-1.0.0-rc.1-mutant-id"
            },
            report_schema_version: 1,
            run_complete: self.run_complete,
            mutators: self.mutators,
            mutator_families: self.mutator_families.clone(),
            mutants_in_functions,
            mutants_outside_functions: mutants_in_selection - mutants_in_functions,
            mutants_outside_selection: self.total - mutants_in_selection,
            files_with_mutants,
            files_without_mutants: total - files_with_mutants,
            files_not_in_report: self.hashes.as_ref().map(|_| total - listed),
            functions_failed_by_mutation_only,
            equivalent_acknowledged: functions.iter().map(|f| f.equivalent).sum(),
            unmatched_equivalents,
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
    ///
    /// A `survived` mutant matching one of `equivalents` (this file's entries,
    /// see `Equivalents::for_path`) on its owner's identity and exact edit
    /// counts as `equivalent` instead. Also returns the indexes of the entries
    /// that matched at least one mutant.
    pub fn assign(
        &self,
        methods: &[Method],
        equivalents: &[(usize, &EquivalentEntry)],
    ) -> (Vec<Counts>, BTreeSet<usize>) {
        let mut counts = vec![Counts::default(); methods.len()];
        let mut matched = BTreeSet::new();
        let mut open: Vec<usize> = Vec::new();
        // An iterator rather than an index: the cursor can only move forward.
        let mut pending = methods.iter().enumerate().peekable();
        for (mutant, &(start, end)) in self.mutants.iter().zip(&self.spans) {
            while let Some((index, method)) = pending.next_if(|(_, m)| m.start <= start) {
                while open.last().is_some_and(|&o| methods[o].end <= method.start) {
                    open.pop();
                }
                open.push(index);
            }
            while open.last().is_some_and(|&o| methods[o].end <= start) {
                open.pop();
            }
            if let Some(&owner) = open.iter().rev().find(|&&o| end <= methods[o].end) {
                // Only a survivor can be equivalent: untested code is not.
                let acknowledged = (mutant.outcome == Outcome::Survived)
                    .then(|| {
                        equivalents
                            .iter()
                            .filter(|(_, entry)| entry.matches(&methods[owner], mutant))
                            .map(|&(index, _)| index)
                            .collect::<Vec<_>>()
                    })
                    .filter(|indexes| !indexes.is_empty());
                match acknowledged {
                    Some(indexes) => {
                        counts[owner].equivalent += 1;
                        matched.extend(indexes);
                    }
                    None => counts[owner].add(mutant.outcome),
                }
            }
        }
        (counts, matched)
    }
}

#[cfg(test)]
mod tests;
