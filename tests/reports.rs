#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use quick_xml::{Reader, events::Event};
use std::{fs, process::Command};

#[test]
fn report_navigation_preserves_full_gate_and_machine_formats() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), "function small(){return 1;}\nfunction tie(a:boolean,b:boolean){if(a)return 1;if(b)return 2;return 3;}\n").unwrap();
    fs::write(dir.path().join("b.ts"), "function largest(a:boolean,b:boolean,c:boolean){if(a)return 1;if(b)return 2;if(c)return 3;return 4;}\nfunction tie(a:boolean,b:boolean){if(a)return 1;if(b)return 2;return 3;}\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    let out = run(&["--only-failing", "--sort", "crap", "--top", "2"]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("4 functions"));
    assert!(!text.contains("  small  "));
    assert!(text.find("  largest  ").unwrap() < text.find("  tie  ").unwrap());
    assert!(
        text.contains("a.ts:2:1  tie"),
        "tie order is stable by location"
    );
    assert!(!text.contains("b.ts:2:1  tie"));
    assert!(text.contains("Showing 2 of 4"));
    let captured = run(&[
        "--top",
        "1",
        "--write-baseline",
        dir.path().join("baseline.json").to_str().unwrap(),
    ]);
    assert_eq!(captured.status.code(), Some(1));
    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("baseline.json")).unwrap()).unwrap();
    assert_eq!(baseline["functions"].as_array().unwrap().len(), 4);
    let html = run(&["--format", "html", "--only-failing", "--top", "1"]);
    assert_eq!(html.status.code(), Some(1));
    let html = String::from_utf8(html.stdout).unwrap();
    assert_eq!(html.matches("<tr data-failed=").count(), 1);
    assert!(html.contains("Showing 1 of 4"));
    assert!(html.contains("id=\"filter\""));
    assert!(html.find("id=\"filter\"") < html.find("<table>"));
    for format in ["json", "xml", "sarif"] {
        for options in [
            vec!["--top", "1"],
            vec!["--only-failing"],
            vec!["--sort", "crap"],
        ] {
            let mut args = vec!["--format", format];
            args.extend(options);
            let out = run(&args);
            assert_eq!(out.status.code(), Some(2));
            assert!(out.stdout.is_empty());
        }
    }
    for args in [
        &["--top", "0"][..],
        &["--top", "-1"],
        &["--sort", "unknown"],
    ] {
        assert_eq!(run(args).status.code(), Some(2));
    }
    let unfiltered = run(&["--format", "json"]);
    let explicit_path = run(&["--format", "json", "--sort", "path"]);
    assert_eq!(unfiltered.stdout, explicit_path.stdout);
    let json: serde_json::Value = serde_json::from_slice(&unfiltered.stdout).unwrap();
    assert_eq!(json["functions"].as_array().unwrap().len(), 4);
    // Empty filtered output cannot conceal a completeness failure or alter a baseline.
    fs::write(dir.path().join("a.ts"), "function small(){return 1;}").unwrap();
    fs::remove_file(dir.path().join("b.ts")).unwrap();
    let empty = run(&["--only-failing", "--require-coverage"]);
    assert_eq!(empty.status.code(), Some(1));
    let text = String::from_utf8(empty.stdout).unwrap();
    assert!(text.contains("Showing 0 of 1"));
    assert!(text.contains("CRAP gate: FAIL"));
    assert!(!text.contains("No implemented"));
}

#[test]
fn syntax_diagnostics_escape_snippet_controls_and_emit_no_report() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("broken.ts"),
        "// heading\nconst invalid = ; // \u{1b}[31mspoof\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("broken.ts:2:"), "{stderr}");
    assert!(stderr.contains("const invalid = ;"));
    assert!(stderr.contains("\\u{1b}"));
    assert!(!stderr.contains('\u{1b}'));
}

#[test]
fn json_xml_html_reports_contain_locations_scores_and_escape_user_source() {
    let dir = tempfile::tempdir().unwrap();
    let filename = if cfg!(windows) {
        "quote'& ü.ts"
    } else {
        "quote'&<file>.ts"
    };
    let source = dir.path().join(filename);
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
                assert_eq!(value["functions"][0]["path"], filename);
                assert_eq!(value["functions"][0]["complexity"], 3);
                assert_eq!(value["functions"][0]["crap"], 12.0);
                assert_eq!(value["failed"], true);
            }
            "xml" => {
                assert!(report.contains("&lt;script&gt;"));
                let escaped = if cfg!(windows) {
                    "quote&apos;&amp; ü.ts"
                } else {
                    "quote&apos;&amp;&lt;file&gt;.ts"
                };
                assert!(report.contains(escaped));
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
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("cannot overwrite a TypeScript source destination")
    );
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

