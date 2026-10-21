//! Fast public-library contracts also used by the focused mutation runner.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use ts_crap::{Format, Report, ScanOptions, render, scan};

fn project(source: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("source.ts"), source).unwrap();
    root
}
fn options(root: &Path) -> ScanOptions {
    ScanOptions {
        root: root.into(),
        ..Default::default()
    }
}
fn analyze(source: &str) -> Report {
    let dir = project(source);
    scan(&options(dir.path())).unwrap()
}
fn cover(root: &Path, lines: serde_json::Value) -> PathBuf {
    let bytes = fs::read(root.join("source.ts")).unwrap();
    let report = serde_json::json!({"version":1,"files":{"source.ts":{
        "sha256":format!("{:x}",Sha256::digest(&bytes)),"lines":lines
    }}});
    let path = root.join("coverage.json");
    fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
    path
}

#[test]
fn known_crap_values_locations_order_and_strict_gate() {
    let root = project(
        "export function low() {\n  return 1;\n}\nexport function high(a:boolean,b:boolean) {\n  if(a && b) return 1;\n  return 0;\n}\n",
    );
    let mut opts = options(root.path());
    opts.threshold = 12.0;
    let r = scan(&opts).unwrap();
    assert_eq!(r.version, 1);
    assert_eq!(r.threshold, 12.0);
    assert_eq!(r.files_scanned, 1);
    assert!(!r.failed);
    assert_eq!(r.functions.len(), 2);
    let f = &r.functions[0];
    assert_eq!(
        (&*f.path, &*f.name, f.line, f.column, f.end_line),
        ("source.ts", "low", 1, 8, 3)
    );
    assert_eq!(
        (f.complexity, f.coverage, f.crap, f.failed),
        (1, 0.0, 2.0, false)
    );
    let f = &r.functions[1];
    assert_eq!(
        (&*f.path, &*f.name, f.line, f.column, f.end_line),
        ("source.ts", "high", 4, 8, 7)
    );
    assert_eq!(
        (f.complexity, f.coverage, f.crap, f.failed),
        (3, 0.0, 12.0, false)
    );
    opts.threshold = 11.9;
    assert!(scan(&opts).unwrap().failed);
    assert!(!scan(&opts).unwrap().functions[0].failed);
    opts.threshold = 0.0;
    assert!(scan(&opts).unwrap().functions[0].failed);
    for threshold in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        opts.threshold = threshold;
        assert!(scan(&opts).is_err());
    }
    let default = ScanOptions::default();
    assert_eq!(default.threshold, 10.0);
    assert_eq!(default.root, PathBuf::from("."));
    assert!(
        default.paths.is_empty()
            && default.diff.is_none()
            && default.pr.is_none()
            && default.coverage.is_none()
    );
}

#[test]
fn runtime_decisions_are_individually_counted_not_types_strings_or_children() {
    let cases = [
        ("if (a) return 1;", 2),
        ("if (a) return 1; else if (b) return 2;", 3),
        ("for(let i=0;i<2;i++) {}", 2),
        ("for(const i in a) {}", 2),
        ("for(const i of a) {}", 2),
        ("while(a) {}", 2),
        ("do {} while(a);", 2),
        ("try {} catch(e) {} finally {}", 2),
        (
            "switch(a){case 1: break; case 2: break; default: break;}",
            3,
        ),
        ("return a ? 1 : 0;", 2),
        ("return a && b;", 2),
        ("return a || b;", 2),
        ("return a ?? b;", 2),
        ("a &&= b;", 2),
        ("a ||= b;", 2),
        ("a ??= b;", 2),
        ("return a?.b;", 1),
        ("let x: A extends B ? C : D; return 'if && ||';", 1),
        ("// if (a) return 1;\nreturn 0;", 1),
        ("function child(){if(a) return 1;} return child;", 1),
        ("return () => a ? 1 : 0;", 1),
    ];
    for (body, expected) in cases {
        let r = analyze(&format!("function test(a:any,b:any){{{body}}}"));
        assert_eq!(r.functions[0].name, "test");
        assert_eq!(r.functions[0].complexity, expected, "{body}");
    }
}

