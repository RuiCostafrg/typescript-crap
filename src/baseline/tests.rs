#![allow(clippy::unwrap_used)]
use super::Baseline;
use crate::{FunctionScore, Report};

fn report(entries: &[(&str, &str, u8, f64)]) -> Report {
    Report {
        version: 1,
        source_paths: Vec::new(),
        threshold: 10.0,
        files_scanned: 1,
        failed: entries.iter().any(|entry| entry.3 > 10.0),
        files_missing_coverage: 0,
        functions_missing_coverage: 0,
        completeness_failed: false,
        functions: entries
            .iter()
            .map(|&(path, name, identity, crap)| FunctionScore {
                path: path.into(),
                name: name.into(),
                identity: format!("{identity:064x}"),
                line: 1,
                column: 1,
                column_utf16: 1,
                end_line: 1,
                complexity: 1,
                coverage: 0.0,
                crap,
                failed: crap > 10.0,
            })
            .collect(),
    }
}

#[test]
fn unique_entry_reordering_preserves_decisions_and_completeness_gate() {
    let before = report(&[("a.ts", "a", 1, 40.0), ("b.ts", "b", 2, 20.0)]);
    for reverse in [false, true] {
        let mut baseline = Baseline::capture(&before);
        if reverse {
            baseline.functions.reverse();
        }
        let mut after = report(&[
            ("b.ts", "b", 2, 30.0),
            ("a.ts", "a", 1, 30.0),
            ("new.ts", "new", 3, 12.0),
        ]);
        if reverse {
            after.functions.reverse();
        }
        Baseline::load(baseline.render().unwrap().as_bytes())
            .unwrap()
            .apply(&mut after, &[])
            .unwrap();
        for function in &after.functions {
            assert_eq!(function.failed, function.name != "a");
        }
        assert!(after.failed);
    }
    let baseline = Baseline::capture(&before);
    let mut after = report(&[("a.ts", "a", 1, 30.0)]);
    after.completeness_failed = true;
    baseline.apply(&mut after, &[]).unwrap();
    assert!(!after.functions[0].failed);
    assert!(after.failed);
}

#[test]
fn ambiguous_names_and_fingerprints_cannot_suppress_new_violations() {
    for (old, current) in [
        (
            vec![("a.ts", "a", 1, 40.0); 2],
            vec![("a.ts", "a", 1, 20.0)],
        ),
        (
            vec![("a.ts", "a", 1, 40.0), ("b.ts", "b", 1, 40.0)],
            vec![("c.ts", "c", 1, 20.0)],
        ),
        (
            vec![("a.ts", "a", 1, 40.0)],
            vec![("b.ts", "b", 1, 20.0), ("c.ts", "c", 1, 20.0)],
        ),
        (
            vec![("a.ts", "a", 1, 40.0)],
            vec![("a.ts", "a", 1, 20.0); 2],
        ),
    ] {
        let mut after = report(&current);
        Baseline::capture(&report(&old))
            .apply(&mut after, &[])
            .unwrap();
        assert!(after.failed);
        assert!(after.functions.iter().all(|function| function.failed));
    }
}

#[test]
fn load_normalizes_digest_case_so_renamed_functions_still_match() {
    let before = report(&[("a.ts", "same", 0xab, 40.0)]);
    let mut baseline = Baseline::capture(&before);
    baseline.functions[0].identity.make_ascii_uppercase();
    assert_ne!(baseline.functions[0].identity, before.functions[0].identity);
    let baseline = Baseline::load(baseline.render().unwrap().as_bytes()).unwrap();
    assert_eq!(baseline.functions[0].identity, before.functions[0].identity);
    // No rename mapping: the changed path must match through its fingerprint.
    let mut after = report(&[("b.ts", "same", 0xab, 20.0)]);
    baseline.apply(&mut after, &[]).unwrap();
    assert!(!after.functions[0].failed);
    assert!(!after.failed);

    let mut changed = report(&[("b.ts", "same", 0xcd, 20.0)]);
    baseline.apply(&mut changed, &[]).unwrap();
    assert!(changed.functions[0].failed);
    assert!(changed.failed);
}

#[test]
fn invalid_threshold_or_rename_plan_fails_without_mutating_report() {
    let before = report(&[("a.ts", "a", 1, 40.0)]);
    let mut baseline = Baseline::capture(&before);
    for renames in [
        vec!["missing"],
        vec!["=a"],
        vec!["a="],
        vec!["a=x", "a=y"],
        vec!["a=x", "b=x"],
    ] {
        let mut after = report(&[("a.ts", "a", 1, 20.0)]);
        let original = serde_json::to_string(&after).unwrap();
        let renames: Vec<_> = renames.into_iter().map(str::to_owned).collect();
        assert!(baseline.apply(&mut after, &renames).is_err());
        assert_eq!(serde_json::to_string(&after).unwrap(), original);
    }
    baseline.threshold = 11.0;
    let mut after = report(&[("a.ts", "a", 1, 20.0)]);
    let original = serde_json::to_string(&after).unwrap();
    assert!(baseline.apply(&mut after, &[]).is_err());
    assert_eq!(serde_json::to_string(&after).unwrap(), original);
}

#[test]
fn baseline_application_is_independent_of_prior_decisions() {
    for completeness in [false, true] {
        let baselines =
            [40.0, 2.0, 12.0].map(|score| Baseline::capture(&report(&[("a.ts", "a", 1, score)])));
        let mut reused = report(&[("a.ts", "a", 1, 12.0)]);
        reused.completeness_failed = completeness;
        for index in [0, 1, 2, 1, 0, 0, 1] {
            let mut fresh = report(&[("a.ts", "a", 1, 12.0)]);
            fresh.completeness_failed = completeness;
            baselines[index].apply(&mut reused, &[]).unwrap();
            baselines[index].apply(&mut fresh, &[]).unwrap();
            assert_eq!(
                serde_json::to_value(&reused).unwrap(),
                serde_json::to_value(&fresh).unwrap()
            );
        }
    }
}

#[test]
fn anonymous_labels_normalize_only_generated_positions() {
    for name in [
        "<anonymous@2:3>",
        "scope::<anonymous@2:3>",
        "<anonymous@1:2>::<anonymous@3:4>",
    ] {
        assert_ne!(super::stable_name(name), name);
    }
    for name in [
        "'<anonymous@2:3>'",
        "[key]::<anonymous@2:3>",
        "<anonymous@x:y>",
        "<anonymous@2:>",
        "<anonymous@2:3",
        "prefix<anonymous@2:3>",
    ] {
        assert_eq!(super::stable_name(name), name);
    }
}
