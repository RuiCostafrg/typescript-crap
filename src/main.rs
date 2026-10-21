mod cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::execute() {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}
