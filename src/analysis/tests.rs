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

// The kind-string and `Node::parent` naming that production replaced with kind
// tables and an ancestor stack, kept as an independent reference.
fn reference_local_name(node: tree_sitter::Node<'_>, source: &str) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return super::text(name, source).into();
    }
    if let Some(parent) = node.parent() {
        match parent.kind() {
            "variable_declarator" | "public_field_definition" | "pair" => {
                if let Some(name) = parent
                    .child_by_field_name("name")
                    .or_else(|| parent.child_by_field_name("key"))
                {
                    return super::text(name, source).into();
                }
            }
            "assignment_expression" => {
                if let Some(left) = parent.child_by_field_name("left") {
                    return super::text(left, source).into();
                }
            }
            _ => {}
        }
    }
    format!(
        "<anonymous@{}:{}>",
        node.start_position().row + 1,
        node.start_position().column + 1
    )
}

#[test]
fn cached_names_and_columns_match_ancestor_and_prefix_reference() {
    isolated(
        "analysis::tests::cached_names_and_columns_match_ancestor_and_prefix_reference",
        || {
            for (path, source) in [
                (
                    "scopes.ts",
                    r#"
function outer(a = () => 1) {
  const before = () => 2;
  class Named extends (class Base { base() { return 0; } }) {
    method(x = () => 3) { function nested() { return 4; } return nested; }
    ['computed']() { return () => 5; }
    get value() { return 6; }
    field = () => class Reset { final() { return 7; } };
  }
  const Anonymous = class { method() { return () => 8; } };
  { const block = () => 9; }
  return () => 10;
}
const Assigned = class Inner { method() { return 11; } };
object.member = function () { return () => 12; };
[1].map(() => () => 13);
function sibling() { return 14; }
function left(){}function right(){}
abstract class Abstract { abstract missing(): void; concrete() { return () => 15; } }
"#,
                ),
                (
                    "unicode.ts",
                    "\u{feff}/* 😀é */ const α = () => 1; /* 🦀 */ const β = () => () => 2;\r\n/* é😀 */ function γ() { return () => 3; }\nfunction δ() { return 4; }",
                ),
                (
                    "view.tsx",
                    "const mark = '😀'; const View = () => <div>{[1].map(() => <b/>)}{(() => <i/>)()}</div>;\nconst Next = () => <span/>;",
                ),
            ] {
                let mut parser = tree_sitter::Parser::new();
                let language = if path.ends_with(".tsx") {
                    tree_sitter_typescript::LANGUAGE_TSX
                } else {
                    tree_sitter_typescript::LANGUAGE_TYPESCRIPT
                };
                parser.set_language(&language.into()).unwrap();
                let tree = parser.parse(source, None).unwrap();
                assert!(!tree.root_node().has_error(), "{path}");
                let mut expected = Vec::new();
                super::walk(tree.root_node(), |node| {
                    if super::is_function(node.kind()) && node.child_by_field_name("body").is_some()
                    {
                        // Deliberately use the original ancestor/prefix approach as
                        // a reference, independent of the production caches.
                        let mut names = vec![reference_local_name(node, source)];
                        let mut ancestor = node.parent();
                        while let Some(parent) = ancestor {
                            if super::is_function(parent.kind()) {
                                names.push(reference_local_name(parent, source));
                            } else if matches!(
                                parent.kind(),
                                "class_declaration" | "abstract_class_declaration" | "class"
                            ) && let Some(name) = parent.child_by_field_name("name")
                            {
                                names.push(super::text(name, source).into());
                                break;
                            }
                            ancestor = parent.parent();
                        }
                        names.reverse();
                        let start = node.start_byte();
                        let utf16 = source[start - node.start_position().column..start]
                            .encode_utf16()
                            .count()
                            + 1;
                        expected.push((names.join("::"), utf16, node.start_position().column + 1));
                    }
                    true
                });
                let methods = super::Analyzer::default().analyze(path, source).unwrap();
                let actual: Vec<_> = methods
                    .iter()
                    .map(|method| (method.name.clone(), method.column_utf16, method.column))
                    .collect();
                assert_eq!(actual, expected, "{path}");
            }
        },
    );
}

