//! Small per-test fixtures; never mutate process-wide cwd or environment.
#![allow(clippy::unwrap_used)]
use std::{fs, path::Path, process::Command};

pub fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("ts crap ü ")
        .tempdir()
        .unwrap();
    for (name, source) in files {
        let path = directory.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    directory
}

pub fn node_script(name: &str) -> Command {
    let mut command = Command::new("node");
    command.arg(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join(name),
    );
    command
}
