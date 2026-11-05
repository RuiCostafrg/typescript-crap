//! Fresh-process measurements of committed-byte verification, excluding parsing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../src/selection/committed.rs"]
// The private test module is included by Cargo's bench cfg but has no harness.
#[allow(dead_code, unused_imports)]
mod committed;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "32".into())
        .parse()
        .unwrap();
    assert!(mib > 0);
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    // Construct the captured source directly, without an equally large temporary.
    let mut source = String::with_capacity(mib * 1024 * 1024);
    let line = "export const value = 'hello';\r\n";
    while source.len() + line.len() <= mib * 1024 * 1024 {
        source.push_str(line);
    }
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let oid = String::from_utf8(output.stdout).unwrap();
    let mut reader = committed::BlobReader::new(root.path()).unwrap();
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        reader
            .verify(oid.trim(), &root.path().join("source.ts"), &source)
            .unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    reader.finish().unwrap();
    let rss = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmHWM:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<usize>().ok())
        });
    println!(
        "{}",
        serde_json::json!({"bytes":source.len(),"samples_ms":samples,"process_peak_rss_kib":rss})
    );
}
