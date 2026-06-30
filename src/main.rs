mod app;
mod bootstrap;
mod cli;
mod error;
mod ext4;
#[cfg(windows)]
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
    if std::env::var_os("PANE_APP_HYDRATE_ONLY").is_some() {
        return match app::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        };
    }

    // Bare `pane` (or a double-click) opens the GUI Control Center; any argument belongs
    // to the CLI so `pane --help` and other flag-only commands never launch a window.
    let has_cli_args = std::env::args_os().nth(1).is_some();
    if !has_cli_args {
        #[cfg(not(windows))]
        {
            eprintln!("Pane's GUI Control Center is currently Windows-only. Run a CLI subcommand instead.");
            return ExitCode::from(1);
        }
        #[cfg(windows)]
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
