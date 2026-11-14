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
