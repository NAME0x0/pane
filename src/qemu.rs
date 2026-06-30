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

#[cfg(windows)]
pub fn host_memory_mb() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
        status.ullTotalPhys / (1024 * 1024)
    } else {
        4096
    }
}

#[cfg(not(windows))]
pub fn host_memory_mb() -> u64 {
    4096
}

/// vCPU count and guest RAM (MB) scaled to the host with sane caps, so Pane runs fast on
/// big machines without over-subscribing small ones: vCPUs = logical cores clamped to
/// [2, 8]; RAM = half of physical clamped to [2048, 8192] MB.
pub fn host_resources() -> (u32, u64) {
    let cores = host_logical_cores();
    let vcpus = cores.clamp(2, 8);
    let ram_mb = (host_memory_mb() / 2).clamp(2048, 8192);
    (vcpus, ram_mb)
}

pub fn host_logical_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
}

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
    pub vcpus: u32,
    pub cmdline: String,
    pub serial_path: PathBuf,
    pub timeout: Duration,
    /// Boot the base disk copy-on-write so the verified base image is never modified.
    pub snapshot: bool,
    /// QEMU `-display` backend for an interactive boot (e.g. "gtk", "sdl"). None = headless
    /// serial console wired to this terminal (-nographic). Ignored by the probe path.
    pub display_backend: Option<String>,
    /// Use VirGL-backed virtio-gpu for graphical desktops. False falls back to software VGA.
    pub gpu_acceleration: bool,
    /// Optional Linux display hint, passed as `video=Virtual-1:<width>x<height>`.
    pub display_resolution: Option<(u32, u32)>,
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

/// Locate the QEMU engine: a bundled `pane-engine.exe` shipped next to pane.exe wins (so
/// the process shows as Pane and works offline); then PATH; then the standard installer
/// paths (winget fallback).
pub fn locate_qemu() -> Option<PathBuf> {
    // Bundled engine next to the running executable (and a couple of common layouts).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for bundled in [
                dir.join("pane-engine.exe"),
                dir.join("engine").join("pane-engine.exe"),
            ] {
                if bundled.exists() {
                    return Some(bundled);
                }
            }
        }
    }
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
/// TCP port for the QMP control channel of a detached VM, so `pane stop` can request a
/// clean ACPI shutdown. Single-session assumption; revisit if Pane runs concurrent VMs.
const QMP_TCP_PORT: u16 = 44510;

/// TCP port for the serial console during one-time provisioning (driving the autologin
/// root shell). Separate from QMP; single-session assumption.
const SERIAL_TCP_PORT: u16 = 44511;

/// The QMP control port used for detached VMs.
pub fn detached_qmp_port() -> u16 {
    QMP_TCP_PORT
}

/// Request a clean ACPI shutdown of a detached VM over QMP (system_powerdown). The guest's
/// init handles the power button and shuts down; the caller should wait for the process to
/// exit and hard-kill only as a fallback.
pub fn graceful_shutdown(qmp_port: u16) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", qmp_port))
        .map_err(|error| format!("QMP connect failed on port {qmp_port}: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    let mut scratch = [0u8; 2048];
    // Server greeting, then capabilities negotiation is required before other commands.
    let _ = stream.read(&mut scratch);
    stream
        .write_all(b"{\"execute\":\"qmp_capabilities\"}\r\n")
        .map_err(|error| format!("QMP handshake write failed: {error}"))?;
    let _ = stream.read(&mut scratch);
    stream
        .write_all(b"{\"execute\":\"system_powerdown\"}\r\n")
        .map_err(|error| format!("QMP system_powerdown failed: {error}"))?;
    let _ = stream.read(&mut scratch);
    Ok(())
}

/// Ensure QEMU is available, installing it via winget on first use if absent. Returns the
/// resolved qemu-system path. winget output is shown so the user sees install progress.
pub fn ensure_qemu_available() -> Result<PathBuf, String> {
    if let Some(qemu) = locate_qemu() {
        return Ok(qemu);
    }
    println!("QEMU not found. Installing it via winget (SoftwareFreedomConservancy.QEMU)...");
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            "SoftwareFreedomConservancy.QEMU",
            "-e",
            "--accept-source-agreements",
            "--accept-package-agreements",
        ])
        .status()
        .map_err(|error| {
            format!("Could not run winget to install QEMU: {error}. Install it manually: winget install SoftwareFreedomConservancy.QEMU")
        })?;
    if !status.success() {
        return Err("winget failed to install QEMU. Install it manually: winget install SoftwareFreedomConservancy.QEMU".to_string());
    }
    locate_qemu().ok_or_else(|| {
        "QEMU was installed but could not be located. Open a new terminal and retry.".to_string()
    })
}