// Committed expectations for every construct the analyzer names, counts or
// hashes. Identities are stored in users' baselines, so a difference here is a
// breaking change, never a reason to regenerate the golden files. A grammar or
// tree-sitter update that changes them changes users' identities too: treat it
// as a compatibility decision (docs/COMPATIBILITY.md) before editing a golden.
const GOLDEN: [(&str, &str, &str); 2] = [
    (
        "golden/constructs.ts",
        include_str!("../../tests/fixtures/golden/constructs.ts"),
        include_str!("../../tests/fixtures/golden/constructs.ts.golden"),
    ),
    (
        "golden/constructs.tsx",
        include_str!("../../tests/fixtures/golden/constructs.tsx"),
        include_str!("../../tests/fixtures/golden/constructs.tsx.golden"),
    ),
];

fn golden_dump(methods: &[super::Method]) -> String {
    let header = "# identity line column column_utf16 end_line complexity owned_lines name";
    let rows = methods.iter().map(|method| {
        let lines: Vec<String> = method.lines.iter().map(usize::to_string).collect();
        let lines = if lines.is_empty() {
            "-".to_owned()
        } else {
            lines.join(",")
        };
        format!(
            "{} {} {} {} {} {} {lines} {}",
            method.identity,
            method.line,
            method.column,
            method.column_utf16,
            method.end_line,
            method.complexity,
            method.name
        )
    });
    std::iter::once(header.to_owned())
        .chain(rows)
        .map(|row| row + "\n")
        .collect()
}

#[test]
fn golden_identities_names_columns_and_metrics_are_pinned() {
    isolated(
        "analysis::tests::golden_identities_names_columns_and_metrics_are_pinned",
        || {
            let mut analyzer = super::Analyzer::default();
            for (path, source, expected) in GOLDEN {
                // The fixtures must keep the bytes they exist to exercise.
                assert!(source.starts_with('\u{feff}'), "{path} lost its BOM");
                // CRLF appears only inside the marked region, so a checkout that
                // rewrote line endings fails here rather than in the goldens.
                let (before, region) = source.split_once("// CRLF region starts below.\n").unwrap();
                let (region, after) = region.split_once("// CRLF region ends above.\n").unwrap();
                assert!(
                    !before.contains('\r') && !after.contains('\r'),
                    "{path} has CR outside its CRLF region"
                );
                assert_eq!(
                    region.matches("\r\n").count(),
                    region.matches('\n').count(),
                    "{path} lost its CRLF region"
                );
                assert!(region.contains("\r\n"), "{path} lost its CRLF region");
                assert!(
                    source.chars().any(|c| c.len_utf16() == 2),
                    "{path} lost its astral characters"
                );
                let actual = golden_dump(&analyzer.analyze(path, source).unwrap());
                if actual != expected {
                    let difference = actual
                        .lines()
                        .zip(expected.lines())
                        .position(|(actual, expected)| actual != expected)
                        .unwrap_or_else(|| actual.lines().count().min(expected.lines().count()));
                    panic!(
                        "{path} differs from its golden file at line {}:\n  expected: {:?}\n  actual:   {:?}\nfull actual output:\n{actual}",
                        difference + 1,
                        expected.lines().nth(difference),
                        actual.lines().nth(difference),
                    );
                }
            }
        },
    );
}

fn kind_trees() -> Vec<(tree_sitter::Language, tree_sitter::Tree)> {
    let mut trees = Vec::new();
    for (path, source, _) in GOLDEN {
        let tsx = path.ends_with(".tsx");
        let language: tree_sitter::Language = if tsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        }
        .into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let malformed: &[&str] = if tsx {
            &["const f = <div>", "const g = () => <b>{</b>;", "<<>>"]
        } else {
            &[
                "function f() {",
                "const broken = ;",
                "class { method( }",
                "export type * from './m';",
                "let x: = 1; )",
            ]
        };
        trees.push((language.clone(), parser.parse(source, None).unwrap()));
        for source in malformed {
            trees.push((language.clone(), parser.parse(source, None).unwrap()));
        }
    }
    trees
}

