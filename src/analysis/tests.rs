#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn parsers_reuse_both_grammars_without_leaking_previous_trees() {
    isolated(
        "analysis::tests::parsers_reuse_both_grammars_without_leaking_previous_trees",
        || {
            let mut analyzer = super::Analyzer::default();
            let state = format!("{analyzer:?}");
            assert!(state.contains("typescript_initialized: false"));
            assert!(state.contains("tsx_initialized: false"));
            for _ in 0..3 {
                let jsx = analyzer
                    .analyze("view.tsx", "export const View = () => <div>😀</div>;")
                    .unwrap();
                assert_eq!(jsx.len(), 1);
                assert_eq!(jsx[0].name, "View");
                assert_eq!(jsx[0].complexity, 1);
                assert!(format!("{analyzer:?}").contains("tsx_initialized: true"));
                let ts = analyzer
                    .analyze(
                        "generic.mts",
                        "export const id = <T>(value: T): T => value;",
                    )
                    .unwrap();
                assert_eq!(ts.len(), 1);
                assert_eq!(ts[0].name, "id");
                assert!(format!("{analyzer:?}").contains("typescript_initialized: true"));
                assert!(analyzer.analyze("bad.ts", "const broken = ;").is_err());
                assert!(
                    analyzer
                        .analyze("empty.cts", "export type Empty = string;")
                        .unwrap()
                        .is_empty()
                );
                assert!(analyzer.analyze("bad.tsx", "const bad = <div>;").is_err());
            }
        },
    );
}

#[test]
fn syntax_diagnostics_include_location_bounded_snippet_and_missing_tokens() {
    isolated(
        "analysis::tests::syntax_diagnostics_include_location_bounded_snippet_and_missing_tokens",
        || {
            let mut analyzer = super::Analyzer::default();
            let error = analyzer
                .analyze("a.ts", "// heading\nconst broken = ;\n")
                .unwrap_err()
                .to_string();
            assert!(error.contains("a.ts:2:"), "{error}");
            assert!(error.contains("const broken = ;"));
            assert!(error.contains("UTF-8 byte column"));
            for (path, source) in [
                ("missing.ts", "function f() {"),
                ("empty.tsx", "const f = <div>"),
            ] {
                let error = analyzer.analyze(path, source).unwrap_err().to_string();
                assert!(error.contains(&format!("{path}:1:")), "{error}");
            }
            let source = format!("const invalid = ;{}", "x".repeat(1000));
            let error = analyzer
                .analyze("long.ts", &source)
                .unwrap_err()
                .to_string();
            assert!(error.len() < 300);
        },
    );
}
#[test]
fn traversal_finishes_and_preserves_sibling_and_parent_ownership() {
    isolated(
        "analysis::tests::traversal_finishes_and_preserves_sibling_and_parent_ownership",
        || {
            let methods = super::Analyzer::default().analyze("a.ts", "function outer(a:boolean){function inner(){return 0;} if(a)return 1;return 2;} function sibling(){return 3;}").unwrap();
            assert_eq!(
                methods
                    .iter()
                    .map(|f| (f.name.as_str(), f.complexity))
                    .collect::<Vec<_>>(),
                vec![("outer", 2), ("outer::inner", 1), ("sibling", 1)]
            );
        },
    );
}

// Every analysis test must be isolated: a looping traversal in another test
// thread would otherwise prevent the runner from reporting a watchdog failure.
fn isolated(test_name: &str, test: impl FnOnce()) {
    const CHILD: &str = "TS_CRAP_ANALYSIS_TEST_CHILD";
    if std::env::var(CHILD).is_ok_and(|name| name == test_name) {
        test();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD, test_name)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("AST traversal exceeded deadline; possible cursor cycle");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn owned_identity_and_metrics_survive_trivia_and_nested_edits() {
    isolated(
        "analysis::tests::owned_identity_and_metrics_survive_trivia_and_nested_edits",
        || {
            for (path, source) in [
                (
                    "a.ts",
                    "const mark = '😀'; function café(x = mark ? 1 : 0) { function inner() { return 1; } if (x) return 2; return 3; }",
                ),
                (
                    "view.tsx",
                    "const mark = '😀'; const View = (x = mark ? 1 : 0) => { const inner = () => <b>😀</b>; return x ? <div/> : <span/>; };",
                ),
            ] {
                let mut analyzer = super::Analyzer::default();
                let original = analyzer.analyze(path, source).unwrap();
                let trivia = source
                    .replace("return", "/* ü trivia */ return")
                    .replace("{", "{\n");
                let shifted = analyzer.analyze(path, &trivia).unwrap();
                assert_eq!(original.len(), shifted.len());
                for (before, after) in original.iter().zip(&shifted) {
                    assert_eq!(
                        (&before.name, &before.identity, before.complexity),
                        (&after.name, &after.identity, after.complexity)
                    );
                }
                assert!(original[0].column > original[0].column_utf16);
                let edited = source
                    .replace("return 1;", "if (true) return 7; return 8;")
                    .replace("<b>😀</b>", "(x ? <i/> : <em/>)");
                let nested = analyzer.analyze(path, &edited).unwrap();
                assert_eq!(original[0].identity, nested[0].identity);
                assert_eq!(original[0].complexity, nested[0].complexity);
                assert_eq!(original[0].lines, nested[0].lines);
                assert_ne!(original[1].identity, nested[1].identity);
                assert!(nested[1].complexity > original[1].complexity);
            }
            let deep = format!("{}return 1;{}", "function f() {".repeat(64), "}".repeat(64));
            let methods = super::Analyzer::default()
                .analyze("deep.ts", &deep)
                .unwrap();
            assert_eq!(methods.len(), 64);
            assert!(methods.iter().all(|method| method.complexity == 1));
            assert_eq!(methods.last().unwrap().name, vec!["f"; 64].join("::"));
        },
    );
}
