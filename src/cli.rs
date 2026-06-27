use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{
    model::{DesktopEnvironment, DisplayMode, RuntimeMode, SharedStorageMode},
    plan::DEFAULT_RUNTIME_CAPACITY_GIB,
};

#[derive(Debug, Parser)]
#[command(
    name = "pane",
    version,
    about = "Prepare and launch Pane-managed Linux environments on Windows.",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create or adopt a Pane-managed Arch environment.
    Init(InitArgs),
    /// Initialize Pane Arch, configure the login user, and verify launch readiness.
    Onboard(OnboardArgs),
    /// Generate or execute the Arch-first MVP launch path.
    Launch(LaunchArgs),
    /// Reapply Pane-managed Arch integration without opening mstsc.exe.
    Repair(RepairArgs),
    /// Refresh Arch packages and reapply the Pane-managed integration layer.
    Update(UpdateArgs),
    /// Inspect WSL, the selected distro, managed environment, and the last generated Pane assets.
    Status(StatusArgs),
    /// Show the app-facing lifecycle, next action, storage, and display transport state.
    AppStatus(AppStatusArgs),
    /// Inspect or prepare Pane-owned runtime storage for the future contained OS engine.
    Runtime(Box<RuntimeArgs>),
    /// Probe host readiness for Pane's future native OS runtime.
    NativePreflight(NativePreflightArgs),
    /// Exercise the first non-persistent WHP partition/vCPU boot-spike host step.
    NativeBootSpike(NativeBootSpikeArgs),
    /// Validate and materialize the native kernel boot layout contract.
    NativeKernelPlan(NativeKernelPlanArgs),
    /// Show the crosvm/rust-vmm foundation plan for the Pane-owned runtime.
    NativeFoundation(NativeFoundationArgs),
    /// Show Pane's managed Linux environment catalog and support tiers.
    Environments(EnvironmentsArgs),
    /// Run support-focused diagnostics before launch or reconnect.
    Doctor(DoctorArgs),
    /// Reopen mstsc.exe for the last generated Pane session.
    Connect(ConnectArgs),
    #[command(hide = true)]
    Relay(RelayArgs),
    /// Open or print PaneShared storage for a session.
    Share(ShareArgs),
    /// Create or repair the default Arch user and WSL config for Pane.
    SetupUser(SetupUserArgs),
    /// Open an interactive terminal inside the managed Arch environment.
    Terminal(TerminalArgs),
    /// Stop the XRDP services inside the selected distro.
    Stop(StopArgs),
    /// Remove Pane-managed local state and optionally purge WSL session wiring.
    Reset(ResetArgs),
    /// Print the last bootstrap transcript plus live XRDP logs when available.
    Logs(LogsArgs),
    /// Create a zipped support bundle with reports, state, and workspace artifacts.
    Bundle(BundleArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// WSL distro name for the Pane-managed Arch environment.
    #[arg(long, default_value = "pane-arch")]
    pub distro_name: String,
    /// Adopt an existing Arch distro into Pane management instead of importing a fresh rootfs.
    #[arg(long)]
    pub existing_distro: Option<String>,
    /// Path to an Arch Linux rootfs tarball to import into WSL as a fresh Pane-managed distro.
    #[arg(long)]
    pub rootfs_tar: Option<PathBuf>,
    /// Optional installation directory used with --rootfs-tar. Defaults to %LOCALAPPDATA%\Pane\distros\<distro-name>.
    #[arg(long)]
    pub install_dir: Option<PathBuf>,
    /// Print the managed-environment plan without changing WSL or Pane state.
    #[arg(long)]
    pub dry_run: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct OnboardArgs {
    /// WSL distro name for the Pane-managed Arch environment.
    #[arg(long, default_value = "pane-arch")]
    pub distro_name: String,
    /// Adopt an existing Arch distro into Pane management instead of installing a fresh Pane-managed distro.
    #[arg(long)]
    pub existing_distro: Option<String>,
    /// Path to an Arch Linux rootfs tarball to import into WSL as a fresh Pane-managed distro.
    #[arg(long)]
    pub rootfs_tar: Option<PathBuf>,
    /// Optional installation directory used with --rootfs-tar. Defaults to %LOCALAPPDATA%\Pane\distros\<distro-name>.
    #[arg(long)]
    pub install_dir: Option<PathBuf>,
    /// Linux username to create or repair during onboarding.
    #[arg(long)]
    pub username: String,
    /// Linux password to set for the user. Prefer --password-stdin so the password does not appear in the process list.
    #[arg(long)]
    pub password: Option<String>,
    /// Read the Linux password from stdin instead of the command line.
    #[arg(long)]
    pub password_stdin: bool,
    /// Desktop environment to validate after onboarding. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the readiness check workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port to validate for the post-onboarding readiness check.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Print the onboarding plan without changing WSL or Pane state.
    #[arg(long)]
    pub dry_run: bool,
    /// Leave WSL running after writing /etc/wsl.conf instead of restarting it immediately.
    #[arg(long)]
    pub no_shutdown: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LaunchArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro. Pass this explicitly only to override.
    #[arg(long)]
    pub distro: Option<String>,
    /// Runtime backend to use. wsl-bridge is current; pane-owned is the native runtime preflight path.
    #[arg(long, value_enum, default_value_t = RuntimeMode::Auto)]
    pub runtime: RuntimeMode,
    /// For --runtime qemu-whpx: guest console — serial (text in this terminal) or a graphical
    /// window (gtk/sdl with a virtio-vga adapter).
    #[arg(long, value_enum, default_value_t = DisplayMode::Serial)]
    pub display: DisplayMode,
    /// For --runtime qemu-whpx: boot the root from a persistent qcow2 overlay backed by the
    /// base image, so installed packages and a desktop survive reboots (base stays immutable).
    #[arg(long)]
    pub persist_root: bool,
    /// For --runtime qemu-whpx: start the VM in the background and return immediately. Stop it
    /// with `pane stop`. Pair with --display gtk for a standalone desktop window.
    #[arg(long)]
    pub detach: bool,
    /// Desktop environment to provision in the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Where PaneShared should live. durable persists across reset; scratch is removed with the session workspace.
    #[arg(long, value_enum, default_value_t = SharedStorageMode::Durable)]
    pub shared_storage: SharedStorageMode,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the bootstrap and RDP assets without executing WSL or mstsc.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the WSL bootstrap execution after writing the assets.
    #[arg(long)]
    pub skip_bootstrap: bool,
    /// Do not open mstsc.exe after bootstrap succeeds.
    #[arg(long)]
    pub no_connect: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct RepairArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro. Pass this explicitly only to override.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to repair inside the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Where PaneShared should live. durable persists across reset; scratch is removed with the session workspace.
    #[arg(long, value_enum, default_value_t = SharedStorageMode::Durable)]
    pub shared_storage: SharedStorageMode,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the repaired bootstrap and RDP assets without executing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro. Pass this explicitly only to override.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to update inside the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Where PaneShared should live. durable persists across reset; scratch is removed with the session workspace.
    #[arg(long, value_enum, default_value_t = SharedStorageMode::Durable)]
    pub shared_storage: SharedStorageMode,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the updated bootstrap and RDP assets without executing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AppStatusArgs {
    /// Session slug to evaluate for the app surface.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RuntimeArgs {
    /// Session slug for the Pane-owned runtime reservation.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Target dedicated runtime capacity in GiB for OS image, packages, user data, and snapshots.
    #[arg(long, default_value_t = DEFAULT_RUNTIME_CAPACITY_GIB)]
    pub capacity_gib: u64,
    /// Create the runtime directory layout and write the runtime manifest.
    #[arg(long)]
    pub prepare: bool,
    /// Copy a local Arch base OS image into Pane's runtime image store.
    #[arg(long)]
    pub register_base_image: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-base-image. Without this, the image is recorded but not trusted.
    #[arg(long)]
    pub expected_sha256: Option<String>,
    /// Require --register-base-image to be a bootable raw disk with a detectable Linux root partition.
    #[arg(long)]
    pub require_native_root_disk: bool,
    /// Register the base disk, Linux kernel, initramfs, and cmdline as one prevalidated native Arch boot set.
    #[arg(long)]
    pub register_native_boot_set: bool,
    /// Register a native Arch boot set from a JSON manifest emitted by a reproducible artifact builder.
    #[arg(long)]
    pub register_native_boot_set_manifest: Option<PathBuf>,
    /// Write a native Arch boot-set manifest template for reproducible artifact builders.
    #[arg(long)]
    pub write_native_boot_set_manifest_template: Option<PathBuf>,
    /// Copy a controlled boot-to-serial loader candidate into Pane's runtime engine store.
    #[arg(long)]
    pub register_boot_loader: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-boot-loader. Without this, the loader is recorded but not trusted.
    #[arg(long)]
    pub boot_loader_expected_sha256: Option<String>,
    /// Serial text the registered boot loader must emit before halting. Supports \n, \r, \t, \0, and \\ escapes.
    #[arg(long)]
    pub boot_loader_expected_serial: Option<String>,
    /// Copy a Linux kernel image into Pane's native runtime boot-plan store.
    #[arg(long)]
    pub register_kernel: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-kernel. Without this, the kernel is recorded but not trusted.
    #[arg(long)]
    pub kernel_expected_sha256: Option<String>,
    /// Copy an initramfs image into Pane's native runtime boot-plan store.
    #[arg(long)]
    pub register_initramfs: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-initramfs. Without this, the initramfs is recorded but not trusted.
    #[arg(long)]
    pub initramfs_expected_sha256: Option<String>,
    /// Kernel command line to persist in the native runtime boot plan.
    #[arg(long)]
    pub kernel_cmdline: Option<String>,
    /// Write Pane's guest-side initramfs driver source bundle for the native storage ABI.
    #[arg(long)]
    pub write_initramfs_driver: bool,
    /// Build the generated discovery initramfs cpio and register it into the verified kernel boot plan.
    #[arg(long)]
    pub build_discovery_initramfs: bool,
    /// Prebuilt Linux ELF /init binary to package instead of compiling pane-init.c locally.
    #[arg(long)]
    pub discovery_init_binary: Option<PathBuf>,
    /// Prebuilt Linux ELF pane-port-probe binary to package instead of compiling pane-port-probe.c locally.
    #[arg(long)]
    pub discovery_probe_binary: Option<PathBuf>,
    /// Build pane-block.ko from the generated source, then register it into the initramfs driver bundle.
    #[arg(long)]
    pub build_pane_block_module: bool,
    /// Linux kernel build directory used by --build-pane-block-module.
    #[arg(long)]
    pub kernel_build_dir: Option<PathBuf>,
    /// Copy a compiled pane-block.ko into the generated initramfs driver bundle.
    #[arg(long)]
    pub register_pane_block_module: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-pane-block-module. Without this, the module is recorded but not trusted.
    #[arg(long)]
    pub pane_block_module_expected_sha256: Option<String>,
    /// Copy a stock virtio_mmio.ko (decompressed from the guest kernel modules tree) into the initramfs driver bundle so /init can load the virtio-mmio bus before mounting the virtio root.
    #[arg(long)]
    pub register_virtio_mmio_module: Option<PathBuf>,
    /// Expected SHA-256 digest for --register-virtio-mmio-module. Required so Pane verifies the module before bundling it.
    #[arg(long)]
    pub virtio_mmio_module_expected_sha256: Option<String>,
    /// Create the Pane-owned user disk descriptor for packages, accounts, and customizations.
    #[arg(long)]
    pub create_user_disk: bool,
    /// Snapshot the Pane-owned user disk into the runtime snapshot store.
    #[arg(long)]
    pub snapshot_user_disk: bool,
    /// Restore the Pane-owned user disk from a verified snapshot metadata JSON file.
    #[arg(long, conflicts_with_all = ["create_user_disk", "snapshot_user_disk"])]
    pub restore_user_disk_snapshot: Option<PathBuf>,
    /// Export the Pane-owned user disk into a portable package directory.
    #[arg(long)]
    pub export_user_disk: Option<PathBuf>,
    /// Import the Pane-owned user disk from a verified export package directory or manifest.
    #[arg(
        long,
        conflicts_with_all = ["create_user_disk", "restore_user_disk_snapshot"]
    )]
    pub import_user_disk: Option<PathBuf>,
    /// Grow the Pane-owned user disk logical capacity in GiB. Shrinking is rejected.
    #[arg(long, conflicts_with = "create_user_disk")]
    pub resize_user_disk_gib: Option<u64>,
    /// Repair Pane user disk metadata when the disk header is still valid.
    #[arg(long, conflicts_with = "create_user_disk")]
    pub repair_user_disk: bool,
    /// Create the Pane-owned serial boot test image used by the WHP boot-spike runner.
    #[arg(long)]
    pub create_serial_boot_image: bool,
    /// Replace an existing registered base image or user disk descriptor.
    #[arg(long)]
    pub force: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct NativePreflightArgs {
    /// Session slug for the Pane-owned runtime reservation.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Prepare the Pane-owned runtime directories, manifests, framebuffer/input contracts, sparse user disk, and serial fixture before reporting readiness.
    #[arg(long)]
    pub prepare_runtime: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct NativeBootSpikeArgs {
    /// Session slug for the Pane-owned runtime reservation.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Prepare the Pane-owned runtime directories, manifests, framebuffer/input contracts, sparse user disk, and serial fixture before evaluating boot artifacts.
    #[arg(long)]
    pub prepare_runtime: bool,
    /// Actually create and tear down a WHP partition/vCPU. Without this flag, Pane prints the plan only.
    #[arg(long)]
    pub execute: bool,
    /// Map guest memory and run a tiny serial I/O fixture after the partition/vCPU is created.
    #[arg(long, conflicts_with_all = ["run_boot_loader", "run_kernel_layout"])]
    pub run_fixture: bool,
    /// Map guest memory and run the registered boot-to-serial loader artifact.
    #[arg(long, conflicts_with_all = ["run_fixture", "run_kernel_layout"])]
    pub run_boot_loader: bool,
    /// Map guest memory and run the materialized kernel-layout artifact under the serial/HALT contract.
    #[arg(long, conflicts_with_all = ["run_fixture", "run_boot_loader"])]
    pub run_kernel_layout: bool,
    /// Boot the registered kernel/initramfs/base-disk through QEMU with the WHP accelerator
    /// (qemu-system-x86_64 -accel whpx) instead of Pane's from-scratch WHP run loop. This is
    /// the validated engine path: it boots the full distro (virtio root, switch_root, systemd,
    /// login) where the from-scratch loop stalls on guest-timer throughput.
    #[arg(long)]
    pub qemu_whpx: bool,
    /// Path to the real distro initramfs (with virtio-blk) for the QEMU engine path. The
    /// registered Pane initramfs is the custom pane-block one, which QEMU cannot use. When
    /// omitted, Pane extracts and caches it from the registered base image automatically.
    #[arg(long)]
    pub qemu_initramfs: Option<PathBuf>,
    /// With --qemu-whpx, run an interactive session: the guest serial console is wired to
    /// this terminal so you get a live Linux shell (Ctrl-A X to quit). Without this flag the
    /// QEMU path runs a headless boot probe and reports milestones.
    #[arg(long)]
    pub interactive: bool,
    /// Console for an interactive QEMU session: serial (text in this terminal) or a graphical
    /// window (gtk/sdl with a virtio-vga adapter).
    #[arg(long, value_enum, default_value_t = DisplayMode::Serial)]
    pub display: DisplayMode,
    /// Boot the QEMU root from a persistent qcow2 overlay backed by the base image, so guest
    /// changes (installed packages, a desktop) survive reboots. The base image stays immutable.
    #[arg(long)]
    pub persist_root: bool,
    /// Start the QEMU VM in the background and return immediately. The guest keeps running;
    /// stop it with `pane stop`. Pair with --display gtk for a standalone desktop window.
    #[arg(long)]
    pub detach: bool,
    /// Write incremental native boot diagnostics to this JSON file while the WHP guest is running.
    #[arg(long)]
    pub trace_checkpoint: Option<PathBuf>,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct NativeKernelPlanArgs {
    /// Session slug for the Pane-owned runtime reservation.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// Write the resolved guest-memory layout into the runtime state directory.
    #[arg(long)]
    pub materialize: bool,
    /// Prepare the Pane-owned runtime directories, manifests, framebuffer/input contracts, sparse user disk, and serial fixture before planning.
    #[arg(long)]
    pub prepare_runtime: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct NativeFoundationArgs {
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct EnvironmentsArgs {
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to validate. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port to validate.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Validate an already-bootstrapped environment instead of a fresh bootstrap path.
    #[arg(long)]
    pub skip_bootstrap: bool,
    /// Skip mstsc.exe validation when you only want bootstrap readiness.
    #[arg(long)]
    pub no_connect: bool,
    /// Do not create or repair Pane workspace directories while checking.
    #[arg(long)]
    pub no_write: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// Session slug to reconnect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// Open mstsc.exe even when readiness checks report a blocker.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RelayArgs {
    #[arg(long)]
    pub distro: String,
    #[arg(long)]
    pub listen_port: u16,
    #[arg(long)]
    pub target_port: u16,
    #[arg(long, default_value_t = 90)]
    pub startup_timeout_seconds: u64,
    #[arg(long)]
    pub log_file: Option<PathBuf>,
    #[arg(long)]
    pub ready_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ShareArgs {
    /// Session slug to inspect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// Which PaneShared storage location to resolve when there is no saved launch for the session.
    #[arg(long, value_enum, default_value_t = SharedStorageMode::Durable)]
    pub shared_storage: SharedStorageMode,
    /// Print the resolved paths without opening Explorer.
    #[arg(long)]
    pub print_only: bool,
}

#[derive(Debug, Args)]
pub struct SetupUserArgs {
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Linux username to create or repair.
    #[arg(long)]
    pub username: String,
    /// Linux password to set for the user. Prefer --password-stdin so the password does not appear in the process list.
    #[arg(long)]
    pub password: Option<String>,
    /// Read the Linux password from stdin instead of the command line.
    #[arg(long)]
    pub password_stdin: bool,
    /// Print the onboarding plan without changing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Leave WSL running after writing /etc/wsl.conf instead of restarting it immediately.
    #[arg(long)]
    pub no_shutdown: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TerminalArgs {
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Optional Linux user for the interactive shell.
    #[arg(long)]
    pub user: Option<String>,
    /// Print the resolved terminal target without opening it.
    #[arg(long)]
    pub print_only: bool,
}

#[derive(Debug, Args)]
pub struct StopArgs {
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
}

#[derive(Debug, Args)]
pub struct ResetArgs {
    /// Session slug to reset. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name used when purging Pane-managed session wiring.
    #[arg(long)]
    pub distro: Option<String>,
    /// Also remove Pane-managed .xsession content and stop XRDP inside WSL.
    #[arg(long)]
    pub purge_wsl: bool,
    /// Also remove durable PaneShared storage for the selected session.
    #[arg(long)]
    pub purge_shared: bool,
    /// Remove Pane's managed-environment ownership record without deleting the distro.
    #[arg(long, conflicts_with = "factory_reset")]
    pub release_managed_environment: bool,
    /// Destroy a Pane-imported managed distro, delete its install root, and clear Pane ownership.
    #[arg(long, conflicts_with = "release_managed_environment")]
    pub factory_reset: bool,
    /// Print the reset plan without changing WSL, local workspaces, or Pane state.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Session slug to inspect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Number of XRDP log lines to fetch from WSL.
    #[arg(long, default_value_t = 50)]
    pub lines: usize,
}

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// Session slug to include. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name. Defaults to the Pane-managed distro or last launched distro when available.
    #[arg(long)]
    pub distro: Option<String>,
    /// Optional zip path for the generated support bundle.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Commands};

    #[test]
    fn runtime_accepts_required_native_root_disk_flag() {
        let cli = Cli::try_parse_from([
            "pane",
            "runtime",
            "--register-base-image",
            "C:\\arch.img",
            "--expected-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--require-native-root-disk",
            "--register-native-boot-set",
            "--register-kernel",
            "C:\\vmlinuz-linux",
            "--kernel-expected-sha256",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--register-initramfs",
            "C:\\initramfs-linux.img",
            "--initramfs-expected-sha256",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "--kernel-cmdline",
            "console=ttyS0 earlyprintk=serial panic=-1",
        ])
        .unwrap();

        match cli.command {
            Commands::Runtime(args) => {
                assert_eq!(
                    args.register_base_image.as_deref(),
                    Some(std::path::Path::new("C:\\arch.img"))
                );
                assert!(args.require_native_root_disk);
                assert!(args.register_native_boot_set);
                assert_eq!(
                    args.register_kernel.as_deref(),
                    Some(std::path::Path::new("C:\\vmlinuz-linux"))
                );
                assert_eq!(
                    args.register_initramfs.as_deref(),
                    Some(std::path::Path::new("C:\\initramfs-linux.img"))
                );
            }
            _ => panic!("expected runtime command"),
        }
    }

    #[test]
    fn runtime_accepts_native_boot_set_manifest() {
        let cli = Cli::try_parse_from([
            "pane",
            "runtime",
            "--register-native-boot-set-manifest",
            "C:\\pane-build\\pane-native-boot-set.json",
        ])
        .unwrap();

        match cli.command {
            Commands::Runtime(args) => {
                assert_eq!(
                    args.register_native_boot_set_manifest.as_deref(),
                    Some(std::path::Path::new(
                        "C:\\pane-build\\pane-native-boot-set.json"
                    ))
                );
            }
            _ => panic!("expected runtime command"),
        }
    }

    #[test]
    fn runtime_accepts_native_boot_set_manifest_template_writer() {
        let cli = Cli::try_parse_from([
            "pane",
            "runtime",
            "--write-native-boot-set-manifest-template",
            "C:\\pane-build\\pane-native-boot-set.json",
        ])
        .unwrap();

        match cli.command {
            Commands::Runtime(args) => {
                assert_eq!(
                    args.write_native_boot_set_manifest_template.as_deref(),
                    Some(std::path::Path::new(
                        "C:\\pane-build\\pane-native-boot-set.json"
                    ))
                );
            }
            _ => panic!("expected runtime command"),
        }
    }

    #[test]
    fn native_preflight_accepts_prepare_runtime_flag() {
        let cli = Cli::try_parse_from(["pane", "native-preflight", "--prepare-runtime"]).unwrap();

        match cli.command {
            Commands::NativePreflight(args) => assert!(args.prepare_runtime),
            _ => panic!("expected native-preflight command"),
        }
    }

    #[test]
    fn native_boot_spike_accepts_prepare_runtime_flag() {
        let cli = Cli::try_parse_from([
            "pane",
            "native-boot-spike",
            "--prepare-runtime",
            "--execute",
            "--run-kernel-layout",
            "--trace-checkpoint",
            "trace.json",
        ])
        .unwrap();

        match cli.command {
            Commands::NativeBootSpike(args) => {
                assert!(args.prepare_runtime);
                assert!(args.execute);
                assert!(args.run_kernel_layout);
                assert_eq!(args.trace_checkpoint, Some(PathBuf::from("trace.json")));
            }
            _ => panic!("expected native-boot-spike command"),
        }
    }

    #[test]
    fn native_kernel_plan_accepts_prepare_runtime_flag() {
        let cli = Cli::try_parse_from([
            "pane",
            "native-kernel-plan",
            "--prepare-runtime",
            "--materialize",
        ])
        .unwrap();

        match cli.command {
            Commands::NativeKernelPlan(args) => {
                assert!(args.prepare_runtime);
                assert!(args.materialize);
            }
            _ => panic!("expected native-kernel-plan command"),
        }
    }

    #[test]
    fn native_foundation_accepts_json_flag() {
        let cli = Cli::try_parse_from(["pane", "native-foundation", "--json"]).unwrap();

        match cli.command {
            Commands::NativeFoundation(args) => assert!(args.json),
            _ => panic!("expected native-foundation command"),
        }
    }
}
