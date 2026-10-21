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
    fs::write(temp.path().join("source.ts"), &source).unwrap();
    let lines: BTreeMap<_, _> = (1..=source.lines().count())
        .map(|line| (line.to_string(), 1))
        .collect();
    let coverage = serde_json::json!({"version":1,"files":{"source.ts":{
        "sha256":format!("{:x}",Sha256::digest(source.as_bytes())),"lines":lines
    }}});
    let coverage_path = temp.path().join("coverage.json");
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
    let options = ScanOptions {
        root: temp.path().to_path_buf(),
        coverage: Some(coverage_path),
        ..Default::default()
    };
    assert_eq!(scan(&options).unwrap().functions.len(), count);
    let mut timings = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let report = black_box(scan(black_box(&options)).unwrap());
        assert_eq!(report.functions.len(), count);
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "functions={count} source_bytes={} samples=5 median_ms={:.3} min_ms={:.3} max_ms={:.3}",
        source.len(),
        timings[2],
        timings[0],
        timings[4]
    );
}
