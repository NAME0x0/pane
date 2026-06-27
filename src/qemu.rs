//! QEMU + Windows Hypervisor Platform (WHPX) engine boot path for Pane.
//!
//! Pane's from-scratch WHP run loop boots a Linux kernel but cannot drive the guest
//! timer fast enough on this host (the exit loop caps at ~15/sec, so jiffies crawl and
//! the root mount stalls). `qemu-system-x86_64 -accel whpx` runs on the same WHP
//! substrate but with a complete, mature VMM and boots Pane's Arch image end to end
//! (virtio root, switch_root, systemd, login) in seconds. This module drives QEMU as a
//! managed subprocess against Pane's registered kernel/initramfs/disk artifacts and
//! watches the serial console for boot milestones. QEMU runs as a separate process
//! (GPLv2-clean).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

/// Inputs for a QEMU-WHPX boot.
#[derive(Debug, Clone)]
pub struct QemuBootConfig {
    pub kernel: PathBuf,
    /// The real distro initramfs (with virtio-blk). NOT Pane's custom pane-block initramfs.
    pub initramfs: PathBuf,
    pub base_disk: PathBuf,
    /// Optional persistent qcow2 overlay backed by `base_disk`. When set it becomes the root
    /// disk (writable, changes survive reboots) instead of a throwaway snapshot of the base,
    /// so software installed in the guest (e.g. a desktop) persists. The base image stays
    /// untouched (it is the qcow2 backing file).
    pub root_overlay: Option<PathBuf>,
    /// Optional writable Pane user disk, attached as a second virtio drive (/dev/vdb).
    pub user_disk: Option<PathBuf>,
    pub memory_mb: u32,
    pub cmdline: String,
    pub serial_path: PathBuf,
    pub timeout: Duration,
    /// Boot the base disk copy-on-write so the verified base image is never modified.
    pub snapshot: bool,
    /// QEMU `-display` backend for an interactive boot (e.g. "gtk", "sdl"). None = headless
    /// serial console wired to this terminal (-nographic). Ignored by the probe path.
    pub display_backend: Option<String>,
}

/// Structured result of a QEMU-WHPX boot attempt.
#[derive(Debug, Clone, Serialize)]
pub struct QemuBootReport {
    pub qemu_path: Option<String>,
    pub launched: bool,
    pub reached_initrd: bool,
    pub mounted_sysroot: bool,
    pub switch_root: bool,
    pub reached_welcome: bool,
    pub reached_login: bool,
    pub user_disk_visible: bool,
    pub user_disk_mounted: bool,
    pub elapsed_seconds: u64,
    pub qemu_exit: Option<i32>,
    pub serial_bytes: u64,
    pub serial_tail: String,
    pub milestones: Vec<String>,
    pub detail: String,
}

