mod cli;
mod output;
use std::process::ExitCode;

// musl's allocator takes a global lock that parallel analysis contends on.
// mimalloc's `override` feature also replaces C malloc for tree-sitter, but
// it only links because this static references it: keep it (CI checks that
// the musl build starts mimalloc).
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    match cli::execute() {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {}", ts_crap::escape_terminal(&format!("{error:#}")));
            ExitCode::from(2)
        }
    }
}
