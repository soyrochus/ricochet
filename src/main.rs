use ricochet::app::{self, Cli};
use std::{io, process::ExitCode};
fn execute() -> Result<(), String> {
    match app::parse(std::env::args().skip(1))? {
        Cli::Help => print!("{}", app::HELP),
        Cli::Version => println!("ricochet {}", env!("CARGO_PKG_VERSION")),
        Cli::Run { fps } => app::run(fps).map_err(|e: io::Error| e.to_string())?,
    }
    Ok(())
}
fn main() -> ExitCode {
    match app::restored_errors(execute) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ricochet: {error}");
            ExitCode::FAILURE
        }
    }
}