fn drive_arg(path: &Path, format: &str, snapshot: bool) -> String {
    // Performance: async-threads I/O always. Ephemeral (snapshot) drives use the fastest
    // cache since writes are thrown away; persistent drives use writeback plus discard/unmap
    // so files deleted in the guest reclaim host space via TRIM (fstrim / fstrim.timer).
    let perf = if snapshot {
        "snapshot=on,cache=unsafe,aio=threads"
    } else {
        "snapshot=off,cache=writeback,discard=unmap,detect-zeroes=unmap,aio=threads"
    };
    format!("file={},format={format},if=virtio,{perf}", path.display())
}

/// VNC display number for the embedded view (TCP port 5900 + this) and the websocket port
/// noVNC connects to. Single-session assumption.
const VNC_DISPLAY: u16 = 0;
const VNC_WS_PORT: u16 = 5700;

/// The websocket port noVNC connects to for the embedded display.
#[allow(dead_code)]
pub fn vnc_websocket_port() -> u16 {
    VNC_WS_PORT
}

/// QEMU args for a graphical window. GPU mode uses VirGL; compatibility mode keeps the
/// old software-rendered stdvga path.
fn graphical_display_args(backend: &str, gpu_acceleration: bool) -> Vec<String> {
    if !gpu_acceleration {
        return vec![
            "-vga".to_string(),
            "std".to_string(),
            "-display".to_string(),
            format!("{backend},gl=off"),
        ];
    }
    // GPU-accelerated: virtio-gpu-gl + gl=on translates guest OpenGL to the host GPU (VirGL),
    // so GNOME/KDE/XFCE render with hardware acceleration in the native window.
    vec![
        "-device".to_string(),
        "virtio-gpu-gl-pci".to_string(),
        "-display".to_string(),
        format!("{backend},gl=on"),
    ]
}

/// Display args for a backend. "vnc" = headless VNC server + websocket (rendered by noVNC in
/// the Pane window); anything else = a native gtk/sdl window.
fn display_args_for(backend: &str, gpu_acceleration: bool) -> Vec<String> {
    if backend == "vnc" {
        if !gpu_acceleration {
            return vec![
                "-monitor".to_string(),
                "none".to_string(),
                "-display".to_string(),
                "none".to_string(),
                "-vga".to_string(),
                "std".to_string(),
                "-vnc".to_string(),
                format!("127.0.0.1:{VNC_DISPLAY},websocket={VNC_WS_PORT}"),
            ];
        }
        vec![
            // Headless, accelerated: egl-headless gives a host GL context and
            // virtio-gpu-gl translates guest OpenGL to the host GPU (VirGL), so GNOME/KDE
            // get real GL acceleration. The framebuffer is served over VNC + websocket.
            "-monitor".to_string(),
            "none".to_string(),
            "-display".to_string(),
            "egl-headless".to_string(),
            "-device".to_string(),
            "virtio-gpu-gl-pci".to_string(),
            "-vnc".to_string(),
            format!("127.0.0.1:{VNC_DISPLAY},websocket={VNC_WS_PORT}"),
        ]
    } else {
        graphical_display_args(backend, gpu_acceleration)
    }
}

