//! Tauri desktop shell for Pane. Running `pane` with no subcommand opens this window (the
//! Control Center). The UI drives the engine by invoking the same `pane` CLI subcommands on
//! this very executable, so there is a single binary and one source of truth for behavior.

use serde::Serialize;

#[derive(Debug, Serialize)]
struct RecommendedSpecs {
    logical_cores: u32,
    total_memory_mb: u64,
    recommended_vcpus: u32,
    recommended_memory_mb: u64,
    recommended_disk_gib: u64,
    recommended_resolution: String,
    gpu_acceleration_supported: bool,
    gpu_name: Option<String>,
    free_disk_gib: Option<u64>,
}

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

#[tauri::command]
fn recommended_specs() -> RecommendedSpecs {
    let (recommended_vcpus, recommended_memory_mb) = crate::qemu::host_resources();
    RecommendedSpecs {
        logical_cores: crate::qemu::host_logical_cores(),
        total_memory_mb: crate::qemu::host_memory_mb(),
        recommended_vcpus,
        recommended_memory_mb,
        recommended_disk_gib: 24,
        recommended_resolution: "1920x1080".to_string(),
        gpu_acceleration_supported: true,
        gpu_name: detect_gpu_name(),
        free_disk_gib: detect_free_disk_gib(),
    }
}

fn powershell_first_line(script: &str) -> Option<String> {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn detect_gpu_name() -> Option<String> {
    powershell_first_line(
        "Get-CimInstance Win32_VideoController | Select-Object -First 1 -ExpandProperty Name",
    )
}

fn detect_free_disk_gib() -> Option<u64> {
    powershell_first_line("[math]::Floor((Get-PSDrive -Name $env:SystemDrive[0]).Free / 1GB)")
        .and_then(|value| value.parse::<u64>().ok())
}

/// Open the Linux desktop in a native QEMU window (smooth, real input). Fire-and-forget: the
/// spawned `pane launch` (interactive) holds the window for its lifetime and is parented to
/// this long-lived GUI, so the window survives while Pane is open without blocking the UI.
#[tauri::command]
fn launch_vm(
    persist: bool,
    vcpus: Option<u32>,
    memory_mb: Option<u32>,
    disk_gib: Option<u64>,
    resolution: Option<String>,
    gpu_acceleration: bool,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.args(["launch", "--runtime", "qemu-whpx", "--display", "gtk"]);
    if persist {
        command.arg("--persist-root");
    }
    if let Some(vcpus) = vcpus {
        command.arg("--vcpus").arg(vcpus.to_string());
    }
    if let Some(memory_mb) = memory_mb {
        command.arg("--memory-mb").arg(memory_mb.to_string());
    }
    if let Some(disk_gib) = disk_gib {
        command.arg("--disk-gib").arg(disk_gib.to_string());
    }
    if let Some(resolution) = resolution.filter(|value| !value.trim().is_empty()) {
        command.arg("--resolution").arg(resolution.trim());
    }
    if !gpu_acceleration {
        command.arg("--no-gpu-acceleration");
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
        .invoke_handler(tauri::generate_handler![
            engine_run,
            recommended_specs,
            launch_vm,
            stop_vm
        ])
        .run(tauri::generate_context!())
        .map_err(|error| format!("Pane window failed to start: {error}"))
}
