mod cli;
mod output;
use std::process::ExitCode;

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