#[test]
fn function_shapes_names_nested_scopes_and_signatures() {
    let r = analyze(
        "function outer(){ function inner(){return 1;} return inner; }\nclass Named { constructor(){} get active(){return 1;} set active(v:number){} run(){return 1;} field = () => 1; }\nconst pair = { prop: () => 1 };\nconst arrow = () => 1;\nlet assigned; assigned = function(){ return 1; };\nfunction* gen(){yield 1;}\nconst expr = function*(){yield 1;};\nfunction over(a:number):number; function over(a:number){return a;}\n[1].map((v)=>v);\n",
    );
    let names: Vec<_> = r.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "outer",
            "outer::inner",
            "Named::constructor",
            "Named::active",
            "Named::active",
            "Named::run",
            "Named::field",
            "prop",
            "arrow",
            "assigned",
            "gen",
            "expr",
            "over",
            "<anonymous@9:9>"
        ]
    );
    assert!(r.functions.iter().all(|f| f.complexity == 1));
    assert!(
        analyze("interface I { f():void; } declare function x():void;")
            .functions
            .is_empty()
    );
    let root = project("const broken = (");
    assert!(scan(&options(root.path())).is_err());
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("view.tsx"),
        "export const View = () => <div>{true ? 'a' : 'b'}</div>;",
    )
    .unwrap();
    let r = scan(&options(root.path())).unwrap();
    assert_eq!(r.functions[0].complexity, 2);
}

#[test]
fn coverage_fractions_missing_lines_and_source_integrity() {
    let root =
        project("function f(a:boolean,b:boolean) {\n  if(a && b) return 1;\n  return 0;\n}\n");
    let mut opts = options(root.path());
    opts.coverage = Some(cover(root.path(), serde_json::json!({"2":1,"3":0})));
    let r = scan(&opts).unwrap();
    let f = &r.functions[0];
    assert_eq!(f.coverage, 0.5);
    assert_eq!(f.crap, 4.125);
    assert!(!f.failed);
    opts.coverage = Some(cover(root.path(), serde_json::json!({"2":1})));
    assert_eq!(scan(&opts).unwrap().functions[0].coverage, 0.5);
    opts.coverage = Some(cover(root.path(), serde_json::json!({"2":1,"3":9})));
    assert_eq!(scan(&opts).unwrap().functions[0].crap, 3.0);
    opts.coverage = Some(cover(root.path(), serde_json::json!({"2":0,"3":0})));
    assert_eq!(scan(&opts).unwrap().functions[0].crap, 12.0);
    opts.coverage = Some(cover(root.path(), serde_json::json!({"4":0}))); // Last source line is valid.
    assert!(scan(&opts).is_ok());
    opts.coverage = Some(cover(root.path(), serde_json::json!({"5":0})));
    assert!(scan(&opts).is_err());
    opts.coverage = Some(cover(root.path(), serde_json::json!({})));
    assert_eq!(scan(&opts).unwrap().functions[0].coverage, 0.0);
    fs::write(root.path().join("source.ts"), "// stale source").unwrap();
    assert!(
        scan(&opts).is_err(),
        "also validate type-only/empty selected source"
    );
}

