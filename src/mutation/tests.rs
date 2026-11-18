#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;

fn diagnostics(entries: &[&str]) -> (MutatorScope, Vec<String>) {
    mutators(&entries.iter().map(|&e| e.to_owned()).collect::<Vec<_>>())
}

#[test]
fn mutator_scope_comes_only_from_agreeing_readable_diagnostics() {
    assert_eq!(diagnostics(&[]), (MutatorScope::Unknown, vec![]));
    assert_eq!(
        diagnostics(&["execution=fresh", "mutator families=all"]),
        (MutatorScope::All, vec![])
    );
    assert_eq!(
        diagnostics(&[
            "mutator families=boolean-literal,arithmetic; exclusions are not part of this run"
        ]),
        (
            MutatorScope::Subset,
            vec!["arithmetic".to_owned(), "boolean-literal".to_owned()]
        )
    );
    // A retried run repeats its diagnostics; identical entries still agree.
    assert_eq!(
        diagnostics(&["mutator families=all", "mutator families=all"]).0,
        MutatorScope::All
    );
    for unreadable in [
        &["mutator families=all", "mutator families=arithmetic"][..],
        &["mutator families="],
        &["mutator families=arithmetic,,boolean-literal"],
        &["mutator families=All Of Them"],
    ] {
        assert_eq!(diagnostics(unreadable), (MutatorScope::Unknown, vec![]));
    }
    // Only a leading marker names the filter.
    assert_eq!(
        diagnostics(&["note: mutator families=all"]).0,
        MutatorScope::Unknown
    );
}

#[test]
fn kill_ratio_follows_the_oximut_denominator() {
    let mut counts = Counts::default();
    assert_eq!((counts.total(), counts.kill_ratio()), (0, None));
    for outcome in [
        Outcome::Invalid,
        Outcome::Timeout,
        Outcome::InfrastructureError,
        Outcome::ResourceExhausted,
        Outcome::Skipped,
    ] {
        counts.add(outcome);
    }
    assert_eq!(counts.total(), 5);
    assert_eq!(counts.kill_ratio(), None);
    counts.add(Outcome::Killed);
    assert_eq!(counts.kill_ratio(), Some(1.0));
    counts.add(Outcome::Survived);
    counts.add(Outcome::NoCoverage);
    counts.add(Outcome::NoCoverage);
    assert_eq!(
        (
            counts.killed,
            counts.survived,
            counts.no_coverage,
            counts.total()
        ),
        (1, 1, 2, 9)
    );
    assert_eq!(counts.kill_ratio(), Some(0.25));
}

fn method(identity: &str, start: usize, end: usize) -> Method {
    Method {
        identity: identity.to_owned(),
        name: String::new(),
        line: 1,
        column: 1,
        column_utf16: 1,
        end_line: 1,
        start: (0, start),
        end: (0, end),
        complexity: 1,
        lines: Vec::new(),
    }
}

fn mutant(outcome: Outcome, at: usize) -> Mutant {
    Mutant {
        id: String::new(),
        operator: "op".into(),
        original: "a".into(),
        replacement: "b".into(),
        start: at,
        end: at + 1,
        outcome,
        start_position: (1, 1),
        end_position: (1, 1),
    }
}

fn entry(identity: &str, replacement: &str) -> EquivalentEntry {
    EquivalentEntry {
        path: "a.ts".into(),
        identity: identity.into(),
        operator: "op".into(),
        original: "a".into(),
        replacement: replacement.into(),
        reason: "r".into(),
    }
}

#[test]
fn only_survivors_with_the_owner_identity_and_exact_edit_are_equivalent() {
    let mutants = [
        mutant(Outcome::Survived, 1),
        mutant(Outcome::Survived, 2),
        mutant(Outcome::NoCoverage, 3),
        mutant(Outcome::Killed, 4),
        mutant(Outcome::Timeout, 5),
    ];
    let spans = (1..=5).map(|i| ((0, i), (0, i + 1))).collect();
    let source = SourceMutants {
        mutants: &mutants,
        spans,
    };
    let methods = [method("id", 0, 10)];
    let (hit, other_id, other_edit) = (entry("id", "b"), entry("zz", "b"), entry("id", "c"));
    let (counts, matched) = source.assign(&methods, &[(0, &hit), (1, &other_id), (2, &other_edit)]);
    // One entry acknowledges both identical survivors; nothing else is touched.
    assert_eq!(matched.into_iter().collect::<Vec<_>>(), [0]);
    let c = counts[0];
    assert_eq!(
        (c.equivalent, c.survived, c.no_coverage, c.killed, c.timeout),
        (2, 0, 1, 1, 1)
    );
    assert_eq!(c.total(), 5);
    // Equivalent mutants leave the score: 1 killed of (killed + no_coverage).
    assert_eq!(c.kill_ratio(), Some(0.5));
    // No entries means no behaviour change.
    let (plain, none) = source.assign(&methods, &[]);
    assert!(none.is_empty());
    assert_eq!((plain[0].survived, plain[0].equivalent), (2, 0));
    // Equivalent-only (plus inconclusive) functions have nothing to score.
    let only = Counts {
        equivalent: 2,
        timeout: 1,
        ..Counts::default()
    };
    assert_eq!(only.kill_ratio(), None);
    assert_eq!(only.total(), 3);
}
