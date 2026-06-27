mod app;
mod bootstrap;
mod cli;
mod error;
mod ext4;
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
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