/// Push the machine definition shared by every boot mode: WHPX accel, memory, the kernel +
/// distro initramfs, the base disk (virtio root) and optional user disk (virtio vdb), the
/// kernel cmdline, and copy-on-write snapshot of the base image.
fn push_machine_args(command: &mut Command, config: &QemuBootConfig) {
    command.args([
        "-accel",
        "whpx",
        // Brand the guest window/title as Pane (not "QEMU").
        "-name",
        "Pane",
        // Modern CPU model (AVX2/SSE4 etc.) for speed. WHPX rejects "host"/"max" (APX/MPX
        // feature conflicts kill the guest before it boots); Skylake-Client is feature-rich
        // and WHPX-compatible.
        "-cpu",
        "Skylake-Client",
        "-m",
        &config.memory_mb.to_string(),
        // Scale vCPUs to the host so the desktop is responsive.
        "-smp",
        &config.vcpus.to_string(),
        "-kernel",
        &config.kernel.display().to_string(),
        "-initrd",
        &config.initramfs.display().to_string(),
    ]);
    match config.root_overlay.as_ref().filter(|p| p.exists()) {
        // Persistent root: qcow2 overlay backed by the base image; guest writes survive.
        Some(overlay) => command.args(["-drive", &drive_arg(overlay, "qcow2", false)]),
        // Ephemeral root: copy-on-write so the verified, SHA-pinned base image is untouched.
        None => command.args([
            "-drive",
            &drive_arg(&config.base_disk, "raw", config.snapshot),
        ]),
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
        // Entropy source so key generation (pacman-key --init) and TLS do not stall.
        "-device",
        "virtio-rng-pci",
        // Absolute pointer + USB keyboard for graphical desktops. Relying on the default
        // PS/2 mouse through GTK/GL makes pointer capture feel laggy or stuck.
        "-device",
        "qemu-xhci,id=pane-usb",
        "-device",
        "usb-tablet,bus=pane-usb.0",
        "-device",
        "usb-kbd,bus=pane-usb.0",
    ]);
    command.args(["-append", &config.cmdline]);
}