// Kind-id tables are only equivalent to kind() string matches if the id of
// every node, including ERROR (65535) and MISSING nodes, maps back to its kind.
#[test]
fn every_node_kind_id_maps_back_to_its_kind_in_both_grammars() {
    isolated(
        "analysis::tests::every_node_kind_id_maps_back_to_its_kind_in_both_grammars",
        || {
            let (mut errors, mut missing, mut nodes) = (0, 0, 0);
            for (language, tree) in kind_trees() {
                super::walk(tree.root_node(), |node| {
                    nodes += 1;
                    errors += usize::from(node.is_error());
                    missing += usize::from(node.is_missing());
                    assert_eq!(
                        language.node_kind_for_id(node.kind_id()),
                        Some(node.kind()),
                        "kind id {} at {:?}",
                        node.kind_id(),
                        node.start_position()
                    );
                    true
                });
            }
            assert!(nodes > 1000 && errors > 0 && missing > 0);
            for language in [
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                tree_sitter_typescript::LANGUAGE_TSX,
            ] {
                let language: tree_sitter::Language = language.into();
                assert_eq!(language.node_kind_for_id(u16::MAX), Some("ERROR"));
                // Several kinds have more than one id, including kinds the
                // analyzer matches, so a kind-id table must be built over all ids:
                // not from the first match (`id_for_node_kind`), nor named ids only.
                let ids = |kind: &str| -> Vec<u16> {
                    (0..u16::try_from(language.node_kind_count()).unwrap())
                        .filter(|&id| language.node_kind_for_id(id) == Some(kind))
                        .collect()
                };
                let class = ids("class");
                assert!(
                    class.iter().any(|&id| language.node_kind_is_named(id))
                        && class.iter().any(|&id| !language.node_kind_is_named(id)),
                    "class: {class:?}"
                );
                for kind in ["required_parameter", "optional_parameter"] {
                    assert!(ids(kind).len() > 1, "{kind}: {:?}", ids(kind));
                }
            }
        },
    );
}

fn languages() -> [(bool, tree_sitter::Language); 2] {
    [
        (false, tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        (true, tree_sitter_typescript::LANGUAGE_TSX.into()),
    ]
}

// The tables must agree with the kind-string matches for every symbol id,
// including duplicate, anonymous and hidden ids, and give nothing outside them.
#[test]
fn kind_tables_match_kind_strings_for_every_id() {
    isolated(
        "analysis::tests::kind_tables_match_kind_strings_for_every_id",
        || {
            for (_, language) in languages() {
                let kinds = super::Kinds::new(&language);
                let count = language.node_kind_count();
                assert_eq!(kinds.flags.len(), count);
                assert_eq!(kinds.names.len(), count);
                for id in 0..u16::try_from(count).unwrap() {
                    let kind = language.node_kind_for_id(id);
                    assert_eq!(kinds.names[usize::from(id)].as_deref(), kind, "id {id}");
                    assert_eq!(
                        kinds.flags[usize::from(id)],
                        kind.map_or(0, super::kind_flags),
                        "id {id} {kind:?}"
                    );
                }
                assert!(kinds.flags.get(usize::from(u16::MAX)).is_none());
                for (field, id) in [
                    ("body", kinds.fields.body),
                    ("key", kinds.fields.key),
                    ("left", kinds.fields.left),
                    ("name", kinds.fields.name),
                    ("operator", kinds.fields.operator),
                    ("parameters", kinds.fields.parameters),
                    ("right", kinds.fields.right),
                    ("value", kinds.fields.value),
                ] {
                    assert_eq!(
                        id,
                        language.field_id_for_name(field).map(|id| id.get()),
                        "{field}"
                    );
                    assert!(id.is_some(), "{field}");
                }
            }
            // Every flag is set by at least one kind, and the known-dead
            // "return_type" arm stays dead (it is a field name, not a kind).
            for flag in [
                super::FUNCTION,
                super::TYPE_ONLY,
                super::DECISION,
                super::LOGICAL,
                super::OWNED_LINE,
                super::PARAMETER,
                super::PATTERN,
                super::CLASS,
                super::COMMENT,
                super::STATEMENT_BLOCK,
                super::NAMING_PARENT,
                super::ASSIGNMENT_PARENT,
            ] {
                for (_, language) in languages() {
                    let kinds = super::Kinds::new(&language);
                    assert!(kinds.flags.iter().any(|flags| flags & flag != 0), "{flag}");
                }
            }
            for (_, language) in languages() {
                let kinds = super::Kinds::new(&language);
                assert!(
                    !kinds
                        .names
                        .iter()
                        .any(|name| name.as_deref() == Some("return_type"))
                );
            }
            // Nodes outside the table (ERROR is 65535) have no flags and keep
            // their own kind string.
            for (path, source) in [
                ("bad.ts", "const broken = ;"),
                ("bad.tsx", "const f = <div>"),
            ] {
                let (_, language) = languages()[usize::from(path.ends_with(".tsx"))].clone();
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&language).unwrap();
                let tree = parser.parse(source, None).unwrap();
                let kinds = super::Kinds::new(&language);
                let mut errors = 0;
                super::walk(tree.root_node(), |node| {
                    if node.kind_id() == u16::MAX {
                        errors += 1;
                        assert_eq!(kinds.flags(node), 0);
                        assert_eq!(kinds.kind(node), "ERROR");
                    } else {
                        assert_eq!(kinds.flags(node), super::kind_flags(node.kind()));
                        assert_eq!(kinds.kind(node), node.kind());
                    }
                    true
                });
                assert!(errors > 0, "{path}");
            }
        },
    );
}

