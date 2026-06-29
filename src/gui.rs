//! Tauri desktop shell for Pane. Running `pane` with no subcommand opens this window (the
//! Control Center). The UI drives the engine by invoking the same `pane` CLI subcommands on
//! this very executable, so there is a single binary and one source of truth for behavior.

/// Run a Pane CLI subcommand on this executable and return its combined output. This is how
/// the GUI performs actions (launch/stop/provision/install-desktop/doctor/status).
///
/// `async` so Tauri runs it off the main thread: a long action (e.g. installing a desktop)
/// blocks a worker, not the UI, so the window stays responsive.
#[tauri::command]
async fn engine_run(args: Vec<String>) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = std::process::Command::new(exe)
        .args(&args)
        .output()
        .map_err(|error| format!("failed to run pane {}: {error}", args.join(" ")))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(text)
}

/// Open the Linux desktop in a native QEMU window (smooth, real input). Fire-and-forget: the
/// spawned `pane launch` (interactive) holds the window for its lifetime and is parented to
/// this long-lived GUI, so the window survives while Pane is open without blocking the UI.
#[tauri::command]
fn launch_vm(persist: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.args(["launch", "--runtime", "qemu-whpx", "--display", "gtk"]);
    if persist {
        command.arg("--persist-root");
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
        .spawn()
        .map_err(|error| format!("Failed to open the desktop window: {error}"))?;
    Ok(())
}

/// Stop the VM: graceful via the CLI if a detached VM is tracked, then make sure the engine
/// process (and thus the window) is gone.
#[tauri::command]
fn stop_vm() -> Result<String, String> {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(&exe).arg("stop").output();
    }
    for image in ["qemu-system-x86_64.exe", "pane-engine.exe"] {
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", image, "/F"])
            .output();
    }
    Ok("Stopped.".to_string())
}

/// Launch the Pane window. Blocks until the window closes.
pub fn run_gui() -> Result<(), String> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![engine_run, launch_vm, stop_vm])
        .run(tauri::generate_context!())
        .map_err(|error| format!("Pane window failed to start: {error}"))
}