#[test]
fn coverage_schema_validation_and_missing_files_are_conservative() {
    let root = project("function f(){return 1;}");
    let mut opts = options(root.path());
    let path = root.path().join("coverage.json");
    opts.coverage = Some(path.clone());
    let hash = format!(
        "{:x}",
        Sha256::digest(fs::read(root.path().join("source.ts")).unwrap())
    );
    for name in [
        "",
        "/a.ts",
        "../a.ts",
        "./a.ts",
        "a//b.ts",
        "a\\b.ts",
        "a:b.ts",
        "a/./b.ts",
        "a/../b.ts",
    ] {
        let data = serde_json::json!({"version":1,"files":{name:{"sha256":hash,"lines":{}}}});
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert!(scan(&opts).is_err(), "{name:?}");
    }
    for hash in ["0".repeat(63), "0".repeat(65), "z".repeat(64)] {
        let data =
            serde_json::json!({"version":1,"files":{"source.ts":{"sha256":hash,"lines":{}}}});
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert!(scan(&opts).is_err());
    }
    for lines in [
        serde_json::json!({"0":1}),
        serde_json::json!({"01":1}),
        serde_json::json!({"x":1}),
        serde_json::json!({"1":-1}),
        serde_json::json!({"1":1.5}),
        serde_json::json!({"1":true}),
    ] {
        opts.coverage = Some(cover(root.path(), lines));
        assert!(scan(&opts).is_err());
    }
    for version in [0, 2] {
        fs::write(&path, format!(r#"{{"version":{version},"files":{{}}}}"#)).unwrap();
        assert!(scan(&opts).is_err());
    }
    fs::write(&path, r#"{"version":1,"files":{}}"#).unwrap();
    assert_eq!(scan(&opts).unwrap().functions[0].coverage, 0.0);
    let valid = serde_json::json!({"version":1,"files":{"source.ts":{"sha256":hash.to_uppercase(),"lines":{"1":1}}}});
    fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
    assert_eq!(scan(&opts).unwrap().functions[0].coverage, 1.0);
}

#[test]
fn owned_lines_exclude_children_and_include_expression_arrows_and_defaults() {
    let source = "function outer() {\n  function child(){\n    if(true) return 1;\n    return 0;\n  }\n  return child;\n}\nconst arrow = () =>\n  1;\nfunction defaults(\n  a = true ? 1 : 0\n) {\n  return a;\n}\nfunction empty() {}\n";
    let root = project(source);
    let mut opts = options(root.path());
    opts.coverage = Some(cover(root.path(), serde_json::json!({"6":1,"9":1,"13":1})));
    let r = scan(&opts).unwrap();
    let values: BTreeMap<_, _> = r
        .functions
        .iter()
        .map(|f| (f.name.as_str(), (f.complexity, f.coverage)))
        .collect();
    assert_eq!(values["outer"], (1, 1.0));
    assert_eq!(values["outer::child"], (2, 0.0));
    assert_eq!(values["arrow"], (1, 1.0));
    assert_eq!(values["defaults"], (2, 0.5));
    assert_eq!(values["empty"], (1, 0.0));
}

#[test]
fn multiline_destructuring_defaults_own_their_initializer_lines() {
    let source = "function f(\n  { a =\n      1\n  } = {},\n  [b =\n      2\n  ] = []\n) {\n  return a + b;\n}\n";
    let root = project(source);
    let mut opts = options(root.path());
    opts.coverage = Some(cover(root.path(), serde_json::json!({"9":1})));
    let r = scan(&opts).unwrap();
    // Inner defaults (3,6), outer defaults (4,7), and return (9) own five lines.
    assert_eq!(r.functions[0].complexity, 1);
    assert_eq!(r.functions[0].coverage, 0.2);
    opts.coverage = Some(cover(
        root.path(),
        serde_json::json!({"3":1,"4":1,"6":1,"7":1,"9":1}),
    ));
    assert_eq!(scan(&opts).unwrap().functions[0].coverage, 1.0);
}

#[test]
fn unselected_coverage_entries_still_require_valid_hash_syntax() {
    let root = project("function f(){return 1;}");
    let mut opts = options(root.path());
    let path = root.path().join("coverage.json");
    opts.coverage = Some(path.clone());
    for hash in ["0".repeat(63), "0".repeat(65), "z".repeat(64)] {
        let value = serde_json::json!({"version":1,"files":{"unselected.ts":{
            "sha256":hash, "lines":{}
        }}});
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = scan(&opts).unwrap_err().to_string();
        assert!(error.contains("invalid SHA256"), "{error}");
    }
}

#[test]
fn reports_replace_forbidden_controls_and_preserve_legal_whitespace() {
    let mut report = analyze("function f(){return 1;}");
    report.functions[0].name = "a\0b\u{1}c\u{7}d\n\r\t".into();
    for format in [Format::Xml, Format::Html] {
        let rendered = render(&report, format).unwrap();
        assert!(rendered.contains("a\u{fffd}b\u{fffd}c\u{fffd}d\n\r\t"));
        assert!(!rendered.contains('\0'));
        assert!(!rendered.contains('\u{1}'));
        assert!(!rendered.contains('\u{7}'));
    }
}

#[test]
fn reports_render_complete_values_and_escaping() {
    let root = project("export const data = { ['a&<b>\\\"\\\'']() { return 1; } };");
    let mut opts = options(root.path());
    opts.threshold = 1.0;
    let r = scan(&opts).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&render(&r, Format::Json).unwrap()).unwrap();
    assert_eq!(value["functions"][0]["crap"], 2.0);
    assert!(value.get("source_paths").is_none());
    let text = render(&r, Format::Text).unwrap();
    assert!(text.contains("CRAP gate: FAIL (threshold 1, 1 files, 1 functions)"));
    assert!(text.contains("source.ts:1:23"));
    assert!(text.contains("0.0%  2.000  FAIL"));
    let xml = render(&r, Format::Xml).unwrap();
    for attr in [
        "version=\"1\"",
        "threshold=\"1\"",
        "files-scanned=\"1\"",
        "failed=\"true\"",
        "line=\"1\"",
        "column=\"23\"",
        "end-line=\"1\"",
        "complexity=\"1\"",
        "coverage=\"0\"",
        "crap=\"2\"",
    ] {
        assert!(xml.contains(attr), "{attr} in {xml}");
    }
    for fmt in [Format::Xml, Format::Html] {
        let text = render(&r, fmt).unwrap();
        for escaped in ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"] {
            assert!(text.contains(escaped), "{escaped}");
        }
    }
    let html = render(&r, Format::Html).unwrap();
    assert!(html.contains("class=\"fail\""));
    assert!(html.contains("<td>0.0%</td><td>2.000</td>"));
    let empty = analyze("export type T = string;");
    assert!(
        render(&empty, Format::Text)
            .unwrap()
            .contains("No implemented TypeScript functions found")
    );
    assert!(render(&empty, Format::Text).unwrap().contains("PASS"));
    assert!(
        render(&empty, Format::Html)
            .unwrap()
            .contains("class=\"pass\"")
    );
}

#[test]
fn root_selection_order_deduplication_and_output_protection() {
    let root = project("function f(){return 0;}");
    fs::write(root.path().join("a.ts"), "function a(){return 1;}").unwrap();
    fs::write(root.path().join("types.ts"), "type T = string;").unwrap();
    let mut opts = options(root.path());
    opts.paths = vec!["a.ts".into(), "source.ts".into(), "a.ts".into()];
    let r = scan(&opts).unwrap();
    assert_eq!(r.files_scanned, 2);
    assert_eq!(r.functions[0].path, "a.ts");
    assert_eq!(r.functions[1].path, "source.ts");
    assert!(r.is_source_path(&root.path().join("source.ts").canonicalize().unwrap()));
    assert!(!r.is_source_path(&root.path().join("missing.ts")));
    opts.paths = vec![];
    let r = scan(&opts).unwrap();
    assert!(r.is_source_path(&root.path().join("types.ts").canonicalize().unwrap()));
    for file in ["missing.ts", "file.js"] {
        opts.paths = vec![file.into()];
        assert!(scan(&opts).is_err());
    }
    let file = root.path().join("source.ts");
    opts.root = file;
    opts.paths.clear();
    assert!(scan(&opts).is_err());
    let mut opts = options(root.path());
    opts.diff = Some("HEAD".into());
    opts.pr = Some("1".into());
    assert!(scan(&opts).is_err());
}