// The ancestor stack must equal the `Node::parent` chain for every node,
// including inside ERROR and around MISSING nodes, and visit the same nodes.
#[test]
fn ancestor_stack_matches_parent_chain_for_every_node() {
    isolated(
        "analysis::tests::ancestor_stack_matches_parent_chain_for_every_node",
        || {
            let mut pruned = 0;
            for (_, tree) in kind_trees() {
                let mut walked = Vec::new();
                super::walk(tree.root_node(), |node| {
                    walked.push(node.id());
                    true
                });
                let mut visited = Vec::new();
                super::walk_with_ancestors(tree.root_node(), |node, ancestors| {
                    visited.push(node.id());
                    let mut chain = Vec::new();
                    let mut parent = node.parent();
                    while let Some(node) = parent {
                        chain.push(node);
                        parent = node.parent();
                    }
                    chain.reverse();
                    assert_eq!(ancestors, chain.as_slice(), "{:?}", node.start_position());
                    true
                });
                assert_eq!(visited, walked);
                // Pruning a subtree skips its descendants and keeps the stack balanced.
                super::walk_with_ancestors(tree.root_node(), |node, ancestors| {
                    assert_eq!(ancestors.last().copied(), node.parent());
                    if node.kind() == "statement_block" {
                        pruned += 1;
                        return false;
                    }
                    true
                });
            }
            assert!(pruned > 0);
        },
    );
}