/// Grow a qcow2 disk's virtual size to `gib` GiB (no-op if already that big or larger). The
/// guest must then extend the partition + filesystem. Only grows; never shrinks.
pub fn resize_qcow2(disk: &Path, gib: u64) -> Result<(), String> {
    let qemu_img = locate_qemu_img().ok_or_else(|| "qemu-img not found".to_string())?;
    let output = Command::new(&qemu_img)
        .args(["resize", &disk.display().to_string(), &format!("{gib}G")])
        .output()
        .map_err(|error| format!("failed to run qemu-img resize: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Refusing to shrink is fine — the disk is already at least this large.
        if stderr.contains("shrink") || stderr.contains("smaller") {
            return Ok(());
        }
        return Err(format!("qemu-img resize failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Compact a qcow2 by rewriting it (drops free space that TRIM/deletes freed). `backing`
/// preserves the overlay's backing file so only live deltas are kept.
pub fn compact_qcow2(disk: &Path, backing: Option<&Path>) -> Result<(), String> {
    let qemu_img = locate_qemu_img().ok_or_else(|| "qemu-img not found".to_string())?;
    let tmp = disk.with_extension("compact.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut command = Command::new(&qemu_img);
    command.args(["convert", "-f", "qcow2", "-O", "qcow2"]);
    if let Some(base) = backing {
        command.args([
            "-o",
            &format!("backing_file={},backing_fmt=raw", base.display()),
        ]);
    }
    command.arg(disk).arg(&tmp);
    let output = command
        .output()
        .map_err(|error| format!("failed to run qemu-img convert: {error}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "qemu-img compact failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    std::fs::rename(&tmp, disk).map_err(|error| format!("replace disk after compact: {error}"))?;
    Ok(())
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
            // Graphical (native window) or headless VNC; serial captured to a file.
            command.args(display_args_for(backend, config.gpu_acceleration));
            command.args(["-serial", &format!("file:{}", config.serial_path.display())]);
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
            command.args(display_args_for(backend, config.gpu_acceleration));
        }
        None => {
            command.args(["-display", "none", "-monitor", "none"]);
        }
    }
    command.args([
        "-serial",
        &format!("file:{}", config.serial_path.display()),
        // QMP control channel so `pane stop` can request a clean ACPI shutdown.
        "-qmp",
        &format!("tcp:127.0.0.1:{QMP_TCP_PORT},server,nowait"),
    ]);
    // Detached: no inherited console. The child outlives this process (Child drop does not
    // kill it on Windows), so it keeps running until stopped via its pid. QEMU stderr is
    // captured next to the serial log for diagnostics.
    let stderr_path = config.serial_path.with_extension("stderr.log");
    let stderr = std::fs::File::create(&stderr_path)
        .map(std::process::Stdio::from)
        .unwrap_or_else(|_| std::process::Stdio::null());
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB so a graphical window survives the
        // launcher exiting (otherwise a job/console teardown closes it).
        command.creation_flags(0x0000_0008 | 0x0100_0000);
    }
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
    let expect_user_disk = config.user_disk.as_ref().is_some_and(|path| path.exists());
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
                "QEMU-WHPX booted the distro to login and mounted the Pane user disk.".to_string()
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

/// Run one-time provisioning commands as root in the guest, then power off. The image
/// autologins root on the serial console, so Pane connects QEMU's serial to a Windows named
/// pipe and feeds the commands to that already-logged-in shell. No image edit, no firstboot,
/// no ext4 write; changes persist when `config` uses a persistent root overlay. Reusable for
/// any root-shell provisioning (credentials now, desktop install later).
pub fn provision_via_serial(
    config: &QemuBootConfig,
    commands: &[String],
    completion_timeout: Duration,
) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    let qemu = locate_qemu().ok_or_else(|| "qemu-system-x86_64 not found".to_string())?;
    for required in [&config.kernel, &config.initramfs, &config.base_disk] {
        if !required.exists() {
            return Err(format!("Required artifact missing: {}", required.display()));
        }
    }
    // Expose the guest serial console over a TCP socket. TCP avoids the Windows named-pipe
    // open() blocking trap and works cleanly with std's networking.
    let mut command = Command::new(&qemu);
    push_machine_args(&mut command, config);
    command.args([
        "-display",
        "none",
        "-monitor",
        "none",
        "-serial",
        &format!("tcp:127.0.0.1:{SERIAL_TCP_PORT},server,nowait"),
    ]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to launch QEMU: {error}"))?;

    // Connect to QEMU's serial socket (retry until it is listening; connect fails fast when
    // it is not, so the deadline is honored).
    let connect_deadline = Instant::now() + Duration::from_secs(30);
    let stream = loop {
        match std::net::TcpStream::connect(("127.0.0.1", SERIAL_TCP_PORT)) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < connect_deadline => {
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!(
                    "Could not connect to the guest serial socket: {error}"
                ));
            }
        }
    };
    let mut writer = stream
        .try_clone()
        .map_err(|error| format!("Could not duplicate the serial socket: {error}"))?;

    // Drain guest output into a shared buffer so we can wait for the login prompt.
    let buffer = Arc::new(Mutex::new(String::new()));
    let buffer_reader = buffer.clone();
    let log_path = config.serial_path.clone();
    let mut reader = stream;
    std::thread::spawn(move || {
        let mut chunk = [0u8; 1024];
        let mut last_flush = Instant::now();
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut text) = buffer_reader.lock() {
                        text.push_str(&String::from_utf8_lossy(&chunk[..n]));
                        // Flush the live transcript periodically so long installs are watchable.
                        if last_flush.elapsed() >= Duration::from_secs(2) {
                            let _ = std::fs::write(&log_path, text.as_bytes());
                            last_flush = Instant::now();
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let wait_for = |needle: &str, secs: u64| -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if buffer.lock().map(|t| t.contains(needle)).unwrap_or(false) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    };
    let wait_for_provision_status = || -> Option<i32> {
        let deadline = Instant::now() + completion_timeout;
        loop {
            if let Ok(text) = buffer.lock() {
                if let Some(status) = parse_provision_status(&text) {
                    return Some(status);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    };

    let mut send = |line: &str| -> Result<(), String> {
        writer
            .write_all(line.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .map_err(|error| format!("Could not write to the guest serial: {error}"))?;
        let _ = writer.flush();
        std::thread::sleep(Duration::from_millis(700));
        Ok(())
    };

    // The serial getty autologins root; wait for that, then let the shell settle.
    if !wait_for("automatic login", 120) && !wait_for("pane-arch", 5) {
        let _ = child.kill();
        return Err("Guest did not reach the autologin root shell in time.".to_string());
    }
    std::thread::sleep(Duration::from_secs(3));

    // Execute provisioning as one fail-fast script. Sending commands one-by-one allowed a
    // failed pacman/keyring step to be followed by later commands, making the UI report
    // success even though the desktop was not actually installed.
    send("cat > /tmp/pane-provision.sh <<'PANE_PROVISION_EOF'")?;
    send("set -e")?;
    for line in commands {
        send(line)?;
    }
    send("PANE_PROVISION_EOF")?;
    send("bash /tmp/pane-provision.sh")?;
    send("echo PANE_PROV_DONE_$?")?;
    let provision_status = wait_for_provision_status();
    let _ = send("sync");
    let _ = send("poweroff -f");

    // Wait for the guest to power off; force only if it overruns.
    let exit_deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < exit_deadline => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Ok(None) => {
                let _ = child.kill();
                break;
            }
            Err(_) => break,
        }
    }
    // Save the transcript for diagnostics (provisioning uses a socket, not the serial file).
    if let Ok(text) = buffer.lock() {
        let _ = std::fs::write(&config.serial_path, text.as_bytes());
    }
    match provision_status {
        Some(0) => {}
        Some(status) => {
            return Err(format!(
                "Provisioning failed with exit status {status}; see {}",
                config.serial_path.display()
            ));
        }
        None => {
            return Err(format!(
                "Provisioning did not confirm success within {}s; see {}",
                completion_timeout.as_secs(),
                config.serial_path.display()
            ));
        }
    }
    Ok(())
}

fn parse_provision_status(text: &str) -> Option<i32> {
    let marker = "PANE_PROV_DONE_";
    let start = text.rfind(marker)? + marker.len();
    let digits: String = text[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn read_serial(path: &Path) -> String {
    let raw = std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    strip_ansi(&raw)
}

/// Remove ANSI/VT escape sequences. systemd colorizes status lines and embeds highlight
/// codes inside the message (e.g. "Mounted \e[..m/sysroot\e[0m."), which breaks plain
/// substring milestone matching. Handles CSI, OSC, and DCS/SOS/PM/APC sequences.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                // CSI: ends at a final byte in 0x40..=0x7E.
                chars.next();
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: ends at BEL or ST (ESC \).
                chars.next();
                let mut saw_escape = false;
                for n in chars.by_ref() {
                    if saw_escape {
                        if n == '\\' {
                            break;
                        }
                        saw_escape = false;
                    }
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        saw_escape = true;
                    }
                }
            }
            Some('P') | Some('X') | Some('^') | Some('_') => {
                // DCS/SOS/PM/APC: ends at ST (ESC \).
                chars.next();
                let mut saw_escape = false;
                for n in chars.by_ref() {
                    if saw_escape {
                        if n == '\\' {
                            break;
                        }
                        saw_escape = false;
                    }
                    if n == '\u{1b}' {
                        saw_escape = true;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
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
    report.mounted_sysroot |= note(
        report,
        serial.contains("Mounted /sysroot"),
        "mounted-sysroot",
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_systemd_highlight_codes() {
        // systemd colorizes the unit name inside the message; plain matching must still work.
        let line = "[  OK  ] Mounted \u{1b}[0;1;39m/sysroot\u{1b}[0m.";
        assert_eq!(strip_ansi(line), "[  OK  ] Mounted /sysroot.");
    }

    #[test]
    fn strip_ansi_removes_osc_boot_marker_and_keeps_plain_text() {
        let osc = "before\u{1b}]3008;start=abc\u{7}after";
        assert_eq!(strip_ansi(osc), "beforeafter");
        assert_eq!(
            strip_ansi("Welcome to Arch Linux!"),
            "Welcome to Arch Linux!"
        );
    }

    #[test]
    fn parses_provision_status_from_serial_transcript() {
        assert_eq!(parse_provision_status("noise\nPANE_PROV_DONE_0\n"), Some(0));
        assert_eq!(
            parse_provision_status("PANE_PROV_DONE_0\nlater\nPANE_PROV_DONE_1\r\n"),
            Some(1)
        );
        assert_eq!(parse_provision_status("PANE_PROV_DONE_\n"), None);
        assert_eq!(parse_provision_status("no sentinel"), None);
    }

    #[test]
    fn graphical_args_use_accelerated_gpu_when_enabled() {
        let args = display_args_for("gtk", true);
        assert!(args.contains(&"virtio-gpu-gl-pci".to_string()));
        assert!(args.contains(&"gtk,gl=on".to_string()));
    }
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
