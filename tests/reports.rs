#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use quick_xml::{Reader, events::Event};
use std::{fs, process::Command};

#[test]
fn json_xml_html_reports_contain_locations_scores_and_escape_user_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("quote'&<file>.ts");
    fs::write(&source,"export const obj = { ['<script>alert(1)</script>'](flag: boolean) { if(flag) return 1; if(!flag) return 2; return 0; } };").unwrap();
    for format in ["json", "xml", "html"] {
        let output = dir.path().join(format!("report.{format}"));
        let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .args([
                "--root",
                dir.path().to_str().unwrap(),
                "--format",
                format,
                "--output",
                output.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
        let report = fs::read_to_string(&output).unwrap();
        assert!(
            !report.contains(&dir.path().display().to_string()),
            "reports use relative paths only"
        );
        match format {
            "json" => {
                let value: serde_json::Value = serde_json::from_str(&report).unwrap();
                assert_eq!(value["functions"][0]["path"], "quote'&<file>.ts");
                assert_eq!(value["functions"][0]["complexity"], 3);
                assert_eq!(value["functions"][0]["crap"], 12.0);
                assert_eq!(value["failed"], true);
            }
            "xml" => {
                assert!(report.contains("&lt;script&gt;"));
                assert!(report.contains("quote&apos;&amp;&lt;file&gt;.ts"));
                let mut reader = Reader::from_str(&report);
                let mut count = 0;
                loop {
                    match reader.read_event().unwrap() {
                        Event::Empty(e) if e.name().as_ref() == "function" => {
                            count += 1;
                            for a in e.attributes() {
                                a.unwrap()
                                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .unwrap();
                            }
                        }
                        Event::Eof => break,
                        _ => {}
                    }
                }
                assert_eq!(count, 1);
            }
            "html" => {
                assert!(report.starts_with("<!doctype html>"));
                assert!(report.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
                assert!(!report.contains("<script>"));
                assert!(report.contains("12.000"));
                assert!(report.contains("FAIL"));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn output_creation_replacement_and_canonical_input_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let text = "export const a = () => 1;";
    fs::write(root.join("a.ts"), text).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    for path in ["report.json", "nested/report.json"] {
        for _ in 0..2 {
            let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
                .current_dir(root)
                .args(["--root", ".", "--format", "json", "--output", path])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap();
            assert_eq!(value["functions"][0]["name"], "a");
        }
    }
    let coverage = root.join("coverage.json");
    let coverage_text = r#"{"version":1,"files":{}}"#;
    fs::write(&coverage, coverage_text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--coverage")
        .arg(&coverage)
        .arg("--output")
        .arg(&coverage)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot overwrite coverage input"));
    assert_eq!(fs::read_to_string(coverage).unwrap(), coverage_text);
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--output")
        .arg(root.join("nested/../a.ts"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot overwrite a source file"));
    assert_eq!(fs::read_to_string(root.join("a.ts")).unwrap(), text);
}

#[test]
fn output_errors_are_not_quality_failures_and_inputs_are_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.ts");
    let text = "export const a = () => 1;";
    fs::write(&source, text).unwrap();
    for output in [&source, &dir.path().join("missing/report.json")] {
        let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(dir.path())
            .arg("--output")
            .arg(output)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert_eq!(fs::read_to_string(&source).unwrap(), text);
    }
}
