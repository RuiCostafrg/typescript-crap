//! Repeatable end-to-end scan microbenchmark; wall-clock numbers are not CI gates.
//! TS_CRAP_BENCH_FUNCTIONS=2000 cargo bench --bench scan -j 1
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, hint::black_box, time::Instant};
use ts_crap::{ScanOptions, scan};
fn main() {
    let count: usize = std::env::var("TS_CRAP_BENCH_FUNCTIONS")
        .ok()
        .map(|v| v.parse().expect("positive integer function count"))
        .unwrap_or(2000);
    assert!(count > 0);
    let temp = tempfile::tempdir().unwrap();
    let mut source = String::new();
    for n in 0..count {
        use std::fmt::Write;
        writeln!(
            source,
            "export function f{n}(value: boolean) {{\n  if (value) return 1;\n  return 0;\n}}"
        )
        .unwrap();
    }
    let files: usize = std::env::var("TS_CRAP_BENCH_FILES")
        .ok()
        .map(|v| v.parse().expect("positive integer file count"))
        .unwrap_or(1);
    assert!(files > 0 && files <= count);
    let mixed = std::env::var_os("TS_CRAP_BENCH_MIXED").is_some();
    let real = std::env::var_os("TS_CRAP_BENCH_REAL").is_some();
    assert!(
        !(real && mixed),
        "choose real modules or synthetic mixed grammars"
    );
    let samples = [
        (include_str!("../examples/shop/src/pricing.mts"), 3),
        (include_str!("../examples/inventory/src/inventory.mts"), 4),
        (include_str!("../examples/retry/src/retry.mts"), 1),
    ];
    let mut expected_functions = 0;
    let mut entries = serde_json::Map::new();
    let mut total_bytes = 0;
    for index in 0..files {
        let extension = if mixed && index % 2 == 1 { "tsx" } else { "ts" };
        let name = format!("source-{index}.{extension}");
        let first = index * count / files;
        let last = (index + 1) * count / files;
        let mut source = if real {
            let (sample, functions) = samples[index % samples.len()];
            expected_functions += functions;
            sample.to_owned()
        } else {
            expected_functions += last - first;
            source
                .lines()
                .skip(first * 4)
                .take((last - first) * 4)
                .collect::<Vec<_>>()
                .join("\n")
        };
        if extension == "tsx" {
            source = source.replace("return 0;", "return <span>empty</span>;");
        }
        total_bytes += source.len();
        fs::write(temp.path().join(&name), &source).unwrap();
        let lines: BTreeMap<_, _> = (1..=source.lines().count())
            .map(|line| (line.to_string(), 1))
            .collect();
        entries.insert(
            name,
            serde_json::json!({
            "sha256":format!("{:x}",Sha256::digest(source.as_bytes())),"lines":lines
            }),
        );
    }
    let coverage = serde_json::json!({"version":1,"files":entries});
    let coverage_path = temp.path().join("coverage.json");
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
    let options = ScanOptions {
        root: temp.path().to_path_buf(),
        coverage: Some(coverage_path),
        ..Default::default()
    };
    assert_eq!(scan(&options).unwrap().functions.len(), expected_functions);
    let mut timings = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let report = black_box(scan(black_box(&options)).unwrap());
        assert_eq!(report.functions.len(), expected_functions);
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "functions={expected_functions} files={files} mixed={mixed} real={real} source_bytes={total_bytes} samples=5 median_ms={:.3} min_ms={:.3} max_ms={:.3}",
        timings[2], timings[0], timings[4]
    );
}