// An oracle written independently of `kind_flags`: every kind the analyzer
// matches and the flags it must carry. Any other kind must have no flags.
#[test]
fn kind_tables_match_an_independent_literal_oracle() {
    isolated(
        "analysis::tests::kind_tables_match_an_independent_literal_oracle",
        || {
            use super::{
                ASSIGNMENT_PARENT, CLASS, COMMENT, DECISION, FUNCTION, LOGICAL, NAMING_PARENT,
                OWNED_LINE, PARAMETER, PATTERN, STATEMENT_BLOCK, TYPE_ONLY,
            };
            let oracle: &[(&str, u16)] = &[
                ("function_declaration", FUNCTION),
                ("generator_function_declaration", FUNCTION),
                ("function_expression", FUNCTION),
                ("generator_function", FUNCTION),
                ("arrow_function", FUNCTION),
                ("method_definition", FUNCTION),
                ("type_annotation", TYPE_ONLY),
                ("type_parameters", TYPE_ONLY),
                ("if_statement", DECISION | OWNED_LINE),
                ("for_statement", DECISION | OWNED_LINE),
                ("for_in_statement", DECISION | OWNED_LINE),
                ("while_statement", DECISION | OWNED_LINE),
                ("do_statement", DECISION | OWNED_LINE),
                ("catch_clause", DECISION),
                ("switch_case", DECISION),
                ("ternary_expression", DECISION),
                ("binary_expression", LOGICAL),
                ("augmented_assignment_expression", LOGICAL),
                ("return_statement", OWNED_LINE),
                ("throw_statement", OWNED_LINE),
                ("expression_statement", OWNED_LINE),
                ("lexical_declaration", OWNED_LINE),
                ("variable_declaration", OWNED_LINE),
                ("switch_statement", OWNED_LINE),
                ("break_statement", OWNED_LINE),
                ("continue_statement", OWNED_LINE),
                ("required_parameter", PARAMETER),
                ("optional_parameter", PARAMETER),
                ("assignment_pattern", PATTERN),
                ("object_assignment_pattern", PATTERN),
                ("class_declaration", CLASS),
                ("abstract_class_declaration", CLASS),
                ("class", CLASS),
                ("comment", COMMENT),
                ("statement_block", STATEMENT_BLOCK),
                ("variable_declarator", NAMING_PARENT),
                ("public_field_definition", NAMING_PARENT),
                ("pair", NAMING_PARENT),
                ("assignment_expression", ASSIGNMENT_PARENT),
            ];
            for (_, language) in languages() {
                let kinds = super::Kinds::new(&language);
                for (id, flags) in kinds.flags.iter().enumerate() {
                    let name = kinds.names[id].as_deref().unwrap_or("");
                    let expected = oracle
                        .iter()
                        .find(|(kind, _)| *kind == name)
                        .map_or(0, |(_, flags)| *flags);
                    assert_eq!(*flags, expected, "id {id} {name:?}");
                }
                // Every oracle kind exists in both grammars.
                for (kind, _) in oracle {
                    assert!(
                        kinds.names.iter().any(|name| name.as_deref() == Some(kind)),
                        "{kind}"
                    );
                }
            }
        },
    );
}

// Symbol ids differ between the grammars, so a tree read with the other
// grammar's table would see wrong kinds. Each tree must use its own.
#[test]
fn each_grammar_needs_its_own_kind_table() {
    isolated(
        "analysis::tests::each_grammar_needs_its_own_kind_table",
        || {
            let [(_, typescript), (_, tsx)] = languages();
            let (typescript, tsx) = (super::Kinds::new(&typescript), super::Kinds::new(&tsx));
            assert_ne!(typescript.names, tsx.names);
            for (path, source, _) in GOLDEN {
                let (own, other) = if path.ends_with(".tsx") {
                    (&tsx, &typescript)
                } else {
                    (&typescript, &tsx)
                };
                let (_, language) = languages()[usize::from(path.ends_with(".tsx"))].clone();
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&language).unwrap();
                let tree = parser.parse(source, None).unwrap();
                let mut mismatches = 0;
                super::walk(tree.root_node(), |node| {
                    assert_eq!(own.kind(node), node.kind());
                    assert_eq!(own.flags(node), super::kind_flags(node.kind()));
                    mismatches += usize::from(other.kind(node) != node.kind());
                    true
                });
                assert!(mismatches > 0, "{path}");
            }
            // One analyzer alternating grammars matches fresh analyzers.
            let mut shared = super::Analyzer::default();
            for _ in 0..2 {
                for (path, source, _) in GOLDEN {
                    let fresh =
                        golden_dump(&super::Analyzer::default().analyze(path, source).unwrap());
                    assert_eq!(
                        golden_dump(&shared.analyze(path, source).unwrap()),
                        fresh,
                        "{path}"
                    );
                }
            }
        },
    );
}
