mod app;
mod bootstrap;
mod cli;
mod error;
mod model;
mod native;
mod plan;
mod rdp;
mod state;
mod wsl;

use std::process::ExitCode;

fn main() -> ExitCode {
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