#[test]
fn every_presentation_combination_preserves_report_gate_totals_and_machine_order() {
    use ts_crap::{Format, RenderOptions, ScanOptions, Sort, render, render_with_options, scan};
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("a.ts"), "function pass(){return 1;} function fail(a:boolean,b:boolean){if(a)return 1;if(b)return 2;return 3;} function another(a:boolean,b:boolean){if(a)return 4;if(b)return 5;return 6;}").unwrap();
    let mut report = scan(&ScanOptions {
        root: directory.path().into(),
        require_coverage: true,
        ..Default::default()
    })
    .unwrap();
    // Machine encoders retain caller order, even when it differs from presentation order.
    report.functions.reverse();
    let unchanged = serde_json::to_string(&report).unwrap();
    let machine: Vec<_> = [Format::Json, Format::Xml, Format::Sarif]
        .into_iter()
        .map(|format| render(&report, format).unwrap())
        .collect();
    for format in [Format::Text, Format::Html] {
        for only_failing in [false, true] {
            for sort in [Sort::Path, Sort::Crap] {
                for top in [None, Some(1), Some(10)] {
                    let rendered = render_with_options(
                        &report,
                        format,
                        &RenderOptions {
                            only_failing,
                            sort,
                            top,
                        },
                    )
                    .unwrap();
                    let eligible = if only_failing { 2 } else { 3 };
                    let expected_rows = top.map_or(eligible, |limit| eligible.min(limit));
                    let row_marker = if matches!(format, Format::Html) {
                        "<tr data-failed="
                    } else {
                        "a.ts:"
                    };
                    assert_eq!(rendered.matches(row_marker).count(), expected_rows);
                    assert!(rendered.contains("FAIL"));
                    assert!(rendered.contains("3 functions"));
                    assert!(rendered.contains("Missing coverage: 1 files, 3 functions"));
                    assert_eq!(serde_json::to_string(&report).unwrap(), unchanged);
                }
            }
        }
    }
    for (index, format) in [Format::Json, Format::Xml, Format::Sarif]
        .into_iter()
        .enumerate()
    {
        assert_eq!(render(&report, format).unwrap(), machine[index]);
        match format {
            Format::Json => {
                let value: serde_json::Value = serde_json::from_str(&machine[index]).unwrap();
                let names: Vec<_> = value["functions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|function| function["name"].as_str().unwrap())
                    .collect();
                assert_eq!(names, ["another", "fail", "pass"]);
            }
            Format::Xml => {
                let positions: Vec<_> = ["another", "fail", "pass"]
                    .into_iter()
                    .map(|name| machine[index].find(&format!("name=\"{name}\"")).unwrap())
                    .collect();
                assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
            }
            Format::Sarif => {
                let value: serde_json::Value = serde_json::from_str(&machine[index]).unwrap();
                let results = value["runs"][0]["results"].as_array().unwrap();
                assert_eq!(results.len(), 3); // two function failures and completeness
                assert!(
                    results[0]["message"]["text"]
                        .as_str()
                        .unwrap()
                        .starts_with("another has CRAP")
                );
                assert!(
                    results[1]["message"]["text"]
                        .as_str()
                        .unwrap()
                        .starts_with("fail has CRAP")
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn destination_alias_conflicts_are_rejected_before_any_publication() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("a.ts"), "function a(){return 1;}").unwrap();
    let report = root.join("report.json");
    let baseline = root.join("baseline.json");
    let run = |report: &std::path::Path, baseline: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_ts-crap"))
            .arg("--root")
            .arg(root)
            .arg("--output")
            .arg(report)
            .arg("--write-baseline")
            .arg(baseline)
            .output()
            .unwrap()
    };
    for existing in [false, true] {
        if existing {
            fs::write(&report, "old report").unwrap();
        }
        let result = run(&report, &root.join("./report.json"));
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("destinations must differ"));
        if existing {
            assert_eq!(fs::read_to_string(&report).unwrap(), "old report");
        } else {
            assert!(!report.exists());
        }
    }
    fs::hard_link(&report, &baseline).unwrap();
    let result = run(&report, &baseline);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("destinations must differ"));
    assert_eq!(fs::read_to_string(&report).unwrap(), "old report");
    assert_eq!(fs::read_to_string(&baseline).unwrap(), "old report");
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
}

#[cfg(unix)]
#[test]
fn missing_destinations_through_directory_symlinks_conflict_before_publication() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("a.ts"), "function a(){return 1;}").unwrap();
    fs::create_dir(root.join("real")).unwrap();
    symlink(root.join("real"), root.join("alias")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--output")
        .arg(root.join("real/report.json"))
        .arg("--write-baseline")
        .arg(root.join("alias/report.json"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("destinations must differ"));
    assert_eq!(fs::read_dir(root.join("real")).unwrap().count(), 0);
}

#[test]
fn both_destinations_protect_all_input_aliases_and_staging_failure_publishes_neither() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("a.ts"), "function a(){return 1;}").unwrap();
    let coverage = root.join("coverage.json");
    let config = root.join("ts-crap.json");
    let baseline = root.join("input-baseline.json");
    fs::write(&coverage, r#"{"version":1,"files":{}}"#).unwrap();
    fs::write(&config, r#"{"version":1}"#).unwrap();
    fs::write(&baseline, "protected baseline").unwrap();
    let alias = root.join("alias.json");
    for input in [&coverage, &config, &baseline] {
        for destination in ["--output", "--write-baseline"] {
            if input == &baseline && destination == "--write-baseline" {
                continue;
            } // clap conflict is separately enforced.
            fs::hard_link(input, &alias).unwrap();
            let original = fs::read(input).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_ts-crap"));
            command
                .arg("--root")
                .arg(root)
                .arg("--coverage")
                .arg(&coverage);
            if input == &baseline {
                command.arg("--baseline").arg(&baseline);
            }
            command.arg(destination).arg(&alias);
            let result = command.output().unwrap();
            assert_eq!(result.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("cannot overwrite coverage input")
            );
            assert_eq!(fs::read(input).unwrap(), original);
            assert_eq!(fs::read(&alias).unwrap(), original);
            fs::remove_file(&alias).unwrap();
        }
    }
    let old = root.join("old-baseline.json");
    fs::write(&old, "unchanged").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ts-crap"))
        .arg("--root")
        .arg(root)
        .arg("--write-baseline")
        .arg(&old)
        .arg("--output")
        .arg(root.join("missing/report.json"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&old).unwrap(), "unchanged");
    assert_eq!(fs::read_dir(root).unwrap().count(), 5);
}
