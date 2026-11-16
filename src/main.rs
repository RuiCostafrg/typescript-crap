mod cli;
mod output;
use std::process::ExitCode;

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