/// Locate `qemu-system-x86_64.exe`: PATH first, then the standard winget/installer paths.
pub fn locate_qemu() -> Option<PathBuf> {
    // PATH probe: a successful `--version` confirms it runs.
    if Command::new("qemu-system-x86_64")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("qemu-system-x86_64"));
    }
    let candidates = [
        r"C:\Program Files\qemu\qemu-system-x86_64.exe",
        r"C:\Program Files\QEMU\qemu-system-x86_64.exe",
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

/// Build a `-drive` spec. `snapshot=on` makes writes copy-on-write and discarded at exit
/// (keeps the verified base image immutable); `snapshot=off` persists writes to the file.
fn drive_arg(path: &Path, format: &str, snapshot: bool) -> String {
    format!(
        "file={},format={format},if=virtio,snapshot={}",
        path.display(),
        if snapshot { "on" } else { "off" }
    )
}

/// Push the machine definition shared by every boot mode: WHPX accel, memory, the kernel +
/// distro initramfs, the base disk (virtio root) and optional user disk (virtio vdb), the
/// kernel cmdline, and copy-on-write snapshot of the base image.
fn push_machine_args(command: &mut Command, config: &QemuBootConfig) {
    command.args([
        "-accel",
        "whpx",
        "-m",
        &config.memory_mb.to_string(),
        "-smp",
        "1",
        "-kernel",
        &config.kernel.display().to_string(),
        "-initrd",
        &config.initramfs.display().to_string(),
    ]);
    match config.root_overlay.as_ref().filter(|p| p.exists()) {
        // Persistent root: qcow2 overlay backed by the base image; guest writes survive.
        Some(overlay) => command.args(["-drive", &drive_arg(overlay, "qcow2", false)]),
        // Ephemeral root: copy-on-write so the verified, SHA-pinned base image is untouched.
        None => command.args(["-drive", &drive_arg(&config.base_disk, "raw", config.snapshot)]),
    };
    if let Some(user_disk) = config.user_disk.as_ref().filter(|p| p.exists()) {
        // User disk: persistent (snapshot=off) so packages/home survive across boots.
        command.args(["-drive", &drive_arg(user_disk, "qcow2", false)]);
    }
    // User-mode networking: the guest gets NAT internet (eth0 via DHCP) with no host setup,
    // so package managers work. virtio-net for performance.
    command.args([
        "-netdev",
        "user,id=net0",
        "-device",
        "virtio-net-pci,netdev=net0",
    ]);
    command.args(["-append", &config.cmdline]);
}

/// Create a persistent qcow2 overlay backed by `base_image` at `overlay` if absent. The
/// overlay holds all guest root writes; the base image is the read-only backing file and is
/// never modified. Lets a desktop / extra packages installed in the guest persist.
pub fn ensure_qcow2_overlay(overlay: &Path, base_image: &Path) -> Result<(), String> {
    if overlay.exists() {
        return Ok(());
    }
    let qemu_img = locate_qemu_img().ok_or_else(|| "qemu-img not found".to_string())?;
    if let Some(parent) = overlay.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let output = Command::new(&qemu_img)
        .args([
            "create",
            "-f",
            "qcow2",
            "-F",
            "raw",
            "-b",
            &base_image.display().to_string(),
            &overlay.display().to_string(),
        ])
        .output()
        .map_err(|error| format!("failed to run qemu-img: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "qemu-img create overlay failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Boot the configured artifacts through QEMU-WHPX as an interactive session: the guest
/// serial console is wired straight to this process's stdio, so the user gets a live Linux
/// shell. Blocks until QEMU exits (Ctrl-A X). Returns QEMU's exit status.
pub fn boot_interactive(config: &QemuBootConfig) -> Result<std::process::ExitStatus, String> {
    let qemu = locate_qemu().ok_or_else(|| {
        "qemu-system-x86_64 not found on PATH or in C:\\Program Files\\qemu. Install QEMU (winget install SoftwareFreedomConservancy.QEMU).".to_string()
    })?;
    for required in [&config.kernel, &config.initramfs, &config.base_disk] {
        if !required.exists() {
            return Err(format!("Required artifact missing: {}", required.display()));
        }
    }
    let mut command = Command::new(&qemu);
    push_machine_args(&mut command, config);
    match config.display_backend.as_deref() {
        None => {
            // -nographic wires the serial console to this process's stdio for an interactive
            // shell (Ctrl-A X to quit, Ctrl-A C for the QEMU monitor).
            command.arg("-nographic");
            command
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
        }
        Some(backend) => {
            // Graphical: virtio-vga adapter rendered in a QEMU window; the serial console is
            // captured to a file for diagnostics. The window is the interactive surface.
            command.args([
                "-vga",
                "virtio",
                "-display",
                backend,
                "-serial",
                &format!("file:{}", config.serial_path.display()),
            ]);
        }
    }
    command
        .status()
        .map_err(|error| format!("Failed to launch QEMU: {error}"))
}

/// Launch QEMU-WHPX detached and return its process id. The guest keeps running after Pane
/// exits: a graphical backend shows a standalone window; otherwise it runs headless with the
/// serial console captured to a file. Use the returned pid (e.g. via `pane stop`) to end it.
pub fn boot_detached(config: &QemuBootConfig) -> Result<u32, String> {
    let qemu = locate_qemu().ok_or_else(|| {
        "qemu-system-x86_64 not found on PATH or in C:\\Program Files\\qemu. Install QEMU (winget install SoftwareFreedomConservancy.QEMU).".to_string()
    })?;
    for required in [&config.kernel, &config.initramfs, &config.base_disk] {
        if !required.exists() {
            return Err(format!("Required artifact missing: {}", required.display()));
        }
    }
    let _ = std::fs::remove_file(&config.serial_path);
    let mut command = Command::new(&qemu);
    push_machine_args(&mut command, config);
    match config.display_backend.as_deref() {
        Some(backend) => {
            command.args(["-vga", "virtio", "-display", backend]);
        }
        None => {
            command.args(["-display", "none", "-monitor", "none"]);
        }
    }
    command.args([
        "-serial",
        &format!("file:{}", config.serial_path.display()),
    ]);
    // Detached: no inherited console. The child outlives this process (Child drop does not
    // kill it on Windows), so it keeps running until stopped via its pid.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = command
        .spawn()
        .map_err(|error| format!("Failed to launch QEMU: {error}"))?;
    Ok(child.id())
}

/// Locate `qemu-img.exe` next to `qemu-system-x86_64`, or on PATH.
pub fn locate_qemu_img() -> Option<PathBuf> {
    if let Some(system) = locate_qemu() {
        if let Some(dir) = system.parent() {
            let sibling = dir.join("qemu-img.exe");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }
    if Command::new("qemu-img")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("qemu-img"));
    }
    None
}

/// Create a sparse qcow2 disk of `capacity_gib` at `path` if it does not exist. qcow2 is
/// QEMU's native sparse format, so the file stays small until the guest writes. The guest
/// formats + mounts it on first boot (x-systemd.makefs), so Pane ships an empty disk.
pub fn ensure_qcow2_disk(path: &Path, capacity_gib: u64) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let qemu_img = locate_qemu_img().ok_or_else(|| "qemu-img not found".to_string())?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let output = Command::new(&qemu_img)
        .args([
            "create",
            "-f",
            "qcow2",
            &path.display().to_string(),
            &format!("{capacity_gib}G"),
        ])
        .output()
        .map_err(|error| format!("failed to run qemu-img: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "qemu-img create failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Boot the configured artifacts through QEMU-WHPX, watching the serial console for
/// milestones until login is reached or the timeout expires, then terminate QEMU.
pub fn boot_via_qemu_whpx(config: &QemuBootConfig) -> QemuBootReport {
    let mut report = QemuBootReport {
        qemu_path: None,
        launched: false,
        reached_initrd: false,
        mounted_sysroot: false,
        switch_root: false,
        reached_welcome: false,
        reached_login: false,
        user_disk_visible: false,
        user_disk_mounted: false,
        elapsed_seconds: 0,
        qemu_exit: None,
        serial_bytes: 0,
        serial_tail: String::new(),
        milestones: Vec::new(),
        detail: String::new(),
    };

    let Some(qemu) = locate_qemu() else {
        report.detail =
            "qemu-system-x86_64 not found on PATH or in C:\\Program Files\\qemu. Install QEMU (winget install SoftwareFreedomConservancy.QEMU).".to_string();
        return report;
    };
    report.qemu_path = Some(qemu.display().to_string());

    for required in [&config.kernel, &config.initramfs, &config.base_disk] {
        if !required.exists() {
            report.detail = format!("Required artifact missing: {}", required.display());
            return report;
        }
    }

    // Best-effort fresh serial file so we only read this boot's output.
    let _ = std::fs::remove_file(&config.serial_path);

    let mut command = Command::new(&qemu);
    push_machine_args(&mut command, config);
    command.args([
        // Headless probe: no graphics, monitor off stdio so only Pane's report prints.
        // Serial console goes to a file Pane tails for milestones.
        "-display",
        "none",
        "-monitor",
        "none",
        "-no-reboot",
        "-serial",
        &format!("file:{}", config.serial_path.display()),
    ]);

    let started_at = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            report.detail = format!("Failed to launch QEMU: {error}");
            return report;
        }
    };
    report.launched = true;

    // When a user disk is attached we want to observe it formatting + mounting, which can
    // lag the login prompt slightly (makefs runs during local-fs target).
    let expect_user_disk = config
        .user_disk
        .as_ref()
        .is_some_and(|path| path.exists());
    let mut welcome_at: Option<Instant> = None;
    loop {
        if started_at.elapsed() >= config.timeout {
            report.detail = if report.reached_welcome {
                "QEMU-WHPX booted the distro to userspace (login prompt not observed within the timeout).".to_string()
            } else {
                "Timed out before the QEMU guest reached userspace.".to_string()
            };
            break;
        }
        let serial = read_serial(&config.serial_path);
        update_milestones(&mut report, &serial);
        if report.reached_login && (!expect_user_disk || report.user_disk_mounted) {
            report.detail = if report.user_disk_mounted {
                "QEMU-WHPX booted the distro to login and mounted the Pane user disk."
                    .to_string()
            } else {
                "QEMU-WHPX booted the distro all the way to the login prompt.".to_string()
            };
            break;
        }
        // Reaching userspace ("Welcome" + switch_root) is success; keep watching ~25s more
        // for the login prompt and the user disk (/dev/vdb) enumeration, then stop.
        if report.reached_welcome && report.switch_root {
            let since = *welcome_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(25) {
                report.detail =
                    "QEMU-WHPX booted the distro to userspace (no login prompt within the post-welcome window).".to_string();
                break;
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                report.qemu_exit = status.code();
                report.detail = "QEMU exited before reaching login.".to_string();
                break;
            }
            Ok(None) => {}
            Err(error) => {
                report.detail = format!("Error polling QEMU: {error}");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let _ = child.kill();
    let _ = child.wait();

    let serial = read_serial(&config.serial_path);
    update_milestones(&mut report, &serial);
    report.serial_bytes = serial.len() as u64;
    report.serial_tail = serial_tail(&serial, 2000);
    report.elapsed_seconds = started_at.elapsed().as_secs();
    report
}

fn read_serial(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn note(report: &mut QemuBootReport, flag: bool, marker: &str) -> bool {
    if flag && !report.milestones.iter().any(|m| m == marker) {
        report.milestones.push(marker.to_string());
    }
    flag
}

fn update_milestones(report: &mut QemuBootReport, serial: &str) {
    report.reached_initrd |= note(
        report,
        serial.contains("Booting initrd of") || serial.contains("Loading initial ramdisk"),
        "initrd",
    );
    report.mounted_sysroot |= note(report, serial.contains("Mounted /sysroot"), "mounted-sysroot");
    report.switch_root |= note(
        report,
        serial.contains("Switch Root") || serial.contains("switch_root"),
        "switch-root",
    );
    report.reached_welcome |= note(report, serial.contains("Welcome to"), "welcome");
    report.reached_login |= note(
        report,
        serial.contains("login:") || serial.contains("(automatic login)"),
        "login",
    );
    report.user_disk_visible |= note(
        report,
        serial.contains("vdb") || serial.contains("/dev/vdb"),
        "user-disk-vdb",
    );
    // systemd formats (first boot) then mounts /dev/vdb at /home. "Mounted /home" appears on
    // every boot; the makefs line appears whenever the format unit runs. Either proves the
    // persistent user disk is in service.
    report.user_disk_mounted |= note(
        report,
        serial.contains("Mounted /home") || serial.contains("Make File System on /dev/vdb"),
        "user-disk-mounted",
    );
}

fn serial_tail(serial: &str, max_bytes: usize) -> String {
    let start = serial.len().saturating_sub(max_bytes);
    // Strip ANSI escapes and non-printables for a readable tail.
    serial[start..]
        .chars()
        .map(|c| {
            if c == '\n' || c == '\t' || (' '..='~').contains(&c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
}
