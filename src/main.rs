mod app;
mod bootstrap;
mod cli;
mod error;
mod ext4;
mod gui;
mod ioapic;
mod lapic;
mod model;
mod mptable;
mod native;
mod plan;
mod qemu;
mod rdp;
mod state;
mod virtio;
mod vmm_foundation;
mod wsl;

use std::process::ExitCode;

fn main() -> ExitCode {
    // Bare `pane` (or a double-click) opens the GUI Control Center; any subcommand runs the
    // CLI. A subcommand is the first non-flag argument.
    let has_subcommand = std::env::args().skip(1).any(|arg| !arg.starts_with('-'));
    if !has_subcommand {
        return match gui::run_gui() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        };
    }
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
