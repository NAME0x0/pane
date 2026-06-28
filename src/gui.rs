//! Tauri desktop shell for Pane. Running `pane` with no subcommand opens this window (the
//! Control Center). The UI drives the engine by invoking the same `pane` CLI subcommands on
//! this very executable, so there is a single binary and one source of truth for behavior.

/// Run a Pane CLI subcommand on this executable and return its combined output. This is how
/// the GUI performs actions (launch/stop/provision/install-desktop/doctor/status).
#[tauri::command]
fn engine_run(args: Vec<String>) -> Result<String, String> {
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

/// Launch the Pane window. Blocks until the window closes.
pub fn run_gui() -> Result<(), String> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![engine_run])
        .run(tauri::generate_context!())
        .map_err(|error| format!("Pane window failed to start: {error}"))
}
