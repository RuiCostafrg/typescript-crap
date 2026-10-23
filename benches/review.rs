//! Stage-separated adversarial measurements; no wall-clock assertions or public APIs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../src/output.rs"]
mod output;
use std::{fmt::Write, fs, hint::black_box, time::Instant};
use ts_crap::{Format, ScanOptions, baseline::Baseline, render, scan};

fn measured(mut operation: impl FnMut()) -> Vec<f64> {
    operation(); // warmup
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples
}

fn peak_rss_kib() -> Option<usize> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|line| line.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .expect("mode: unicode, nested, render-FORMAT, validation or end-to-end");
    let count: usize = args.next().expect("size").parse().unwrap();
    assert!(count > 0);
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut source = String::new();
    if mode == "nested" {
        for index in 0..count {
            writeln!(source, "function f{index}() {{").unwrap();
        }
        source.push_str("return '😀';\n");
        source.push_str(&"}\n".repeat(count));
    } else {
        source.push_str("const marker = '😀'; ");
        for index in 0..if mode == "validation" || mode == "end-to-end" {
            10
        } else {
            count
        } {
            write!(
                source,
                "function f{index}(flag: boolean) {{ return flag ? marker : ''; }} "
            )
            .unwrap();
            if mode != "unicode" {
                source.push('\n');
            }
        }
    }
    fs::write(root.join("source.ts"), &source).unwrap();
    let options = ScanOptions {
        root: root.into(),
        threshold: 0.0,
        ..Default::default()
    };
    let mut report = scan(&options).unwrap();
    let expected = if mode == "validation" || mode == "end-to-end" {
        10
    } else {
        count
    };
    assert_eq!(report.functions.len(), expected);
    assert!(
        report
            .functions
            .iter()
            .all(|function| function.coverage == 0.0)
    );
    let name_bytes: usize = report
        .functions
        .iter()
        .map(|function| function.name.len())
        .sum();
    let mut output_bytes = 0;
    let samples = if let Some(format) = mode.strip_prefix("render-") {
        let format = match format {
            "json" => Format::Json,
            "xml" => Format::Xml,
            "sarif" => Format::Sarif,
            "text" => Format::Text,
            "html" => Format::Html,
            _ => panic!("unknown format"),
        };
        report.functions.reverse(); // machine order must remain caller order
        let reference = render(&report, format).unwrap();
        output_bytes = reference.len();
        measured(|| {
            black_box(render(black_box(&report), format).unwrap());
        })
    } else if mode == "validation" || mode == "end-to-end" {
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::create_dir(root.join("node_modules/dependency")).unwrap();
        for index in 0..count {
            fs::write(
                root.join(format!("node_modules/dependency/ignored-{index}.ts")),
                "const value = 1;",
            )
            .unwrap();
        }
        let first = root.join("report.json");
        let second = root.join("baseline.json");
        let coverage = root.join("coverage.json");
        let config = root.join("config.json");
        for path in [&first, &second, &coverage, &config] {
            fs::write(path, "{}").unwrap();
        }
        if mode == "validation" {
            measured(|| {
                black_box(
                    output::DestinationPlan::new(
                        root,
                        vec![&second, &first],
                        vec![&coverage, &config],
                    )
                    .unwrap(),
                );
            })
        } else {
            measured(|| {
                let plan = output::DestinationPlan::new(
                    root,
                    vec![&second, &first],
                    vec![&coverage, &config],
                )
                .unwrap();
                let report = scan(black_box(&options)).unwrap();
                let contents = vec![
                    Baseline::capture(&report).render().unwrap(),
                    render(&report, Format::Json).unwrap(),
                ];
                plan.publish(&contents).unwrap();
            })
        }
    } else {
        assert!(matches!(mode.as_str(), "unicode" | "nested"));
        measured(|| {
            black_box(scan(black_box(&options)).unwrap());
        })
    };
    // Correctness checks and fixture construction are outside timed regions.
    assert_eq!(scan(&options).unwrap().functions.len(), expected);
    println!(
        "{}",
        serde_json::json!({
            "mode":mode, "size":count, "source_bytes":source.len(), "name_bytes":name_bytes,
            "output_bytes":output_bytes, "samples_ms":samples, "median_ms":samples[2],
            "peak_rss_kib":peak_rss_kib(),
        })
    );
}
