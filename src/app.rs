#![allow(clippy::uninlined_format_args)]

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::{
    bootstrap::{render_bootstrap_script, render_update_script},
    cli::{
        AppStatusArgs, BundleArgs, Cli, Commands, ConnectArgs, DoctorArgs, EnvironmentsArgs,
        InitArgs, LaunchArgs, LogsArgs, NativeBootSpikeArgs, NativeKernelPlanArgs,
        NativePreflightArgs, OnboardArgs, RelayArgs, RepairArgs, ResetArgs, RuntimeArgs,
        SetupUserArgs, ShareArgs, StatusArgs, StopArgs, TerminalArgs, UpdateArgs,
    },
    error::{AppError, AppResult},
    model::{
        managed_environment_catalog, DesktopEnvironment, DistroFamily, DistroRecord,
        ManagedEnvironment, RuntimeMode, SharedStorageMode,
    },
    plan::{
        app_root, managed_distro_install_root, shared_dir_for_workspace, windows_to_wsl_path,
        workspace_for, workspace_for_with_shared_storage, LaunchPlan, RuntimePaths, WorkspacePaths,
        DEFAULT_RUNTIME_CAPACITY_GIB, MINIMUM_RUNTIME_CAPACITY_GIB,
    },
    rdp::render_rdp_profile,
    state::{
        clear_managed_environment, clear_state, load_state, save_managed_environment,
        save_state_record, LaunchStage, LaunchTransport, ManagedEnvironmentOwnership,
        ManagedEnvironmentState, PaneState, StoredLaunch,
    },
    wsl::{
        self, probe_inventory, run_wsl_shell_as_user_capture,
        run_wsl_shell_as_user_capture_with_input, shell_quote, PasswordStatus, WslInventory,
    },
};

#[derive(Debug)]
struct LaunchTarget {
    distro: DistroRecord,
    hypothetical: bool,
}

#[derive(Debug)]
enum InitSource {
    AdoptExisting {
        distro_name: String,
    },
    InstallOnline {
        distro_name: String,
        install_dir: PathBuf,
    },
    ImportRootfs {
        distro_name: String,
        rootfs_tar: PathBuf,
        install_dir: PathBuf,
    },
}

#[derive(Debug, Serialize)]
struct InitReport {
    product_shape: &'static str,
    managed_environment: ManagedEnvironmentState,
    dry_run: bool,
    present_in_inventory: bool,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SetupUserReport {
    product_shape: &'static str,
    distro: String,
    username: String,
    dry_run: bool,
    password_updated: bool,
    default_user_configured: bool,
    systemd_configured: bool,
    wsl_shutdown: bool,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct OnboardReport {
    product_shape: &'static str,
    managed_environment: ManagedEnvironmentState,
    setup_user: SetupUserReport,
    launch_readiness: Option<DoctorReport>,
    dry_run: bool,
    ready_for_launch: bool,
    notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct DoctorRequest {
    distro: Option<String>,
    session_name: String,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_requested: bool,
    connect_requested: bool,
    write_probes_enabled: bool,
}

#[derive(Debug, Serialize)]
struct StatusReport {
    platform: &'static str,
    wsl_available: bool,
    wsl_version_banner: Option<String>,
    wsl_default_distro: Option<String>,
    managed_environment: Option<ManagedEnvironmentState>,
    selected_distro: Option<DistroHealth>,
    known_distros: Vec<DistroRecord>,
    last_launch: Option<StoredLaunch>,
    last_launch_workspace: Option<WorkspaceHealth>,
}

#[derive(Debug, Serialize)]
struct AppStatusReport {
    product_shape: &'static str,
    session_name: String,
    phase: AppLifecyclePhase,
    next_action: AppNextAction,
    next_action_label: &'static str,
    next_action_summary: String,
    supported_profile: AppProfileReport,
    runtime: RuntimeReport,
    storage: AppStorageReport,
    display: AppDisplayReport,
    managed_environment: Option<ManagedEnvironmentState>,
    selected_distro: Option<DistroHealth>,
    last_launch: Option<StoredLaunch>,
    workspace: WorkspaceHealth,
    blockers: Vec<AppBlocker>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    product_shape: &'static str,
    session_name: String,
    current_engine: PaneRuntimeEngine,
    current_engine_label: &'static str,
    target_engine: PaneRuntimeEngine,
    target_engine_label: &'static str,
    prepared: bool,
    dedicated_space_root: String,
    directories: RuntimeDirectoryReport,
    storage_budget: RuntimeStorageBudget,
    ownership: RuntimeOwnershipReport,
    artifacts: RuntimeArtifactReport,
    native_host: crate::native::NativeHostPreflightReport,
    native_runtime: NativeRuntimeReport,
    current_limitation: &'static str,
    next_steps: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NativePreflightReport {
    product_shape: &'static str,
    session_name: String,
    host: crate::native::NativeHostPreflightReport,
    runtime: RuntimeReport,
    ready_for_boot_spike: bool,
    blockers: Vec<String>,
    next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NativeBootSpikeReport {
    product_shape: &'static str,
    session_name: String,
    execute_requested: bool,
    fixture_requested: bool,
    boot_loader_requested: bool,
    kernel_layout_requested: bool,
    host: crate::native::NativeHostPreflightReport,
    runtime: RuntimeReport,
    partition_smoke: crate::native::NativePartitionSmokeReport,
    ready_for_serial_kernel_spike: bool,
    blockers: Vec<String>,
    next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NativeKernelPlanReport {
    product_shape: &'static str,
    session_name: String,
    materialize_requested: bool,
    runtime: RuntimeReport,
    ready_for_kernel_entry_spike: bool,
    layout: Option<KernelBootLayout>,
    blockers: Vec<String>,
    next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RuntimeDirectoryReport {
    downloads: String,
    images: String,
    disks: String,
    snapshots: String,
    state: String,
    engines: String,
    logs: String,
    base_os_image: String,
    serial_boot_image: String,
    boot_loader_image: String,
    kernel_image: String,
    initramfs_image: String,
    user_disk: String,
    base_os_metadata: String,
    serial_boot_metadata: String,
    boot_loader_metadata: String,
    kernel_boot_metadata: String,
    user_disk_metadata: String,
    runtime_config: String,
    native_manifest: String,
    kernel_boot_layout: String,
    framebuffer_contract: String,
    input_contract: String,
    manifest: String,
}

#[derive(Debug, Serialize)]
struct RuntimeStorageBudget {
    requested_capacity_gib: u64,
    base_os_budget_gib: u64,
    user_packages_and_customizations_gib: u64,
    snapshot_budget_gib: u64,
    minimum_recommended_gib: u64,
}

#[derive(Debug, Serialize)]
struct RuntimeOwnershipReport {
    app_owned_storage: bool,
    app_owned_boot_engine_available: bool,
    app_owned_display_available: bool,
    external_runtime_required_for_current_launch: bool,
    current_external_dependencies: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct RuntimeArtifactReport {
    base_os_image_exists: bool,
    base_os_image_bytes: Option<u64>,
    base_os_image_sha256: Option<String>,
    base_os_image_verified: bool,
    base_os_metadata_exists: bool,
    serial_boot_image_exists: bool,
    serial_boot_image_bytes: Option<u64>,
    serial_boot_image_sha256: Option<String>,
    serial_boot_image_ready: bool,
    serial_boot_banner: Option<String>,
    serial_boot_metadata_exists: bool,
    boot_loader_image_exists: bool,
    boot_loader_image_bytes: Option<u64>,
    boot_loader_image_sha256: Option<String>,
    boot_loader_image_verified: bool,
    boot_loader_expected_serial: Option<String>,
    boot_loader_metadata_exists: bool,
    kernel_image_exists: bool,
    kernel_image_bytes: Option<u64>,
    kernel_image_sha256: Option<String>,
    kernel_image_verified: bool,
    kernel_format: Option<String>,
    kernel_linux_boot_protocol: Option<String>,
    kernel_linux_protected_mode_offset: Option<u64>,
    kernel_linux_protected_mode_bytes: Option<u64>,
    initramfs_image_exists: bool,
    initramfs_image_bytes: Option<u64>,
    initramfs_image_sha256: Option<String>,
    initramfs_image_verified: bool,
    kernel_cmdline: Option<String>,
    kernel_boot_plan_ready: bool,
    kernel_boot_metadata_exists: bool,
    kernel_boot_layout_exists: bool,
    kernel_boot_layout_ready: bool,
    framebuffer_contract_exists: bool,
    framebuffer_contract_ready: bool,
    framebuffer_resolution: Option<String>,
    input_contract_exists: bool,
    input_contract_ready: bool,
    user_disk_exists: bool,
    user_disk_capacity_gib: Option<u64>,
    user_disk_format: Option<String>,
    user_disk_ready: bool,
    user_disk_metadata_exists: bool,
    runtime_manifest_exists: bool,
    runtime_config_exists: bool,
    native_manifest_exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaseOsImageMetadata {
    schema_version: u32,
    distro_family: DistroFamily,
    image_kind: String,
    source_path: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    expected_sha256: Option<String>,
    verified: bool,
    registered_at_epoch_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SerialBootImageMetadata {
    schema_version: u32,
    image_kind: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    serial_banner: String,
    guest_entry_gpa: String,
    created_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BootLoaderImageMetadata {
    schema_version: u32,
    image_kind: String,
    source_path: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    expected_sha256: Option<String>,
    verified: bool,
    expected_serial_text: String,
    guest_entry_gpa: String,
    registered_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KernelBootMetadata {
    schema_version: u32,
    image_kind: String,
    kernel_source_path: String,
    kernel_stored_path: String,
    kernel_bytes: u64,
    kernel_sha256: String,
    kernel_expected_sha256: Option<String>,
    kernel_verified: bool,
    kernel_format: String,
    linux_boot_protocol: Option<String>,
    linux_setup_sectors: Option<u8>,
    linux_setup_bytes: Option<u64>,
    linux_protected_mode_offset: Option<u64>,
    linux_protected_mode_bytes: Option<u64>,
    linux_loadflags: Option<u8>,
    linux_preferred_load_address: Option<String>,
    initramfs_source_path: Option<String>,
    initramfs_stored_path: Option<String>,
    initramfs_bytes: Option<u64>,
    initramfs_sha256: Option<String>,
    initramfs_expected_sha256: Option<String>,
    initramfs_verified: bool,
    cmdline: String,
    expected_serial_device: String,
    kernel_load_gpa: String,
    initramfs_load_gpa: Option<String>,
    registered_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KernelBootLayout {
    schema_version: u32,
    layout_kind: String,
    session_name: String,
    boot_params_gpa: String,
    cmdline_gpa: String,
    kernel_load_gpa: String,
    initramfs_load_gpa: Option<String>,
    kernel_path: String,
    kernel_bytes: u64,
    kernel_sha256: String,
    kernel_format: String,
    linux_boot_protocol: Option<String>,
    linux_setup_sectors: Option<u8>,
    linux_setup_bytes: Option<u64>,
    linux_protected_mode_offset: Option<u64>,
    linux_protected_mode_bytes: Option<u64>,
    linux_loadflags: Option<u8>,
    linux_preferred_load_address: Option<String>,
    linux_entry_point_gpa: Option<String>,
    linux_boot_params_register: Option<String>,
    linux_expected_entry_mode: Option<String>,
    guest_memory_map: Vec<KernelGuestMemoryRange>,
    initramfs_path: Option<String>,
    initramfs_bytes: Option<u64>,
    initramfs_sha256: Option<String>,
    cmdline: String,
    expected_serial_device: String,
    storage: Option<KernelStorageAttachment>,
    framebuffer: Option<FramebufferContract>,
    input: Option<InputContract>,
    materialized_at_epoch_seconds: Option<u64>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KernelGuestMemoryRange {
    label: String,
    start_gpa: String,
    size_bytes: u64,
    region_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct KernelStorageAttachment {
    schema_version: u32,
    base_os_path: String,
    base_os_sha256: String,
    base_os_bytes: u64,
    user_disk_path: String,
    user_disk_capacity_gib: u64,
    user_disk_format: String,
    root_device: String,
    user_device: String,
    readonly_base: bool,
    writable_user_disk: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FramebufferContract {
    schema_version: u32,
    device: String,
    width: u32,
    height: u32,
    stride_bytes: u32,
    bytes_per_pixel: u32,
    format: String,
    guest_gpa: String,
    size_bytes: u64,
    resize_policy: String,
}

impl FramebufferContract {
    fn resolution_label(&self) -> String {
        format!(
            "{}x{}x{}",
            self.width,
            self.height,
            self.bytes_per_pixel * 8
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct InputContract {
    schema_version: u32,
    keyboard_device: String,
    pointer_device: String,
    transport: String,
    coordinate_space: String,
    guest_queue_gpa: String,
    queue_size_bytes: u64,
    event_record_bytes: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserDiskMetadata {
    schema_version: u32,
    format: String,
    disk_path: String,
    capacity_gib: u64,
    logical_size_bytes: u64,
    block_size_bytes: u64,
    sparse_backing: bool,
    allocated_header_bytes: u64,
    header_sha256: String,
    materialized_block_device: bool,
    created_at_epoch_seconds: u64,
    notes: Vec<String>,
}

const PANE_USER_DISK_FORMAT: &str = "pane-sparse-user-disk-v1";
const PANE_USER_DISK_MAGIC: &str = "PANE_USER_DISK_V1\n";
const PANE_USER_DISK_BLOCK_SIZE_BYTES: u64 = 4096;

#[derive(Debug, Serialize)]
struct NativeRuntimeReport {
    state: NativeRuntimeState,
    state_label: &'static str,
    bootable: bool,
    host_ready: bool,
    ready_for_boot_spike: bool,
    requires_wsl: bool,
    requires_mstsc: bool,
    requires_xrdp: bool,
    launch_contract: &'static str,
    blockers: Vec<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum NativeRuntimeState {
    StorageNotPrepared,
    MissingBaseImage,
    UnverifiedBaseImage,
    MissingUserDisk,
    HostNotReady,
    EngineNotImplemented,
}

impl NativeRuntimeState {
    fn display_name(self) -> &'static str {
        match self {
            Self::StorageNotPrepared => "storage-not-prepared",
            Self::MissingBaseImage => "missing-base-image",
            Self::UnverifiedBaseImage => "unverified-base-image",
            Self::MissingUserDisk => "missing-user-disk",
            Self::HostNotReady => "host-not-ready",
            Self::EngineNotImplemented => "engine-not-implemented",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PaneRuntimeEngine {
    WslXrdpBridge,
    PaneOwnedOsRuntime,
}

impl PaneRuntimeEngine {
    fn display_name(self) -> &'static str {
        match self {
            Self::WslXrdpBridge => "WSL2 + XRDP bridge",
            Self::PaneOwnedOsRuntime => "Pane-owned OS runtime",
        }
    }
}

#[derive(Debug, Serialize)]
struct AppProfileReport {
    label: &'static str,
    distro_family: DistroFamily,
    desktop_environment: DesktopEnvironment,
    launchable_now: bool,
}

#[derive(Debug, Serialize)]
struct AppStorageReport {
    default_mode: SharedStorageMode,
    durable_shared_dir: String,
    scratch_shared_dir: String,
    policy: &'static str,
}

#[derive(Debug, Serialize)]
struct AppDisplayReport {
    current_mode: AppDisplayMode,
    current_mode_label: &'static str,
    contained_window_available: bool,
    user_visible_handoff: bool,
    planned_modes: Vec<AppDisplayMode>,
    notes: Vec<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AppDisplayMode {
    ExternalMstscRdp,
    EmbeddedRdpWindow,
    NativePaneTransport,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AppLifecyclePhase {
    UnsupportedHost,
    HostNeedsWsl,
    NeedsManagedEnvironment,
    NeedsUserSetup,
    ReadyToLaunch,
    ReconnectReady,
    LaunchFailed,
    NeedsRepair,
}

impl AppLifecyclePhase {
    fn display_name(self) -> &'static str {
        match self {
            Self::UnsupportedHost => "unsupported-host",
            Self::HostNeedsWsl => "host-needs-wsl",
            Self::NeedsManagedEnvironment => "needs-managed-environment",
            Self::NeedsUserSetup => "needs-user-setup",
            Self::ReadyToLaunch => "ready-to-launch",
            Self::ReconnectReady => "reconnect-ready",
            Self::LaunchFailed => "launch-failed",
            Self::NeedsRepair => "needs-repair",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AppNextAction {
    InstallWsl,
    OnboardArch,
    SetupUser,
    LaunchArch,
    Reconnect,
    RepairArch,
    CollectSupportBundle,
}

impl AppNextAction {
    fn label(self) -> &'static str {
        match self {
            Self::InstallWsl => "Install WSL2",
            Self::OnboardArch => "Onboard Arch",
            Self::SetupUser => "Setup User",
            Self::LaunchArch => "Launch Arch",
            Self::Reconnect => "Reconnect",
            Self::RepairArch => "Repair Arch",
            Self::CollectSupportBundle => "Collect Support Bundle",
        }
    }
}

#[derive(Debug, Serialize)]
struct AppBlocker {
    id: String,
    summary: String,
    remediation: Option<String>,
}

#[derive(Debug, Serialize)]
struct EnvironmentCatalogReport {
    product_shape: &'static str,
    strategy: &'static str,
    environments: Vec<ManagedEnvironment>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DistroHealth {
    distro: DistroRecord,
    supported_for_mvp: bool,
    present_in_inventory: bool,
    checked_port: u16,
    systemd_configured: Option<bool>,
    xrdp_installed: Option<bool>,
    xrdp_service_active: Option<bool>,
    xrdp_listening: Option<bool>,
    localhost_reachable: Option<bool>,
    pane_relay_available: Option<bool>,
    preferred_transport: Option<LaunchTransport>,
    xsession_present: Option<bool>,
    pane_session_assets_ready: Option<bool>,
    user_home_ready: Option<bool>,
    default_user_password_status: Option<PasswordStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PreparedTransport {
    kind: LaunchTransport,
    host: String,
}

impl PreparedTransport {
    fn direct_localhost() -> Self {
        Self {
            kind: LaunchTransport::DirectLocalhost,
            host: "localhost".to_string(),
        }
    }

    fn direct_wsl_ip(host: String) -> Self {
        Self {
            kind: LaunchTransport::DirectWslIp,
            host,
        }
    }

    fn pane_relay() -> Self {
        Self {
            kind: LaunchTransport::PaneRelay,
            host: "localhost".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct WorkspaceHealth {
    root_exists: bool,
    shared_dir_exists: bool,
    bootstrap_script_exists: bool,
    rdp_profile_exists: bool,
    bootstrap_log_exists: bool,
    transport_log_exists: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CheckStatus {
    Pass,
    Fail,
    Skipped,
}

impl CheckStatus {
    fn display_name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skipped => "SKIP",
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    id: String,
    status: CheckStatus,
    summary: String,
    remediation: Option<String>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    target_distro: Option<String>,
    session_name: String,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_requested: bool,
    connect_requested: bool,
    write_probes_enabled: bool,
    supported_for_mvp: bool,
    ready: bool,
    selected_distro: Option<DistroHealth>,
    workspace: WorkspaceHealth,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }
}

#[derive(Debug, Serialize)]
struct BundleManifest {
    created_at_epoch_seconds: u64,
    session_name: String,
    selected_distro: Option<String>,
    output_zip: String,
    included_files: Vec<String>,
    notes: Vec<String>,
}

pub fn run() -> AppResult<()> {
    if std::env::args_os().len() == 1 {
        return open_default_app_entrypoint();
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => init(args),
        Commands::Onboard(args) => onboard(args),
        Commands::Launch(args) => launch(args),
        Commands::Repair(args) => repair(args),
        Commands::Update(args) => update(args),
        Commands::Status(args) => status(args),
        Commands::AppStatus(args) => app_status(args),
        Commands::Runtime(args) => runtime(args),
        Commands::NativePreflight(args) => native_preflight(args),
        Commands::NativeBootSpike(args) => native_boot_spike(args),
        Commands::NativeKernelPlan(args) => native_kernel_plan(args),
        Commands::Environments(args) => environments(args),
        Commands::Doctor(args) => doctor(args),
        Commands::Connect(args) => connect(args),
        Commands::Relay(args) => relay(args),
        Commands::Share(args) => share(args),
        Commands::SetupUser(args) => setup_user(args),
        Commands::Terminal(args) => terminal(args),
        Commands::Stop(args) => stop(args),
        Commands::Reset(args) => reset(args),
        Commands::Logs(args) => logs(args),
        Commands::Bundle(args) => bundle(args),
    }
}

const EMBEDDED_APP_ASSETS: &[(&str, &str)] = &[
    (
        "Pane Control Center.ps1",
        include_str!("../scripts/package-assets/Pane Control Center.ps1"),
    ),
    (
        "Pane Control Center.cmd",
        include_str!("../scripts/package-assets/Pane Control Center.cmd"),
    ),
    (
        "Launch Pane Arch.ps1",
        include_str!("../scripts/package-assets/Launch Pane Arch.ps1"),
    ),
    (
        "Launch Pane Arch.cmd",
        include_str!("../scripts/package-assets/Launch Pane Arch.cmd"),
    ),
    (
        "Open Pane Arch Terminal.ps1",
        include_str!("../scripts/package-assets/Open Pane Arch Terminal.ps1"),
    ),
    (
        "Open Pane Arch Terminal.cmd",
        include_str!("../scripts/package-assets/Open Pane Arch Terminal.cmd"),
    ),
    (
        "Open Pane Shared Folder.ps1",
        include_str!("../scripts/package-assets/Open Pane Shared Folder.ps1"),
    ),
    (
        "Open Pane Shared Folder.cmd",
        include_str!("../scripts/package-assets/Open Pane Shared Folder.cmd"),
    ),
    (
        "Collect Pane Support Bundle.ps1",
        include_str!("../scripts/package-assets/Collect Pane Support Bundle.ps1"),
    ),
    (
        "Collect Pane Support Bundle.cmd",
        include_str!("../scripts/package-assets/Collect Pane Support Bundle.cmd"),
    ),
    (
        "Install Pane Shortcuts.ps1",
        include_str!("../scripts/package-assets/Install Pane Shortcuts.ps1"),
    ),
    (
        "Install Pane Shortcuts.cmd",
        include_str!("../scripts/package-assets/Install Pane Shortcuts.cmd"),
    ),
];

fn open_default_app_entrypoint() -> AppResult<()> {
    if !cfg!(windows) {
        println!("Pane");
        println!("  The app Control Center is only available on Windows.");
        println!("  Use `pane --help` to inspect CLI commands on this platform.");
        return Ok(());
    }

    let executable = std::env::current_exe().map_err(|error| {
        AppError::message(format!(
            "failed to locate the Pane executable for app startup: {error}"
        ))
    })?;
    let package_root = executable.parent().ok_or_else(|| {
        AppError::message("failed to resolve the Pane package directory for app startup")
    })?;
    let control_center = package_root.join("Pane Control Center.ps1");

    let control_center = if control_center.exists() {
        control_center
    } else {
        hydrate_embedded_app_bundle(&executable)?
    };

    if std::env::var_os("PANE_APP_HYDRATE_ONLY").is_some() {
        println!("Pane app entrypoint hydrated.");
        println!("  Control Center {}", control_center.display());
        return Ok(());
    }

    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&control_center)
        .spawn()
        .map_err(|error| {
            AppError::message(format!(
                "failed to open the Pane Control Center at {}: {error}",
                control_center.display()
            ))
        })?;

    Ok(())
}

fn hydrate_embedded_app_bundle(executable: &Path) -> AppResult<PathBuf> {
    let app_dir = app_root().join("app");
    fs::create_dir_all(&app_dir).map_err(|error| {
        AppError::message(format!(
            "failed to create the Pane app directory at {}: {error}",
            app_dir.display()
        ))
    })?;

    let app_exe = app_dir.join("pane.exe");
    let same_exe =
        app_exe.exists() && executable.canonicalize().ok() == app_exe.canonicalize().ok();
    if !same_exe {
        fs::copy(executable, &app_exe).map_err(|error| {
            AppError::message(format!(
                "failed to hydrate Pane app executable at {}: {error}",
                app_exe.display()
            ))
        })?;
    }

    for (name, contents) in EMBEDDED_APP_ASSETS {
        let path = app_dir.join(name);
        fs::write(&path, contents).map_err(|error| {
            AppError::message(format!(
                "failed to hydrate Pane app asset at {}: {error}",
                path.display()
            ))
        })?;
    }

    Ok(app_dir.join("Pane Control Center.ps1"))
}

fn init(args: InitArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = initialize_managed_arch_environment(&args, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_init_report(&report);
    Ok(())
}

fn onboard(args: OnboardArgs) -> AppResult<()> {
    let init_report = initialize_managed_arch_environment(
        &InitArgs {
            distro_name: args.distro_name.clone(),
            existing_distro: args.existing_distro.clone(),
            rootfs_tar: args.rootfs_tar.clone(),
            install_dir: args.install_dir.clone(),
            dry_run: args.dry_run,
            json: false,
        },
        &probe_inventory(),
        load_state()?.as_ref(),
    )?;

    let setup_report = if args.dry_run && !init_report.present_in_inventory {
        build_planned_setup_user_report(
            &SetupUserArgs {
                distro: Some(init_report.managed_environment.distro_name.clone()),
                username: args.username.clone(),
                password: args.password.clone(),
                password_stdin: args.password_stdin,
                dry_run: true,
                no_shutdown: args.no_shutdown,
                json: false,
            },
            &init_report.managed_environment.distro_name,
        )?
    } else {
        configure_arch_user(
            &SetupUserArgs {
                distro: Some(init_report.managed_environment.distro_name.clone()),
                username: args.username.clone(),
                password: args.password.clone(),
                password_stdin: args.password_stdin,
                dry_run: args.dry_run,
                no_shutdown: args.no_shutdown,
                json: false,
            },
            &probe_inventory(),
            load_state()?.as_ref(),
        )?
    };

    let mut notes = init_report.notes.clone();
    notes.extend(setup_report.notes.iter().cloned());

    let (launch_readiness, ready_for_launch) = if args.dry_run {
        notes.push(
            "Dry run did not execute the final readiness check. Run `pane onboard` without --dry-run for a real launch-readiness result."
                .to_string(),
        );
        (None, false)
    } else {
        let post_setup_inventory = probe_inventory();
        let post_setup_state = load_state()?;
        let readiness = evaluate_doctor(
            &DoctorRequest {
                distro: Some(setup_report.distro.clone()),
                session_name: crate::plan::sanitize_session_name(&args.session_name),
                desktop_environment: args.de,
                port: args.port,
                bootstrap_requested: true,
                connect_requested: true,
                write_probes_enabled: true,
            },
            &post_setup_inventory,
            post_setup_state.as_ref(),
        )?;
        let ready = readiness.ready && readiness.supported_for_mvp;
        notes.push(if ready {
            "Pane verified that Arch is ready for the supported launch path. Use `pane launch` or the Launch Arch button next."
                .to_string()
        } else {
            "Pane completed onboarding, but launch readiness still has blockers. Review the embedded doctor report before launching."
                .to_string()
        });
        (Some(readiness), ready)
    };

    let report = OnboardReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        managed_environment: init_report.managed_environment,
        setup_user: setup_report,
        launch_readiness,
        dry_run: args.dry_run,
        ready_for_launch,
        notes,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_onboard_report(&report);
    }

    Ok(())
}

fn build_planned_setup_user_report(
    args: &SetupUserArgs,
    distro: &str,
) -> AppResult<SetupUserReport> {
    validate_setup_username(&args.username)?;
    let password_updated = match &args.password {
        Some(password) => {
            validate_setup_password(password)?;
            true
        }
        None => args.password_stdin,
    };

    let mut notes = vec![format!(
        "Pane would configure '{}' as the default WSL user for {} after the managed Arch distro is provisioned.",
        args.username, distro
    )];
    notes.push(
        "Pane would also ensure /etc/wsl.conf advertises systemd=true so the Arch desktop path can start cleanly."
            .to_string(),
    );
    if args.password_stdin {
        notes.push(
            "Dry run mode did not read the password from stdin, but the live onboarding flow would apply it during user setup."
                .to_string(),
        );
    }
    if !args.no_shutdown {
        notes.push(
            "WSL would be shut down after setup so the new default user and systemd settings take effect immediately."
                .to_string(),
        );
    }

    Ok(SetupUserReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        distro: distro.to_string(),
        username: args.username.clone(),
        dry_run: true,
        password_updated,
        default_user_configured: true,
        systemd_configured: true,
        wsl_shutdown: false,
        notes,
    })
}

fn setup_user(args: SetupUserArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = configure_arch_user(&args, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_setup_user_report(&report);
    }

    Ok(())
}

fn configure_arch_user(
    args: &SetupUserArgs,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<SetupUserReport> {
    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only configure a user inside a live WSL installation.",
        ));
    }

    let distro = resolve_operational_distro(args.distro.as_deref(), inventory, saved_state)?;
    if !inventory_contains_distro(inventory, &distro) {
        return Err(AppError::message(format!(
            "Resolved distro '{}' is not currently installed in WSL. Run `pane init` first or pass --distro <arch-distro-name>.",
            distro
        )));
    }

    validate_setup_username(&args.username)?;
    let password = resolve_setup_user_password(args)?;
    let distro_record = wsl::inspect_distro(&distro, inventory)?;
    if !distro_record.is_mvp_supported() {
        return Err(AppError::message(format!(
            "Pane MVP currently supports user setup only for Arch Linux + XFCE paths. Resolved distro: {}.",
            distro_record.label()
        )));
    }

    let current_wsl_conf = wsl::run_wsl_shell_as_user(
        &distro,
        Some("root"),
        "cat /etc/wsl.conf 2>/dev/null || true",
    )
    .unwrap_or_default();
    let updated_wsl_conf = ensure_wsl_conf_setting(
        &ensure_wsl_conf_setting(&current_wsl_conf, "boot", "systemd", "true"),
        "user",
        "default",
        &args.username,
    );

    let mut notes = vec![format!(
        "Pane will configure '{}' as the default WSL user for {}.",
        args.username, distro
    )];
    notes.push(
        "Pane also ensures /etc/wsl.conf advertises systemd=true so the Arch desktop path can start cleanly."
            .to_string(),
    );
    if args.dry_run {
        if !args.no_shutdown {
            notes.push(
                "WSL would be shut down after setup so the new default user and systemd settings take effect immediately."
                    .to_string(),
            );
        }
        return Ok(SetupUserReport {
            product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
            distro,
            username: args.username.clone(),
            dry_run: true,
            password_updated: password.is_some(),
            default_user_configured: true,
            systemd_configured: true,
            wsl_shutdown: false,
            notes,
        });
    }

    let setup_command = build_setup_user_shell_command(&args.username);
    let password = password.expect("validated password for non-dry-run setup");
    let credentials = format!("{}:{}\n", args.username, password);
    let setup_transcript = run_wsl_shell_as_user_capture_with_input(
        &distro,
        Some("root"),
        &setup_command,
        &credentials,
    )?;
    if !setup_transcript.success {
        return Err(AppError::message(format!(
            "Pane could not configure user '{}' inside {}: {}",
            args.username,
            distro,
            setup_transcript.combined_output().trim()
        )));
    }

    let write_conf_command =
        format!("cat > /etc/wsl.conf <<'__PANE_WSL_CONF__'\n{updated_wsl_conf}__PANE_WSL_CONF__");
    let write_conf = run_wsl_shell_as_user_capture(&distro, Some("root"), &write_conf_command)?;
    if !write_conf.success {
        return Err(AppError::message(format!(
            "Pane could not update /etc/wsl.conf inside {}: {}",
            distro,
            write_conf.combined_output().trim()
        )));
    }

    let wsl_shutdown = if args.no_shutdown {
        notes.push(
            "WSL was left running. Restart WSL manually before relying on the new default user or systemd state."
                .to_string(),
        );
        false
    } else {
        let shutdown = wsl::shutdown_wsl()?;
        if !shutdown.success {
            return Err(AppError::message(format!(
                "Pane configured '{}' but could not restart WSL: {}",
                args.username,
                shutdown.combined_output().trim()
            )));
        }
        notes.push(
            "WSL was shut down so the new default user and systemd settings will apply on the next launch."
                .to_string(),
        );
        true
    };

    Ok(SetupUserReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        distro,
        username: args.username.clone(),
        dry_run: false,
        password_updated: true,
        default_user_configured: true,
        systemd_configured: true,
        wsl_shutdown,
        notes,
    })
}

fn launch(args: LaunchArgs) -> AppResult<()> {
    if args.runtime == RuntimeMode::PaneOwned {
        return launch_pane_owned_runtime(args);
    }

    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for_with_shared_storage(&session_name, args.shared_storage);

    let plan = LaunchPlan {
        steps: build_steps(
            &target.distro,
            args.de,
            args.port,
            !args.skip_bootstrap,
            !args.no_connect,
        ),
        bootstrap_script: render_bootstrap_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: !args.no_connect,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch = StoredLaunch::planned_from_plan(
        &plan,
        args.dry_run,
        target.hypothetical,
        !args.skip_bootstrap,
        !args.no_connect,
    );
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: !args.skip_bootstrap,
        connect_requested: !args.no_connect,
        write_probes_enabled: true,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_launch_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane launch", &doctor_report)),
        ));
    }

    if !args.skip_bootstrap {
        if let Err(error) = execute_bootstrap(&plan) {
            return Err(fail_launch(&mut stored_launch, error));
        }

        stored_launch.mark_bootstrapped();
        save_state_record(stored_launch.clone())?;
        println!(
            "Bootstrap completed inside {}. Transcript: {}",
            plan.distro.name,
            plan.workspace.bootstrap_log.display()
        );

        if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
            return Err(fail_launch(
                &mut stored_launch,
                AppError::message(format!(
                    "XRDP did not become ready inside WSL on port {} after bootstrap. Review {} or run `pane logs`.",
                    plan.port,
                    plan.workspace.bootstrap_log.display()
                )),
            ));
        }
    } else {
        println!("Skipped the WSL bootstrap step.");
    }

    if !args.no_connect {
        let transport = ensure_transport_ready(&plan.distro.name, plan.port, &plan.workspace)
            .map_err(|error| fail_launch(&mut stored_launch, error))?;
        if let Err(error) = write_runtime_rdp_profile(
            &plan.workspace.rdp_profile,
            &plan.distro,
            &transport.host,
            plan.port,
        ) {
            return Err(fail_launch(&mut stored_launch, error));
        }
        if let Err(error) = open_rdp_profile(&plan.workspace.rdp_profile) {
            return Err(fail_launch(&mut stored_launch, error));
        }

        stored_launch.mark_rdp_launched(transport.kind);
        save_state_record(stored_launch.clone())?;
        println!(
            "Opened mstsc.exe with {} over {} targeting {}:{}.",
            plan.workspace.rdp_profile.display(),
            transport.kind.display_name(),
            transport.host,
            plan.port,
        );
    } else {
        println!(
            "RDP profile written to {}. Open it manually when ready.",
            plan.workspace.rdp_profile.display()
        );
    }

    Ok(())
}

fn launch_pane_owned_runtime(args: LaunchArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let report = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, true)?;

    println!("Pane-Owned Runtime Launch");
    println!("  Session        {}", report.session_name);
    println!("  Requested      {}", args.runtime.display_name());
    println!("  Runtime Root   {}", report.dedicated_space_root);
    println!("  State          {}", report.native_runtime.state_label);
    println!(
        "  Host Ready     {}",
        yes_no(report.native_runtime.host_ready)
    );
    println!(
        "  Boot Spike     {}",
        yes_no(report.native_runtime.ready_for_boot_spike)
    );
    println!(
        "  Bootable       {}",
        yes_no(report.native_runtime.bootable)
    );
    println!(
        "  Uses WSL       {}",
        yes_no(report.native_runtime.requires_wsl)
    );
    println!(
        "  Uses mstsc.exe {}",
        yes_no(report.native_runtime.requires_mstsc)
    );
    println!(
        "  Uses XRDP      {}",
        yes_no(report.native_runtime.requires_xrdp)
    );
    println!("  Contract       {}", report.native_runtime.launch_contract);
    if !report.native_runtime.blockers.is_empty() {
        println!("Blockers");
        for blocker in &report.native_runtime.blockers {
            println!("  - {}", blocker);
        }
    }

    if args.dry_run {
        println!("  Dry Run        native runtime storage prepared; no OS boot attempted");
        return Ok(());
    }

    Err(AppError::message(format!(
        "Pane-owned runtime launch is not bootable yet. Current blockers:\n{}",
        format_blocker_list(&report.native_runtime.blockers)
    )))
}

fn repair(args: RepairArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for_with_shared_storage(&session_name, args.shared_storage);

    let plan = LaunchPlan {
        steps: build_steps(&target.distro, args.de, args.port, true, false),
        bootstrap_script: render_bootstrap_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: false,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch =
        StoredLaunch::planned_from_plan(&plan, args.dry_run, target.hypothetical, true, false);
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: true,
        connect_requested: false,
        write_probes_enabled: true,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_repair_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane repair", &doctor_report)),
        ));
    }

    if let Err(error) = execute_bootstrap(&plan) {
        return Err(fail_launch(&mut stored_launch, error));
    }

    stored_launch.mark_bootstrapped();
    save_state_record(stored_launch.clone())?;
    println!(
        "Repair completed inside {}. Transcript: {}",
        plan.distro.name,
        plan.workspace.bootstrap_log.display()
    );

    if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format!(
                "XRDP did not become ready inside WSL on port {} after repair. Review {} or run `pane logs`.",
                plan.port,
                plan.workspace.bootstrap_log.display()
            )),
        ));
    }

    println!(
        "Pane repair finished. Reconnect with `pane connect --session-name {}` or use the Control Center.",
        plan.session_name
    );

    Ok(())
}

fn update(args: UpdateArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for_with_shared_storage(&session_name, args.shared_storage);

    let plan = LaunchPlan {
        steps: build_update_steps(&target.distro, args.de, args.port),
        bootstrap_script: render_update_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: false,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch =
        StoredLaunch::planned_from_plan(&plan, args.dry_run, target.hypothetical, true, false);
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: true,
        connect_requested: false,
        write_probes_enabled: true,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_update_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane update", &doctor_report)),
        ));
    }

    if let Err(error) = execute_bootstrap(&plan) {
        return Err(fail_launch(&mut stored_launch, error));
    }

    stored_launch.mark_bootstrapped();
    save_state_record(stored_launch.clone())?;
    println!(
        "Update completed inside {}. Transcript: {}",
        plan.distro.name,
        plan.workspace.bootstrap_log.display()
    );

    if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format!(
                "XRDP did not become ready inside WSL on port {} after update. Review {} or run `pane logs`.",
                plan.port,
                plan.workspace.bootstrap_log.display()
            )),
        ));
    }

    println!(
        "Pane update finished. Reconnect with `pane connect --session-name {}` or use the Control Center.",
        plan.session_name
    );

    Ok(())
}

fn status(args: StatusArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = build_status_report(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_status_report(&report);
    Ok(())
}

fn app_status(args: AppStatusArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = build_app_status_report(&args.session_name, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_app_status_report(&report);
    Ok(())
}

fn runtime(args: RuntimeArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let paths = crate::plan::runtime_for(&session_name);
    let budget = runtime_storage_budget(args.capacity_gib);
    let has_runtime_mutation = args.register_base_image.is_some()
        || args.register_boot_loader.is_some()
        || args.register_kernel.is_some()
        || args.register_initramfs.is_some()
        || args.kernel_cmdline.is_some()
        || args.create_user_disk
        || args.create_serial_boot_image
        || args.prepare;

    if has_runtime_mutation {
        prepare_runtime_paths(&paths)?;
        write_runtime_config(&paths, &session_name, &budget)?;
        write_native_runtime_manifest(&paths, &session_name)?;
        write_framebuffer_contract(&paths)?;
        write_input_contract(&paths)?;
    }

    if let Some(source_image) = args.register_base_image.as_deref() {
        register_base_os_image(
            &paths,
            source_image,
            args.expected_sha256.as_deref(),
            args.force,
        )?;
    } else if args.expected_sha256.is_some() {
        return Err(AppError::message(
            "--expected-sha256 requires --register-base-image.",
        ));
    }

    if let Some(source_image) = args.register_boot_loader.as_deref() {
        let expected_serial = decode_serial_text(
            args.boot_loader_expected_serial
                .as_deref()
                .unwrap_or(crate::native::SERIAL_BOOT_BANNER_TEXT),
        )?;
        register_boot_loader_image(
            &paths,
            source_image,
            args.boot_loader_expected_sha256.as_deref(),
            &expected_serial,
            args.force,
        )?;
    } else if args.boot_loader_expected_sha256.is_some()
        || args.boot_loader_expected_serial.is_some()
    {
        return Err(AppError::message(
            "--boot-loader-expected-sha256 and --boot-loader-expected-serial require --register-boot-loader.",
        ));
    }

    if args.register_kernel.is_some()
        || args.register_initramfs.is_some()
        || args.kernel_cmdline.is_some()
    {
        register_kernel_boot_plan(
            &paths,
            args.register_kernel.as_deref(),
            args.kernel_expected_sha256.as_deref(),
            args.register_initramfs.as_deref(),
            args.initramfs_expected_sha256.as_deref(),
            args.kernel_cmdline.as_deref(),
            args.force,
        )?;
    } else if args.kernel_expected_sha256.is_some() || args.initramfs_expected_sha256.is_some() {
        return Err(AppError::message(
            "--kernel-expected-sha256 requires --register-kernel, and --initramfs-expected-sha256 requires --register-initramfs.",
        ));
    }

    if args.create_user_disk {
        create_user_disk_descriptor(&paths, &budget, args.force)?;
    }

    if args.create_serial_boot_image {
        create_serial_boot_image_artifact(&paths, args.force)?;
    }

    let mut report = build_runtime_report(&session_name, args.capacity_gib, false)?;
    if has_runtime_mutation {
        write_runtime_manifest(&paths, &report)?;
        report = build_runtime_report(&session_name, args.capacity_gib, false)?;
        write_runtime_manifest(&paths, &report)?;
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_runtime_report(&report);
    Ok(())
}

fn native_preflight(args: NativePreflightArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let host = runtime.native_host.clone();
    let ready_for_boot_spike = runtime.native_runtime.ready_for_boot_spike;
    let blockers = runtime.native_runtime.blockers.clone();
    let mut next_steps = host.next_steps.clone();
    next_steps.extend(runtime.next_steps.clone());
    next_steps.dedup();

    let report = NativePreflightReport {
        product_shape: "Pane native-runtime preflight for moving from runtime artifacts to a WHP boot/display engine.",
        session_name,
        host,
        runtime,
        ready_for_boot_spike,
        blockers,
        next_steps,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_native_preflight_report(&report);
    Ok(())
}

fn native_boot_spike(args: NativeBootSpikeArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let runtime_paths = crate::plan::runtime_for(&session_name);
    let runtime_budget = runtime_storage_budget(DEFAULT_RUNTIME_CAPACITY_GIB);
    let run_guest_image = args.run_fixture || args.run_boot_loader || args.run_kernel_layout;
    let mut runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let boot_image = if args.execute && args.run_fixture {
        prepare_runtime_paths(&runtime_paths)?;
        write_runtime_config(&runtime_paths, &session_name, &runtime_budget)?;
        write_native_runtime_manifest(&runtime_paths, &session_name)?;
        write_framebuffer_contract(&runtime_paths)?;
        write_input_contract(&runtime_paths)?;
        create_serial_boot_image_artifact(&runtime_paths, false)?;
        let image = load_serial_boot_image_artifact(&runtime_paths)?;
        runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
        Some(image)
    } else if args.execute && args.run_boot_loader && runtime.artifacts.boot_loader_image_verified {
        Some(load_boot_loader_image_artifact(&runtime_paths)?)
    } else if args.execute && args.run_kernel_layout && runtime.artifacts.kernel_boot_layout_ready {
        Some(load_kernel_layout_boot_image_artifact(&runtime_paths)?)
    } else {
        None
    };
    let host = runtime.native_host.clone();
    let partition_smoke = crate::native::run_partition_smoke(
        args.execute,
        run_guest_image,
        boot_image.as_ref(),
        &host,
    );
    let protected_linux_entry_requested =
        partition_smoke.entry_mode.as_deref() == Some("linux-protected-mode-32");
    let ready_for_serial_kernel_spike = args.execute
        && run_guest_image
        && partition_smoke.status == crate::native::NativePartitionSmokeStatus::Pass
        && host.ready_for_boot_spike
        && runtime.prepared
        && runtime.artifacts.runtime_config_exists
        && runtime.artifacts.native_manifest_exists
        && (!args.run_fixture || runtime.artifacts.serial_boot_image_ready)
        && (!args.run_boot_loader || runtime.artifacts.boot_loader_image_verified)
        && (!args.run_kernel_layout || runtime.artifacts.kernel_boot_layout_ready)
        && (!protected_linux_entry_requested || partition_smoke.serial_text.is_some());

    let mut blockers = Vec::new();
    if !args.execute {
        blockers.push(
            "WHP boot spike was not executed. Rerun with `--execute` to exercise the partition/vCPU lifecycle."
                .to_string(),
        );
    }
    if args.run_fixture && !args.execute {
        blockers.push(
            "Serial fixture was requested but not executed. Rerun with `--execute --run-fixture`."
                .to_string(),
        );
    }
    if args.run_boot_loader && !args.execute {
        blockers.push(
            "Boot-loader candidate was requested but not executed. Rerun with `--execute --run-boot-loader`."
                .to_string(),
        );
    }
    if args.run_kernel_layout && !args.execute {
        blockers.push(
            "Kernel-layout candidate was requested but not executed. Rerun with `--execute --run-kernel-layout`."
                .to_string(),
        );
    }
    if args.run_boot_loader && !runtime.artifacts.boot_loader_image_verified {
        blockers.push(
            "No verified Pane-owned boot-to-serial loader exists. Register one with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256>`."
                .to_string(),
        );
    }
    if args.run_kernel_layout && !runtime.artifacts.kernel_boot_layout_ready {
        blockers.push(
            "No materialized Pane kernel boot layout exists. Run `pane native-kernel-plan --materialize` after registering a verified kernel plan."
                .to_string(),
        );
    }
    if run_guest_image {
        if !runtime.prepared {
            blockers.push(
                "Dedicated runtime directories have not been prepared. Run `pane runtime --prepare`."
                    .to_string(),
            );
        }
        if !runtime.artifacts.runtime_config_exists {
            blockers.push("The runtime config file is missing.".to_string());
        }
        if !runtime.artifacts.native_manifest_exists {
            blockers.push("The native runtime manifest is missing.".to_string());
        }
        if args.run_fixture && !runtime.artifacts.serial_boot_image_ready {
            blockers.push(
                "No valid Pane-owned serial boot test image exists. Run `pane runtime --create-serial-boot-image`."
                    .to_string(),
            );
        }
        for check in &host.checks {
            if check.status == crate::native::NativePreflightStatus::Fail {
                let mut blocker =
                    format!("Native host check `{}` failed: {}", check.id, check.summary);
                if let Some(remediation) = &check.remediation {
                    blocker.push_str(" Fix: ");
                    blocker.push_str(remediation);
                }
                blockers.push(blocker);
            }
        }
    } else if !runtime.native_runtime.ready_for_boot_spike {
        blockers.extend(runtime.native_runtime.blockers.clone());
    }
    if let Some(blocker) = &partition_smoke.blocker {
        blockers.push(blocker.clone());
    }
    if args.execute
        && partition_smoke.status != crate::native::NativePartitionSmokeStatus::Pass
        && partition_smoke.blocker.is_none()
    {
        blockers.push(
            "WHP boot spike did not pass; inspect the call report for HRESULT details.".to_string(),
        );
    }

    let report = NativeBootSpikeReport {
        product_shape:
            "Pane WHP boot-spike host step for moving from preflight to boot-to-serial execution.",
        session_name,
        execute_requested: args.execute,
        fixture_requested: args.run_fixture,
        boot_loader_requested: args.run_boot_loader,
        kernel_layout_requested: args.run_kernel_layout,
        host,
        runtime,
        partition_smoke,
        ready_for_serial_kernel_spike,
        blockers,
        next_steps: native_boot_spike_next_steps(
            args.execute,
            args.run_fixture,
            args.run_boot_loader,
            args.run_kernel_layout,
        ),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_native_boot_spike_report(&report);
    Ok(())
}

fn native_boot_spike_next_steps(
    execute: bool,
    run_fixture: bool,
    run_boot_loader: bool,
    run_kernel_layout: bool,
) -> Vec<String> {
    if !execute {
        if run_kernel_layout {
            return vec![
                "Rerun with `--execute --run-kernel-layout` to consume the materialized kernel layout in WHP."
                    .to_string(),
                "Use only a controlled small serial/HALT candidate until the Linux boot-protocol runner exists."
                    .to_string(),
            ];
        }
        return vec![
            "Rerun with `--execute` to create and tear down the guarded WHP partition/vCPU."
                .to_string(),
            "Then rerun with `--execute --run-fixture` to prove guest memory, register setup, vCPU execution, and serial I/O exit handling."
                .to_string(),
        ];
    }

    if run_boot_loader {
        return vec![
            "Replace the controlled boot-to-serial loader with the first real kernel/initramfs boot path."
                .to_string(),
            "Connect that runner to Pane's verified Arch base image and user disk once serial boot output is deterministic.".to_string(),
            "Keep GUI work behind the boot-to-serial milestone so Pane does not advertise a rendered OS before it can boot one.".to_string(),
        ];
    }

    if run_kernel_layout {
        return vec![
            "Inspect the Linux protected-mode entry probe exit reason and serial output, if any."
                .to_string(),
            "Expand the guest memory map and CPU setup until a real bzImage produces deterministic early serial output."
                .to_string(),
            "Only promote this to a real Arch boot milestone after protected-mode kernel entry produces deterministic serial output."
                .to_string(),
        ];
    }

    if !run_fixture {
        return vec![
            "Rerun with `--execute --run-fixture` to map guest memory and execute the deterministic serial test image."
                .to_string(),
            "Only after the serial test image passes, replace it with a boot-to-serial kernel or loader."
                .to_string(),
        ];
    }

    if !run_boot_loader {
        return vec![
            "Register a controlled boot-to-serial loader with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256>`."
                .to_string(),
            "Run `pane native-boot-spike --execute --run-boot-loader` to prove Pane can execute a runtime-provided boot candidate."
                .to_string(),
            "Materialize a kernel boot layout with `pane native-kernel-plan --materialize`, then run `pane native-boot-spike --execute --run-kernel-layout` with a controlled small candidate."
                .to_string(),
            "Connect that loader to Pane's verified Arch base image and user disk only after its serial contract is deterministic."
                .to_string(),
        ];
    }

    vec![
        "Register a controlled boot-to-serial loader with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256>`."
            .to_string(),
        "Run `pane native-boot-spike --execute --run-boot-loader` to prove Pane can execute a runtime-provided boot candidate."
            .to_string(),
        "Materialize a kernel boot layout with `pane native-kernel-plan --materialize`, then run `pane native-boot-spike --execute --run-kernel-layout` with a controlled small candidate."
            .to_string(),
        "Connect that loader to Pane's verified Arch base image and user disk only after its serial contract is deterministic."
            .to_string(),
    ]
}

fn native_kernel_plan(args: NativeKernelPlanArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let paths = crate::plan::runtime_for(&session_name);
    let runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let mut blockers = Vec::new();

    if !runtime.prepared {
        blockers.push(
            "Dedicated runtime directories have not been prepared. Run `pane runtime --prepare`."
                .to_string(),
        );
    }
    if !runtime.artifacts.kernel_boot_plan_ready {
        blockers.push(
            "No verified kernel boot plan exists. Run `pane runtime --register-kernel <path> --kernel-expected-sha256 <sha256> --kernel-cmdline \"console=ttyS0 ...\"`."
                .to_string(),
        );
    }
    if !runtime.artifacts.framebuffer_contract_ready {
        blockers.push(
            "No valid Pane framebuffer contract exists. Run `pane runtime --prepare`.".to_string(),
        );
    }
    if !runtime.artifacts.input_contract_ready {
        blockers
            .push("No valid Pane input contract exists. Run `pane runtime --prepare`.".to_string());
    }

    let layout = if blockers.is_empty() {
        Some(build_kernel_boot_layout(
            &paths,
            &session_name,
            args.materialize,
        )?)
    } else {
        None
    };
    let runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let ready_for_kernel_entry_spike =
        blockers.is_empty() && layout.is_some() && runtime.artifacts.kernel_boot_layout_ready;

    let report = NativeKernelPlanReport {
        product_shape:
            "Pane native kernel boot-layout contract for the future WHP kernel-entry spike.",
        session_name,
        materialize_requested: args.materialize,
        runtime,
        ready_for_kernel_entry_spike,
        layout,
        blockers,
        next_steps: vec![
            "Implement WHP mapping for boot params, cmdline, kernel, and optional initramfs using this layout."
                .to_string(),
            "When storage artifacts are verified, expose the attached base OS image as the read-only root device and the Pane user disk as writable user/package storage."
                .to_string(),
            "Expose the framebuffer and input contracts as guest-visible devices before attempting a desktop session."
                .to_string(),
            "Enter the Linux boot protocol only after serial console output can be captured deterministically."
                .to_string(),
            "Keep GUI/display work behind a successful boot-to-serial kernel-entry spike."
                .to_string(),
        ],
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_native_kernel_plan_report(&report);
    Ok(())
}

fn environments(args: EnvironmentsArgs) -> AppResult<()> {
    let report = build_environment_catalog_report();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_environment_catalog_report(&report);
    Ok(())
}

fn doctor(args: DoctorArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let request = DoctorRequest {
        distro: args.distro,
        session_name: crate::plan::sanitize_session_name(&args.session_name),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: !args.skip_bootstrap,
        connect_requested: !args.no_connect,
        write_probes_enabled: !args.no_write,
    };
    let report = evaluate_doctor(&request, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_doctor_report(&report);
    Ok(())
}

fn connect(args: ConnectArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let mut launch = resolve_saved_launch(args.session_name.as_deref(), saved_state.as_ref())?;
    let report = evaluate_doctor(
        &DoctorRequest {
            distro: Some(launch.distro.name.clone()),
            session_name: launch.session_name.clone(),
            desktop_environment: launch.desktop_environment,
            port: launch.port,
            bootstrap_requested: false,
            connect_requested: true,
            write_probes_enabled: true,
        },
        &inventory,
        saved_state.as_ref(),
    )?;

    if report.has_failures() && !args.force {
        return Err(AppError::message(format_doctor_blockers(
            "pane connect",
            &report,
        )));
    }

    if !launch.workspace.rdp_profile.exists() {
        return Err(AppError::message(format!(
            "The saved RDP profile was not found at {}. Run `pane launch` again.",
            launch.workspace.rdp_profile.display()
        )));
    }

    let transport = ensure_transport_ready(&launch.distro.name, launch.port, &launch.workspace)?;
    write_runtime_rdp_profile(
        &launch.workspace.rdp_profile,
        &launch.distro,
        &transport.host,
        launch.port,
    )?;
    open_rdp_profile(&launch.workspace.rdp_profile)?;
    launch.mark_rdp_launched(transport.kind);
    save_state_record(launch.clone())?;
    println!(
        "Opened mstsc.exe with the saved Pane profile over {} targeting {}:{}.",
        transport.kind.display_name(),
        transport.host,
        launch.port,
    );
    Ok(())
}

fn relay(args: RelayArgs) -> AppResult<()> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], args.listen_port)))
        .map_err(|error| {
            AppError::message(format!(
                "failed to bind the Pane relay to 127.0.0.1:{}: {error}",
                args.listen_port
            ))
        })?;
    listener.set_nonblocking(true).map_err(|error| {
        AppError::message(format!(
            "failed to configure the Pane relay listener on 127.0.0.1:{}: {error}",
            args.listen_port
        ))
    })?;

    if !relay_backend_available(&args.distro, args.target_port, args.log_file.as_deref()) {
        return Err(AppError::message(format!(
            "the Pane relay could not verify a backend path into {}:{} before accepting RDP traffic",
            args.distro, args.target_port
        )));
    }

    if let Some(ready_file) = args.ready_file.as_deref() {
        if let Some(parent) = ready_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(ready_file, "ready\n").map_err(|error| {
            AppError::message(format!(
                "failed to publish the Pane relay readiness file at {}: {error}",
                ready_file.display()
            ))
        })?;
    }

    log_transport_event(
        args.log_file.as_deref(),
        &format!(
            "relay listening on 127.0.0.1:{} for {}:{}",
            args.listen_port, args.distro, args.target_port
        ),
    );

    let deadline = Instant::now() + Duration::from_secs(args.startup_timeout_seconds.max(1));
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                log_transport_event(
                    args.log_file.as_deref(),
                    &format!(
                        "relay accepted {} for {}:{}",
                        peer, args.distro, args.target_port
                    ),
                );
                relay_connection(
                    &args.distro,
                    args.target_port,
                    stream,
                    args.log_file.as_deref(),
                )?;
                log_transport_event(
                    args.log_file.as_deref(),
                    &format!(
                        "relay session finished for {}:{}",
                        args.distro, args.target_port
                    ),
                );
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    log_transport_event(
                        args.log_file.as_deref(),
                        &format!(
                            "relay timed out waiting for a client on 127.0.0.1:{}",
                            args.listen_port
                        ),
                    );
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(AppError::message(format!(
                    "the Pane relay failed while waiting on 127.0.0.1:{}: {error}",
                    args.listen_port
                )));
            }
        }
    }
}

fn relay_backend_available(distro: &str, target_port: u16, log_file: Option<&Path>) -> bool {
    if let Some(address) =
        wsl::distro_ipv4_address(distro).map(|ip| SocketAddr::from((ip, target_port)))
    {
        if socket_reachable(address) {
            log_transport_event(
                log_file,
                &format!("relay backend probe reached {address} directly"),
            );
            return true;
        }

        log_transport_event(
            log_file,
            &format!("relay backend direct probe to {address} failed; trying WSL nc"),
        );
    }

    if !wsl::distro_command_exists(distro, "nc") {
        log_transport_event(
            log_file,
            &format!("relay backend probe cannot use nc because it is missing inside {distro}"),
        );
        return false;
    }

    let probe_command = format!("nc -z 127.0.0.1 {target_port}");
    match run_wsl_shell_as_user_capture(distro, None, &probe_command) {
        Ok(transcript) if transcript.success => {
            log_transport_event(
                log_file,
                &format!("relay backend probe reached 127.0.0.1:{target_port} through WSL nc"),
            );
            true
        }
        Ok(transcript) => {
            log_transport_event(
                log_file,
                &format!(
                    "relay backend nc probe failed for {distro}:{target_port}: {}",
                    transcript.combined_output().trim()
                ),
            );
            false
        }
        Err(error) => {
            log_transport_event(
                log_file,
                &format!("relay backend probe failed for {distro}:{target_port}: {error}"),
            );
            false
        }
    }
}

fn share(args: ShareArgs) -> AppResult<()> {
    let saved_state = load_state()?;
    let (session_name, saved_launch, mut workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    if saved_launch.is_none() {
        workspace = workspace_for_with_shared_storage(&session_name, args.shared_storage);
    }
    let shared_directory = shared_dir_for_workspace(&workspace);
    fs::create_dir_all(&shared_directory)?;
    let shared_wsl_path = windows_to_wsl_path(&shared_directory);

    if !args.print_only {
        open_directory_in_explorer(&shared_directory)?;
    }

    println!("Pane Shared Directory");
    println!("  Session        {}", session_name);
    println!("  Windows Path   {}", shared_directory.display());
    println!("  WSL Path       {}", shared_wsl_path);
    println!("  Linux Link     ~/PaneShared");
    if !args.print_only {
        println!("  Explorer       opened");
    }

    Ok(())
}

fn terminal(args: TerminalArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;

    if args.print_only {
        let resolved =
            resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())
                .unwrap_or_else(|_| {
                    args.distro
                        .clone()
                        .unwrap_or_else(|| "pane-arch".to_string())
                });
        let selected_user = args.user.as_deref().unwrap_or("default");

        println!("Pane Arch Terminal");
        println!("  Distro         {}", resolved);
        println!("  User           {}", selected_user);
        println!("  Print Only     yes");
        println!(
            "  Managed Flow   Use this shell for first-run setup, package installs, dotfiles, and desktop customization."
        );
        return Ok(());
    }

    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only open a terminal inside a live WSL installation.",
        ));
    }

    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if !inventory_contains_distro(&inventory, &distro) {
        return Err(AppError::message(format!(
            "Resolved distro '{}' is not currently installed in WSL. Run `pane init` first or pass --distro <arch-distro-name>.",
            distro
        )));
    }

    let default_user = wsl::inspect_distro(&distro, &inventory)
        .ok()
        .and_then(|record| record.default_user);
    let selected_user = args.user.clone().or(default_user);

    println!("Pane Arch Terminal");
    println!("  Distro         {}", distro);
    println!(
        "  User           {}",
        selected_user.as_deref().unwrap_or("default")
    );
    println!(
        "  Managed Flow   Use this shell for first-run setup, package installs, dotfiles, and desktop customization."
    );

    wsl::open_interactive_terminal(&distro, args.user.as_deref())
}

fn stop(args: StopArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only stop XRDP inside a live WSL installation.",
        ));
    }

    let output = wsl::stop_xrdp_services(&distro)?;
    println!("Stopped XRDP services inside {distro}.");
    if !output.trim().is_empty() {
        println!("{}", output.trim());
    }
    Ok(())
}

fn reset(args: ResetArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (normalized_session, saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    let durable_shared =
        workspace_for_with_shared_storage(&normalized_session, SharedStorageMode::Durable)
            .shared_dir;
    let managed_environment = resolve_managed_environment_for_reset(&args, saved_state.as_ref())?;
    let managed_distro = managed_environment
        .as_ref()
        .map(|environment| resolve_reset_distro_name(args.distro.as_deref(), environment))
        .transpose()?;

    if args.dry_run {
        println!("Pane Reset Plan");
        if workspace.root.exists() {
            println!(
                "  Session Workspace  would remove {}",
                workspace.root.display()
            );
        } else {
            println!(
                "  Session Workspace  no workspace exists at {}",
                workspace.root.display()
            );
        }
        if args.purge_shared {
            if durable_shared.exists() {
                println!(
                    "  Durable Shared     would remove {}",
                    durable_shared.display()
                );
            } else {
                println!(
                    "  Durable Shared     no durable shared directory exists at {}",
                    durable_shared.display()
                );
            }
        } else {
            println!(
                "  Durable Shared     would preserve {}",
                durable_shared.display()
            );
        }
        if args.purge_wsl {
            let purge_target = managed_distro.clone().or_else(|| {
                resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())
                    .ok()
            });
            let distro = purge_target.as_deref().unwrap_or("not resolved");
            println!(
                "  WSL Purge          would stop XRDP and remove Pane session assets in {}",
                distro
            );
        }
        if let Some(environment) = &managed_environment {
            if args.release_managed_environment {
                println!(
                    "  Managed Reset      would release Pane management for {} without deleting the distro",
                    environment.distro_name
                );
            }
            if args.factory_reset {
                println!(
                    "  Managed Reset      would unregister {} from WSL and clear Pane ownership",
                    environment.distro_name
                );
                if let Some(install_dir) = &environment.install_dir {
                    println!(
                        "  Install Root       would remove {}",
                        install_dir.display()
                    );
                }
            }
        }
        if saved_launch.is_some() || args.session_name.is_none() {
            println!("  Saved State        would clear the saved launch state");
        }
        println!("  Dry Run            no files, WSL distros, or Pane state were changed");
        return Ok(());
    }

    if args.purge_wsl && !args.factory_reset {
        if let Ok(distro) =
            resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())
        {
            for note in purge_wsl_integration(&distro, &inventory)? {
                println!("WSL purge: {note}");
            }
        }
    }

    if let Some(environment) = &managed_environment {
        let distro_name = managed_distro
            .as_deref()
            .unwrap_or(&environment.distro_name);

        if args.factory_reset {
            if inventory.available && inventory_contains_distro(&inventory, distro_name) {
                for note in purge_wsl_integration(distro_name, &inventory)? {
                    println!("Factory reset: {note}");
                }
                let transcript = wsl::unregister_distro(distro_name)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL unregister failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }
                println!("Unregistered {distro_name} from WSL.");
            } else {
                println!("Managed distro {distro_name} was not present in WSL.");
            }

            if let Some(install_dir) = &environment.install_dir {
                if install_dir.exists() {
                    fs::remove_dir_all(install_dir)?;
                    println!("Removed {}.", install_dir.display());
                } else {
                    println!(
                        "No managed install root existed at {}.",
                        install_dir.display()
                    );
                }
            }
        }

        clear_managed_environment(Some(distro_name))?;
        if args.factory_reset {
            println!("Cleared Pane ownership for {distro_name} after factory reset.");
        } else {
            println!("Released Pane management for {distro_name} without deleting the distro.");
        }
    }

    if workspace.root.exists() {
        fs::remove_dir_all(&workspace.root)?;
        println!("Removed {}.", workspace.root.display());
    } else {
        println!("No Pane workspace existed at {}.", workspace.root.display());
    }

    if args.purge_shared {
        if durable_shared.exists() {
            fs::remove_dir_all(&durable_shared)?;
            println!(
                "Removed durable shared storage {}.",
                durable_shared.display()
            );
        } else {
            println!(
                "No durable shared storage existed at {}.",
                durable_shared.display()
            );
        }
    } else {
        println!(
            "Preserved durable shared storage at {}.",
            durable_shared.display()
        );
    }

    if saved_launch.is_some() || args.session_name.is_none() {
        clear_state()?;
        println!("Cleared saved Pane state.");
    }

    Ok(())
}

fn logs(args: LogsArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (normalized_session, _saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());

    println!("Pane Logs");
    println!("  Session        {}", normalized_session);
    println!("  Bootstrap Log  {}", workspace.bootstrap_log.display());
    println!();

    if workspace.bootstrap_log.exists() {
        println!("Bootstrap Transcript");
        println!("{}", fs::read_to_string(&workspace.bootstrap_log)?);
    } else {
        println!("Bootstrap Transcript");
        println!("  No bootstrap log has been captured for this session.");
    }

    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref()).ok();
    if let Some(distro) = distro.filter(|_| inventory.available) {
        let live_logs = wsl::tail_xrdp_logs(&distro, args.lines)?;
        println!();
        println!("Live XRDP Logs");
        if live_logs.trim().is_empty() {
            println!("  No XRDP log output was found inside {distro}.");
        } else {
            println!("{}", live_logs.trim_end());
        }
    }

    Ok(())
}

fn bundle(args: BundleArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (session_name, saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    let status_report =
        build_status_report(args.distro.as_deref(), &inventory, saved_state.as_ref())?;
    let doctor_request = build_bundle_doctor_request(
        &session_name,
        args.distro,
        saved_launch.as_ref(),
        saved_state.as_ref(),
        &inventory,
    )?;
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;
    let output_zip = resolve_bundle_output_path(args.output.as_deref(), &session_name);
    let staging_root = app_root().join("support").join(format!(
        "bundle-{}-{}",
        session_name,
        current_epoch_seconds()
    ));

    if staging_root.exists() {
        fs::remove_dir_all(&staging_root)?;
    }
    fs::create_dir_all(&staging_root)?;

    let manifest = write_support_bundle(
        &staging_root,
        &output_zip,
        &session_name,
        saved_state.as_ref(),
        &workspace,
        &status_report,
        &doctor_report,
    )?;

    if let Err(error) = compress_bundle_dir(&staging_root, &output_zip) {
        return Err(AppError::message(format!(
            "{error} Staged files remain at {}.",
            staging_root.display()
        )));
    }

    let _ = fs::remove_dir_all(&staging_root);

    println!("Pane Support Bundle");
    println!("  Session        {}", session_name);
    println!("  Output         {}", output_zip.display());
    println!("  Included Files {}", manifest.included_files.len());
    if !manifest.notes.is_empty() {
        println!("Notes");
        for note in manifest.notes {
            println!("  - {}", note);
        }
    }

    Ok(())
}

fn build_status_report(
    explicit_distro: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<StatusReport> {
    let distro_name = resolve_status_distro(explicit_distro, inventory, saved_state)?;
    let selected_distro = distro_name
        .as_deref()
        .map(|name| build_distro_health(name, inventory, saved_state, None))
        .transpose()?;
    let last_launch_workspace = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| inspect_workspace(&launch.workspace));

    Ok(StatusReport {
        platform: std::env::consts::OS,
        wsl_available: inventory.available,
        wsl_version_banner: inventory.version_banner.clone(),
        wsl_default_distro: inventory.default_distro.clone(),
        managed_environment: saved_state.and_then(|state| state.managed_environment.clone()),
        selected_distro,
        known_distros: inventory.distros.clone(),
        last_launch: saved_state.and_then(|state| state.last_launch.clone()),
        last_launch_workspace,
    })
}

fn build_app_status_report(
    session_name: &str,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<AppStatusReport> {
    let (normalized_session, saved_launch, workspace) =
        resolve_session_context(Some(session_name), saved_state);
    let status_report = build_status_report(None, inventory, saved_state)?;
    let target_distro = status_report
        .selected_distro
        .as_ref()
        .map(|health| health.distro.name.clone())
        .or_else(|| {
            status_report
                .managed_environment
                .as_ref()
                .map(|environment| environment.distro_name.clone())
        });
    let port = target_distro
        .as_deref()
        .map(|name| status_port_for(name, saved_state))
        .or_else(|| saved_launch.as_ref().map(|launch| launch.port))
        .unwrap_or(3390);
    let doctor_request = DoctorRequest {
        distro: target_distro,
        session_name: normalized_session.clone(),
        desktop_environment: DesktopEnvironment::Xfce,
        port,
        bootstrap_requested: true,
        connect_requested: false,
        write_probes_enabled: false,
    };
    let doctor_report = evaluate_doctor(&doctor_request, inventory, saved_state)?;
    let (phase, next_action, next_action_summary, mut notes) =
        determine_app_lifecycle(&status_report, &doctor_report, saved_launch.as_ref());

    notes.push(
        "The current desktop path is still an external mstsc.exe + XRDP handoff; Pane owns setup, repair, transport selection, and support around it."
            .to_string(),
    );
    notes.push(
        "Pane-owned runtime storage is now first-class, but boot and display ownership are still gated until the native engine can launch an OS image without WSL, mstsc.exe, or XRDP."
            .to_string(),
    );

    Ok(AppStatusReport {
        product_shape: "Windows app for owning Linux environment setup, launch, repair, shared storage, and support; Arch + XFCE is the current launchable profile.",
        session_name: normalized_session.clone(),
        phase,
        next_action,
        next_action_label: next_action.label(),
        next_action_summary,
        supported_profile: AppProfileReport {
            label: "Arch Linux + XFCE",
            distro_family: DistroFamily::Arch,
            desktop_environment: DesktopEnvironment::Xfce,
            launchable_now: true,
        },
        runtime: build_runtime_report(&normalized_session, DEFAULT_RUNTIME_CAPACITY_GIB, false)?,
        storage: build_app_storage_report(&normalized_session),
        display: build_app_display_report(),
        managed_environment: status_report.managed_environment,
        selected_distro: status_report.selected_distro,
        last_launch: saved_launch,
        workspace: inspect_workspace(&workspace),
        blockers: doctor_report
            .checks
            .into_iter()
            .filter(|check| check.status == CheckStatus::Fail)
            .map(|check| AppBlocker {
                id: check.id,
                summary: check.summary,
                remediation: check.remediation,
            })
            .collect(),
        notes,
    })
}

fn build_runtime_report(
    session_name: &str,
    capacity_gib: u64,
    prepare: bool,
) -> AppResult<RuntimeReport> {
    let normalized_session = crate::plan::sanitize_session_name(session_name);
    let paths = crate::plan::runtime_for(&normalized_session);

    if prepare {
        prepare_runtime_paths(&paths)?;
        write_runtime_config(
            &paths,
            &normalized_session,
            &runtime_storage_budget(capacity_gib),
        )?;
        write_native_runtime_manifest(&paths, &normalized_session)?;
        write_framebuffer_contract(&paths)?;
        write_input_contract(&paths)?;
    }

    let prepared = runtime_paths_prepared(&paths);
    let budget = runtime_storage_budget(capacity_gib);
    let artifacts = build_runtime_artifact_report(&paths);
    let native_host = crate::native::probe_native_host();
    let native_runtime = build_native_runtime_report(prepared, &artifacts, &native_host);
    let ownership = build_runtime_ownership_report(&native_runtime);
    let mut report = RuntimeReport {
        product_shape: "Pane-owned runtime boundary for a future contained OS engine. Storage is app-owned now; boot/display are not yet implemented.",
        session_name: normalized_session,
        current_engine: PaneRuntimeEngine::WslXrdpBridge,
        current_engine_label: PaneRuntimeEngine::WslXrdpBridge.display_name(),
        target_engine: PaneRuntimeEngine::PaneOwnedOsRuntime,
        target_engine_label: PaneRuntimeEngine::PaneOwnedOsRuntime.display_name(),
        prepared,
        dedicated_space_root: paths.root.display().to_string(),
        directories: RuntimeDirectoryReport {
            downloads: paths.downloads.display().to_string(),
            images: paths.images.display().to_string(),
            disks: paths.disks.display().to_string(),
            snapshots: paths.snapshots.display().to_string(),
            state: paths.state.display().to_string(),
            engines: paths.engines.display().to_string(),
            logs: paths.logs.display().to_string(),
            base_os_image: paths.base_os_image.display().to_string(),
            serial_boot_image: paths.serial_boot_image.display().to_string(),
            boot_loader_image: paths.boot_loader_image.display().to_string(),
            kernel_image: paths.kernel_image.display().to_string(),
            initramfs_image: paths.initramfs_image.display().to_string(),
            user_disk: paths.user_disk.display().to_string(),
            base_os_metadata: paths.base_os_metadata.display().to_string(),
            serial_boot_metadata: paths.serial_boot_metadata.display().to_string(),
            boot_loader_metadata: paths.boot_loader_metadata.display().to_string(),
            kernel_boot_metadata: paths.kernel_boot_metadata.display().to_string(),
            user_disk_metadata: paths.user_disk_metadata.display().to_string(),
            runtime_config: paths.runtime_config.display().to_string(),
            native_manifest: paths.native_manifest.display().to_string(),
            kernel_boot_layout: paths.kernel_boot_layout.display().to_string(),
            framebuffer_contract: paths.framebuffer_contract.display().to_string(),
            input_contract: paths.input_contract.display().to_string(),
            manifest: paths.manifest.display().to_string(),
        },
        storage_budget: budget,
        ownership,
        artifacts,
        native_host,
        native_runtime,
        current_limitation: "Pane owns the runtime storage layout, config, manifests, and host preflight now. It still cannot boot a Pane-owned OS image or draw a Pane-owned desktop window without the current WSL/XRDP bridge.",
        next_steps: vec![
            "Run `pane native-preflight --json` to prove the host can support the first WHP boot-to-serial spike."
                .to_string(),
            "Register a Pane-approved Arch base OS image with `pane runtime --register-base-image <path> --expected-sha256 <sha256>`."
                .to_string(),
            "Create the Pane-owned sparse user disk with `pane runtime --create-user-disk`."
                .to_string(),
            "Create the runtime-backed serial boot image with `pane runtime --create-serial-boot-image`."
                .to_string(),
            "Run `pane native-boot-spike --execute --run-fixture` to prove WHP guest memory, register setup, vCPU execution, and serial I/O."
                .to_string(),
            "Register and run a controlled boot-to-serial loader candidate with `pane runtime --register-boot-loader` and `pane native-boot-spike --execute --run-boot-loader`."
                .to_string(),
            "Register a verified kernel/initramfs boot plan with `pane runtime --register-kernel` and an explicit serial console cmdline."
                .to_string(),
            "Materialize the WHP kernel boot layout with `pane native-kernel-plan --materialize`."
                .to_string(),
            "Use the kernel layout storage attachment to connect the verified Arch base image and Pane user disk to the native boot path."
                .to_string(),
            "Use the framebuffer/input contracts as the first Pane-owned display boundary before attempting a full desktop compositor."
                .to_string(),
            "Replace the prepared kernel boot plan with actual WHP kernel entry, boot params, initramfs placement, and serial output capture."
                .to_string(),
            "Implement a Pane-owned display boundary instead of handing off to mstsc.exe over XRDP."
                .to_string(),
            "Move package installs, user customizations, snapshots, export/import, and repair semantics onto this runtime contract."
                .to_string(),
        ],
        notes: vec![
            "This is intentionally separate from PaneShared. PaneShared is user file exchange; the runtime user disk is the future Linux system/user storage."
                .to_string(),
            "No latency and full hardware compatibility cannot be guaranteed as absolute properties; the product target is no noticeable latency for normal desktop use with a measured compatibility envelope."
                .to_string(),
        ],
    };

    if prepare {
        write_runtime_manifest(&paths, &report)?;
        report.artifacts = build_runtime_artifact_report(&paths);
        report.native_host = crate::native::probe_native_host();
        report.native_runtime =
            build_native_runtime_report(prepared, &report.artifacts, &report.native_host);
        report.ownership = build_runtime_ownership_report(&report.native_runtime);
        write_runtime_manifest(&paths, &report)?;
    }

    Ok(report)
}

fn runtime_storage_budget(capacity_gib: u64) -> RuntimeStorageBudget {
    let requested_capacity_gib = capacity_gib.max(MINIMUM_RUNTIME_CAPACITY_GIB);
    let base_os_budget_gib = if requested_capacity_gib <= DEFAULT_RUNTIME_CAPACITY_GIB {
        4
    } else {
        (requested_capacity_gib / 4).clamp(4, 12)
    };
    let snapshot_budget_gib = (requested_capacity_gib / 8).clamp(1, 16);
    let user_packages_and_customizations_gib =
        requested_capacity_gib.saturating_sub(base_os_budget_gib + snapshot_budget_gib);

    RuntimeStorageBudget {
        requested_capacity_gib,
        base_os_budget_gib,
        user_packages_and_customizations_gib,
        snapshot_budget_gib,
        minimum_recommended_gib: MINIMUM_RUNTIME_CAPACITY_GIB,
    }
}

fn prepare_runtime_paths(paths: &RuntimePaths) -> AppResult<()> {
    for path in [
        &paths.root,
        &paths.downloads,
        &paths.images,
        &paths.disks,
        &paths.snapshots,
        &paths.state,
        &paths.engines,
        &paths.logs,
    ] {
        fs::create_dir_all(path)?;
    }

    write_framebuffer_contract(paths)?;
    write_input_contract(paths)?;

    Ok(())
}

fn default_framebuffer_contract() -> FramebufferContract {
    let width = 1024;
    let height = 768;
    let bytes_per_pixel = 4;
    FramebufferContract {
        schema_version: 1,
        device: "pane-linear-framebuffer-v1".to_string(),
        width,
        height,
        stride_bytes: width * bytes_per_pixel,
        bytes_per_pixel,
        format: "x8r8g8b8".to_string(),
        guest_gpa: "0x0e000000".to_string(),
        size_bytes: u64::from(width * height * bytes_per_pixel),
        resize_policy: "fixed-until-display-device-milestone".to_string(),
    }
}

fn default_input_contract() -> InputContract {
    InputContract {
        schema_version: 1,
        keyboard_device: "pane-ps2-keyboard-v1".to_string(),
        pointer_device: "pane-absolute-pointer-v1".to_string(),
        transport: "pane-host-event-queue".to_string(),
        coordinate_space: "framebuffer-pixels".to_string(),
        guest_queue_gpa: "0x0dff0000".to_string(),
        queue_size_bytes: 0x00001000,
        event_record_bytes: 32,
    }
}

fn write_framebuffer_contract(paths: &RuntimePaths) -> AppResult<()> {
    write_json_file(&paths.framebuffer_contract, &default_framebuffer_contract())
}

fn write_input_contract(paths: &RuntimePaths) -> AppResult<()> {
    write_json_file(&paths.input_contract, &default_input_contract())
}

fn runtime_paths_prepared(paths: &RuntimePaths) -> bool {
    [
        &paths.root,
        &paths.downloads,
        &paths.images,
        &paths.disks,
        &paths.snapshots,
        &paths.state,
        &paths.engines,
        &paths.logs,
    ]
    .iter()
    .all(|path| path.is_dir())
}

fn register_base_os_image(
    paths: &RuntimePaths,
    source_image: &Path,
    expected_sha256: Option<&str>,
    force: bool,
) -> AppResult<()> {
    if !source_image.is_file() {
        return Err(AppError::message(format!(
            "Base OS image source does not exist or is not a file: {}",
            source_image.display()
        )));
    }

    let expected_sha256 = expected_sha256.map(normalize_sha256_hex).transpose()?;
    let actual_sha256 = sha256_file(source_image)?;
    let verified = expected_sha256
        .as_deref()
        .map(|expected| expected == actual_sha256)
        .unwrap_or(false);

    if let Some(expected) = expected_sha256.as_deref() {
        if expected != actual_sha256 {
            return Err(AppError::message(format!(
                "Base OS image SHA-256 mismatch. expected {expected}, got {actual_sha256}."
            )));
        }
    }

    if let Some(parent) = paths.base_os_image.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.base_os_metadata.parent() {
        fs::create_dir_all(parent)?;
    }

    let same_target = paths.base_os_image.exists()
        && source_image.canonicalize().ok() == paths.base_os_image.canonicalize().ok();
    if paths.base_os_image.exists() && !force && !same_target {
        return Err(AppError::message(format!(
            "A registered base OS image already exists at {}. Pass --force to replace it.",
            paths.base_os_image.display()
        )));
    }

    if !same_target {
        let temp_image = paths.base_os_image.with_extension("paneimg.tmp");
        if temp_image.exists() {
            fs::remove_file(&temp_image)?;
        }
        fs::copy(source_image, &temp_image)?;
        if paths.base_os_image.exists() {
            fs::remove_file(&paths.base_os_image)?;
        }
        fs::rename(&temp_image, &paths.base_os_image)?;
    }

    let bytes = fs::metadata(&paths.base_os_image)?.len();
    let metadata = BaseOsImageMetadata {
        schema_version: 1,
        distro_family: DistroFamily::Arch,
        image_kind: "arch-base-os-image".to_string(),
        source_path: source_image
            .canonicalize()
            .unwrap_or_else(|_| source_image.to_path_buf())
            .display()
            .to_string(),
        stored_path: paths.base_os_image.display().to_string(),
        bytes,
        sha256: actual_sha256,
        expected_sha256,
        verified,
        registered_at_epoch_seconds: current_epoch_seconds(),
    };
    write_json_file(&paths.base_os_metadata, &metadata)
}

fn create_user_disk_descriptor(
    paths: &RuntimePaths,
    budget: &RuntimeStorageBudget,
    force: bool,
) -> AppResult<()> {
    if paths.user_disk.exists() && !force {
        if user_disk_artifact_ready(
            paths,
            &read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).ok(),
        ) {
            return Ok(());
        }

        if !legacy_user_disk_descriptor_exists(paths) {
            return Err(AppError::message(format!(
                "A user disk artifact already exists at {}, but its sparse-disk metadata is missing or invalid. Pass --force to replace it.",
                paths.user_disk.display()
            )));
        }
    }

    if let Some(parent) = paths.user_disk.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.user_disk_metadata.parent() {
        fs::create_dir_all(parent)?;
    }

    let capacity_gib = budget.user_packages_and_customizations_gib.max(1);
    let logical_size_bytes = user_disk_logical_size_bytes(capacity_gib)?;
    let header = user_disk_header_bytes(logical_size_bytes);
    let header_sha256 = sha256_bytes(&header);
    let metadata = UserDiskMetadata {
        schema_version: 1,
        format: PANE_USER_DISK_FORMAT.to_string(),
        disk_path: paths.user_disk.display().to_string(),
        capacity_gib,
        logical_size_bytes,
        block_size_bytes: PANE_USER_DISK_BLOCK_SIZE_BYTES,
        sparse_backing: true,
        allocated_header_bytes: header.len() as u64,
        header_sha256,
        materialized_block_device: true,
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This sparse Pane disk is the durable Linux user/package/customization storage artifact."
                .to_string(),
            "Only the header is allocated now; unallocated logical blocks are zero-filled by the future block-device engine."
                .to_string(),
        ],
    };

    fs::write(&paths.user_disk, header)?;
    write_json_file(&paths.user_disk_metadata, &metadata)
}

fn user_disk_logical_size_bytes(capacity_gib: u64) -> AppResult<u64> {
    capacity_gib
        .checked_mul(1024)
        .and_then(|value| value.checked_mul(1024))
        .and_then(|value| value.checked_mul(1024))
        .ok_or_else(|| AppError::message("Pane user disk capacity overflows u64 byte sizing."))
}

fn user_disk_header_bytes(logical_size_bytes: u64) -> Vec<u8> {
    format!(
        "{PANE_USER_DISK_MAGIC}format={PANE_USER_DISK_FORMAT}\nlogical_size_bytes={logical_size_bytes}\nblock_size_bytes={PANE_USER_DISK_BLOCK_SIZE_BYTES}\nzero_fill_unallocated=true\n\n"
    )
    .into_bytes()
}

fn legacy_user_disk_descriptor_exists(paths: &RuntimePaths) -> bool {
    [&paths.user_disk, &paths.user_disk_metadata]
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .any(|value| {
            value.get("format").and_then(|format| format.as_str())
                == Some("pane-user-disk-descriptor-v1")
        })
}

fn user_disk_artifact_ready(paths: &RuntimePaths, metadata: &Option<UserDiskMetadata>) -> bool {
    let Some(metadata) = metadata else {
        return false;
    };
    if !paths.user_disk.is_file()
        || metadata.schema_version != 1
        || metadata.format != PANE_USER_DISK_FORMAT
        || metadata.disk_path != paths.user_disk.display().to_string()
        || metadata.capacity_gib == 0
        || metadata.block_size_bytes != PANE_USER_DISK_BLOCK_SIZE_BYTES
        || !metadata.sparse_backing
        || !metadata.materialized_block_device
        || metadata.allocated_header_bytes == 0
    {
        return false;
    }

    let Ok(expected_logical_size) = user_disk_logical_size_bytes(metadata.capacity_gib) else {
        return false;
    };
    if metadata.logical_size_bytes != expected_logical_size {
        return false;
    }

    let Ok(header) = fs::read(&paths.user_disk) else {
        return false;
    };

    header.len() as u64 == metadata.allocated_header_bytes
        && header.starts_with(PANE_USER_DISK_MAGIC.as_bytes())
        && sha256_bytes(&header) == metadata.header_sha256
}

fn create_serial_boot_image_artifact(paths: &RuntimePaths, force: bool) -> AppResult<()> {
    let image = crate::native::serial_boot_test_image_bytes();
    let sha256 = sha256_bytes(&image);

    if paths.serial_boot_image.exists() && !force {
        if let Ok(metadata) = read_json_file::<SerialBootImageMetadata>(&paths.serial_boot_metadata)
        {
            let existing_len = fs::metadata(&paths.serial_boot_image)
                .ok()
                .map(|metadata| metadata.len());
            if metadata.schema_version == 1
                && metadata.image_kind == "pane-serial-boot-test-image"
                && metadata.bytes == image.len() as u64
                && metadata.sha256 == sha256
                && existing_len == Some(image.len() as u64)
            {
                return Ok(());
            }
        }
    }

    if let Some(parent) = paths.serial_boot_image.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.serial_boot_metadata.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_image = paths.serial_boot_image.with_extension("paneimg.tmp");
    if temp_image.exists() {
        fs::remove_file(&temp_image)?;
    }
    fs::write(&temp_image, &image)?;
    if paths.serial_boot_image.exists() {
        fs::remove_file(&paths.serial_boot_image)?;
    }
    fs::rename(&temp_image, &paths.serial_boot_image)?;

    let metadata = SerialBootImageMetadata {
        schema_version: 1,
        image_kind: "pane-serial-boot-test-image".to_string(),
        stored_path: paths.serial_boot_image.display().to_string(),
        bytes: image.len() as u64,
        sha256,
        serial_banner: crate::native::SERIAL_BOOT_BANNER_TEXT.to_string(),
        guest_entry_gpa: "0x1000".to_string(),
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This is Pane's deterministic WHP boot-spike image, not an Arch base OS image."
                .to_string(),
            "The native runner maps this image first so boot execution is tied to runtime artifacts instead of generated-only memory."
                .to_string(),
        ],
    };
    write_json_file(&paths.serial_boot_metadata, &metadata)
}

fn load_serial_boot_image_artifact(
    paths: &RuntimePaths,
) -> AppResult<crate::native::NativeSerialBootImage> {
    let metadata = read_json_file::<SerialBootImageMetadata>(&paths.serial_boot_metadata)?;
    let bytes = fs::read(&paths.serial_boot_image)?;
    let actual_sha256 = sha256_bytes(&bytes);
    if metadata.schema_version != 1
        || metadata.image_kind != "pane-serial-boot-test-image"
        || metadata.bytes != bytes.len() as u64
        || metadata.sha256 != actual_sha256
        || metadata.serial_banner != crate::native::SERIAL_BOOT_BANNER_TEXT
    {
        return Err(AppError::message(format!(
            "Pane serial boot image metadata does not match the artifact at {}. Rerun `pane runtime --create-serial-boot-image --force`.",
            paths.serial_boot_image.display()
        )));
    }

    Ok(crate::native::NativeSerialBootImage {
        source_label: "pane-runtime-serial-boot-image".to_string(),
        path: Some(paths.serial_boot_image.display().to_string()),
        bytes,
        expected_serial_text: crate::native::SERIAL_BOOT_BANNER_TEXT.to_string(),
        guest_entry_gpa: 0x1000,
        entry_mode: crate::native::NativeGuestEntryMode::RealModeSerial,
        boot_params_gpa: None,
        extra_regions: Vec::new(),
    })
}

fn register_boot_loader_image(
    paths: &RuntimePaths,
    source_image: &Path,
    expected_sha256: Option<&str>,
    expected_serial_text: &str,
    force: bool,
) -> AppResult<()> {
    if !source_image.is_file() {
        return Err(AppError::message(format!(
            "Boot-loader source does not exist or is not a file: {}",
            source_image.display()
        )));
    }
    if expected_serial_text.is_empty() {
        return Err(AppError::message(
            "Boot-loader expected serial text must not be empty.",
        ));
    }
    if expected_serial_text.len() > 1024 {
        return Err(AppError::message(
            "Boot-loader expected serial text must be 1024 bytes or less for the current WHP serial spike.",
        ));
    }

    let source_len = fs::metadata(source_image)?.len();
    if source_len > crate::native::SERIAL_BOOT_TEST_IMAGE_SIZE as u64 {
        return Err(AppError::message(format!(
            "Boot-loader source is {source_len} bytes; the current WHP boot-to-serial spike supports at most {} bytes.",
            crate::native::SERIAL_BOOT_TEST_IMAGE_SIZE
        )));
    }

    let expected_sha256 = expected_sha256.map(normalize_sha256_hex).transpose()?;
    let actual_sha256 = sha256_file(source_image)?;
    let verified = expected_sha256
        .as_deref()
        .map(|expected| expected == actual_sha256)
        .unwrap_or(false);

    if let Some(expected) = expected_sha256.as_deref() {
        if expected != actual_sha256 {
            return Err(AppError::message(format!(
                "Boot-loader SHA-256 mismatch. expected {expected}, got {actual_sha256}."
            )));
        }
    }

    if let Some(parent) = paths.boot_loader_image.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.boot_loader_metadata.parent() {
        fs::create_dir_all(parent)?;
    }

    let same_target = paths.boot_loader_image.exists()
        && source_image.canonicalize().ok() == paths.boot_loader_image.canonicalize().ok();
    if paths.boot_loader_image.exists() && !force && !same_target {
        return Err(AppError::message(format!(
            "A registered boot-loader image already exists at {}. Pass --force to replace it.",
            paths.boot_loader_image.display()
        )));
    }

    if !same_target {
        let temp_image = paths.boot_loader_image.with_extension("paneimg.tmp");
        if temp_image.exists() {
            fs::remove_file(&temp_image)?;
        }
        fs::copy(source_image, &temp_image)?;
        if paths.boot_loader_image.exists() {
            fs::remove_file(&paths.boot_loader_image)?;
        }
        fs::rename(&temp_image, &paths.boot_loader_image)?;
    }

    let bytes = fs::metadata(&paths.boot_loader_image)?.len();
    let metadata = BootLoaderImageMetadata {
        schema_version: 1,
        image_kind: "pane-boot-to-serial-loader-image".to_string(),
        source_path: source_image
            .canonicalize()
            .unwrap_or_else(|_| source_image.to_path_buf())
            .display()
            .to_string(),
        stored_path: paths.boot_loader_image.display().to_string(),
        bytes,
        sha256: actual_sha256,
        expected_sha256,
        verified,
        expected_serial_text: expected_serial_text.to_string(),
        guest_entry_gpa: "0x1000".to_string(),
        registered_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This is a controlled boot-to-serial candidate for the Pane WHP runner, not a complete Arch boot claim."
                .to_string(),
            "Pane requires SHA-256 verification and an explicit serial-output contract before executing this artifact."
                .to_string(),
        ],
    };
    write_json_file(&paths.boot_loader_metadata, &metadata)
}

fn load_boot_loader_image_artifact(
    paths: &RuntimePaths,
) -> AppResult<crate::native::NativeSerialBootImage> {
    let metadata = read_json_file::<BootLoaderImageMetadata>(&paths.boot_loader_metadata)?;
    let bytes = fs::read(&paths.boot_loader_image)?;
    let actual_sha256 = sha256_bytes(&bytes);
    if metadata.schema_version != 1
        || metadata.image_kind != "pane-boot-to-serial-loader-image"
        || metadata.bytes != bytes.len() as u64
        || metadata.sha256 != actual_sha256
        || !metadata.verified
        || metadata.expected_serial_text.is_empty()
        || metadata.guest_entry_gpa != "0x1000"
    {
        return Err(AppError::message(format!(
            "Pane boot-loader metadata does not match the artifact at {}. Re-register it with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256> --force`.",
            paths.boot_loader_image.display()
        )));
    }

    Ok(crate::native::NativeSerialBootImage {
        source_label: "pane-runtime-boot-to-serial-loader".to_string(),
        path: Some(paths.boot_loader_image.display().to_string()),
        bytes,
        expected_serial_text: metadata.expected_serial_text,
        guest_entry_gpa: 0x1000,
        entry_mode: crate::native::NativeGuestEntryMode::RealModeSerial,
        boot_params_gpa: None,
        extra_regions: Vec::new(),
    })
}

fn load_kernel_layout_boot_image_artifact(
    paths: &RuntimePaths,
) -> AppResult<crate::native::NativeSerialBootImage> {
    let artifacts = build_runtime_artifact_report(paths);
    if !artifacts.kernel_boot_layout_ready {
        return Err(AppError::message(
            "Pane kernel boot layout is missing or stale. Run `pane native-kernel-plan --materialize` after registering a verified kernel plan.",
        ));
    }
    let layout = read_json_file::<KernelBootLayout>(&paths.kernel_boot_layout)?;
    if layout.kernel_path != paths.kernel_image.display().to_string() {
        return Err(AppError::message(format!(
            "Kernel layout points at {}, but this runtime expects {}.",
            layout.kernel_path,
            paths.kernel_image.display()
        )));
    }

    let bytes = fs::read(&paths.kernel_image)?;
    let actual_sha256 = sha256_bytes(&bytes);
    if layout.kernel_bytes != bytes.len() as u64 || layout.kernel_sha256 != actual_sha256 {
        return Err(AppError::message(
            "Kernel layout no longer matches the registered kernel artifact. Re-run `pane native-kernel-plan --materialize`.",
        ));
    }

    let (kernel_execution_bytes, kernel_entry_gpa, mut extra_regions) =
        kernel_layout_execution_image(&layout, &bytes)?;
    extra_regions.extend(linux_guest_mapped_regions(&layout)?);
    if layout.kernel_format == "linux-bzimage" {
        extra_regions.push(crate::native::NativeGuestMemoryRegion {
            label: "linux-boot-gdt".to_string(),
            guest_gpa: crate::native::LINUX_BOOT_GDT_GPA,
            bytes: crate::native::linux_boot_gdt_page_bytes(),
            writable: false,
            executable: false,
        });
    }
    let boot_params = build_linux_boot_params_page(&layout, Some(&bytes))?;
    extra_regions.push(crate::native::NativeGuestMemoryRegion {
        label: "linux-boot-params".to_string(),
        guest_gpa: parse_guest_physical_address(&layout.boot_params_gpa)?,
        bytes: boot_params,
        writable: true,
        executable: false,
    });

    let mut cmdline_bytes = layout.cmdline.as_bytes().to_vec();
    cmdline_bytes.push(0);
    extra_regions.push(crate::native::NativeGuestMemoryRegion {
        label: "linux-kernel-cmdline".to_string(),
        guest_gpa: parse_guest_physical_address(&layout.cmdline_gpa)?,
        bytes: cmdline_bytes,
        writable: false,
        executable: false,
    });

    if let Some(initramfs_path) = layout.initramfs_path.as_deref() {
        let Some(initramfs_load_gpa) = layout.initramfs_load_gpa.as_deref() else {
            return Err(AppError::message(
                "Kernel layout has an initramfs path but no initramfs load GPA.",
            ));
        };
        extra_regions.push(crate::native::NativeGuestMemoryRegion {
            label: "linux-initramfs".to_string(),
            guest_gpa: parse_guest_physical_address(initramfs_load_gpa)?,
            bytes: fs::read(initramfs_path)?,
            writable: false,
            executable: false,
        });
    }

    Ok(crate::native::NativeSerialBootImage {
        source_label: if layout.kernel_format == "linux-bzimage" {
            "pane-runtime-linux-bzimage-protected-mode".to_string()
        } else {
            "pane-runtime-kernel-layout".to_string()
        },
        path: Some(layout.kernel_path),
        bytes: kernel_execution_bytes,
        expected_serial_text: if layout.kernel_format == "linux-bzimage" {
            String::new()
        } else {
            crate::native::SERIAL_BOOT_BANNER_TEXT.to_string()
        },
        guest_entry_gpa: kernel_entry_gpa,
        entry_mode: if layout.kernel_format == "linux-bzimage" {
            crate::native::NativeGuestEntryMode::LinuxProtectedMode32
        } else {
            crate::native::NativeGuestEntryMode::RealModeSerial
        },
        boot_params_gpa: (layout.kernel_format == "linux-bzimage")
            .then(|| parse_guest_physical_address(&layout.boot_params_gpa))
            .transpose()?,
        extra_regions,
    })
}

fn kernel_layout_execution_image(
    layout: &KernelBootLayout,
    kernel_bytes: &[u8],
) -> AppResult<(Vec<u8>, u64, Vec<crate::native::NativeGuestMemoryRegion>)> {
    let kernel_load_gpa = parse_guest_physical_address(&layout.kernel_load_gpa)?;
    if layout.kernel_format == "controlled-serial-candidate" {
        return Ok((kernel_bytes.to_vec(), kernel_load_gpa, Vec::new()));
    }
    if layout.kernel_format != "linux-bzimage" {
        return Err(AppError::message(format!(
            "Unsupported kernel layout format `{}`.",
            layout.kernel_format
        )));
    }

    let setup_bytes = layout
        .linux_setup_bytes
        .ok_or_else(|| AppError::message("Linux bzImage layout is missing setup byte metadata."))?
        as usize;
    let protected_mode_offset = layout.linux_protected_mode_offset.ok_or_else(|| {
        AppError::message("Linux bzImage layout is missing protected-mode payload offset.")
    })? as usize;
    let protected_mode_bytes = layout.linux_protected_mode_bytes.ok_or_else(|| {
        AppError::message("Linux bzImage layout is missing protected-mode payload length.")
    })? as usize;

    if setup_bytes > kernel_bytes.len()
        || protected_mode_offset > kernel_bytes.len()
        || protected_mode_offset.saturating_add(protected_mode_bytes) > kernel_bytes.len()
    {
        return Err(AppError::message(
            "Linux bzImage layout offsets no longer match the registered kernel artifact.",
        ));
    }

    let setup_region = crate::native::NativeGuestMemoryRegion {
        label: "linux-bzimage-setup".to_string(),
        guest_gpa: 0x0009_0000,
        bytes: kernel_bytes[..setup_bytes].to_vec(),
        writable: true,
        executable: false,
    };
    let protected_mode_payload =
        kernel_bytes[protected_mode_offset..protected_mode_offset + protected_mode_bytes].to_vec();

    Ok((protected_mode_payload, kernel_load_gpa, vec![setup_region]))
}

fn linux_guest_mapped_regions(
    layout: &KernelBootLayout,
) -> AppResult<Vec<crate::native::NativeGuestMemoryRegion>> {
    if layout.kernel_format != "linux-bzimage" {
        return Ok(Vec::new());
    }

    layout
        .guest_memory_map
        .iter()
        .filter(|range| {
            matches!(
                range.region_type.as_str(),
                "usable" | "mmio-stub" | "framebuffer" | "input-queue"
            )
        })
        .map(|range| {
            let size: usize = range.size_bytes.try_into().map_err(|_| {
                AppError::message(format!(
                    "Guest memory range `{}` is too large to map on this host.",
                    range.label
                ))
            })?;
            let label = match range.region_type.as_str() {
                "usable" => format!("linux-ram-{}", range.label),
                "framebuffer" | "input-queue" => range.label.clone(),
                _ => format!("linux-{}", range.label),
            };
            Ok(crate::native::NativeGuestMemoryRegion {
                label,
                guest_gpa: parse_guest_physical_address(&range.start_gpa)?,
                bytes: vec![0_u8; size],
                writable: true,
                executable: range.region_type == "usable",
            })
        })
        .collect()
}

fn build_linux_boot_params_page(
    layout: &KernelBootLayout,
    kernel_bytes: Option<&[u8]>,
) -> AppResult<Vec<u8>> {
    let mut boot_params = vec![0_u8; 4096];
    if layout.kernel_format == "linux-bzimage" {
        copy_linux_setup_header_to_boot_params(
            &mut boot_params,
            kernel_bytes.ok_or_else(|| {
                AppError::message(
                    "Linux boot params require the original bzImage setup header bytes.",
                )
            })?,
        )?;
    }
    let cmdline_gpa = checked_u32_gpa(&layout.cmdline_gpa, "kernel cmdline")?;
    write_u16_le(&mut boot_params, 0x1fe, 0xaa55);
    boot_params[0x202..0x206].copy_from_slice(b"HdrS");
    let protocol = layout
        .linux_boot_protocol
        .as_deref()
        .and_then(parse_hex_u16)
        .unwrap_or(0x020f);
    write_u16_le(&mut boot_params, 0x206, protocol);
    boot_params[0x210] = 0xff;
    boot_params[0x211] |= 0x80;
    write_u32_le(
        &mut boot_params,
        0x214,
        checked_u32_gpa(&layout.kernel_load_gpa, "kernel entry")?,
    );
    write_u32_le(&mut boot_params, 0x228, cmdline_gpa);
    write_u32_le(&mut boot_params, 0x22c, 0x7fff_ffff);
    write_u32_le(
        &mut boot_params,
        0x238,
        layout.cmdline.len().saturating_add(1) as u32,
    );

    if layout.initramfs_path.is_some() {
        let initramfs_gpa = layout
            .initramfs_load_gpa
            .as_deref()
            .ok_or_else(|| AppError::message("Initramfs layout is missing a load GPA."))?;
        write_u32_le(
            &mut boot_params,
            0x218,
            checked_u32_gpa(initramfs_gpa, "initramfs")?,
        );
        write_u32_le(
            &mut boot_params,
            0x21c,
            layout
                .initramfs_bytes
                .ok_or_else(|| AppError::message("Initramfs layout is missing its byte length."))?
                .try_into()
                .map_err(|_| {
                    AppError::message("Initramfs is too large for the 32-bit boot protocol field.")
                })?,
        );
    }

    write_linux_e820_table(&mut boot_params, &layout.guest_memory_map)?;

    Ok(boot_params)
}

fn copy_linux_setup_header_to_boot_params(
    boot_params: &mut [u8],
    kernel_bytes: &[u8],
) -> AppResult<()> {
    if kernel_bytes.len() < 0x264 {
        return Err(AppError::message(
            "Linux bzImage is too small to copy its setup header into boot params.",
        ));
    }
    if read_u16_le_at(kernel_bytes, 0x1fe) != Some(0xaa55)
        || kernel_bytes.get(0x202..0x206) != Some(b"HdrS")
    {
        return Err(AppError::message(
            "Linux bzImage setup header cannot be copied because boot flag or HdrS magic is missing.",
        ));
    }

    let header_end = linux_setup_header_end(kernel_bytes)?;
    boot_params[0x1f1..header_end].copy_from_slice(&kernel_bytes[0x1f1..header_end]);
    Ok(())
}

fn linux_setup_header_end(kernel_bytes: &[u8]) -> AppResult<usize> {
    let advertised_end = kernel_bytes
        .get(0x201)
        .map(|header_len| 0x202 + usize::from(*header_len))
        .ok_or_else(|| AppError::message("Linux bzImage is missing setup header length."))?;
    let header_end = advertised_end.max(0x264);
    if header_end > kernel_bytes.len() || header_end > 4096 {
        return Err(AppError::message(format!(
            "Linux bzImage setup header end offset 0x{header_end:x} is outside the boot params scaffold."
        )));
    }
    Ok(header_end)
}

fn write_linux_e820_table(
    boot_params: &mut [u8],
    ranges: &[KernelGuestMemoryRange],
) -> AppResult<()> {
    if ranges.len() > u8::MAX as usize {
        return Err(AppError::message(
            "Linux E820 table cannot contain more than 255 entries.",
        ));
    }
    if ranges.len() > 128 {
        return Err(AppError::message(
            "Linux E820 table exceeds the boot params table capacity.",
        ));
    }

    boot_params[0x1e8] = ranges.len() as u8;
    for (index, range) in ranges.iter().enumerate() {
        let offset = 0x2d0 + index * 20;
        let start = parse_guest_physical_address(&range.start_gpa)?;
        let region_type = match range.region_type.as_str() {
            "usable" => 1,
            "reserved" | "mmio-stub" | "framebuffer" | "input-queue" => 2,
            other => {
                return Err(AppError::message(format!(
                    "Unsupported Linux E820 range type `{other}` for `{}`.",
                    range.label
                )))
            }
        };
        write_u64_le(boot_params, offset, start);
        write_u64_le(boot_params, offset + 8, range.size_bytes);
        write_u32_le(boot_params, offset + 16, region_type);
    }

    Ok(())
}

fn checked_u32_gpa(value: &str, label: &str) -> AppResult<u32> {
    parse_guest_physical_address(value)?
        .try_into()
        .map_err(|_| {
            AppError::message(format!(
                "{label} GPA must fit in 32 bits for this boot-params scaffold."
            ))
        })
}

fn write_u16_le(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_le(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u16_le_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u64_le_at(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn parse_hex_u16(value: &str) -> Option<u16> {
    let trimmed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    u16::from_str_radix(trimmed, 16).ok()
}

fn parse_guest_physical_address(value: &str) -> AppResult<u64> {
    let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    else {
        return Err(AppError::message(format!(
            "Guest physical address `{value}` must use 0x-prefixed hexadecimal notation."
        )));
    };
    u64::from_str_radix(hex, 16).map_err(|_| {
        AppError::message(format!(
            "Guest physical address `{value}` is not valid hexadecimal."
        ))
    })
}

fn format_guest_physical_address(value: u64) -> String {
    format!("0x{value:08x}")
}

fn register_kernel_boot_plan(
    paths: &RuntimePaths,
    kernel_source: Option<&Path>,
    kernel_expected_sha256: Option<&str>,
    initramfs_source: Option<&Path>,
    initramfs_expected_sha256: Option<&str>,
    cmdline: Option<&str>,
    force: bool,
) -> AppResult<()> {
    if kernel_expected_sha256.is_some() && kernel_source.is_none() {
        return Err(AppError::message(
            "--kernel-expected-sha256 requires --register-kernel.",
        ));
    }
    if initramfs_expected_sha256.is_some() && initramfs_source.is_none() {
        return Err(AppError::message(
            "--initramfs-expected-sha256 requires --register-initramfs.",
        ));
    }

    let previous = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).ok();
    if kernel_source.is_none() && previous.is_none() {
        return Err(AppError::message(
            "--kernel-cmdline or --register-initramfs requires an existing kernel boot plan or --register-kernel.",
        ));
    }

    let kernel_record = if let Some(source) = kernel_source {
        Some(copy_verified_runtime_artifact(
            source,
            &paths.kernel_image,
            kernel_expected_sha256,
            "Kernel",
            force,
        )?)
    } else {
        previous.as_ref().map(|metadata| ArtifactRegistration {
            source_path: metadata.kernel_source_path.clone(),
            stored_path: metadata.kernel_stored_path.clone(),
            bytes: metadata.kernel_bytes,
            sha256: metadata.kernel_sha256.clone(),
            expected_sha256: metadata.kernel_expected_sha256.clone(),
            verified: metadata.kernel_verified,
        })
    }
    .ok_or_else(|| AppError::message("Kernel boot plan is missing a kernel artifact."))?;

    let initramfs_record = if let Some(source) = initramfs_source {
        Some(copy_verified_runtime_artifact(
            source,
            &paths.initramfs_image,
            initramfs_expected_sha256,
            "Initramfs",
            force,
        )?)
    } else {
        previous.as_ref().and_then(|metadata| {
            metadata
                .initramfs_stored_path
                .as_ref()
                .map(|stored_path| ArtifactRegistration {
                    source_path: metadata.initramfs_source_path.clone().unwrap_or_default(),
                    stored_path: stored_path.clone(),
                    bytes: metadata.initramfs_bytes.unwrap_or_default(),
                    sha256: metadata.initramfs_sha256.clone().unwrap_or_default(),
                    expected_sha256: metadata.initramfs_expected_sha256.clone(),
                    verified: metadata.initramfs_verified,
                })
        })
    };

    let cmdline = cmdline
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| previous.as_ref().map(|metadata| metadata.cmdline.clone()))
        .unwrap_or_else(|| "console=ttyS0 earlyprintk=serial panic=-1".to_string());
    validate_kernel_cmdline(&cmdline)?;
    let kernel_inspection = inspect_kernel_image_artifact(&paths.kernel_image)?;

    let metadata = KernelBootMetadata {
        schema_version: 1,
        image_kind: "pane-linux-kernel-boot-plan-v1".to_string(),
        kernel_source_path: kernel_record.source_path,
        kernel_stored_path: kernel_record.stored_path,
        kernel_bytes: kernel_record.bytes,
        kernel_sha256: kernel_record.sha256,
        kernel_expected_sha256: kernel_record.expected_sha256,
        kernel_verified: kernel_record.verified,
        kernel_format: kernel_inspection.format,
        linux_boot_protocol: kernel_inspection.linux_boot_protocol,
        linux_setup_sectors: kernel_inspection.linux_setup_sectors,
        linux_setup_bytes: kernel_inspection.linux_setup_bytes,
        linux_protected_mode_offset: kernel_inspection.linux_protected_mode_offset,
        linux_protected_mode_bytes: kernel_inspection.linux_protected_mode_bytes,
        linux_loadflags: kernel_inspection.linux_loadflags,
        linux_preferred_load_address: kernel_inspection.linux_preferred_load_address,
        initramfs_source_path: initramfs_record
            .as_ref()
            .map(|record| record.source_path.clone()),
        initramfs_stored_path: initramfs_record
            .as_ref()
            .map(|record| record.stored_path.clone()),
        initramfs_bytes: initramfs_record.as_ref().map(|record| record.bytes),
        initramfs_sha256: initramfs_record
            .as_ref()
            .map(|record| record.sha256.clone()),
        initramfs_expected_sha256: initramfs_record
            .as_ref()
            .and_then(|record| record.expected_sha256.clone()),
        initramfs_verified: initramfs_record
            .as_ref()
            .map(|record| record.verified)
            .unwrap_or(false),
        cmdline,
        expected_serial_device: "ttyS0".to_string(),
        kernel_load_gpa: "0x00100000".to_string(),
        initramfs_load_gpa: initramfs_record.as_ref().map(|_| "0x04000000".to_string()),
        registered_at_epoch_seconds: current_epoch_seconds(),
        notes: {
            let mut notes = vec![
            "This is Pane's first native kernel/initramfs boot-plan contract; it is not yet executed by WHP."
                .to_string(),
            "The command line must keep serial console output enabled so the next WHP milestone can prove boot progress without a GUI."
                .to_string(),
            ];
            notes.extend(kernel_inspection.notes);
            notes
        },
    };

    write_json_file(&paths.kernel_boot_metadata, &metadata)
}

fn build_kernel_boot_layout(
    paths: &RuntimePaths,
    session_name: &str,
    materialize: bool,
) -> AppResult<KernelBootLayout> {
    let metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata)?;
    if metadata.schema_version != 1 || metadata.image_kind != "pane-linux-kernel-boot-plan-v1" {
        return Err(AppError::message(
            "Kernel boot metadata is not a Pane kernel boot-plan v1 document.",
        ));
    }
    if !metadata.kernel_verified {
        return Err(AppError::message(
            "Kernel artifact is not hash-verified. Re-register it with --kernel-expected-sha256.",
        ));
    }
    validate_kernel_cmdline(&metadata.cmdline)?;
    if metadata.expected_serial_device != "ttyS0" {
        return Err(AppError::message(
            "Kernel boot plan must target ttyS0 as Pane's observable serial console.",
        ));
    }
    if metadata.kernel_stored_path != paths.kernel_image.display().to_string() {
        return Err(AppError::message(format!(
            "Kernel boot metadata points at {}, but this runtime expects {}.",
            metadata.kernel_stored_path,
            paths.kernel_image.display()
        )));
    }
    if metadata.kernel_bytes != fs::metadata(&paths.kernel_image)?.len()
        || metadata.kernel_sha256 != sha256_file(&paths.kernel_image)?
    {
        return Err(AppError::message(
            "Kernel artifact no longer matches its boot metadata. Re-register the kernel.",
        ));
    }

    if let Some(initramfs_path) = metadata.initramfs_stored_path.as_deref() {
        if initramfs_path != paths.initramfs_image.display().to_string() {
            return Err(AppError::message(format!(
                "Initramfs boot metadata points at {initramfs_path}, but this runtime expects {}.",
                paths.initramfs_image.display()
            )));
        }
        if !metadata.initramfs_verified {
            return Err(AppError::message(
                "Initramfs artifact is not hash-verified. Re-register it with --initramfs-expected-sha256.",
            ));
        }
        if metadata.initramfs_bytes != fs::metadata(&paths.initramfs_image).ok().map(|m| m.len())
            || metadata.initramfs_sha256.as_deref()
                != Some(sha256_file(&paths.initramfs_image)?.as_str())
        {
            return Err(AppError::message(
                "Initramfs artifact no longer matches its boot metadata. Re-register the initramfs.",
            ));
        }
    }

    let is_linux_bzimage = metadata.kernel_format == "linux-bzimage";
    let linux_entry_point_gpa = is_linux_bzimage.then(|| metadata.kernel_load_gpa.clone());
    let linux_boot_params_register = is_linux_bzimage.then(|| "rsi".to_string());
    let linux_expected_entry_mode = is_linux_bzimage.then(|| "x86-protected-mode-32".to_string());
    let mut guest_memory_map = if is_linux_bzimage {
        default_linux_guest_memory_map(metadata.initramfs_bytes.unwrap_or(0))
    } else {
        Vec::new()
    };
    let artifact_report = build_runtime_artifact_report(paths);
    let storage = build_kernel_storage_attachment(paths, &artifact_report)?;
    let framebuffer = read_json_file::<FramebufferContract>(&paths.framebuffer_contract)
        .unwrap_or_else(|_| default_framebuffer_contract());
    let input = read_json_file::<InputContract>(&paths.input_contract)
        .unwrap_or_else(|_| default_input_contract());
    if is_linux_bzimage {
        guest_memory_map.extend(runtime_contract_guest_memory_ranges(&framebuffer, &input)?);
        validate_guest_memory_ranges_do_not_overlap(&guest_memory_map)?;
    }

    let layout = KernelBootLayout {
        schema_version: 1,
        layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
        session_name: session_name.to_string(),
        boot_params_gpa: "0x00007000".to_string(),
        cmdline_gpa: "0x00020000".to_string(),
        kernel_load_gpa: metadata.kernel_load_gpa,
        initramfs_load_gpa: metadata.initramfs_load_gpa,
        kernel_path: metadata.kernel_stored_path,
        kernel_bytes: metadata.kernel_bytes,
        kernel_sha256: metadata.kernel_sha256,
        kernel_format: metadata.kernel_format,
        linux_boot_protocol: metadata.linux_boot_protocol,
        linux_setup_sectors: metadata.linux_setup_sectors,
        linux_setup_bytes: metadata.linux_setup_bytes,
        linux_protected_mode_offset: metadata.linux_protected_mode_offset,
        linux_protected_mode_bytes: metadata.linux_protected_mode_bytes,
        linux_loadflags: metadata.linux_loadflags,
        linux_preferred_load_address: metadata.linux_preferred_load_address,
        linux_entry_point_gpa,
        linux_boot_params_register,
        linux_expected_entry_mode,
        guest_memory_map,
        initramfs_path: metadata.initramfs_stored_path,
        initramfs_bytes: metadata.initramfs_bytes,
        initramfs_sha256: metadata.initramfs_sha256,
        cmdline: metadata.cmdline,
        expected_serial_device: metadata.expected_serial_device,
        storage,
        framebuffer: Some(framebuffer),
        input: Some(input),
        materialized_at_epoch_seconds: materialize.then(current_epoch_seconds),
        notes: vec![
            "This layout is Pane's deterministic handoff from artifact registration to the WHP Linux boot-protocol runner."
                .to_string(),
            "When base OS and user disk artifacts are verified, this layout carries their root/user storage attachment into the boot contract."
                .to_string(),
            "The framebuffer and input contracts are mapped into guest memory by the WHP kernel-layout runner; they are not a full desktop device model yet."
                .to_string(),
            "The next native milestone must map these guest physical addresses and prove serial boot output before GUI/display work."
                .to_string(),
        ],
    };

    if materialize {
        write_json_file(&paths.kernel_boot_layout, &layout)?;
    }

    Ok(layout)
}

fn build_kernel_storage_attachment(
    paths: &RuntimePaths,
    artifacts: &RuntimeArtifactReport,
) -> AppResult<Option<KernelStorageAttachment>> {
    if !artifacts.base_os_image_verified || !artifacts.user_disk_ready {
        return Ok(None);
    }

    let user_disk_metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
    let base_sha256 = artifacts.base_os_image_sha256.clone().ok_or_else(|| {
        AppError::message("Verified base OS image is missing its recorded SHA-256 digest.")
    })?;
    let base_bytes = artifacts.base_os_image_bytes.ok_or_else(|| {
        AppError::message("Verified base OS image is missing its recorded byte length.")
    })?;

    Ok(Some(KernelStorageAttachment {
        schema_version: 1,
        base_os_path: paths.base_os_image.display().to_string(),
        base_os_sha256: base_sha256,
        base_os_bytes: base_bytes,
        user_disk_path: user_disk_metadata.disk_path,
        user_disk_capacity_gib: user_disk_metadata.capacity_gib,
        user_disk_format: user_disk_metadata.format,
        root_device: "/dev/pane0".to_string(),
        user_device: "/dev/pane1".to_string(),
        readonly_base: true,
        writable_user_disk: true,
    }))
}

fn runtime_contract_guest_memory_ranges(
    framebuffer: &FramebufferContract,
    input: &InputContract,
) -> AppResult<Vec<KernelGuestMemoryRange>> {
    let framebuffer_gpa = parse_guest_physical_address(&framebuffer.guest_gpa)?;
    let input_gpa = parse_guest_physical_address(&input.guest_queue_gpa)?;

    Ok(vec![
        KernelGuestMemoryRange {
            label: "pane-framebuffer".to_string(),
            start_gpa: format_guest_physical_address(framebuffer_gpa),
            size_bytes: page_align_guest_range(framebuffer.size_bytes),
            region_type: "framebuffer".to_string(),
        },
        KernelGuestMemoryRange {
            label: "pane-input-queue".to_string(),
            start_gpa: format_guest_physical_address(input_gpa),
            size_bytes: page_align_guest_range(input.queue_size_bytes),
            region_type: "input-queue".to_string(),
        },
    ])
}

fn validate_guest_memory_ranges_do_not_overlap(ranges: &[KernelGuestMemoryRange]) -> AppResult<()> {
    let mut parsed = Vec::with_capacity(ranges.len());
    for range in ranges {
        let start = parse_guest_physical_address(&range.start_gpa)?;
        let end = start.checked_add(range.size_bytes).ok_or_else(|| {
            AppError::message(format!("Guest range `{}` overflows.", range.label))
        })?;
        parsed.push((start, end, range.label.as_str()));
    }

    parsed.sort_by_key(|(start, _, _)| *start);
    for window in parsed.windows(2) {
        let [left, right] = window else {
            continue;
        };
        if left.1 > right.0 {
            return Err(AppError::message(format!(
                "Guest memory ranges `{}` and `{}` overlap.",
                left.2, right.2
            )));
        }
    }

    Ok(())
}

fn default_linux_guest_memory_map(initramfs_bytes: u64) -> Vec<KernelGuestMemoryRange> {
    let mut ranges = vec![
        KernelGuestMemoryRange {
            label: "boot-params".to_string(),
            start_gpa: "0x00007000".to_string(),
            size_bytes: 0x00001000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "boot-gdt".to_string(),
            start_gpa: "0x00008000".to_string(),
            size_bytes: 0x00001000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "cmdline".to_string(),
            start_gpa: "0x00020000".to_string(),
            size_bytes: 0x00001000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "low-ram".to_string(),
            start_gpa: "0x00030000".to_string(),
            size_bytes: 0x00060000,
            region_type: "usable".to_string(),
        },
        KernelGuestMemoryRange {
            label: "bzimage-setup".to_string(),
            start_gpa: "0x00090000".to_string(),
            size_bytes: 0x00010000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "bios-reserved".to_string(),
            start_gpa: "0x000f0000".to_string(),
            size_bytes: 0x00010000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "kernel-payload".to_string(),
            start_gpa: "0x00100000".to_string(),
            size_bytes: 0x02000000,
            region_type: "reserved".to_string(),
        },
    ];

    if initramfs_bytes > 0 {
        ranges.push(KernelGuestMemoryRange {
            label: "initramfs".to_string(),
            start_gpa: "0x04000000".to_string(),
            size_bytes: page_align_guest_range(initramfs_bytes),
            region_type: "reserved".to_string(),
        });
    }

    ranges.extend([
        KernelGuestMemoryRange {
            label: "high-ram".to_string(),
            start_gpa: "0x08000000".to_string(),
            size_bytes: 0x04000000,
            region_type: "usable".to_string(),
        },
        KernelGuestMemoryRange {
            label: "io-apic-mmio".to_string(),
            start_gpa: "0xfec00000".to_string(),
            size_bytes: 0x00001000,
            region_type: "mmio-stub".to_string(),
        },
        KernelGuestMemoryRange {
            label: "local-apic-mmio".to_string(),
            start_gpa: "0xfee00000".to_string(),
            size_bytes: 0x00001000,
            region_type: "mmio-stub".to_string(),
        },
    ]);

    ranges
}

fn page_align_guest_range(bytes: u64) -> u64 {
    const PAGE_SIZE: u64 = 0x1000;
    bytes.saturating_add(PAGE_SIZE - 1) / PAGE_SIZE * PAGE_SIZE
}

fn inspect_kernel_image_artifact(path: &Path) -> AppResult<KernelImageInspection> {
    let bytes = fs::read(path)?;
    if bytes == crate::native::serial_boot_test_image_bytes() {
        return Ok(KernelImageInspection {
            format: "controlled-serial-candidate".to_string(),
            linux_boot_protocol: None,
            linux_setup_sectors: None,
            linux_setup_bytes: None,
            linux_protected_mode_offset: None,
            linux_protected_mode_bytes: None,
            linux_loadflags: None,
            linux_preferred_load_address: None,
            notes: vec![
                "This artifact is Pane's controlled serial/HALT candidate, not a Linux bzImage. It is allowed only for deterministic WHP runner certification."
                    .to_string(),
            ],
        });
    }

    if bytes.len() < 0x264 {
        return Err(AppError::message(format!(
            "Kernel artifact at {} is too small to be a Linux bzImage and is not Pane's controlled serial candidate.",
            path.display()
        )));
    }
    if read_u16_le_at(&bytes, 0x1fe) != Some(0xaa55) || bytes.get(0x202..0x206) != Some(b"HdrS") {
        return Err(AppError::message(format!(
            "Kernel artifact at {} is not a supported Linux bzImage. Expected boot flag 0xaa55 and setup header magic HdrS.",
            path.display()
        )));
    }

    let protocol = read_u16_le_at(&bytes, 0x206).ok_or_else(|| {
        AppError::message("Linux kernel setup header is missing protocol version.")
    })?;
    if protocol < 0x0200 {
        return Err(AppError::message(format!(
            "Linux boot protocol {:#06x} is too old for Pane's native runner scaffold.",
            protocol
        )));
    }

    let setup_sectors = bytes[0x1f1];
    let normalized_setup_sectors = if setup_sectors == 0 { 4 } else { setup_sectors };
    let setup_bytes = (normalized_setup_sectors as usize + 1) * 512;
    if setup_bytes >= bytes.len() {
        return Err(AppError::message(format!(
            "Linux bzImage setup area is {setup_bytes} bytes, but the artifact is only {} bytes.",
            bytes.len()
        )));
    }
    let protected_mode_bytes = bytes.len() - setup_bytes;
    let preferred_load = (protocol >= 0x020a)
        .then(|| read_u64_le_at(&bytes, 0x258))
        .flatten()
        .map(|value| format!("{value:#018x}"));

    Ok(KernelImageInspection {
        format: "linux-bzimage".to_string(),
        linux_boot_protocol: Some(format!("{:#06x}", protocol)),
        linux_setup_sectors: Some(normalized_setup_sectors),
        linux_setup_bytes: Some(setup_bytes as u64),
        linux_protected_mode_offset: Some(setup_bytes as u64),
        linux_protected_mode_bytes: Some(protected_mode_bytes as u64),
        linux_loadflags: bytes.get(0x211).copied(),
        linux_preferred_load_address: preferred_load,
        notes: vec![
            "Linux bzImage header validated; Pane has not yet entered the real Linux boot protocol."
                .to_string(),
        ],
    })
}

struct ArtifactRegistration {
    source_path: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    expected_sha256: Option<String>,
    verified: bool,
}

#[derive(Clone, Debug)]
struct KernelImageInspection {
    format: String,
    linux_boot_protocol: Option<String>,
    linux_setup_sectors: Option<u8>,
    linux_setup_bytes: Option<u64>,
    linux_protected_mode_offset: Option<u64>,
    linux_protected_mode_bytes: Option<u64>,
    linux_loadflags: Option<u8>,
    linux_preferred_load_address: Option<String>,
    notes: Vec<String>,
}

fn copy_verified_runtime_artifact(
    source: &Path,
    destination: &Path,
    expected_sha256: Option<&str>,
    label: &str,
    force: bool,
) -> AppResult<ArtifactRegistration> {
    if !source.is_file() {
        return Err(AppError::message(format!(
            "{label} source does not exist or is not a file: {}",
            source.display()
        )));
    }

    let expected_sha256 = expected_sha256.map(normalize_sha256_hex).transpose()?;
    let actual_sha256 = sha256_file(source)?;
    let verified = expected_sha256
        .as_deref()
        .map(|expected| expected == actual_sha256)
        .unwrap_or(false);
    if let Some(expected) = expected_sha256.as_deref() {
        if expected != actual_sha256 {
            return Err(AppError::message(format!(
                "{label} SHA-256 mismatch. expected {expected}, got {actual_sha256}."
            )));
        }
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let same_target =
        destination.exists() && source.canonicalize().ok() == destination.canonicalize().ok();
    if destination.exists() && !force && !same_target {
        return Err(AppError::message(format!(
            "A registered {label} artifact already exists at {}. Pass --force to replace it.",
            destination.display()
        )));
    }

    if !same_target {
        let temp_image = destination.with_extension("tmp");
        if temp_image.exists() {
            fs::remove_file(&temp_image)?;
        }
        fs::copy(source, &temp_image)?;
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(&temp_image, destination)?;
    }

    Ok(ArtifactRegistration {
        source_path: source
            .canonicalize()
            .unwrap_or_else(|_| source.to_path_buf())
            .display()
            .to_string(),
        stored_path: destination.display().to_string(),
        bytes: fs::metadata(destination)?.len(),
        sha256: actual_sha256,
        expected_sha256,
        verified,
    })
}

fn validate_kernel_cmdline(value: &str) -> AppResult<()> {
    if value.len() > 2048 {
        return Err(AppError::message(
            "Kernel command line must be 2048 bytes or less.",
        ));
    }
    if value
        .chars()
        .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(AppError::message(
            "Kernel command line must be a single line with no NUL, CR, or LF characters.",
        ));
    }
    if !value.contains("console=ttyS0") {
        return Err(AppError::message(
            "Kernel command line must include `console=ttyS0` so Pane can observe serial boot progress.",
        ));
    }
    Ok(())
}

fn decode_serial_text(value: &str) -> AppResult<String> {
    let mut decoded = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        let Some(escaped) = chars.next() else {
            return Err(AppError::message(
                "Serial text escape sequence cannot end with a trailing backslash.",
            ));
        };
        match escaped {
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            '0' => decoded.push('\0'),
            '\\' => decoded.push('\\'),
            other => {
                return Err(AppError::message(format!(
                    "Unsupported serial text escape sequence: \\{other}. Supported escapes are \\n, \\r, \\t, \\0, and \\\\."
                )));
            }
        }
    }
    Ok(decoded)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> AppResult<T> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn normalize_sha256_hex(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(AppError::message(
            "Expected SHA-256 must be exactly 64 hexadecimal characters.",
        ));
    }
    Ok(normalized)
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex_encode(&hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    message_len_bytes: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            message_len_bytes: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.message_len_bytes = self.message_len_bytes.saturating_add(input.len() as u64);

        if self.buffer_len > 0 {
            let needed = 64 - self.buffer_len;
            let take = needed.min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];

            if self.buffer_len == 64 {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        while input.len() >= 64 {
            self.process_block(&input[..64]);
            input = &input[64..];
        }

        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.message_len_bytes.saturating_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            for byte in &mut self.buffer[self.buffer_len..] {
                *byte = 0;
            }
            let block = self.buffer;
            self.process_block(&block);
            self.buffer_len = 0;
        }

        for byte in &mut self.buffer[self.buffer_len..56] {
            *byte = 0;
        }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.process_block(&block);

        let mut output = [0_u8; 32];
        for (chunk, value) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&value.to_be_bytes());
        }
        output
    }

    fn process_block(&mut self, block: &[u8]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        let mut words = [0_u32; 64];
        for (index, chunk) in block.chunks_exact(4).take(16).enumerate() {
            words[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn write_runtime_config(
    paths: &RuntimePaths,
    session_name: &str,
    budget: &RuntimeStorageBudget,
) -> AppResult<()> {
    if let Some(parent) = paths.runtime_config.parent() {
        fs::create_dir_all(parent)?;
    }

    let config = serde_json::json!({
        "schema_version": 1,
        "session_name": session_name,
        "default_capacity_gib": DEFAULT_RUNTIME_CAPACITY_GIB,
        "minimum_capacity_gib": MINIMUM_RUNTIME_CAPACITY_GIB,
        "requested_capacity_gib": budget.requested_capacity_gib,
        "target_engine": "pane-owned-os-runtime",
        "base_os_image": paths.base_os_image.display().to_string(),
        "serial_boot_image": paths.serial_boot_image.display().to_string(),
        "boot_loader_image": paths.boot_loader_image.display().to_string(),
        "kernel_image": paths.kernel_image.display().to_string(),
        "initramfs_image": paths.initramfs_image.display().to_string(),
        "kernel_boot_layout": paths.kernel_boot_layout.display().to_string(),
        "user_disk": paths.user_disk.display().to_string(),
        "policy": {
            "pane_shared_is_user_file_exchange": true,
            "runtime_user_disk_is_linux_system_storage": true,
            "current_launch_bridge": "wsl-xrdp-bridge"
        }
    });

    fs::write(
        &paths.runtime_config,
        serde_json::to_string_pretty(&config)?,
    )?;
    Ok(())
}

fn write_native_runtime_manifest(paths: &RuntimePaths, session_name: &str) -> AppResult<()> {
    if let Some(parent) = paths.native_manifest.parent() {
        fs::create_dir_all(parent)?;
    }

    let manifest = serde_json::json!({
        "schema_version": 1,
        "session_name": session_name,
        "engine": "pane-owned-os-runtime",
        "bootable": false,
        "external_integrations_required_by_target": {
            "wsl": false,
            "mstsc": false,
            "xrdp": false
        },
        "required_artifacts": {
            "base_os_image": paths.base_os_image.display().to_string(),
            "base_os_metadata": paths.base_os_metadata.display().to_string(),
            "serial_boot_image": paths.serial_boot_image.display().to_string(),
            "serial_boot_metadata": paths.serial_boot_metadata.display().to_string(),
            "boot_loader_image": paths.boot_loader_image.display().to_string(),
            "boot_loader_metadata": paths.boot_loader_metadata.display().to_string(),
            "kernel_image": paths.kernel_image.display().to_string(),
            "initramfs_image": paths.initramfs_image.display().to_string(),
            "kernel_boot_metadata": paths.kernel_boot_metadata.display().to_string(),
            "kernel_boot_layout": paths.kernel_boot_layout.display().to_string(),
            "user_disk": paths.user_disk.display().to_string(),
            "user_disk_metadata": paths.user_disk_metadata.display().to_string(),
            "framebuffer_contract": paths.framebuffer_contract.display().to_string(),
            "input_contract": paths.input_contract.display().to_string()
        },
        "blockers": [
            "verified base OS image must exist before native boot",
            "runtime-backed serial boot image must exist before the WHP boot-to-serial spike",
            "verified boot-to-serial loader must exist before runtime-provided boot-candidate execution",
            "verified kernel boot plan must exist before WHP kernel-entry execution",
            "kernel boot layout must be materialized before WHP kernel-entry execution",
            "Pane sparse user disk must exist before native boot",
            "Pane framebuffer contract must exist before native display work",
            "Pane input contract must exist before native display work",
            "Windows Hypervisor Platform host preflight must pass before the boot spike",
            "Pane-owned WHP boot engine is not implemented",
            "Pane-owned display transport is not implemented"
        ]
    });

    fs::write(
        &paths.native_manifest,
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(())
}

fn build_runtime_artifact_report(paths: &RuntimePaths) -> RuntimeArtifactReport {
    let base_metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).ok();
    let base_image_bytes = fs::metadata(&paths.base_os_image)
        .ok()
        .map(|metadata| metadata.len());
    let base_os_image_verified = base_metadata
        .as_ref()
        .zip(base_image_bytes)
        .map(|(metadata, bytes)| metadata.verified && metadata.bytes == bytes)
        .unwrap_or(false);

    let serial_boot_metadata =
        read_json_file::<SerialBootImageMetadata>(&paths.serial_boot_metadata).ok();
    let serial_boot_image_bytes = fs::metadata(&paths.serial_boot_image)
        .ok()
        .map(|metadata| metadata.len());
    let serial_boot_actual_sha256 = sha256_file(&paths.serial_boot_image).ok();
    let serial_boot_image_ready = serial_boot_metadata
        .as_ref()
        .zip(serial_boot_image_bytes)
        .map(|(metadata, bytes)| {
            metadata.schema_version == 1
                && metadata.image_kind == "pane-serial-boot-test-image"
                && metadata.bytes == bytes
                && Some(metadata.sha256.as_str()) == serial_boot_actual_sha256.as_deref()
                && metadata.serial_banner == crate::native::SERIAL_BOOT_BANNER_TEXT
        })
        .unwrap_or(false);

    let boot_loader_metadata =
        read_json_file::<BootLoaderImageMetadata>(&paths.boot_loader_metadata).ok();
    let boot_loader_image_bytes = fs::metadata(&paths.boot_loader_image)
        .ok()
        .map(|metadata| metadata.len());
    let boot_loader_actual_sha256 = sha256_file(&paths.boot_loader_image).ok();
    let boot_loader_image_verified = boot_loader_metadata
        .as_ref()
        .zip(boot_loader_image_bytes)
        .map(|(metadata, bytes)| {
            metadata.schema_version == 1
                && metadata.image_kind == "pane-boot-to-serial-loader-image"
                && metadata.verified
                && metadata.bytes == bytes
                && bytes <= crate::native::SERIAL_BOOT_TEST_IMAGE_SIZE as u64
                && Some(metadata.sha256.as_str()) == boot_loader_actual_sha256.as_deref()
                && !metadata.expected_serial_text.is_empty()
                && metadata.guest_entry_gpa == "0x1000"
        })
        .unwrap_or(false);

    let kernel_boot_metadata =
        read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).ok();
    let kernel_image_bytes = fs::metadata(&paths.kernel_image)
        .ok()
        .map(|metadata| metadata.len());
    let kernel_actual_sha256 = sha256_file(&paths.kernel_image).ok();
    let initramfs_image_bytes = fs::metadata(&paths.initramfs_image)
        .ok()
        .map(|metadata| metadata.len());
    let initramfs_actual_sha256 = sha256_file(&paths.initramfs_image).ok();
    let kernel_image_verified = kernel_boot_metadata
        .as_ref()
        .zip(kernel_image_bytes)
        .map(|(metadata, bytes)| {
            metadata.schema_version == 1
                && metadata.image_kind == "pane-linux-kernel-boot-plan-v1"
                && metadata.kernel_verified
                && metadata.kernel_bytes == bytes
                && Some(metadata.kernel_sha256.as_str()) == kernel_actual_sha256.as_deref()
                && metadata.kernel_stored_path == paths.kernel_image.display().to_string()
                && matches!(
                    metadata.kernel_format.as_str(),
                    "linux-bzimage" | "controlled-serial-candidate"
                )
        })
        .unwrap_or(false);
    let initramfs_image_verified = kernel_boot_metadata
        .as_ref()
        .map(|metadata| {
            if metadata.initramfs_stored_path.is_none() {
                true
            } else {
                metadata.initramfs_verified
                    && metadata.initramfs_bytes == initramfs_image_bytes
                    && metadata.initramfs_sha256.as_deref() == initramfs_actual_sha256.as_deref()
                    && metadata.initramfs_stored_path.as_deref()
                        == Some(paths.initramfs_image.display().to_string().as_str())
            }
        })
        .unwrap_or(false);
    let kernel_boot_plan_ready = kernel_boot_metadata
        .as_ref()
        .map(|metadata| {
            kernel_image_verified
                && initramfs_image_verified
                && metadata.cmdline.contains("console=ttyS0")
                && metadata.kernel_load_gpa == "0x00100000"
        })
        .unwrap_or(false);
    let kernel_boot_layout = read_json_file::<KernelBootLayout>(&paths.kernel_boot_layout).ok();
    let framebuffer_contract =
        read_json_file::<FramebufferContract>(&paths.framebuffer_contract).ok();
    let framebuffer_contract_ready = framebuffer_contract
        .as_ref()
        .map(|contract| {
            contract.schema_version == 1
                && contract.device == "pane-linear-framebuffer-v1"
                && contract.width > 0
                && contract.height > 0
                && contract.bytes_per_pixel == 4
                && contract.stride_bytes == contract.width * contract.bytes_per_pixel
                && contract.size_bytes
                    == u64::from(contract.stride_bytes) * u64::from(contract.height)
                && parse_guest_physical_address(&contract.guest_gpa).is_ok()
        })
        .unwrap_or(false);

    let input_contract = read_json_file::<InputContract>(&paths.input_contract).ok();
    let input_contract_ready = input_contract
        .as_ref()
        .map(|contract| {
            contract.schema_version == 1
                && contract.keyboard_device == "pane-ps2-keyboard-v1"
                && contract.pointer_device == "pane-absolute-pointer-v1"
                && contract.transport == "pane-host-event-queue"
                && contract.coordinate_space == "framebuffer-pixels"
                && contract.queue_size_bytes >= u64::from(contract.event_record_bytes)
                && contract.event_record_bytes > 0
                && parse_guest_physical_address(&contract.guest_queue_gpa).is_ok()
        })
        .unwrap_or(false);

    let kernel_boot_layout_ready = kernel_boot_layout
        .as_ref()
        .zip(kernel_boot_metadata.as_ref())
        .map(|(layout, metadata)| {
            let expected_guest_memory_map = if metadata.kernel_format == "linux-bzimage" {
                framebuffer_contract
                    .as_ref()
                    .zip(input_contract.as_ref())
                    .and_then(|(framebuffer, input)| {
                        let mut ranges =
                            default_linux_guest_memory_map(metadata.initramfs_bytes.unwrap_or(0));
                        ranges
                            .extend(runtime_contract_guest_memory_ranges(framebuffer, input).ok()?);
                        Some(ranges)
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            kernel_boot_plan_ready
                && layout.schema_version == 1
                && layout.layout_kind == "pane-linux-kernel-boot-layout-v1"
                && layout.boot_params_gpa == "0x00007000"
                && layout.cmdline_gpa == "0x00020000"
                && layout.kernel_load_gpa == metadata.kernel_load_gpa
                && layout.initramfs_load_gpa == metadata.initramfs_load_gpa
                && layout.kernel_path == paths.kernel_image.display().to_string()
                && layout.kernel_bytes == metadata.kernel_bytes
                && layout.kernel_sha256 == metadata.kernel_sha256
                && layout.kernel_format == metadata.kernel_format
                && layout.linux_boot_protocol == metadata.linux_boot_protocol
                && layout.linux_setup_sectors == metadata.linux_setup_sectors
                && layout.linux_setup_bytes == metadata.linux_setup_bytes
                && layout.linux_protected_mode_offset == metadata.linux_protected_mode_offset
                && layout.linux_protected_mode_bytes == metadata.linux_protected_mode_bytes
                && layout.linux_loadflags == metadata.linux_loadflags
                && layout.linux_preferred_load_address == metadata.linux_preferred_load_address
                && layout.linux_entry_point_gpa
                    == (metadata.kernel_format == "linux-bzimage")
                        .then(|| metadata.kernel_load_gpa.clone())
                && layout.linux_boot_params_register
                    == (metadata.kernel_format == "linux-bzimage").then(|| "rsi".to_string())
                && layout.linux_expected_entry_mode
                    == (metadata.kernel_format == "linux-bzimage")
                        .then(|| "x86-protected-mode-32".to_string())
                && layout.guest_memory_map == expected_guest_memory_map
                && layout.initramfs_path == metadata.initramfs_stored_path
                && layout.initramfs_bytes == metadata.initramfs_bytes
                && layout.initramfs_sha256 == metadata.initramfs_sha256
                && layout.cmdline == metadata.cmdline
                && layout.cmdline.contains("console=ttyS0")
                && layout.expected_serial_device == "ttyS0"
                && layout.framebuffer.as_ref().is_some_and(|contract| {
                    contract.schema_version == 1
                        && contract.device == "pane-linear-framebuffer-v1"
                        && contract.size_bytes
                            == u64::from(contract.stride_bytes) * u64::from(contract.height)
                })
                && layout.input.as_ref().is_some_and(|contract| {
                    contract.schema_version == 1
                        && contract.keyboard_device == "pane-ps2-keyboard-v1"
                        && contract.pointer_device == "pane-absolute-pointer-v1"
                        && contract.guest_queue_gpa == "0x0dff0000"
                        && contract.queue_size_bytes == 0x00001000
                })
        })
        .unwrap_or(false);

    let user_disk_metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).ok();
    let user_disk_ready = user_disk_artifact_ready(paths, &user_disk_metadata);

    RuntimeArtifactReport {
        base_os_image_exists: paths.base_os_image.is_file(),
        base_os_image_bytes: base_image_bytes,
        base_os_image_sha256: base_metadata
            .as_ref()
            .map(|metadata| metadata.sha256.clone()),
        base_os_image_verified,
        base_os_metadata_exists: paths.base_os_metadata.is_file(),
        serial_boot_image_exists: paths.serial_boot_image.is_file(),
        serial_boot_image_bytes,
        serial_boot_image_sha256: serial_boot_metadata
            .as_ref()
            .map(|metadata| metadata.sha256.clone()),
        serial_boot_image_ready,
        serial_boot_banner: serial_boot_metadata
            .as_ref()
            .map(|metadata| metadata.serial_banner.clone()),
        serial_boot_metadata_exists: paths.serial_boot_metadata.is_file(),
        boot_loader_image_exists: paths.boot_loader_image.is_file(),
        boot_loader_image_bytes,
        boot_loader_image_sha256: boot_loader_metadata
            .as_ref()
            .map(|metadata| metadata.sha256.clone()),
        boot_loader_image_verified,
        boot_loader_expected_serial: boot_loader_metadata
            .as_ref()
            .map(|metadata| metadata.expected_serial_text.clone()),
        boot_loader_metadata_exists: paths.boot_loader_metadata.is_file(),
        kernel_image_exists: paths.kernel_image.is_file(),
        kernel_image_bytes,
        kernel_image_sha256: kernel_boot_metadata
            .as_ref()
            .map(|metadata| metadata.kernel_sha256.clone()),
        kernel_image_verified,
        kernel_format: kernel_boot_metadata
            .as_ref()
            .map(|metadata| metadata.kernel_format.clone()),
        kernel_linux_boot_protocol: kernel_boot_metadata
            .as_ref()
            .and_then(|metadata| metadata.linux_boot_protocol.clone()),
        kernel_linux_protected_mode_offset: kernel_boot_metadata
            .as_ref()
            .and_then(|metadata| metadata.linux_protected_mode_offset),
        kernel_linux_protected_mode_bytes: kernel_boot_metadata
            .as_ref()
            .and_then(|metadata| metadata.linux_protected_mode_bytes),
        initramfs_image_exists: paths.initramfs_image.is_file(),
        initramfs_image_bytes,
        initramfs_image_sha256: kernel_boot_metadata
            .as_ref()
            .and_then(|metadata| metadata.initramfs_sha256.clone()),
        initramfs_image_verified,
        kernel_cmdline: kernel_boot_metadata
            .as_ref()
            .map(|metadata| metadata.cmdline.clone()),
        kernel_boot_plan_ready,
        kernel_boot_metadata_exists: paths.kernel_boot_metadata.is_file(),
        kernel_boot_layout_exists: paths.kernel_boot_layout.is_file(),
        kernel_boot_layout_ready,
        framebuffer_contract_exists: paths.framebuffer_contract.is_file(),
        framebuffer_contract_ready,
        framebuffer_resolution: framebuffer_contract
            .as_ref()
            .map(FramebufferContract::resolution_label),
        input_contract_exists: paths.input_contract.is_file(),
        input_contract_ready,
        user_disk_exists: paths.user_disk.is_file(),
        user_disk_capacity_gib: user_disk_metadata
            .as_ref()
            .map(|metadata| metadata.capacity_gib),
        user_disk_format: user_disk_metadata
            .as_ref()
            .map(|metadata| metadata.format.clone()),
        user_disk_ready,
        user_disk_metadata_exists: paths.user_disk_metadata.is_file(),
        runtime_manifest_exists: paths.manifest.is_file(),
        runtime_config_exists: paths.runtime_config.is_file(),
        native_manifest_exists: paths.native_manifest.is_file(),
    }
}

fn build_native_runtime_report(
    prepared: bool,
    artifacts: &RuntimeArtifactReport,
    native_host: &crate::native::NativeHostPreflightReport,
) -> NativeRuntimeReport {
    let mut blockers = Vec::new();
    let state = if !prepared {
        blockers.push(
            "Dedicated runtime directories have not been prepared. Run `pane runtime --prepare`."
                .to_string(),
        );
        NativeRuntimeState::StorageNotPrepared
    } else {
        if !artifacts.base_os_image_exists {
            blockers.push(
                "No verified Pane-owned Arch base OS image exists in the runtime images directory."
                    .to_string(),
            );
        } else if !artifacts.base_os_image_verified {
            blockers.push(
                "Pane has a base OS image, but it is not trusted. Re-register it with --expected-sha256 so Pane can verify the image before boot."
                    .to_string(),
            );
        }
        if !artifacts.user_disk_ready {
            blockers.push(
                "No valid Pane-owned sparse user disk exists for packages, user accounts, and customization data. Run `pane runtime --create-user-disk`."
                    .to_string(),
            );
        }
        if !artifacts.serial_boot_image_ready {
            blockers.push(
                "No valid Pane-owned serial boot test image exists. Run `pane runtime --create-serial-boot-image`."
                    .to_string(),
            );
        }
        if !artifacts.framebuffer_contract_ready {
            blockers.push(
                "No valid Pane framebuffer contract exists. Run `pane runtime --prepare` to write the first display boundary contract."
                    .to_string(),
            );
        }
        if !artifacts.input_contract_ready {
            blockers.push(
                "No valid Pane input contract exists. Run `pane runtime --prepare` to write the keyboard/pointer boundary contract."
                    .to_string(),
            );
        }

        if !artifacts.base_os_image_exists {
            NativeRuntimeState::MissingBaseImage
        } else if !artifacts.base_os_image_verified {
            NativeRuntimeState::UnverifiedBaseImage
        } else if !artifacts.user_disk_ready {
            NativeRuntimeState::MissingUserDisk
        } else if !native_host.ready_for_boot_spike {
            NativeRuntimeState::HostNotReady
        } else {
            NativeRuntimeState::EngineNotImplemented
        }
    };

    if !matches!(state, NativeRuntimeState::StorageNotPrepared) && !artifacts.runtime_config_exists
    {
        blockers.push("The runtime config file is missing.".to_string());
    }
    if !matches!(state, NativeRuntimeState::StorageNotPrepared) && !artifacts.native_manifest_exists
    {
        blockers.push("The native runtime manifest is missing.".to_string());
    }
    for check in &native_host.checks {
        if check.status == crate::native::NativePreflightStatus::Fail {
            let mut blocker = format!("Native host check `{}` failed: {}", check.id, check.summary);
            if let Some(remediation) = &check.remediation {
                blocker.push_str(" Fix: ");
                blocker.push_str(remediation);
            }
            blockers.push(blocker);
        }
    }
    blockers.push(
        "Pane-owned boot and display execution are not implemented yet; current launch still uses the WSL/XRDP bridge."
            .to_string(),
    );

    let ready_for_boot_spike = prepared
        && artifacts.base_os_image_verified
        && artifacts.user_disk_ready
        && artifacts.serial_boot_image_ready
        && artifacts.runtime_config_exists
        && artifacts.native_manifest_exists
        && artifacts.framebuffer_contract_ready
        && artifacts.input_contract_ready
        && native_host.ready_for_boot_spike;

    NativeRuntimeReport {
        state,
        state_label: state.display_name(),
        bootable: false,
        host_ready: native_host.ready_for_boot_spike,
        ready_for_boot_spike,
        requires_wsl: false,
        requires_mstsc: false,
        requires_xrdp: false,
        launch_contract: "Pane-owned runtime must boot from Pane's base OS image plus user disk and render inside a Pane-owned app window without WSL, mstsc.exe, or XRDP.",
        blockers,
    }
}

fn build_runtime_ownership_report(native_runtime: &NativeRuntimeReport) -> RuntimeOwnershipReport {
    RuntimeOwnershipReport {
        app_owned_storage: true,
        app_owned_boot_engine_available: native_runtime.bootable,
        app_owned_display_available: false,
        external_runtime_required_for_current_launch: true,
        current_external_dependencies: vec!["wsl.exe", "mstsc.exe", "xrdp"],
    }
}

fn write_runtime_manifest(paths: &RuntimePaths, report: &RuntimeReport) -> AppResult<()> {
    if let Some(parent) = paths.manifest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.manifest, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

fn build_app_storage_report(session_name: &str) -> AppStorageReport {
    let durable_workspace = workspace_for(session_name);
    let scratch_workspace =
        workspace_for_with_shared_storage(session_name, SharedStorageMode::Scratch);

    AppStorageReport {
        default_mode: SharedStorageMode::Durable,
        durable_shared_dir: shared_dir_for_workspace(&durable_workspace)
            .display()
            .to_string(),
        scratch_shared_dir: shared_dir_for_workspace(&scratch_workspace)
            .display()
            .to_string(),
        policy: "Durable PaneShared is user storage and survives reset by default; scratch PaneShared is disposable session storage.",
    }
}

fn build_app_display_report() -> AppDisplayReport {
    AppDisplayReport {
        current_mode: AppDisplayMode::ExternalMstscRdp,
        current_mode_label: "External mstsc.exe + XRDP handoff",
        contained_window_available: false,
        user_visible_handoff: true,
        planned_modes: vec![
            AppDisplayMode::EmbeddedRdpWindow,
            AppDisplayMode::NativePaneTransport,
        ],
        notes: vec![
            "Pane currently launches the Windows Remote Desktop client after preparing the Arch session."
                .to_string(),
            "The next display milestone is a Pane-owned window that embeds the session before any non-RDP transport work."
                .to_string(),
        ],
    }
}

fn determine_app_lifecycle(
    status: &StatusReport,
    doctor: &DoctorReport,
    saved_launch: Option<&StoredLaunch>,
) -> (AppLifecyclePhase, AppNextAction, String, Vec<String>) {
    if !cfg!(windows) {
        return (
            AppLifecyclePhase::UnsupportedHost,
            AppNextAction::CollectSupportBundle,
            "Pane's app surface currently requires Windows. Use the CLI only for inspection on this platform."
                .to_string(),
            vec!["Run Pane from Windows 10 or 11 for the supported MVP path.".to_string()],
        );
    }

    if !status.wsl_available {
        return (
            AppLifecyclePhase::HostNeedsWsl,
            AppNextAction::InstallWsl,
            "Install or repair WSL2 before Pane can create a Linux desktop environment."
                .to_string(),
            vec!["After WSL2 is available, return to Pane and run Onboard Arch.".to_string()],
        );
    }

    if status.managed_environment.is_none()
        || status.selected_distro.as_ref().map_or(true, |health| {
            !health.present_in_inventory || !health.supported_for_mvp
        })
    {
        return (
            AppLifecyclePhase::NeedsManagedEnvironment,
            AppNextAction::OnboardArch,
            "Create or adopt the Pane-managed Arch environment, then configure the Linux login."
                .to_string(),
            vec!["Pane keeps Ubuntu, Debian, KDE, GNOME, and Niri hidden until their lifecycle path is supportable.".to_string()],
        );
    }

    if status
        .selected_distro
        .as_ref()
        .is_some_and(distro_needs_user_setup)
    {
        return (
            AppLifecyclePhase::NeedsUserSetup,
            AppNextAction::SetupUser,
            "Create or repair the non-root Arch login and systemd WSL config before launching the desktop."
                .to_string(),
            vec!["XRDP needs a regular Linux user with a usable password; Pane does not grant passwordless sudo.".to_string()],
        );
    }

    if let Some(launch) = saved_launch {
        if launch.stage == LaunchStage::Failed {
            return (
                AppLifecyclePhase::LaunchFailed,
                AppNextAction::RepairArch,
                "The last launch failed. Repair the Pane-managed Arch integration before reconnecting."
                    .to_string(),
                vec!["Collect a support bundle if repair does not resolve the failure.".to_string()],
            );
        }

        if matches!(
            launch.stage,
            LaunchStage::Bootstrapped | LaunchStage::RdpLaunched
        ) {
            return (
                AppLifecyclePhase::ReconnectReady,
                AppNextAction::Reconnect,
                "The Arch session has been bootstrapped. Reconnect or relaunch from the Control Center."
                    .to_string(),
                vec!["If the desktop opens blank or disconnects, use Repair Arch before asking for support.".to_string()],
            );
        }
    }

    if doctor.has_failures() {
        return (
            AppLifecyclePhase::NeedsRepair,
            AppNextAction::RepairArch,
            "Pane found blockers in the supported Arch + XFCE path. Repair before launch."
                .to_string(),
            vec!["Open Doctor or collect a support bundle to inspect exact blockers.".to_string()],
        );
    }

    (
        AppLifecyclePhase::ReadyToLaunch,
        AppNextAction::LaunchArch,
        "Arch + XFCE is ready for the current Pane launch path.".to_string(),
        vec!["Launch Arch opens the current XRDP bridge; the contained Pane window remains a later transport milestone.".to_string()],
    )
}

fn distro_needs_user_setup(health: &DistroHealth) -> bool {
    let default_user_ok = health
        .distro
        .default_user
        .as_deref()
        .is_some_and(|user| !user.eq_ignore_ascii_case("root"));
    !default_user_ok
        || !health
            .default_user_password_status
            .is_some_and(PasswordStatus::is_usable)
        || health.systemd_configured != Some(true)
}

fn resolve_session_context(
    session_name: Option<&str>,
    saved_state: Option<&PaneState>,
) -> (String, Option<StoredLaunch>, WorkspacePaths) {
    let requested_session = session_name
        .map(ToString::to_string)
        .or_else(|| {
            saved_state
                .and_then(|state| state.last_launch.as_ref())
                .map(|launch| launch.session_name.clone())
        })
        .unwrap_or_else(|| "pane".to_string());
    let normalized_session = crate::plan::sanitize_session_name(&requested_session);
    let saved_launch = saved_state
        .and_then(|state| state.last_launch.clone())
        .filter(|launch| launch.session_name == normalized_session);
    let workspace = saved_launch
        .as_ref()
        .map(|launch| launch.workspace.clone())
        .unwrap_or_else(|| workspace_for(&normalized_session));

    (normalized_session, saved_launch, workspace)
}

fn build_bundle_doctor_request(
    session_name: &str,
    explicit_distro: Option<String>,
    saved_launch: Option<&StoredLaunch>,
    saved_state: Option<&PaneState>,
    inventory: &WslInventory,
) -> AppResult<DoctorRequest> {
    let distro = explicit_distro
        .or_else(|| saved_launch.map(|launch| launch.distro.name.clone()))
        .or(resolve_status_distro(None, inventory, saved_state)?);
    let port = distro
        .as_deref()
        .map(|name| status_port_for(name, saved_state))
        .or_else(|| saved_launch.map(|launch| launch.port))
        .unwrap_or(3390);

    Ok(DoctorRequest {
        distro,
        session_name: session_name.to_string(),
        desktop_environment: saved_launch
            .map(|launch| launch.desktop_environment)
            .unwrap_or(DesktopEnvironment::Xfce),
        port,
        bootstrap_requested: saved_launch
            .map(|launch| {
                launch.bootstrap_requested && launch.bootstrapped_at_epoch_seconds.is_none()
            })
            .unwrap_or(true),
        connect_requested: saved_launch
            .map(|launch| launch.connect_requested)
            .unwrap_or(true),
        write_probes_enabled: true,
    })
}

fn resolve_bundle_output_path(explicit: Option<&Path>, session_name: &str) -> PathBuf {
    let default_name = default_bundle_file_name(session_name);

    match explicit {
        Some(path) if path.is_dir() => path.join(default_name),
        Some(path) if path.extension().is_none() => path.with_extension("zip"),
        Some(path) => path.to_path_buf(),
        None => app_root().join("support").join(default_name),
    }
}

fn default_bundle_file_name(session_name: &str) -> String {
    format!(
        "pane-support-{}-{}.zip",
        session_name,
        current_epoch_seconds()
    )
}

fn write_support_bundle(
    staging_root: &Path,
    output_zip: &Path,
    session_name: &str,
    saved_state: Option<&PaneState>,
    workspace: &WorkspacePaths,
    status_report: &StatusReport,
    doctor_report: &DoctorReport,
) -> AppResult<BundleManifest> {
    let mut included_files = Vec::new();
    let mut notes = Vec::new();

    write_bundle_json(
        staging_root,
        "status.json",
        status_report,
        &mut included_files,
    )?;
    write_bundle_json(
        staging_root,
        "doctor.json",
        doctor_report,
        &mut included_files,
    )?;

    if let Some(state) = saved_state {
        write_bundle_json(staging_root, "state.json", state, &mut included_files)?;
    } else {
        notes.push("Pane state has not been written yet.".to_string());
    }

    let shared_directory = shared_dir_for_workspace(workspace);
    let shared_details = format!(
        "Session: {}\nWindows Path: {}\nWSL Path: {}\nLinux Link: ~/PaneShared\n",
        session_name,
        shared_directory.display(),
        windows_to_wsl_path(&shared_directory)
    );
    write_bundle_text(
        staging_root,
        "shared-directory.txt",
        &shared_details,
        &mut included_files,
    )?;

    copy_bundle_file_if_exists(
        &workspace.bootstrap_script,
        staging_root,
        "workspace/pane-bootstrap.sh",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.rdp_profile,
        staging_root,
        "workspace/pane.rdp",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.bootstrap_log,
        staging_root,
        "workspace/bootstrap.log",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.transport_log,
        staging_root,
        "workspace/transport.log",
        &mut included_files,
        &mut notes,
    )?;

    if let Some(distro) = doctor_report
        .target_distro
        .as_deref()
        .filter(|_| status_report.wsl_available)
    {
        match wsl::tail_xrdp_logs(distro, 100) {
            Ok(logs) if logs.trim().is_empty() => {
                notes.push(format!(
                    "No live XRDP logs were available inside {}.",
                    distro
                ));
            }
            Ok(logs) => {
                write_bundle_text(
                    staging_root,
                    "wsl-xrdp-logs.txt",
                    &logs,
                    &mut included_files,
                )?;
            }
            Err(error) => {
                notes.push(format!(
                    "Could not collect live XRDP logs from {}: {}",
                    distro, error
                ));
            }
        }
    } else {
        notes.push("No live WSL distro was available for XRDP log collection.".to_string());
    }

    let mut manifest_files = included_files.clone();
    manifest_files.push("manifest.json".to_string());
    let manifest = BundleManifest {
        created_at_epoch_seconds: current_epoch_seconds(),
        session_name: session_name.to_string(),
        selected_distro: doctor_report.target_distro.clone(),
        output_zip: output_zip.display().to_string(),
        included_files: manifest_files,
        notes,
    };

    write_bundle_json(
        staging_root,
        "manifest.json",
        &manifest,
        &mut included_files,
    )?;
    Ok(manifest)
}

fn write_bundle_json<T: Serialize>(
    staging_root: &Path,
    relative_path: &str,
    value: &T,
    included_files: &mut Vec<String>,
) -> AppResult<()> {
    let path = staging_root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(
        &path,
        redact_support_text(&serde_json::to_string_pretty(value)?),
    )?;
    included_files.push(relative_path.replace('\\', "/"));
    Ok(())
}

fn write_bundle_text(
    staging_root: &Path,
    relative_path: &str,
    value: &str,
    included_files: &mut Vec<String>,
) -> AppResult<()> {
    let path = staging_root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, redact_support_text(value))?;
    included_files.push(relative_path.replace('\\', "/"));
    Ok(())
}

fn copy_bundle_file_if_exists(
    source: &Path,
    staging_root: &Path,
    relative_path: &str,
    included_files: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> AppResult<()> {
    if source.exists() {
        let destination = staging_root.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(source, &destination)?;
        included_files.push(relative_path.replace('\\', "/"));
    } else {
        notes.push(format!(
            "{} was not found at {}.",
            relative_path.replace('\\', "/"),
            source.display()
        ));
    }

    Ok(())
}

fn compress_bundle_dir(staging_root: &Path, output_zip: &Path) -> AppResult<()> {
    if let Some(parent) = output_zip.parent() {
        fs::create_dir_all(parent)?;
    }

    let archive_input = format!("{}\\*", staging_root.display());
    let command = format!(
        "Compress-Archive -Path {} -DestinationPath {} -Force",
        powershell_quote(&archive_input),
        powershell_quote(&output_zip.display().to_string()),
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &command])
        .output()
        .map_err(|error| {
            AppError::message(format!(
                "failed to run PowerShell compression for {}: {error}",
                output_zip.display()
            ))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let details = format!("{} {}", stdout.trim(), stderr.trim())
        .trim()
        .to_string();

    Err(AppError::message(format!(
        "failed to create support bundle at {}{}",
        output_zip.display(),
        if details.is_empty() {
            ".".to_string()
        } else {
            format!(": {}", details)
        }
    )))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn redact_support_text(raw: &str) -> String {
    let mut redacted = raw.to_string();
    for (key, label) in [
        ("USERPROFILE", "<windows-user-profile>"),
        ("LOCALAPPDATA", "<local-app-data>"),
        ("APPDATA", "<app-data>"),
        ("USERNAME", "<windows-user>"),
        ("COMPUTERNAME", "<computer-name>"),
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                redacted = redacted.replace(trimmed, label);
                redacted = redacted.replace(&trimmed.replace('\\', "/"), label);
            }
        }
    }

    redacted
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn initialize_managed_arch_environment(
    args: &InitArgs,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<InitReport> {
    let source = resolve_init_source(args, inventory)?;
    let mut notes = Vec::new();
    let existing_managed_environment = saved_state
        .and_then(|state| state.managed_environment.as_ref())
        .filter(|environment| environment.is_arch())
        .cloned();

    let managed_environment = match source {
        InitSource::AdoptExisting { distro_name } => {
            let distro = validate_arch_distro(&distro_name, inventory)?;
            if args.existing_distro.is_some()
                && !distro.name.eq_ignore_ascii_case(&args.distro_name)
            {
                notes.push(format!(
                    "Adopted the existing WSL distro '{}' instead of creating a new '{}'.",
                    distro.name, args.distro_name
                ));
            } else {
                notes.push(format!(
                    "Pane will use the existing Arch distro '{}' as its managed environment.",
                    distro.name
                ));
            }

            if let Some(existing) = existing_managed_environment
                .as_ref()
                .filter(|environment| environment.distro_name.eq_ignore_ascii_case(&distro.name))
            {
                notes.push(format!(
                    "Preserved Pane ownership metadata for '{}' because it is already the managed Arch environment.",
                    distro.name
                ));
                ManagedEnvironmentState {
                    environment_id: existing.environment_id.clone(),
                    distro_name: distro.name,
                    family: DistroFamily::Arch,
                    ownership: existing.ownership,
                    install_dir: existing.install_dir.clone(),
                    source_rootfs: existing.source_rootfs.clone(),
                    created_at_epoch_seconds: existing.created_at_epoch_seconds,
                }
            } else {
                ManagedEnvironmentState {
                    environment_id: "arch".to_string(),
                    distro_name: distro.name,
                    family: DistroFamily::Arch,
                    ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                    install_dir: None,
                    source_rootfs: None,
                    created_at_epoch_seconds: current_epoch_seconds(),
                }
            }
        }
        InitSource::InstallOnline {
            distro_name,
            install_dir,
        } => {
            if args.dry_run {
                notes.push(format!(
                    "Pane would install the official Arch WSL image as '{}' into {} using the WSL online install path.",
                    distro_name,
                    install_dir.display()
                ));
            } else {
                if !inventory.available {
                    return Err(AppError::message(
                        "wsl.exe was not found. Install WSL2 before Pane can provision Arch automatically.",
                    ));
                }

                ensure_managed_install_dir_available(&install_dir)?;
                let transcript =
                    wsl::install_online_distro("archlinux", &distro_name, &install_dir)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL online install failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }

                let refreshed_inventory = probe_inventory();
                let installed = validate_arch_distro(&distro_name, &refreshed_inventory)?;
                notes.push(format!(
                    "Installed the official Arch WSL image as '{}' in {}.",
                    installed.name,
                    install_dir.display()
                ));
            }

            ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name,
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::InstalledOnline,
                install_dir: Some(install_dir),
                source_rootfs: None,
                created_at_epoch_seconds: current_epoch_seconds(),
            }
        }
        InitSource::ImportRootfs {
            distro_name,
            rootfs_tar,
            install_dir,
        } => {
            if !rootfs_tar.exists() {
                return Err(AppError::message(format!(
                    "The Arch rootfs tarball was not found at {}.",
                    rootfs_tar.display()
                )));
            }

            if args.dry_run {
                notes.push(format!(
                    "Pane would import {} into {} from {}.",
                    distro_name,
                    install_dir.display(),
                    rootfs_tar.display()
                ));
            } else {
                ensure_managed_install_dir_available(&install_dir)?;
                let transcript = wsl::import_distro(&distro_name, &install_dir, &rootfs_tar)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL import failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }

                let refreshed_inventory = probe_inventory();
                let imported = validate_arch_distro(&distro_name, &refreshed_inventory)?;
                notes.push(format!(
                    "Imported {} into {} as a Pane-managed Arch environment.",
                    imported.name,
                    install_dir.display()
                ));
            }

            ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name,
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::ImportedRootfs,
                install_dir: Some(install_dir),
                source_rootfs: Some(rootfs_tar),
                created_at_epoch_seconds: current_epoch_seconds(),
            }
        }
    };

    let present_in_inventory =
        inventory_contains_distro(&probe_inventory(), &managed_environment.distro_name);
    if !args.dry_run {
        save_managed_environment(managed_environment.clone())?;
    }

    if args.dry_run {
        notes.push("Dry run only. No WSL distro or Pane state was modified.".to_string());
    } else {
        notes.push(format!(
            "Pane will now prefer '{}' when no --distro override is provided.",
            managed_environment.distro_name
        ));
    }

    Ok(InitReport {
        product_shape: "Pane owns a dedicated Arch environment first, then layers launch/bootstrap/recovery on top of it.",
        managed_environment,
        dry_run: args.dry_run,
        present_in_inventory,
        notes,
    })
}

fn resolve_init_source(args: &InitArgs, inventory: &WslInventory) -> AppResult<InitSource> {
    if args.existing_distro.is_some() && args.rootfs_tar.is_some() {
        return Err(AppError::message(
            "Choose either --existing-distro or --rootfs-tar for `pane init`, not both.",
        ));
    }

    if args.existing_distro.is_some() && args.install_dir.is_some() {
        return Err(AppError::message(
            "--install-dir is only valid when Pane is provisioning a managed distro, not with --existing-distro.",
        ));
    }

    if let Some(existing_distro) = &args.existing_distro {
        return Ok(InitSource::AdoptExisting {
            distro_name: existing_distro.clone(),
        });
    }

    if let Some(rootfs_tar) = &args.rootfs_tar {
        return Ok(InitSource::ImportRootfs {
            distro_name: args.distro_name.clone(),
            rootfs_tar: rootfs_tar.clone(),
            install_dir: args
                .install_dir
                .clone()
                .unwrap_or_else(|| managed_distro_install_root(&args.distro_name)),
        });
    }

    if inventory.available && inventory_contains_distro(inventory, &args.distro_name) {
        return Ok(InitSource::AdoptExisting {
            distro_name: args.distro_name.clone(),
        });
    }

    Ok(InitSource::InstallOnline {
        distro_name: args.distro_name.clone(),
        install_dir: args
            .install_dir
            .clone()
            .unwrap_or_else(|| managed_distro_install_root(&args.distro_name)),
    })
}

fn validate_arch_distro(name: &str, inventory: &WslInventory) -> AppResult<DistroRecord> {
    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Install WSL2 before initializing Pane Arch.",
        ));
    }

    if !inventory_contains_distro(inventory, name) {
        return Err(AppError::message(format!(
            "WSL distro '{}' was not found. Available distros: {}",
            name,
            available_distros(inventory)
        )));
    }

    let distro = wsl::inspect_distro(name, inventory)?;
    if distro.family != DistroFamily::Arch {
        return Err(AppError::message(format!(
            "Pane can only initialize Arch right now, but '{}' resolved to {}.",
            distro.name,
            distro.family.display_name()
        )));
    }

    Ok(distro)
}

fn ensure_managed_install_dir_available(install_dir: &Path) -> AppResult<()> {
    if install_dir.exists() {
        if !install_dir.is_dir() {
            return Err(AppError::message(format!(
                "{} already exists and is not a directory.",
                install_dir.display()
            )));
        }

        let mut entries = fs::read_dir(install_dir)?;
        if entries.next().transpose()?.is_some() {
            return Err(AppError::message(format!(
                "{} is not empty. Choose an empty install directory for Pane-managed Arch provisioning.",
                install_dir.display()
            )));
        }
    } else {
        fs::create_dir_all(install_dir)?;
    }

    Ok(())
}

fn resolve_managed_environment_for_reset(
    args: &ResetArgs,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<ManagedEnvironmentState>> {
    if !args.release_managed_environment && !args.factory_reset {
        return Ok(None);
    }

    let Some(environment) = saved_state.and_then(|state| state.managed_environment.clone()) else {
        return Err(AppError::message(
            "Pane is not currently managing a distro. Run `pane init` first.",
        ));
    };

    if args.factory_reset && !environment.ownership.can_factory_reset() {
        return Err(AppError::message(format!(
            "Factory reset is only supported for Pane-provisioned distros. '{}' is {}. Use `pane reset --release-managed-environment` instead.",
            environment.distro_name,
            environment.ownership.display_name()
        )));
    }

    Ok(Some(environment))
}

fn resolve_reset_distro_name(
    explicit: Option<&str>,
    managed_environment: &ManagedEnvironmentState,
) -> AppResult<String> {
    if let Some(name) = explicit {
        if !name.eq_ignore_ascii_case(&managed_environment.distro_name) {
            return Err(AppError::message(format!(
                "The requested reset target '{}' does not match the managed distro '{}'.",
                name, managed_environment.distro_name
            )));
        }

        return Ok(name.to_string());
    }

    Ok(managed_environment.distro_name.clone())
}

fn purge_wsl_integration(distro: &str, inventory: &WslInventory) -> AppResult<Vec<String>> {
    if !inventory.available || !inventory_contains_distro(inventory, distro) {
        return Ok(vec![format!(
            "No live WSL distro named '{}' was available for purge.",
            distro
        )]);
    }

    let mut notes = Vec::new();
    let stopped = wsl::stop_xrdp_services(distro)?;
    notes.push(format!("XRDP stop result: {}", stopped.trim()));

    let inspected = wsl::inspect_distro(distro, inventory)?;
    if let Some(user) = inspected
        .default_user
        .as_deref()
        .filter(|user| !user.eq_ignore_ascii_case("root"))
    {
        let result = wsl::remove_pane_session_assets(distro, user)?;
        notes.push(format!("Pane session asset cleanup: {}", result.trim()));
    } else {
        notes.push(
            "No non-root default user was available for Pane session-asset cleanup.".to_string(),
        );
    }

    Ok(notes)
}

fn managed_arch_name(saved_state: Option<&PaneState>) -> Option<String> {
    saved_state
        .and_then(|state| state.managed_environment.as_ref())
        .filter(|environment| environment.is_arch())
        .map(|environment| environment.distro_name.clone())
}

fn resolve_launch_target(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
    dry_run: bool,
) -> AppResult<LaunchTarget> {
    if let Some(name) = explicit {
        if inventory.available {
            if !inventory_contains_distro(inventory, name) {
                if dry_run {
                    return Ok(LaunchTarget {
                        distro: DistroRecord {
                            name: name.to_string(),
                            family: DistroFamily::Arch,
                            pretty_name: Some(name.to_string()),
                            ..DistroRecord::default()
                        },
                        hypothetical: true,
                    });
                }

                return Err(AppError::message(format!(
                    "WSL distro '{}' was not found. Available distros: {}",
                    name,
                    available_distros(inventory)
                )));
            }

            return Ok(LaunchTarget {
                distro: wsl::inspect_distro(name, inventory)?,
                hypothetical: false,
            });
        }

        if dry_run {
            return Ok(LaunchTarget {
                distro: DistroRecord {
                    name: name.to_string(),
                    family: DistroFamily::Arch,
                    pretty_name: Some(name.to_string()),
                    ..DistroRecord::default()
                },
                hypothetical: true,
            });
        }

        return Err(AppError::message(
            "wsl.exe was not found. Install WSL2 or rerun with --dry-run --distro <arch-distro-name>.",
        ));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        if inventory.available && inventory_contains_distro(inventory, &name) {
            return Ok(LaunchTarget {
                distro: wsl::inspect_distro(&name, inventory)?,
                hypothetical: false,
            });
        }

        if dry_run {
            return Ok(LaunchTarget {
                distro: DistroRecord {
                    name: name.clone(),
                    family: DistroFamily::Arch,
                    pretty_name: Some(name),
                    ..DistroRecord::default()
                },
                hypothetical: true,
            });
        }

        return Err(AppError::message(
            "Pane has a managed Arch environment configured, but it is not currently installed in WSL. Re-run `pane init` or pass --distro <arch-distro-name> to override it.",
        ));
    }

    if !inventory.available {
        return Err(AppError::message(
            "No WSL installation was found. Install WSL2, then run `pane onboard` or rerun with --dry-run --distro <arch-distro-name>.",
        ));
    }

    Err(AppError::message(format!(
        "Pane is not managing an Arch environment yet. Run `pane onboard` to create pane-arch, or run `pane init --existing-distro <arch-distro-name>` to adopt an existing Arch distro. Installed distros: {}",
        available_distros(inventory)
    )))
}

fn resolve_status_distro(
    explicit: Option<&str>,
    _inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(name.to_string()));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        return Ok(Some(name));
    }

    if let Some(name) = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| launch.distro.name.clone())
    {
        return Ok(Some(name));
    }

    Ok(None)
}

fn resolve_operational_distro(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<String> {
    resolve_status_distro(explicit, inventory, saved_state)?.ok_or_else(|| {
        AppError::message(
            "No WSL distro could be resolved. Run `pane doctor` or pass --distro <arch-distro-name>.",
        )
    })
}

fn evaluate_doctor(
    request: &DoctorRequest,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<DoctorReport> {
    let mut checks = Vec::new();
    let workspace = workspace_for(&request.session_name);
    let workspace_health = inspect_workspace(&workspace);
    let windows_host = cfg!(windows);

    push_check(
        &mut checks,
        if windows_host {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "windows-host",
        if windows_host {
            "Pane MVP is running on Windows.".to_string()
        } else {
            "Pane MVP currently supports Windows hosts only.".to_string()
        },
        (!windows_host).then_some(
            "Run Pane from Windows 10 or 11, then use WSL2 for the Linux side.".to_string(),
        ),
    );

    push_check(
        &mut checks,
        if inventory.available {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "wsl-available",
        if inventory.available {
            "wsl.exe is available on the Windows host.".to_string()
        } else {
            "WSL is not available on this host.".to_string()
        },
        (!inventory.available)
            .then_some("Install WSL2, install Arch Linux, then rerun `pane doctor`.".to_string()),
    );

    if request.write_probes_enabled {
        let workspace_status = if ensure_workspace_writable(&workspace) {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };
        push_check(
            &mut checks,
            workspace_status,
            "workspace-writable",
            if workspace_status == CheckStatus::Pass {
                format!("Pane can write assets under {}.", workspace.root.display())
            } else {
                format!(
                    "Pane could not write assets under {}.",
                    workspace.root.display()
                )
            },
            (workspace_status == CheckStatus::Fail).then_some(
                "Ensure your LOCALAPPDATA directory is writable, then rerun `pane doctor`."
                    .to_string(),
            ),
        );

        let shared_directory_status = if ensure_shared_dir_writable(&workspace) {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };
        push_check(
            &mut checks,
            shared_directory_status,
            "shared-directory-writable",
            if shared_directory_status == CheckStatus::Pass {
                format!(
                    "Pane can create the shared directory under {}.",
                    shared_dir_for_workspace(&workspace).display()
                )
            } else {
                format!(
                    "Pane could not create the shared directory under {}.",
                    shared_dir_for_workspace(&workspace).display()
                )
            },
            (shared_directory_status == CheckStatus::Fail).then_some(
                "Ensure your LOCALAPPDATA directory is writable so Pane can create the shared Windows-side workspace."
                    .to_string(),
            ),
        );
    } else {
        push_check(
            &mut checks,
            CheckStatus::Skipped,
            "workspace-writable",
            if workspace_health.root_exists {
                format!(
                    "Write probe skipped; existing Pane workspace is at {}.",
                    workspace.root.display()
                )
            } else {
                format!(
                    "Write probe skipped; Pane workspace does not exist yet at {}.",
                    workspace.root.display()
                )
            },
            Some(
                "Rerun `pane doctor` without `--no-write` or run `pane launch` when you want Pane to create and verify the workspace."
                    .to_string(),
            ),
        );

        let shared_dir = shared_dir_for_workspace(&workspace);
        push_check(
            &mut checks,
            CheckStatus::Skipped,
            "shared-directory-writable",
            if workspace_health.shared_dir_exists {
                format!(
                    "Shared-directory write probe skipped; existing PaneShared directory is at {}.",
                    shared_dir.display()
                )
            } else {
                format!(
                    "Shared-directory write probe skipped; PaneShared does not exist yet at {}.",
                    shared_dir.display()
                )
            },
            Some(
                "Rerun `pane doctor` without `--no-write` when you want Pane to create and verify PaneShared."
                    .to_string(),
            ),
        );
    }

    if request.connect_requested {
        let mstsc_status = if mstsc_available() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };
        push_check(
            &mut checks,
            mstsc_status,
            "mstsc-available",
            if mstsc_status == CheckStatus::Pass {
                "mstsc.exe is available for the Windows RDP handoff.".to_string()
            } else {
                "mstsc.exe was not found on the Windows host.".to_string()
            },
            (mstsc_status == CheckStatus::Fail).then_some(
                "Enable the built-in Remote Desktop Connection client or restore mstsc.exe on Windows.".to_string(),
            ),
        );
    }

    let target_name = select_doctor_target(request.distro.as_deref(), inventory, saved_state)?;
    let selected_distro = target_name
        .as_deref()
        .map(|name| build_distro_health(name, inventory, saved_state, Some(request.port)))
        .transpose()?;

    if let Some(name) = &target_name {
        let present_in_inventory = inventory_contains_distro(inventory, name);
        push_check(
            &mut checks,
            if inventory.available && present_in_inventory {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "distro-present",
            if inventory.available && present_in_inventory {
                format!("WSL distro '{name}' is installed.")
            } else {
                format!("WSL distro '{name}' is not installed.")
            },
            (!(inventory.available && present_in_inventory)).then_some(format!(
                "Install or import an Arch Linux distro, then rerun with `--distro {name}` if needed."
            )),
        );
    } else {
        push_check(
            &mut checks,
            CheckStatus::Fail,
            "distro-selected",
            "No WSL distro could be selected for the MVP path.".to_string(),
            Some(
                "Install Arch Linux for WSL, or pass --distro <arch-distro-name> to Pane once it exists.".to_string(),
            ),
        );
    }

    let desktop_supported = request.desktop_environment.is_mvp_supported();
    push_check(
        &mut checks,
        if desktop_supported {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "desktop-supported",
        if desktop_supported {
            format!(
                "{} is the supported MVP desktop environment.",
                request.desktop_environment.display_name()
            )
        } else {
            format!(
                "{} is not supported in the current MVP.",
                request.desktop_environment.display_name()
            )
        },
        (!desktop_supported).then_some(
            "Use `--de xfce`. KDE and GNOME are intentionally deferred until after the MVP."
                .to_string(),
        ),
    );

    if let Some(health) = &selected_distro {
        push_check(
            &mut checks,
            if health.supported_for_mvp {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "distro-supported",
            if health.supported_for_mvp {
                format!(
                    "{} matches the Arch Linux MVP support boundary.",
                    health.distro.label()
                )
            } else {
                format!(
                    "{} is not supported by the current MVP.",
                    health.distro.label()
                )
            },
            (!health.supported_for_mvp).then_some(
                "Install Arch Linux for WSL and rerun Pane against that distro.".to_string(),
            ),
        );

        let default_user_ok = health
            .distro
            .default_user
            .as_deref()
            .is_some_and(|user| !user.eq_ignore_ascii_case("root"));
        push_check(
            &mut checks,
            if default_user_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "default-user",
            if default_user_ok {
                format!(
                    "The default WSL user is '{}'.",
                    health.distro.default_user.as_deref().unwrap_or_default()
                )
            } else {
                "The default WSL user is missing or still set to root.".to_string()
            },
            (!default_user_ok).then_some(format!(
                "Use the Pane Control Center's Setup User flow or run `pane setup-user --username <linux-user> --password-stdin` for {} before using Pane.",
                health.distro.name
            )),
        );

        let password_status = health.default_user_password_status;
        let password_ok = password_status.is_some_and(PasswordStatus::is_usable);
        push_check(
            &mut checks,
            if password_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "default-user-password",
            if let Some(status) = password_status {
                format!(
                    "The default user password state is {}.",
                    status.display_name()
                )
            } else {
                "Pane could not verify the default user password state.".to_string()
            },
            (!password_ok).then_some(format!(
                "Use the Pane Control Center's Setup User flow or rerun `pane setup-user --username <linux-user> --password-stdin --distro {}` so XRDP has a usable login password.",
                health.distro.name
            )),
        );

        let systemd_configured = health.systemd_configured == Some(true);
        push_check(
            &mut checks,
            if systemd_configured {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "systemd-configured",
            if systemd_configured {
                "`/etc/wsl.conf` advertises systemd=true.".to_string()
            } else {
                "`/etc/wsl.conf` does not yet advertise systemd=true.".to_string()
            },
            (!systemd_configured).then_some(format!(
                "Use the Pane Control Center's Setup User flow or run `pane setup-user --username <linux-user> --password-stdin --distro {}` to write systemd=true and restart WSL.",
                health.distro.name
            )),
        );

        let systemd_active = wsl::distro_systemd_active(&health.distro.name) == Some(true);
        push_check(
            &mut checks,
            if systemd_active {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "systemd-active",
            if systemd_active {
                "systemd is active in the running WSL instance.".to_string()
            } else {
                "systemd is not active in the running WSL instance.".to_string()
            },
            (!systemd_active).then_some(
                "After enabling systemd in /etc/wsl.conf, run `wsl --shutdown` and start the distro again before retrying Pane.".to_string(),
            ),
        );

        if !request.bootstrap_requested {
            push_check(
                &mut checks,
                if health.xrdp_installed == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-installed",
                if health.xrdp_installed == Some(true) {
                    "XRDP is installed inside the distro.".to_string()
                } else {
                    "XRDP is not installed inside the distro.".to_string()
                },
                Some(
                    "Run `pane launch` without `--skip-bootstrap` to install and configure XRDP."
                        .to_string(),
                ),
            );
            push_check(
                &mut checks,
                if health.pane_session_assets_ready == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "pane-session-assets",
                if health.pane_session_assets_ready == Some(true) {
                    "Pane-managed XRDP session assets are present for the default user.".to_string()
                } else {
                    "Pane-managed XRDP session assets are missing or stale for the default user.".to_string()
                },
                Some("Run `pane repair` or `pane launch` to rewrite the Pane-managed session launcher, XRDP user files, and notifyd override.".to_string()),
            );
            push_check(
                &mut checks,
                if health.user_home_ready == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "user-home-ready",
                if health.user_home_ready == Some(true) {
                    "The default user owns and can write the required XFCE config/cache directories.".to_string()
                } else {
                    "The default user home/config layout is missing required directories or is not writable by the Linux user.".to_string()
                },
                Some("Run `pane repair` to recreate and re-own the Pane-managed XFCE config, cache, and local-state directories.".to_string()),
            );
            push_check(
                &mut checks,
                if health.xrdp_service_active == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-active",
                if health.xrdp_service_active == Some(true) {
                    "The XRDP service is active inside WSL.".to_string()
                } else {
                    "The XRDP service is not active inside WSL.".to_string()
                },
                Some("Run `pane launch` or `pane stop` followed by `pane launch` to restart XRDP cleanly.".to_string()),
            );
            push_check(
                &mut checks,
                if health.xrdp_listening == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-listening",
                if health.xrdp_listening == Some(true) {
                    format!("XRDP is listening on port {} inside WSL.", request.port)
                } else {
                    format!("XRDP is not listening on port {} inside WSL.", request.port)
                },
                Some(
                    "Run `pane logs` and inspect the bootstrap transcript or XRDP service logs."
                        .to_string(),
                ),
            );
            if request.connect_requested {
                if let Some(check) = windows_transport_check(request.port, health) {
                    checks.push(check);
                }
            }
        }
    }

    let supported_for_mvp = selected_distro
        .as_ref()
        .is_some_and(|health| health.supported_for_mvp)
        && request.desktop_environment.is_mvp_supported();
    let ready = !checks.iter().any(|check| check.status == CheckStatus::Fail);

    Ok(DoctorReport {
        target_distro: target_name,
        session_name: request.session_name.clone(),
        desktop_environment: request.desktop_environment,
        port: request.port,
        bootstrap_requested: request.bootstrap_requested,
        connect_requested: request.connect_requested,
        write_probes_enabled: request.write_probes_enabled,
        supported_for_mvp,
        ready,
        selected_distro,
        workspace: workspace_health,
        checks,
    })
}

fn select_doctor_target(
    explicit: Option<&str>,
    _inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(name.to_string()));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        return Ok(Some(name));
    }

    if let Some(name) = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| launch.distro.name.clone())
    {
        return Ok(Some(name));
    }

    Ok(None)
}

fn build_distro_health(
    name: &str,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
    explicit_port: Option<u16>,
) -> AppResult<DistroHealth> {
    let present_in_inventory = inventory_contains_distro(inventory, name);
    let checked_port = explicit_port.unwrap_or_else(|| status_port_for(name, saved_state));
    let fallback_record = saved_state
        .and_then(|state| {
            state
                .managed_environment
                .as_ref()
                .filter(|environment| environment.distro_name.eq_ignore_ascii_case(name))
                .map(|environment| DistroRecord {
                    name: environment.distro_name.clone(),
                    family: environment.family,
                    pretty_name: Some(environment.distro_name.clone()),
                    ..DistroRecord::default()
                })
                .or_else(|| {
                    state
                        .last_launch
                        .as_ref()
                        .filter(|launch| launch.distro.name.eq_ignore_ascii_case(name))
                        .map(|launch| launch.distro.clone())
                })
        })
        .unwrap_or_else(|| DistroRecord {
            name: name.to_string(),
            family: DistroFamily::Unknown,
            ..DistroRecord::default()
        });

    if inventory.available && present_in_inventory {
        let distro = wsl::inspect_distro(name, inventory)?;
        let xrdp_listening = wsl::distro_port_listening(name, checked_port);
        let localhost_reachable = Some(local_port_reachable(checked_port));
        let wsl_ip = wsl::distro_ipv4_address(name);
        let wsl_ip_reachable = wsl_ip
            .map(|ip| SocketAddr::from((ip, checked_port)))
            .map(socket_reachable);
        let pane_relay_available = Some(wsl_ip.is_some() || wsl::distro_command_exists(name, "nc"));
        let pane_default_user = distro
            .default_user
            .as_deref()
            .and_then(|user| (!user.eq_ignore_ascii_case("root")).then_some(user));
        return Ok(DistroHealth {
            supported_for_mvp: distro.is_mvp_supported(),
            present_in_inventory,
            checked_port,
            systemd_configured: wsl::distro_systemd_configured(name),
            xrdp_installed: Some(wsl::distro_command_exists(name, "xrdp")),
            xrdp_service_active: wsl::distro_service_active(name, "xrdp"),
            xrdp_listening,
            localhost_reachable,
            pane_relay_available,
            preferred_transport: preferred_transport(
                xrdp_listening,
                localhost_reachable,
                wsl_ip_reachable,
                pane_relay_available,
            ),
            xsession_present: Some(wsl::distro_file_exists(name, ".xsession")),
            pane_session_assets_ready: pane_default_user
                .and_then(|user| wsl::distro_pane_session_assets_ready(name, user)),
            user_home_ready: pane_default_user
                .and_then(|user| wsl::distro_user_home_ready(name, user)),
            default_user_password_status: pane_default_user
                .and_then(|user| wsl::distro_user_password_status(name, user)),
            distro,
        });
    }

    Ok(DistroHealth {
        supported_for_mvp: fallback_record.is_mvp_supported(),
        distro: fallback_record,
        present_in_inventory,
        checked_port,
        systemd_configured: None,
        xrdp_installed: None,
        xrdp_service_active: None,
        xrdp_listening: None,
        localhost_reachable: None,
        pane_relay_available: None,
        preferred_transport: None,
        xsession_present: None,
        pane_session_assets_ready: None,
        user_home_ready: None,
        default_user_password_status: None,
    })
}

fn preferred_transport(
    xrdp_listening: Option<bool>,
    localhost_reachable: Option<bool>,
    wsl_ip_reachable: Option<bool>,
    pane_relay_available: Option<bool>,
) -> Option<LaunchTransport> {
    if xrdp_listening != Some(true) {
        return None;
    }

    if localhost_reachable == Some(true) {
        Some(LaunchTransport::DirectLocalhost)
    } else if wsl_ip_reachable == Some(true) {
        Some(LaunchTransport::DirectWslIp)
    } else if pane_relay_available == Some(true) {
        Some(LaunchTransport::PaneRelay)
    } else {
        None
    }
}

fn windows_transport_check(port: u16, health: &DistroHealth) -> Option<DoctorCheck> {
    if health.xrdp_listening != Some(true) {
        return None;
    }

    Some(DoctorCheck {
        id: "windows-transport".to_string(),
        status: if health.preferred_transport.is_some() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        summary: match health.preferred_transport {
            Some(LaunchTransport::DirectLocalhost) => {
                format!("Windows can reach localhost:{} directly.", port)
            }
            Some(LaunchTransport::DirectWslIp) => {
                format!("Windows can reach the distro IP directly on port {}.", port)
            }
            Some(LaunchTransport::PaneRelay) => {
                format!("Pane will bridge localhost:{} with pane-relay.", port)
            }
            None => format!(
                "Windows cannot reach localhost:{}, the distro IP directly, or pane-relay inside WSL.",
                port
            ),
        },
        remediation: if health.preferred_transport.is_some() {
            None
        } else {
            Some(
                "Run `pane repair` or `pane launch` to restore the Pane relay path and WSL networking, then retry the connection."
                    .to_string(),
            )
        },
    })
}

fn build_steps(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_enabled: bool,
    connect_enabled: bool,
) -> Vec<String> {
    let mut steps = vec![
        format!(
            "Generate a {} bootstrap script for {}.",
            desktop_environment.display_name(),
            distro.label()
        ),
        format!(
            "Write an RDP profile that can target localhost, the distro IP, or pane-relay on port {port}."
        ),
        "Prepare PaneShared storage that appears inside Arch as ~/PaneShared."
            .to_string(),
        "Run preflight diagnostics that block unsupported or broken MVP setups.".to_string(),
    ];

    if bootstrap_enabled {
        steps.push(format!(
            "Run the bootstrap script inside {} as root and write ~/.xsession for the default WSL user.",
            distro.name
        ));
        steps.push("Wait for XRDP to listen before reporting success.".to_string());
    }

    if connect_enabled {
        steps.push("Launch mstsc.exe with the generated RDP profile.".to_string());
    }

    steps
}

fn build_update_steps(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
) -> Vec<String> {
    vec![
        format!(
            "Generate a {} update script for {}.",
            desktop_environment.display_name(),
            distro.label()
        ),
        format!(
            "Refresh Arch packages and reapply the Pane-managed desktop integration inside {}.",
            distro.name
        ),
        format!("Write an RDP profile that can target localhost, the distro IP, or pane-relay on port {port}."),
        "Prepare PaneShared storage that appears inside Arch as ~/PaneShared."
            .to_string(),
        "Run preflight diagnostics that block unsupported or broken MVP setups.".to_string(),
        format!(
            "Run the update script inside {} as root to refresh packages and restore Pane session wiring.",
            distro.name
        ),
        "Wait for XRDP to listen before reporting success.".to_string(),
    ]
}

fn execute_bootstrap(plan: &LaunchPlan) -> AppResult<()> {
    let target_user = plan.distro.default_user.as_deref().unwrap_or("root");
    let script_path = windows_to_wsl_path(&plan.workspace.bootstrap_script);
    let shared_directory = windows_to_wsl_path(&shared_dir_for_workspace(&plan.workspace));
    let command = format!(
        "chmod +x {script} && PANE_TARGET_USER={user} PANE_SHARED_DIR={shared} {script}",
        script = shell_quote(&script_path),
        user = shell_quote(target_user),
        shared = shell_quote(&shared_directory),
    );

    let transcript = run_wsl_shell_as_user_capture(&plan.distro.name, Some("root"), &command)?;
    write_bootstrap_log(&plan.workspace.bootstrap_log, plan, &command, &transcript)?;

    if transcript.success {
        Ok(())
    } else {
        Err(AppError::message(format!(
            "Bootstrap failed for {}. Review {} for details.",
            plan.distro.name,
            plan.workspace.bootstrap_log.display()
        )))
    }
}

fn write_runtime_rdp_profile(
    profile_path: &Path,
    distro: &DistroRecord,
    host: &str,
    port: u16,
) -> AppResult<()> {
    fs::write(profile_path, render_rdp_profile(distro, host, port)).map_err(|error| {
        AppError::message(format!(
            "failed to write the runtime RDP profile at {}: {error}",
            profile_path.display()
        ))
    })
}

fn open_rdp_profile(profile_path: &Path) -> AppResult<()> {
    Command::new("mstsc.exe")
        .arg(profile_path)
        .spawn()
        .map_err(|error| {
            AppError::message(format!(
                "failed to launch mstsc.exe for {}: {error}",
                profile_path.display()
            ))
        })?;

    Ok(())
}

fn open_directory_in_explorer(path: &Path) -> AppResult<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AppError::message(format!(
                "failed to launch explorer.exe for {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

fn fail_launch(stored_launch: &mut StoredLaunch, error: AppError) -> AppError {
    stored_launch.mark_failed(error.to_string());
    let _ = save_state_record(stored_launch.clone());
    error
}

fn build_environment_catalog_report() -> EnvironmentCatalogReport {
    EnvironmentCatalogReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        strategy: "Arch is the current flagship. Ubuntu LTS is next as the second first-class managed environment. Debian follows later as a curated preview environment.",
        environments: managed_environment_catalog(),
        notes: vec![
            "Arch Linux is the current first-class managed environment and the reference path for Pane.".to_string(),
            "Ubuntu LTS is next because it broadens adoption without changing the product model.".to_string(),
            "Debian follows later as a curated preview once distro lifecycle ownership is stronger.".to_string(),
            "Kali and wider distro imports are intentionally not part of the first three managed environments.".to_string(),
        ],
    }
}

fn resolve_saved_launch(
    session_name: Option<&str>,
    saved_state: Option<&PaneState>,
) -> AppResult<StoredLaunch> {
    let Some(launch) = saved_state.and_then(|state| state.last_launch.clone()) else {
        return Err(AppError::message(
            "Pane has no saved launch state yet. Run `pane launch` first.",
        ));
    };

    if let Some(expected_session) = session_name {
        let normalized = crate::plan::sanitize_session_name(expected_session);
        if launch.session_name != normalized {
            return Err(AppError::message(format!(
                "Pane only tracks one active session in the MVP. The saved session is '{}', not '{}'.",
                launch.session_name,
                normalized
            )));
        }
    }

    Ok(launch)
}

fn inventory_contains_distro(inventory: &WslInventory, name: &str) -> bool {
    inventory
        .distros
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case(name))
}

fn available_distros(inventory: &WslInventory) -> String {
    if inventory.distros.is_empty() {
        "none".to_string()
    } else {
        inventory
            .distros
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn status_port_for(name: &str, saved_state: Option<&PaneState>) -> u16 {
    saved_state
        .and_then(|state| state.last_launch.as_ref())
        .filter(|launch| launch.distro.name.eq_ignore_ascii_case(name))
        .map(|launch| launch.port)
        .unwrap_or(3390)
}

fn inspect_workspace(workspace: &WorkspacePaths) -> WorkspaceHealth {
    WorkspaceHealth {
        root_exists: workspace.root.exists(),
        shared_dir_exists: shared_dir_for_workspace(workspace).exists(),
        bootstrap_script_exists: workspace.bootstrap_script.exists(),
        rdp_profile_exists: workspace.rdp_profile.exists(),
        bootstrap_log_exists: workspace.bootstrap_log.exists(),
        transport_log_exists: workspace.transport_log.exists(),
    }
}

fn ensure_workspace_writable(workspace: &WorkspacePaths) -> bool {
    fs::create_dir_all(&workspace.root).is_ok()
}

fn ensure_shared_dir_writable(workspace: &WorkspacePaths) -> bool {
    fs::create_dir_all(shared_dir_for_workspace(workspace)).is_ok()
}

fn validate_setup_username(username: &str) -> AppResult<()> {
    if username.eq_ignore_ascii_case("root") {
        return Err(AppError::message(
            "Pane setup-user only supports regular Linux users. Choose a non-root username.",
        ));
    }

    let mut chars = username.chars();
    let Some(first) = chars.next() else {
        return Err(AppError::message(
            "Pane setup-user requires --username <linux-user>.",
        ));
    };
    if !first.is_ascii_lowercase() && first != '_' {
        return Err(AppError::message(
            "Linux usernames must start with a lowercase letter or underscore.",
        ));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
        return Err(AppError::message(
            "Linux usernames may only contain lowercase letters, digits, underscores, and dashes.",
        ));
    }
    Ok(())
}

fn validate_setup_password(password: &str) -> AppResult<()> {
    if password.is_empty() {
        return Err(AppError::message(
            "Pane setup-user requires a non-empty password.",
        ));
    }
    if password.contains(':') {
        return Err(AppError::message(
            "Pane setup-user passwords cannot contain ':' because the password is passed to chpasswd.",
        ));
    }
    if password.contains('\n') || password.contains('\r') {
        return Err(AppError::message(
            "Pane setup-user passwords cannot contain line breaks.",
        ));
    }
    Ok(())
}

fn resolve_setup_user_password(args: &SetupUserArgs) -> AppResult<Option<String>> {
    if args.dry_run {
        return Ok(args.password.clone());
    }

    if args.password.is_some() && args.password_stdin {
        return Err(AppError::message(
            "Pass the password either with --password or --password-stdin, not both.",
        ));
    }

    let password = if args.password_stdin {
        let mut raw = String::new();
        std::io::stdin().read_to_string(&mut raw)?;
        raw.trim_end_matches(['\r', '\n']).to_string()
    } else {
        args.password.clone().ok_or_else(|| {
            AppError::message(
                "Provide a password with --password-stdin or --password when using pane setup-user.",
            )
        })?
    };

    validate_setup_password(&password)?;
    Ok(Some(password))
}

fn build_setup_user_shell_command(username: &str) -> String {
    format!(
        "set -euo pipefail\n\n\
         if ! id -u {username} >/dev/null 2>&1; then\n\
           useradd -m -s /bin/bash {username}\n\
         else\n\
           if command -v chsh >/dev/null 2>&1; then\n\
             chsh -s /bin/bash {username} >/dev/null 2>&1 || true\n\
           fi\n\
         fi\n\
         chpasswd\n",
        username = username
    )
}

fn ensure_wsl_conf_setting(raw: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines = Vec::new();
    let mut section_found = false;
    let mut in_target_section = false;
    let mut key_written_in_section = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(section_name) = parse_ini_section(trimmed) {
            if in_target_section && !key_written_in_section {
                lines.push(format!("{key}={value}"));
            }
            in_target_section = section_name.eq_ignore_ascii_case(section);
            if in_target_section {
                section_found = true;
                key_written_in_section = false;
            }
            lines.push(line.to_string());
            continue;
        }

        if in_target_section {
            if let Some((existing_key, _)) = trimmed.split_once('=') {
                if existing_key.trim().eq_ignore_ascii_case(key) {
                    if !key_written_in_section {
                        lines.push(format!("{key}={value}"));
                        key_written_in_section = true;
                    }
                    continue;
                }
            }
        }

        lines.push(line.to_string());
    }

    if section_found {
        if in_target_section && !key_written_in_section {
            lines.push(format!("{key}={value}"));
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(format!("{key}={value}"));
    }

    let mut rendered = lines.join("\n");
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn parse_ini_section(line: &str) -> Option<&str> {
    line.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .map(str::trim)
        .filter(|section| !section.is_empty())
}

fn print_launch_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane MVP Launch Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_repair_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane Repair Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("  Outcome        Reapply Pane-managed Arch integration without opening mstsc.exe");
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_update_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane Update Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("  Outcome        Refresh Arch packages and reapply Pane-managed integration without opening mstsc.exe");
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_init_report(report: &InitReport) {
    println!("Pane Init");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!("  In Inventory   {}", yes_no(report.present_in_inventory));
    println!("Managed Environment");
    println!(
        "  Id             {}",
        report.managed_environment.environment_id
    );
    println!(
        "  Distro         {}",
        report.managed_environment.distro_name
    );
    println!(
        "  Family         {}",
        report.managed_environment.family.display_name()
    );
    println!(
        "  Ownership      {}",
        report.managed_environment.ownership.display_name()
    );
    if let Some(install_dir) = &report.managed_environment.install_dir {
        println!("  Install Dir    {}", install_dir.display());
    }
    if let Some(rootfs) = &report.managed_environment.source_rootfs {
        println!("  Rootfs Tar     {}", rootfs.display());
    }
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}

fn print_onboard_report(report: &OnboardReport) {
    println!("Pane Onboarding");
    println!("  Product Shape  {}", report.product_shape);
    println!("Managed Environment");
    println!(
        "  Distro         {}",
        report.managed_environment.distro_name
    );
    println!(
        "  Family         {}",
        report.managed_environment.family.display_name()
    );
    println!(
        "  Ownership      {}",
        report.managed_environment.ownership.display_name()
    );
    if let Some(install_dir) = &report.managed_environment.install_dir {
        println!("  Install Dir    {}", install_dir.display());
    }
    if let Some(rootfs) = &report.managed_environment.source_rootfs {
        println!("  Rootfs Tar     {}", rootfs.display());
    }
    println!("Setup User");
    println!("  Username       {}", report.setup_user.username);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!(
        "  Password       {}",
        yes_no(report.setup_user.password_updated)
    );
    println!(
        "  Default User   {}",
        yes_no(report.setup_user.default_user_configured)
    );
    println!(
        "  systemd=true   {}",
        yes_no(report.setup_user.systemd_configured)
    );
    println!(
        "  WSL Shutdown   {}",
        yes_no(report.setup_user.wsl_shutdown)
    );
    println!("Ready For Launch {}", yes_no(report.ready_for_launch));
    if let Some(readiness) = &report.launch_readiness {
        println!("Launch Readiness");
        println!("  Ready          {}", yes_no(readiness.ready));
        println!("  Supported MVP  {}", yes_no(readiness.supported_for_mvp));
        println!(
            "  Target Distro  {}",
            readiness.target_distro.as_deref().unwrap_or("unresolved")
        );
        println!("  Bootstrap      {}", yes_no(readiness.bootstrap_requested));
        println!("  Connect        {}", yes_no(readiness.connect_requested));
        for check in readiness
            .checks
            .iter()
            .filter(|check| check.status == CheckStatus::Fail)
        {
            println!("  Failure        [{}] {}", check.id, check.summary);
        }
    }
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}
fn print_setup_user_report(report: &SetupUserReport) {
    println!("Pane Setup User");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Distro         {}", report.distro);
    println!("  Username       {}", report.username);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!("  Password       {}", yes_no(report.password_updated));
    println!(
        "  Default User   {}",
        yes_no(report.default_user_configured)
    );
    println!("  systemd=true   {}", yes_no(report.systemd_configured));
    println!("  WSL Shutdown   {}", yes_no(report.wsl_shutdown));
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}

fn print_status_report(report: &StatusReport) {
    println!("Pane Status");
    println!("  Platform       {}", report.platform);
    println!(
        "  WSL Available  {}",
        if report.wsl_available { "yes" } else { "no" }
    );
    if let Some(version) = &report.wsl_version_banner {
        println!("  WSL Version    {version}");
    }
    if let Some(default_distro) = &report.wsl_default_distro {
        println!("  WSL Default    {default_distro}");
    }
    println!("  Known Distros  {}", report.known_distros.len());

    if let Some(managed_environment) = &report.managed_environment {
        println!("Managed Environment");
        println!("  Id             {}", managed_environment.environment_id);
        println!("  Distro         {}", managed_environment.distro_name);
        println!(
            "  Family         {}",
            managed_environment.family.display_name()
        );
        println!(
            "  Ownership      {}",
            managed_environment.ownership.display_name()
        );
        if let Some(install_dir) = &managed_environment.install_dir {
            println!("  Install Dir    {}", install_dir.display());
        }
        if let Some(rootfs) = &managed_environment.source_rootfs {
            println!("  Rootfs Tar     {}", rootfs.display());
        }
    }

    if let Some(distro) = &report.selected_distro {
        println!("Selected Distro");
        println!("  Name           {}", distro.distro.label());
        println!("  In Inventory   {}", yes_no(distro.present_in_inventory));
        println!("  Supported MVP  {}", yes_no(distro.supported_for_mvp));
        println!("  Family         {}", distro.distro.family.display_name());
        println!("  Checked Port   {}", distro.checked_port);
        if let Some(user) = &distro.distro.default_user {
            println!("  Default User   {user}");
        }
        if let Some(status) = distro.default_user_password_status {
            println!("  Password       {}", status.display_name());
        }
        if let Some(systemd) = distro.systemd_configured {
            println!("  systemd=true   {}", yes_no(systemd));
        }
        if let Some(installed) = distro.xrdp_installed {
            println!("  XRDP Installed {}", yes_no(installed));
        }
        if let Some(active) = distro.xrdp_service_active {
            println!("  XRDP Active    {}", yes_no(active));
        }
        if let Some(listening) = distro.xrdp_listening {
            println!("  XRDP Listening {}", yes_no(listening));
        }
        if let Some(reachable) = distro.localhost_reachable {
            println!("  localhost Port {}", yes_no(reachable));
        }
        if let Some(relay) = distro.pane_relay_available {
            println!("  Pane Relay     {}", yes_no(relay));
        }
        if let Some(transport) = distro.preferred_transport {
            println!("  Transport      {}", transport.display_name());
        }
        if let Some(xsession) = distro.xsession_present {
            println!("  .xsession      {}", yes_no(xsession));
        }
        if let Some(assets_ready) = distro.pane_session_assets_ready {
            println!("  Session Assets {}", yes_no(assets_ready));
        }
        if let Some(home_ready) = distro.user_home_ready {
            println!("  Home Ready     {}", yes_no(home_ready));
        }
    }

    if let Some(last_launch) = &report.last_launch {
        println!("Last Launch");
        println!("  Session        {}", last_launch.session_name);
        println!("  Distro         {}", last_launch.distro.label());
        println!(
            "  Desktop        {}",
            last_launch.desktop_environment.display_name()
        );
        println!("  Stage          {}", last_launch.stage.display_name());
        println!("  Dry Run        {}", yes_no(last_launch.dry_run));
        println!("  Hypothetical   {}", yes_no(last_launch.hypothetical));
        println!("  Port           {}", last_launch.port);
        if let Some(transport) = last_launch.transport {
            println!("  Transport      {}", transport.display_name());
        }
        if let Some(error) = &last_launch.last_error {
            println!("  Last Error     {error}");
        }
        if let Some(workspace) = &report.last_launch_workspace {
            println!("Workspace Assets");
            println!("  Root           {}", yes_no(workspace.root_exists));
            println!("  Shared Dir     {}", yes_no(workspace.shared_dir_exists));
            println!(
                "  Bootstrap      {}",
                yes_no(workspace.bootstrap_script_exists)
            );
            println!("  RDP Profile    {}", yes_no(workspace.rdp_profile_exists));
            println!(
                "  Bootstrap Log  {}",
                yes_no(workspace.bootstrap_log_exists)
            );
            println!(
                "  Transport Log  {}",
                yes_no(workspace.transport_log_exists)
            );
        }
    }
}

fn print_app_status_report(report: &AppStatusReport) {
    println!("Pane App Status");
    println!("  Session        {}", report.session_name);
    println!("  Phase          {}", report.phase.display_name());
    println!("  Next Action    {}", report.next_action_label);
    println!("  Summary        {}", report.next_action_summary);
    println!("  Profile        {}", report.supported_profile.label);
    println!("Runtime");
    println!("  Current Engine {}", report.runtime.current_engine_label);
    println!("  Target Engine  {}", report.runtime.target_engine_label);
    println!("  Prepared       {}", yes_no(report.runtime.prepared));
    println!(
        "  Host Ready     {}",
        yes_no(report.runtime.native_runtime.host_ready)
    );
    println!(
        "  Boot Spike     {}",
        yes_no(report.runtime.native_runtime.ready_for_boot_spike)
    );
    println!("  Dedicated Root {}", report.runtime.dedicated_space_root);
    println!(
        "  Capacity       {} GiB",
        report.runtime.storage_budget.requested_capacity_gib
    );
    println!("  Storage        {}", report.storage.policy);
    println!("  Durable Shared {}", report.storage.durable_shared_dir);
    println!("  Scratch Shared {}", report.storage.scratch_shared_dir);
    println!("Display");
    println!("  Current        {}", report.display.current_mode_label);
    println!(
        "  Contained      {}",
        yes_no(report.display.contained_window_available)
    );
    println!(
        "  Visible Handoff {}",
        yes_no(report.display.user_visible_handoff)
    );
    if let Some(environment) = &report.managed_environment {
        println!("Managed Environment");
        println!("  Distro         {}", environment.distro_name);
        println!("  Family         {}", environment.family.display_name());
        println!("  Ownership      {}", environment.ownership.display_name());
    }
    if let Some(distro) = &report.selected_distro {
        println!("Selected Distro");
        println!("  Name           {}", distro.distro.label());
        println!("  Supported MVP  {}", yes_no(distro.supported_for_mvp));
        if let Some(user) = &distro.distro.default_user {
            println!("  Default User   {user}");
        }
        if let Some(password) = distro.default_user_password_status {
            println!("  Password       {}", password.display_name());
        }
    }
    if let Some(launch) = &report.last_launch {
        println!("Last Launch");
        println!("  Stage          {}", launch.stage.display_name());
        println!("  Port           {}", launch.port);
        if let Some(transport) = launch.transport {
            println!("  Transport      {}", transport.display_name());
        }
        if let Some(error) = &launch.last_error {
            println!("  Last Error     {error}");
        }
    }
    if !report.blockers.is_empty() {
        println!("Blockers");
        for blocker in &report.blockers {
            println!("  [{}] {}", blocker.id, blocker.summary);
            if let Some(remediation) = &blocker.remediation {
                println!("       fix: {remediation}");
            }
        }
    }
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}

fn print_runtime_report(report: &RuntimeReport) {
    println!("Pane Runtime");
    println!("  Session        {}", report.session_name);
    println!("  Current Engine {}", report.current_engine_label);
    println!("  Target Engine  {}", report.target_engine_label);
    println!("  Prepared       {}", yes_no(report.prepared));
    println!("  Dedicated Root {}", report.dedicated_space_root);
    println!(
        "  Capacity       {} GiB",
        report.storage_budget.requested_capacity_gib
    );
    println!(
        "  Base OS Budget {} GiB",
        report.storage_budget.base_os_budget_gib
    );
    println!(
        "  User Budget    {} GiB",
        report.storage_budget.user_packages_and_customizations_gib
    );
    println!(
        "  Snapshot Budget {} GiB",
        report.storage_budget.snapshot_budget_gib
    );
    println!("Directories");
    println!("  Downloads      {}", report.directories.downloads);
    println!("  Images         {}", report.directories.images);
    println!("  Disks          {}", report.directories.disks);
    println!("  Snapshots      {}", report.directories.snapshots);
    println!("  State          {}", report.directories.state);
    println!("  Engines        {}", report.directories.engines);
    println!("  Logs           {}", report.directories.logs);
    println!("  Base OS Image  {}", report.directories.base_os_image);
    println!("  Serial Boot    {}", report.directories.serial_boot_image);
    println!("  Boot Loader    {}", report.directories.boot_loader_image);
    println!("  Kernel Image   {}", report.directories.kernel_image);
    println!("  Initramfs      {}", report.directories.initramfs_image);
    println!("  User Disk      {}", report.directories.user_disk);
    println!("  Base Metadata  {}", report.directories.base_os_metadata);
    println!(
        "  Serial Metadata {}",
        report.directories.serial_boot_metadata
    );
    println!(
        "  Loader Metadata {}",
        report.directories.boot_loader_metadata
    );
    println!(
        "  Kernel Metadata {}",
        report.directories.kernel_boot_metadata
    );
    println!("  Kernel Layout {}", report.directories.kernel_boot_layout);
    println!(
        "  Framebuffer    {}",
        report.directories.framebuffer_contract
    );
    println!("  Input          {}", report.directories.input_contract);
    println!("  Disk Metadata  {}", report.directories.user_disk_metadata);
    println!("  Runtime Config {}", report.directories.runtime_config);
    println!("  Native Manifest {}", report.directories.native_manifest);
    println!("  Manifest       {}", report.directories.manifest);
    println!("Artifacts");
    println!(
        "  Base Image     {}",
        yes_no(report.artifacts.base_os_image_exists)
    );
    println!(
        "  Base Verified  {}",
        yes_no(report.artifacts.base_os_image_verified)
    );
    if let Some(sha256) = &report.artifacts.base_os_image_sha256 {
        println!("  Base SHA-256   {}", sha256);
    }
    if let Some(bytes) = report.artifacts.base_os_image_bytes {
        println!("  Base Bytes     {}", bytes);
    }
    println!(
        "  Serial Boot    {}",
        yes_no(report.artifacts.serial_boot_image_exists)
    );
    println!(
        "  Serial Ready   {}",
        yes_no(report.artifacts.serial_boot_image_ready)
    );
    if let Some(sha256) = &report.artifacts.serial_boot_image_sha256 {
        println!("  Serial SHA-256 {}", sha256);
    }
    if let Some(banner) = &report.artifacts.serial_boot_banner {
        println!("  Serial Banner  {:?}", banner);
    }
    if let Some(bytes) = report.artifacts.serial_boot_image_bytes {
        println!("  Serial Bytes   {}", bytes);
    }
    println!(
        "  Boot Loader    {}",
        yes_no(report.artifacts.boot_loader_image_exists)
    );
    println!(
        "  Loader Verified {}",
        yes_no(report.artifacts.boot_loader_image_verified)
    );
    if let Some(sha256) = &report.artifacts.boot_loader_image_sha256 {
        println!("  Loader SHA-256 {}", sha256);
    }
    if let Some(expected) = &report.artifacts.boot_loader_expected_serial {
        println!("  Loader Serial  {:?}", expected);
    }
    if let Some(bytes) = report.artifacts.boot_loader_image_bytes {
        println!("  Loader Bytes   {}", bytes);
    }
    println!(
        "  Kernel Image   {}",
        yes_no(report.artifacts.kernel_image_exists)
    );
    println!(
        "  Kernel Verified {}",
        yes_no(report.artifacts.kernel_image_verified)
    );
    if let Some(sha256) = &report.artifacts.kernel_image_sha256 {
        println!("  Kernel SHA-256 {}", sha256);
    }
    if let Some(bytes) = report.artifacts.kernel_image_bytes {
        println!("  Kernel Bytes   {}", bytes);
    }
    if let Some(format) = &report.artifacts.kernel_format {
        println!("  Kernel Format  {}", format);
    }
    if let Some(protocol) = &report.artifacts.kernel_linux_boot_protocol {
        println!("  Linux Protocol {}", protocol);
    }
    if let Some(offset) = report.artifacts.kernel_linux_protected_mode_offset {
        println!("  Linux PM Offset {}", offset);
    }
    if let Some(bytes) = report.artifacts.kernel_linux_protected_mode_bytes {
        println!("  Linux PM Bytes {}", bytes);
    }
    println!(
        "  Initramfs      {}",
        yes_no(report.artifacts.initramfs_image_exists)
    );
    println!(
        "  Initramfs Verified {}",
        yes_no(report.artifacts.initramfs_image_verified)
    );
    if let Some(sha256) = &report.artifacts.initramfs_image_sha256 {
        println!("  Initramfs SHA-256 {}", sha256);
    }
    if let Some(bytes) = report.artifacts.initramfs_image_bytes {
        println!("  Initramfs Bytes {}", bytes);
    }
    println!(
        "  Kernel Plan    {}",
        yes_no(report.artifacts.kernel_boot_plan_ready)
    );
    println!(
        "  Kernel Layout  {}",
        yes_no(report.artifacts.kernel_boot_layout_ready)
    );
    println!(
        "  Framebuffer    {}",
        yes_no(report.artifacts.framebuffer_contract_ready)
    );
    if let Some(resolution) = &report.artifacts.framebuffer_resolution {
        println!("  FB Resolution  {}", resolution);
    }
    println!(
        "  Input Contract {}",
        yes_no(report.artifacts.input_contract_ready)
    );
    if let Some(cmdline) = &report.artifacts.kernel_cmdline {
        println!("  Kernel Cmdline {:?}", cmdline);
    }
    println!(
        "  User Disk      {}",
        yes_no(report.artifacts.user_disk_exists)
    );
    println!(
        "  User Disk Ready {}",
        yes_no(report.artifacts.user_disk_ready)
    );
    if let Some(capacity_gib) = report.artifacts.user_disk_capacity_gib {
        println!("  User Disk GiB  {}", capacity_gib);
    }
    if let Some(format) = &report.artifacts.user_disk_format {
        println!("  User Format    {}", format);
    }
    println!("Ownership");
    println!(
        "  App Storage    {}",
        yes_no(report.ownership.app_owned_storage)
    );
    println!(
        "  Boot Engine    {}",
        yes_no(report.ownership.app_owned_boot_engine_available)
    );
    println!(
        "  Display        {}",
        yes_no(report.ownership.app_owned_display_available)
    );
    println!(
        "  Current Bridge {}",
        yes_no(
            report
                .ownership
                .external_runtime_required_for_current_launch
        )
    );
    println!("Native Runtime");
    println!("  State          {}", report.native_runtime.state_label);
    println!(
        "  Host Ready     {}",
        yes_no(report.native_runtime.host_ready)
    );
    println!(
        "  Boot Spike     {}",
        yes_no(report.native_runtime.ready_for_boot_spike)
    );
    println!(
        "  Bootable       {}",
        yes_no(report.native_runtime.bootable)
    );
    println!(
        "  Requires WSL   {}",
        yes_no(report.native_runtime.requires_wsl)
    );
    println!(
        "  Requires mstsc {}",
        yes_no(report.native_runtime.requires_mstsc)
    );
    println!(
        "  Requires XRDP  {}",
        yes_no(report.native_runtime.requires_xrdp)
    );
    if !report.native_runtime.blockers.is_empty() {
        println!("Native Runtime Blockers");
        for blocker in &report.native_runtime.blockers {
            println!("  - {}", blocker);
        }
    }
    println!("Native Host");
    println!("  OS             {}", report.native_host.host_os);
    println!("  Arch           {}", report.native_host.host_arch);
    println!(
        "  Windows Host   {}",
        yes_no(report.native_host.windows_host)
    );
    println!(
        "  Supported Arch {}",
        yes_no(report.native_host.supported_arch)
    );
    println!(
        "  WHP Library    {}",
        yes_no(report.native_host.whp.dll_loaded)
    );
    println!(
        "  WHP Hypervisor {}",
        report
            .native_host
            .whp
            .hypervisor_present
            .map(yes_no)
            .unwrap_or("unknown")
    );
    println!("Native Host Checks");
    for check in &report.native_host.checks {
        println!("  [{}] {}", check.status.display_name(), check.summary);
        if let Some(remediation) = &check.remediation {
            println!("       fix: {remediation}");
        }
    }
    println!("Current Limitation");
    println!("  {}", report.current_limitation);
    println!("Next Steps");
    for step in &report.next_steps {
        println!("  - {}", step);
    }
    println!("Notes");
    for note in &report.notes {
        println!("  - {}", note);
    }
}

fn print_native_preflight_report(report: &NativePreflightReport) {
    println!("Pane Native Runtime Preflight");
    println!("  Session        {}", report.session_name);
    println!("  Ready          {}", yes_no(report.ready_for_boot_spike));
    println!("Host");
    println!("  OS             {}", report.host.host_os);
    println!("  Arch           {}", report.host.host_arch);
    println!("  Windows Host   {}", yes_no(report.host.windows_host));
    println!("  Supported Arch {}", yes_no(report.host.supported_arch));
    println!("WHP");
    println!("  Library        {}", yes_no(report.host.whp.dll_loaded));
    println!(
        "  GetCapability  {}",
        yes_no(report.host.whp.get_capability_available)
    );
    println!(
        "  Hypervisor     {}",
        report
            .host
            .whp
            .hypervisor_present
            .map(yes_no)
            .unwrap_or("unknown")
    );
    if let Some(hresult) = &report.host.whp.get_capability_hresult {
        println!("  HRESULT        {}", hresult);
    }
    println!("Checks");
    for check in &report.host.checks {
        println!("  [{}] {}", check.status.display_name(), check.summary);
        if let Some(remediation) = &check.remediation {
            println!("       fix: {remediation}");
        }
    }
    println!("Runtime Artifacts");
    println!("  Prepared       {}", yes_no(report.runtime.prepared));
    println!(
        "  Base Verified  {}",
        yes_no(report.runtime.artifacts.base_os_image_verified)
    );
    println!(
        "  User Disk Ready {}",
        yes_no(report.runtime.artifacts.user_disk_ready)
    );
    println!(
        "  Kernel Plan    {}",
        yes_no(report.runtime.artifacts.kernel_boot_plan_ready)
    );
    println!(
        "  Kernel Layout  {}",
        yes_no(report.runtime.artifacts.kernel_boot_layout_ready)
    );
    println!(
        "  Boot Spike     {}",
        yes_no(report.runtime.native_runtime.ready_for_boot_spike)
    );
    if !report.blockers.is_empty() {
        println!("Blockers");
        for blocker in &report.blockers {
            println!("  - {}", blocker);
        }
    }
    println!("Next Steps");
    for step in &report.next_steps {
        println!("  - {}", step);
    }
}

fn print_native_kernel_plan_report(report: &NativeKernelPlanReport) {
    println!("Pane Native Kernel Plan");
    println!("  Session        {}", report.session_name);
    println!("  Materialize    {}", yes_no(report.materialize_requested));
    println!(
        "  Ready          {}",
        yes_no(report.ready_for_kernel_entry_spike)
    );
    println!("Runtime");
    println!("  Prepared       {}", yes_no(report.runtime.prepared));
    println!(
        "  Kernel Plan    {}",
        yes_no(report.runtime.artifacts.kernel_boot_plan_ready)
    );
    println!(
        "  Kernel Layout  {}",
        yes_no(report.runtime.artifacts.kernel_boot_layout_ready)
    );
    if let Some(layout) = &report.layout {
        println!("Layout");
        println!("  Kernel         {}", layout.kernel_path);
        println!("  Kernel GPA     {}", layout.kernel_load_gpa);
        println!("  Boot Params    {}", layout.boot_params_gpa);
        println!("  Cmdline GPA    {}", layout.cmdline_gpa);
        if let Some(initramfs_path) = &layout.initramfs_path {
            println!("  Initramfs      {}", initramfs_path);
        }
        if let Some(initramfs_gpa) = &layout.initramfs_load_gpa {
            println!("  Initramfs GPA  {}", initramfs_gpa);
        }
        if let Some(entry_gpa) = &layout.linux_entry_point_gpa {
            println!("  Linux Entry    {}", entry_gpa);
        }
        if let Some(register) = &layout.linux_boot_params_register {
            println!("  Boot Params Reg {}", register);
        }
        if let Some(mode) = &layout.linux_expected_entry_mode {
            println!("  Entry Mode     {}", mode);
        }
        if !layout.guest_memory_map.is_empty() {
            println!("Guest Memory Map");
            for range in &layout.guest_memory_map {
                println!(
                    "  {:<16} {} +{} {}",
                    range.label, range.start_gpa, range.size_bytes, range.region_type
                );
            }
        }
        if let Some(storage) = &layout.storage {
            println!("Storage");
            println!("  Root Device    {}", storage.root_device);
            println!("  Base Image     {}", storage.base_os_path);
            println!("  User Device    {}", storage.user_device);
            println!("  User Disk      {}", storage.user_disk_path);
        }
        if let Some(framebuffer) = &layout.framebuffer {
            println!("Display Contract");
            println!("  Device         {}", framebuffer.device);
            println!("  Resolution     {}", framebuffer.resolution_label());
            println!("  Guest GPA      {}", framebuffer.guest_gpa);
        }
        if let Some(input) = &layout.input {
            println!("Input Contract");
            println!("  Keyboard       {}", input.keyboard_device);
            println!("  Pointer        {}", input.pointer_device);
            println!("  Queue GPA      {}", input.guest_queue_gpa);
            println!("  Queue Bytes    {}", input.queue_size_bytes);
        }
        println!("  Serial Device  {}", layout.expected_serial_device);
        println!("  Cmdline        {:?}", layout.cmdline);
    }
    if !report.blockers.is_empty() {
        println!("Blockers");
        for blocker in &report.blockers {
            println!("  - {}", blocker);
        }
    }
    println!("Next Steps");
    for step in &report.next_steps {
        println!("  - {}", step);
    }
}

fn print_native_boot_spike_report(report: &NativeBootSpikeReport) {
    println!("Pane Native Boot Spike");
    println!("  Session        {}", report.session_name);
    println!("  Execute        {}", yes_no(report.execute_requested));
    println!("  Fixture        {}", yes_no(report.fixture_requested));
    println!("  Boot Loader    {}", yes_no(report.boot_loader_requested));
    println!(
        "  Kernel Layout  {}",
        yes_no(report.kernel_layout_requested)
    );
    println!(
        "  Serial Ready   {}",
        yes_no(report.ready_for_serial_kernel_spike)
    );
    println!("Host");
    println!("  OS             {}", report.host.host_os);
    println!("  Arch           {}", report.host.host_arch);
    println!(
        "  WHP Ready      {}",
        yes_no(report.host.ready_for_boot_spike)
    );
    println!("Runtime");
    println!(
        "  Artifacts Ready {}",
        yes_no(report.runtime.native_runtime.ready_for_boot_spike)
    );
    println!(
        "  Base Verified  {}",
        yes_no(report.runtime.artifacts.base_os_image_verified)
    );
    println!(
        "  User Disk Ready {}",
        yes_no(report.runtime.artifacts.user_disk_ready)
    );
    println!(
        "  Loader Verified {}",
        yes_no(report.runtime.artifacts.boot_loader_image_verified)
    );
    println!("Boot Spike");
    println!("  Status         {}", report.partition_smoke.status_label);
    println!(
        "  Attempted      {}",
        yes_no(report.partition_smoke.attempted)
    );
    println!(
        "  Partition      {}",
        yes_no(report.partition_smoke.partition_created)
    );
    println!(
        "  Processor Cfg  {}",
        yes_no(report.partition_smoke.processor_count_configured)
    );
    println!(
        "  Setup          {}",
        yes_no(report.partition_smoke.partition_setup)
    );
    println!(
        "  vCPU Created   {}",
        yes_no(report.partition_smoke.virtual_processor_created)
    );
    println!(
        "  Memory Mapped  {}",
        yes_no(report.partition_smoke.memory_mapped)
    );
    println!(
        "  Registers      {}",
        yes_no(report.partition_smoke.registers_configured)
    );
    println!(
        "  vCPU Ran       {}",
        yes_no(report.partition_smoke.virtual_processor_ran)
    );
    if let Some(source) = &report.partition_smoke.boot_image_source {
        println!("  Boot Image Src {}", source);
    }
    if let Some(path) = &report.partition_smoke.boot_image_path {
        println!("  Boot Image     {}", path);
    }
    if let Some(bytes) = report.partition_smoke.boot_image_bytes {
        println!("  Boot Bytes     {}", bytes);
    }
    if let Some(mode) = &report.partition_smoke.entry_mode {
        println!("  Entry Mode     {}", mode);
    }
    if let Some(gpa) = &report.partition_smoke.boot_params_gpa {
        println!("  Boot Params    {}", gpa);
    }
    println!(
        "  Serial Exits   {}",
        report.partition_smoke.serial_io_exit_count
    );
    println!(
        "  Halt Observed  {}",
        yes_no(report.partition_smoke.halt_observed)
    );
    println!(
        "  Memory Unmapped {}",
        yes_no(report.partition_smoke.memory_unmapped)
    );
    if let Some(label) = &report.partition_smoke.exit_reason_label {
        println!("  Exit Reason    {}", label);
    }
    if let Some(port) = report.partition_smoke.serial_port {
        println!("  Serial Port    0x{port:04x}");
    }
    if let Some(byte) = report.partition_smoke.serial_byte {
        println!("  Serial Byte    0x{byte:02x}");
    }
    if let Some(expected) = &report.partition_smoke.serial_expected_text {
        println!("  Serial Expect  {:?}", expected);
    }
    if let Some(text) = &report.partition_smoke.serial_text {
        println!("  Serial Text    {:?}", text);
    }
    println!(
        "  vCPU Deleted   {}",
        yes_no(report.partition_smoke.virtual_processor_deleted)
    );
    println!(
        "  Partition Del  {}",
        yes_no(report.partition_smoke.partition_deleted)
    );
    if !report.partition_smoke.calls.is_empty() {
        println!("WHP Calls");
        for call in &report.partition_smoke.calls {
            println!(
                "  [{}] {} {}",
                yes_no(call.ok),
                call.name,
                call.hresult.as_deref().unwrap_or("")
            );
            println!("       {}", call.detail);
        }
    }
    if !report.blockers.is_empty() {
        println!("Blockers");
        for blocker in &report.blockers {
            println!("  - {}", blocker);
        }
    }
    println!("Next Steps");
    for step in &report.next_steps {
        println!("  - {}", step);
    }
}

fn print_environment_catalog_report(report: &EnvironmentCatalogReport) {
    println!("Pane Environments");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Strategy       {}", report.strategy);
    println!("Managed Environments");
    for environment in &report.environments {
        println!(
            "  {:<7} {}",
            environment.stage.display_name(),
            environment.display_name
        );
        println!("    Id           {}", environment.id);
        println!("    Family       {}", environment.family.display_name());
        println!("    Tier         {}", environment.tier.display_name());
        println!("    Launchable   {}", yes_no(environment.launchable_now));
        println!(
            "    Profile      {}",
            environment
                .starter_profile
                .as_deref()
                .unwrap_or("not assigned yet")
        );
        println!("    Summary      {}", environment.summary);
    }
    println!("Notes");
    for note in &report.notes {
        println!("  - {}", note);
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!("Pane Doctor");
    println!(
        "  Target Distro  {}",
        report.target_distro.as_deref().unwrap_or("not selected")
    );
    println!(
        "  Desktop        {}",
        report.desktop_environment.display_name()
    );
    println!("  Session        {}", report.session_name);
    println!("  Port           {}", report.port);
    println!("  Write Probes   {}", yes_no(report.write_probes_enabled));
    println!("  Supported MVP  {}", yes_no(report.supported_for_mvp));
    println!("  Ready          {}", yes_no(report.ready));
    println!("Checks");
    for check in &report.checks {
        println!("  {:<4} {}", check.status.display_name(), check.summary);
        if let Some(remediation) = &check.remediation {
            println!("       fix: {remediation}");
        }
    }
}

fn log_transport_event(path: Option<&Path>, message: &str) {
    let Some(path) = path else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {}", current_epoch_seconds(), message);
    }
}

fn relay_connection(
    distro: &str,
    target_port: u16,
    stream: TcpStream,
    log_file: Option<&Path>,
) -> AppResult<()> {
    if let Some(address) =
        wsl::distro_ipv4_address(distro).map(|ip| SocketAddr::from((ip, target_port)))
    {
        log_transport_event(
            log_file,
            &format!("relay targeting {address} via direct WSL TCP"),
        );
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(remote) => {
                relay_tcp_streams(stream, remote)?;
                return Ok(());
            }
            Err(error) => {
                log_transport_event(
                    log_file,
                    &format!(
                        "direct WSL TCP relay path to {address} failed: {error}; falling back to wsl.exe stdio tunnel"
                    ),
                );
            }
        }
    }

    relay_connection_via_stdio(distro, target_port, stream, log_file)
}

fn relay_tcp_streams(stream: TcpStream, remote: TcpStream) -> AppResult<()> {
    let mut upstream_reader = stream.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the local TCP stream: {error}"
        ))
    })?;
    let mut downstream_writer = stream;
    let mut remote_reader = remote.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the WSL TCP stream: {error}"
        ))
    })?;
    let mut remote_writer = remote;

    let _ = downstream_writer.set_nodelay(true);
    let _ = remote_writer.set_nodelay(true);

    let upstream = thread::spawn(move || {
        let result = std::io::copy(&mut upstream_reader, &mut remote_writer);
        let _ = remote_writer.flush();
        let _ = remote_writer.shutdown(Shutdown::Write);
        result
    });
    let downstream = thread::spawn(move || {
        let result = std::io::copy(&mut remote_reader, &mut downstream_writer);
        let _ = downstream_writer.shutdown(Shutdown::Write);
        result
    });

    let _ = upstream.join();
    let _ = downstream.join();
    Ok(())
}

fn relay_connection_via_stdio(
    distro: &str,
    target_port: u16,
    stream: TcpStream,
    log_file: Option<&Path>,
) -> AppResult<()> {
    let relay_command = format!("exec nc 127.0.0.1 {target_port}");
    let mut child = Command::new("wsl.exe");
    child
        .arg("-d")
        .arg(distro)
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg(&relay_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = child.spawn().map_err(|error| {
        AppError::message(format!(
            "failed to start the Pane relay tunnel into {}:{}: {error}",
            distro, target_port
        ))
    })?;
    let mut child_stdin = child.stdin.take().ok_or_else(|| {
        AppError::message("the Pane relay could not capture stdin for the WSL tunnel")
    })?;
    let mut child_stdout = child.stdout.take().ok_or_else(|| {
        AppError::message("the Pane relay could not capture stdout for the WSL tunnel")
    })?;

    let mut upstream_reader = stream.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the local TCP stream: {error}"
        ))
    })?;
    let mut downstream_writer = stream;
    let _ = downstream_writer.set_nodelay(true);

    let upstream = thread::spawn(move || {
        let result = std::io::copy(&mut upstream_reader, &mut child_stdin);
        let _ = child_stdin.flush();
        drop(child_stdin);
        result
    });
    let downstream = thread::spawn(move || {
        let result = std::io::copy(&mut child_stdout, &mut downstream_writer);
        let _ = downstream_writer.shutdown(Shutdown::Write);
        result
    });

    let _ = upstream.join();
    let _ = downstream.join();

    let status = child.wait().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not wait for the WSL tunnel process: {error}"
        ))
    })?;
    if status.success() {
        Ok(())
    } else {
        let exit = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        log_transport_event(
            log_file,
            &format!(
                "relay tunnel exited with {exit} for {}:{}",
                distro, target_port
            ),
        );
        Err(AppError::message(format!(
            "the Pane relay tunnel into {}:{} exited with {}",
            distro, target_port, exit
        )))
    }
}

fn ensure_transport_ready(
    distro: &str,
    port: u16,
    workspace: &WorkspacePaths,
) -> AppResult<PreparedTransport> {
    log_transport_event(
        Some(&workspace.transport_log),
        &format!("checking transport for {} on localhost:{}", distro, port),
    );

    let service_active = wsl::distro_service_active(distro, "xrdp") == Some(true);
    let listening = wsl::distro_port_listening(distro, port) == Some(true);
    if !service_active || !listening {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "XRDP inside {} is not ready for transport: active={}, listening={}",
                distro, service_active, listening
            ),
        );
        return Err(AppError::message(format!(
            "XRDP is not ready inside {} on port {}. Run pane logs or pane repair before reconnecting.",
            distro, port
        )));
    }

    if local_port_reachable(port) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!("using direct-localhost transport on localhost:{}", port),
        );
        return Ok(PreparedTransport::direct_localhost());
    }

    if let Some(address) = wsl::distro_ipv4_address(distro).map(|ip| SocketAddr::from((ip, port))) {
        if socket_reachable(address) {
            log_transport_event(
                Some(&workspace.transport_log),
                &format!("using direct-wsl-ip transport on {address}"),
            );
            return Ok(PreparedTransport::direct_wsl_ip(address.ip().to_string()));
        }

        log_transport_event(
            Some(&workspace.transport_log),
            &format!("Windows could not reach {address} directly; falling back to pane-relay"),
        );
    }

    if !(wsl::distro_ipv4_address(distro).is_some() || wsl::distro_command_exists(distro, "nc")) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "pane-relay is unavailable because Pane could not resolve a WSL IP address and nc is missing inside {}",
                distro
            ),
        );
        return Err(AppError::message(format!(
            "Windows could not reach localhost:{}, the distro IP directly, or a Pane relay path into {}. Run pane repair or pane launch to restore the relay helper and WSL network state.",
            port, distro
        )));
    }

    log_transport_event(
        Some(&workspace.transport_log),
        &format!(
            "localhost:{} and the distro IP are not reachable from Windows; starting pane-relay",
            port
        ),
    );
    spawn_pane_relay(distro, port, workspace)?;
    let ready_file = relay_ready_path(workspace);
    if wait_for_path(&ready_file, Duration::from_secs(5)) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!("pane-relay is serving localhost:{}", port),
        );
        Ok(PreparedTransport::pane_relay())
    } else {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "pane-relay did not publish readiness for localhost:{}",
                port
            ),
        );
        Err(AppError::message(format!(
            "Windows could not reach localhost:{}, the distro IP directly, and the Pane relay did not come up in time. Review {} or run pane logs.",
            port,
            workspace.transport_log.display()
        )))
    }
}
fn relay_ready_path(workspace: &WorkspacePaths) -> PathBuf {
    workspace.root.join("transport.ready")
}

fn spawn_pane_relay(distro: &str, port: u16, workspace: &WorkspacePaths) -> AppResult<()> {
    let executable = std::env::current_exe().map_err(|error| {
        AppError::message(format!(
            "failed to locate the Pane executable for pane-relay startup: {error}"
        ))
    })?;
    let ready_file = relay_ready_path(workspace);
    match fs::remove_file(&ready_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AppError::message(format!(
                "failed to clear the stale Pane relay readiness file at {}: {error}",
                ready_file.display()
            )));
        }
    }

    let mut command = Command::new(executable);
    command
        .arg("relay")
        .arg("--distro")
        .arg(distro)
        .arg("--listen-port")
        .arg(port.to_string())
        .arg("--target-port")
        .arg(port.to_string())
        .arg("--startup-timeout-seconds")
        .arg("90")
        .arg("--log-file")
        .arg(&workspace.transport_log)
        .arg("--ready-file")
        .arg(&ready_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        command.creation_flags(0x0000_0008 | 0x0800_0000);
    }

    command.spawn().map_err(|error| {
        AppError::message(format!(
            "failed to start the Pane relay for {} on localhost:{}: {error}",
            distro, port
        ))
    })?;

    Ok(())
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }

    false
}

fn socket_reachable(address: SocketAddr) -> bool {
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

fn local_port_reachable(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    socket_reachable(address)
}

fn mstsc_available() -> bool {
    if !cfg!(windows) {
        return false;
    }

    Command::new("where.exe")
        .arg("mstsc.exe")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn wait_for_runtime_ready(distro: &str, port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let service_active = wsl::distro_service_active(distro, "xrdp") == Some(true);
        let listening = wsl::distro_port_listening(distro, port) == Some(true);
        if service_active && listening {
            return true;
        }

        thread::sleep(Duration::from_millis(500));
    }

    false
}

fn write_bootstrap_log(
    path: &Path,
    plan: &LaunchPlan,
    command: &str,
    transcript: &wsl::CommandTranscript,
) -> AppResult<()> {
    let payload = format!(
        "Pane Bootstrap Transcript\n\nSession: {}\nDistro: {}\nDesktop: {}\nPort: {}\nCommand: {}\nSuccess: {}\n\n--- STDOUT ---\n{}\n\n--- STDERR ---\n{}\n",
        plan.session_name,
        plan.distro.name,
        plan.desktop_environment.display_name(),
        plan.port,
        command,
        yes_no(transcript.success),
        transcript.stdout.trim_end(),
        transcript.stderr.trim_end(),
    );

    fs::write(path, payload)?;
    Ok(())
}

fn format_doctor_blockers(command: &str, report: &DoctorReport) -> String {
    let mut lines = vec![format!("{command} is blocked by the following checks:")];
    for check in report
        .checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
    {
        lines.push(format!("- {}", check.summary));
        if let Some(remediation) = &check.remediation {
            lines.push(format!("  fix: {remediation}"));
        }
    }
    lines.push("Run `pane doctor` for the full report.".to_string());
    lines.join("\n")
}

fn format_blocker_list(blockers: &[String]) -> String {
    if blockers.is_empty() {
        return "- no blockers reported".to_string();
    }

    blockers
        .iter()
        .map(|blocker| format!("- {blocker}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_check(
    checks: &mut Vec<DoctorCheck>,
    status: CheckStatus,
    id: impl Into<String>,
    summary: impl Into<String>,
    remediation: Option<String>,
) {
    checks.push(DoctorCheck {
        id: id.into(),
        status,
        summary: summary.into(),
        remediation,
    });
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{InitArgs, ResetArgs},
        model::{DesktopEnvironment, DistroFamily, DistroRecord},
        native::test_native_host_report,
        plan::{LaunchPlan, RuntimePaths, WorkspacePaths},
        state::{
            LaunchStage, LaunchTransport, ManagedEnvironmentOwnership, ManagedEnvironmentState,
            StoredLaunch,
        },
    };

    use super::{
        build_bundle_doctor_request, build_distro_health, build_environment_catalog_report,
        build_kernel_boot_layout, build_linux_boot_params_page, build_native_runtime_report,
        build_runtime_artifact_report, build_steps, build_update_steps,
        create_serial_boot_image_artifact, create_user_disk_descriptor,
        default_framebuffer_contract, default_input_contract, default_linux_guest_memory_map,
        determine_app_lifecycle, ensure_wsl_conf_setting, format_doctor_blockers,
        initialize_managed_arch_environment, inspect_kernel_image_artifact, inspect_workspace,
        inventory_contains_distro, kernel_layout_execution_image, linux_guest_mapped_regions,
        load_kernel_layout_boot_image_artifact, parse_guest_physical_address, preferred_transport,
        read_json_file, register_base_os_image, register_boot_loader_image,
        register_kernel_boot_plan, resolve_bundle_output_path, resolve_init_source,
        resolve_launch_target, resolve_managed_environment_for_reset, resolve_saved_launch,
        resolve_session_context, resolve_status_distro, runtime_contract_guest_memory_ranges,
        runtime_storage_budget, sha256_file, status_port_for, user_disk_artifact_ready,
        validate_setup_password, validate_setup_username, windows_transport_check, write_json_file,
        AppLifecyclePhase, AppNextAction, CheckStatus, DistroHealth, DoctorCheck, DoctorReport,
        FramebufferContract, InitSource, KernelBootLayout, KernelBootMetadata, NativeRuntimeState,
        StatusReport, UserDiskMetadata, WorkspaceHealth, WslInventory, EMBEDDED_APP_ASSETS,
    };

    fn empty_workspace_health() -> WorkspaceHealth {
        WorkspaceHealth {
            root_exists: false,
            shared_dir_exists: false,
            bootstrap_script_exists: false,
            rdp_profile_exists: false,
            bootstrap_log_exists: false,
            transport_log_exists: false,
        }
    }

    fn temp_runtime_paths(name: &str) -> RuntimePaths {
        let root = std::env::temp_dir().join(format!(
            "pane-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let downloads = root.join("downloads");
        let images = root.join("images");
        let disks = root.join("disks");
        let snapshots = root.join("snapshots");
        let state = root.join("state");
        let engines = root.join("engines");
        let logs = root.join("logs");

        RuntimePaths {
            base_os_image: images.join("arch-base.paneimg"),
            serial_boot_image: engines.join("serial-boot.paneimg"),
            boot_loader_image: engines.join("boot-to-serial-loader.paneimg"),
            kernel_image: engines.join("linux-kernel.paneimg"),
            initramfs_image: engines.join("initramfs.paneinitrd"),
            user_disk: disks.join("user-data.panedisk"),
            base_os_metadata: state.join("base-os-image.json"),
            serial_boot_metadata: state.join("serial-boot-image.json"),
            boot_loader_metadata: state.join("boot-to-serial-loader.json"),
            kernel_boot_metadata: state.join("kernel-boot.json"),
            kernel_boot_layout: state.join("kernel-boot-layout.json"),
            user_disk_metadata: state.join("user-disk.json"),
            runtime_config: root.join("pane-runtime.config.json"),
            native_manifest: root.join("pane-native-runtime.json"),
            framebuffer_contract: state.join("framebuffer-contract.json"),
            input_contract: state.join("input-contract.json"),
            manifest: root.join("pane-runtime.json"),
            downloads,
            images,
            disks,
            snapshots,
            state,
            engines,
            logs,
            root,
        }
    }

    fn fake_linux_bzimage() -> Vec<u8> {
        let mut bytes = vec![0_u8; 4096];
        bytes[0x1f1] = 4;
        bytes[0x1fe..0x200].copy_from_slice(&0xaa55_u16.to_le_bytes());
        bytes[0x201] = 0x62;
        bytes[0x202..0x206].copy_from_slice(b"HdrS");
        bytes[0x206..0x208].copy_from_slice(&0x020f_u16.to_le_bytes());
        bytes[0x211] = 0x01;
        bytes[0x234] = 1;
        bytes[0x236..0x238].copy_from_slice(&0x0001_u16.to_le_bytes());
        bytes[0x258..0x260].copy_from_slice(&0x0100_0000_u64.to_le_bytes());
        bytes[2560..].fill(0xcc);
        bytes
    }

    fn empty_doctor_report() -> DoctorReport {
        DoctorReport {
            target_distro: None,
            session_name: "pane".to_string(),
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            bootstrap_requested: true,
            connect_requested: false,
            write_probes_enabled: false,
            supported_for_mvp: false,
            ready: true,
            selected_distro: None,
            workspace: empty_workspace_health(),
            checks: Vec::new(),
        }
    }

    fn empty_status_report(wsl_available: bool) -> StatusReport {
        StatusReport {
            platform: "windows",
            wsl_available,
            wsl_version_banner: None,
            wsl_default_distro: None,
            managed_environment: None,
            selected_distro: None,
            known_distros: Vec::new(),
            last_launch: None,
            last_launch_workspace: None,
        }
    }

    #[test]
    fn build_steps_omits_rdp_handoff_when_connect_is_disabled() {
        let steps = build_steps(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
            true,
            false,
        );

        assert!(steps
            .iter()
            .any(|step| step.contains("Run the bootstrap script")));
        assert!(!steps.iter().any(|step| step.contains("Launch mstsc.exe")));
    }

    #[test]
    fn build_update_steps_refresh_packages_and_omit_rdp_handoff() {
        let steps = build_update_steps(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
        );

        assert!(steps
            .iter()
            .any(|step| step.contains("Refresh Arch packages")));
        assert!(!steps.iter().any(|step| step.contains("Launch mstsc.exe")));
    }

    #[test]
    fn embedded_app_assets_include_control_center_and_launchers() {
        let names = EMBEDDED_APP_ASSETS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"Pane Control Center.ps1"));
        assert!(names.contains(&"Launch Pane Arch.ps1"));
        assert!(names.contains(&"Open Pane Shared Folder.ps1"));
        assert!(names.contains(&"Install Pane Shortcuts.ps1"));
        assert!(EMBEDDED_APP_ASSETS
            .iter()
            .all(|(_, contents)| !contents.trim().is_empty()));
    }

    #[test]
    fn runtime_budget_reserves_space_for_base_os_user_data_and_snapshots() {
        let budget = runtime_storage_budget(8);

        assert_eq!(budget.requested_capacity_gib, 8);
        assert_eq!(budget.base_os_budget_gib, 4);
        assert_eq!(budget.snapshot_budget_gib, 1);
        assert_eq!(budget.user_packages_and_customizations_gib, 3);
        assert_eq!(budget.minimum_recommended_gib, 8);
    }

    #[test]
    fn runtime_budget_enforces_minimum_capacity() {
        let budget = runtime_storage_budget(4);

        assert_eq!(budget.requested_capacity_gib, 8);
        assert_eq!(budget.base_os_budget_gib, 4);
        assert_eq!(budget.snapshot_budget_gib, 1);
        assert_eq!(budget.user_packages_and_customizations_gib, 3);
    }

    #[test]
    fn sha256_file_matches_known_digest() {
        let paths = temp_runtime_paths("sha256");
        std::fs::create_dir_all(&paths.root).unwrap();
        let source = paths.root.join("source.img");
        std::fs::write(&source, b"abc").unwrap();

        assert_eq!(
            sha256_file(&source).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn inspects_linux_bzimage_header_for_boot_protocol_metadata() {
        let paths = temp_runtime_paths("inspect-bzimage");
        std::fs::create_dir_all(&paths.root).unwrap();
        let kernel = paths.root.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();

        let inspection = inspect_kernel_image_artifact(&kernel).unwrap();
        assert_eq!(inspection.format, "linux-bzimage");
        assert_eq!(inspection.linux_boot_protocol.as_deref(), Some("0x020f"));
        assert_eq!(inspection.linux_setup_sectors, Some(4));
        assert_eq!(inspection.linux_setup_bytes, Some(2560));
        assert_eq!(inspection.linux_protected_mode_offset, Some(2560));
        assert_eq!(inspection.linux_protected_mode_bytes, Some(1536));
        assert_eq!(inspection.linux_loadflags, Some(0x01));
        assert_eq!(
            inspection.linux_preferred_load_address.as_deref(),
            Some("0x0000000001000000")
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn rejects_unknown_kernel_artifacts_before_native_execution() {
        let paths = temp_runtime_paths("inspect-unknown-kernel");
        std::fs::create_dir_all(&paths.root).unwrap();
        let kernel = paths.root.join("not-a-kernel.bin");
        std::fs::write(&kernel, b"not a kernel").unwrap();

        let error = inspect_kernel_image_artifact(&kernel)
            .unwrap_err()
            .to_string();
        assert!(error.contains("too small"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn creates_runtime_backed_serial_boot_image() {
        let paths = temp_runtime_paths("serial-boot-image");
        super::prepare_runtime_paths(&paths).unwrap();

        create_serial_boot_image_artifact(&paths, false).unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.serial_boot_image_exists);
        assert!(artifacts.serial_boot_metadata_exists);
        assert!(artifacts.serial_boot_image_ready);
        assert!(artifacts.framebuffer_contract_exists);
        assert!(artifacts.framebuffer_contract_ready);
        assert_eq!(
            artifacts.framebuffer_resolution.as_deref(),
            Some("1024x768x32")
        );
        assert!(artifacts.input_contract_exists);
        assert!(artifacts.input_contract_ready);
        assert_eq!(
            artifacts.serial_boot_banner.as_deref(),
            Some(crate::native::SERIAL_BOOT_BANNER_TEXT)
        );
        assert_eq!(
            std::fs::read(&paths.serial_boot_image).unwrap(),
            crate::native::serial_boot_test_image_bytes()
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_verified_boot_to_serial_loader_candidate() {
        let paths = temp_runtime_paths("boot-loader-image");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("loader.img");
        std::fs::write(&source, crate::native::serial_boot_test_image_bytes()).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_boot_loader_image(
            &paths,
            &source,
            Some(&expected),
            crate::native::SERIAL_BOOT_BANNER_TEXT,
            false,
        )
        .unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.boot_loader_image_exists);
        assert!(artifacts.boot_loader_metadata_exists);
        assert!(artifacts.boot_loader_image_verified);
        assert_eq!(
            artifacts.boot_loader_expected_serial.as_deref(),
            Some(crate::native::SERIAL_BOOT_BANNER_TEXT)
        );
        assert_eq!(
            std::fs::read(&paths.boot_loader_image).unwrap(),
            crate::native::serial_boot_test_image_bytes()
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn unverified_boot_to_serial_loader_is_not_ready() {
        let paths = temp_runtime_paths("unverified-boot-loader-image");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("loader.img");
        std::fs::write(&source, crate::native::serial_boot_test_image_bytes()).unwrap();

        register_boot_loader_image(
            &paths,
            &source,
            None,
            crate::native::SERIAL_BOOT_BANNER_TEXT,
            false,
        )
        .unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.boot_loader_image_exists);
        assert!(!artifacts.boot_loader_image_verified);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_verified_kernel_boot_plan() {
        let paths = temp_runtime_paths("kernel-boot-plan");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(&initramfs, b"pane initramfs").unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();

        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            Some(&initramfs),
            Some(&initramfs_sha),
            Some("console=ttyS0 root=/dev/pane0 rw"),
            false,
        )
        .unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.kernel_image_exists);
        assert!(artifacts.kernel_image_verified);
        assert!(artifacts.initramfs_image_exists);
        assert!(artifacts.initramfs_image_verified);
        assert!(artifacts.kernel_boot_plan_ready);
        assert_eq!(
            artifacts.kernel_cmdline.as_deref(),
            Some("console=ttyS0 root=/dev/pane0 rw")
        );
        assert_eq!(artifacts.kernel_format.as_deref(), Some("linux-bzimage"));
        assert_eq!(
            artifacts.kernel_linux_boot_protocol.as_deref(),
            Some("0x020f")
        );
        assert_eq!(artifacts.kernel_linux_protected_mode_offset, Some(2560));
        let metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).unwrap();
        assert_eq!(metadata.kernel_format, "linux-bzimage");
        assert_eq!(metadata.linux_boot_protocol.as_deref(), Some("0x020f"));
        assert_eq!(metadata.linux_protected_mode_offset, Some(2560));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_plan_requires_serial_console() {
        let paths = temp_runtime_paths("kernel-boot-plan-no-serial");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        let error = register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("root=/dev/pane0 rw"),
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("console=ttyS0"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn materializes_kernel_boot_layout() {
        let paths = temp_runtime_paths("kernel-boot-layout");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(&initramfs, b"pane initramfs").unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();

        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            Some(&initramfs),
            Some(&initramfs_sha),
            Some("console=ttyS0 root=/dev/pane0 rw"),
            false,
        )
        .unwrap();

        let layout = build_kernel_boot_layout(&paths, "pane", true).unwrap();
        assert_eq!(layout.layout_kind, "pane-linux-kernel-boot-layout-v1");
        assert_eq!(layout.boot_params_gpa, "0x00007000");
        assert_eq!(layout.cmdline_gpa, "0x00020000");
        assert_eq!(layout.kernel_load_gpa, "0x00100000");
        assert_eq!(layout.initramfs_load_gpa.as_deref(), Some("0x04000000"));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "high-ram"
                && range.start_gpa == "0x08000000"
                && range.region_type == "usable"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "boot-gdt"
                && range.start_gpa == "0x00008000"
                && range.region_type == "reserved"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "io-apic-mmio"
                && range.start_gpa == "0xfec00000"
                && range.region_type == "mmio-stub"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "local-apic-mmio"
                && range.start_gpa == "0xfee00000"
                && range.region_type == "mmio-stub"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "initramfs"
                && range.start_gpa == "0x04000000"
                && range.size_bytes == 0x00001000
                && range.region_type == "reserved"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "pane-framebuffer"
                && range.start_gpa == "0x0e000000"
                && range.size_bytes == 0x00300000
                && range.region_type == "framebuffer"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "pane-input-queue"
                && range.start_gpa == "0x0dff0000"
                && range.size_bytes == 0x00001000
                && range.region_type == "input-queue"
        }));
        assert!(layout.materialized_at_epoch_seconds.is_some());

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.kernel_boot_layout_exists);
        assert!(artifacts.kernel_boot_layout_ready);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn loads_kernel_layout_candidate_from_materialized_layout() {
        let paths = temp_runtime_paths("kernel-layout-candidate");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        std::fs::write(&kernel, crate::native::serial_boot_test_image_bytes()).unwrap();
        std::fs::write(&initramfs, b"pane initramfs").unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();

        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            Some(&initramfs),
            Some(&initramfs_sha),
            Some("console=ttyS0 panic=-1"),
            false,
        )
        .unwrap();
        build_kernel_boot_layout(&paths, "pane", true).unwrap();

        let image = load_kernel_layout_boot_image_artifact(&paths).unwrap();
        assert_eq!(image.source_label, "pane-runtime-kernel-layout");
        assert_eq!(image.guest_entry_gpa, 0x0010_0000);
        assert_eq!(
            image.expected_serial_text,
            crate::native::SERIAL_BOOT_BANNER_TEXT
        );
        assert_eq!(image.bytes, crate::native::serial_boot_test_image_bytes());
        assert!(image
            .extra_regions
            .iter()
            .any(|region| region.label == "linux-boot-params" && region.guest_gpa == 0x7000));
        assert!(image.extra_regions.iter().any(|region| {
            region.label == "linux-kernel-cmdline"
                && region.guest_gpa == 0x20000
                && region.bytes.ends_with(&[0])
        }));
        assert!(image.extra_regions.iter().any(|region| {
            region.label == "linux-initramfs"
                && region.guest_gpa == 0x0400_0000
                && region.bytes == b"pane initramfs"
        }));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn linux_boot_params_page_points_at_cmdline_and_initramfs() {
        let layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            boot_params_gpa: "0x00007000".to_string(),
            cmdline_gpa: "0x00020000".to_string(),
            kernel_load_gpa: "0x00100000".to_string(),
            initramfs_load_gpa: Some("0x04000000".to_string()),
            kernel_path: "kernel".to_string(),
            kernel_bytes: 4096,
            kernel_sha256: "0".repeat(64),
            kernel_format: "linux-bzimage".to_string(),
            linux_boot_protocol: Some("0x020f".to_string()),
            linux_setup_sectors: Some(4),
            linux_setup_bytes: Some(2560),
            linux_protected_mode_offset: Some(2560),
            linux_protected_mode_bytes: Some(1536),
            linux_loadflags: Some(0x80),
            linux_preferred_load_address: Some("0x0000000001000000".to_string()),
            linux_entry_point_gpa: Some("0x00100000".to_string()),
            linux_boot_params_register: Some("rsi".to_string()),
            linux_expected_entry_mode: Some("x86-protected-mode-32".to_string()),
            guest_memory_map: default_linux_guest_memory_map(1234),
            initramfs_path: Some("initramfs".to_string()),
            initramfs_bytes: Some(1234),
            initramfs_sha256: Some("1".repeat(64)),
            cmdline: "console=ttyS0 panic=-1".to_string(),
            expected_serial_device: "ttyS0".to_string(),
            storage: None,
            framebuffer: Some(default_framebuffer_contract()),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };

        let kernel = fake_linux_bzimage();
        let page = build_linux_boot_params_page(&layout, Some(&kernel)).unwrap();
        assert_eq!(&page[0x1fe..0x200], &0xaa55_u16.to_le_bytes());
        assert_eq!(&page[0x202..0x206], b"HdrS");
        assert_eq!(&page[0x206..0x208], &0x020f_u16.to_le_bytes());
        assert_eq!(page[0x211], 0x81);
        assert_eq!(page[0x234], 1);
        assert_eq!(&page[0x236..0x238], &0x0001_u16.to_le_bytes());
        assert_eq!(&page[0x258..0x260], &0x0100_0000_u64.to_le_bytes());
        assert_eq!(&page[0x228..0x22c], &0x0002_0000_u32.to_le_bytes());
        assert_eq!(&page[0x218..0x21c], &0x0400_0000_u32.to_le_bytes());
        assert_eq!(&page[0x21c..0x220], &1234_u32.to_le_bytes());
        assert_eq!(page[0x1e8], 11);
        assert_eq!(&page[0x2d0..0x2d8], &0x0000_7000_u64.to_le_bytes());
        assert_eq!(&page[0x2d0 + 16..0x2d0 + 20], &2_u32.to_le_bytes());
        let boot_gdt_offset = 0x2d0 + 1 * 20;
        assert_eq!(
            &page[boot_gdt_offset..boot_gdt_offset + 8],
            &0x0000_8000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[boot_gdt_offset + 16..boot_gdt_offset + 20],
            &2_u32.to_le_bytes()
        );
        let initramfs_offset = 0x2d0 + 7 * 20;
        assert_eq!(
            &page[initramfs_offset..initramfs_offset + 8],
            &0x0400_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[initramfs_offset + 8..initramfs_offset + 16],
            &0x0000_1000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[initramfs_offset + 16..initramfs_offset + 20],
            &2_u32.to_le_bytes()
        );
        let high_ram_offset = 0x2d0 + 8 * 20;
        assert_eq!(
            &page[high_ram_offset..high_ram_offset + 8],
            &0x0800_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[high_ram_offset + 16..high_ram_offset + 20],
            &1_u32.to_le_bytes()
        );
        let local_apic_offset = 0x2d0 + 10 * 20;
        assert_eq!(
            &page[local_apic_offset..local_apic_offset + 8],
            &0xfee0_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[local_apic_offset + 16..local_apic_offset + 20],
            &2_u32.to_le_bytes()
        );
    }

    #[test]
    fn linux_bzimage_execution_image_splits_setup_and_payload() {
        let mut layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            boot_params_gpa: "0x00007000".to_string(),
            cmdline_gpa: "0x00020000".to_string(),
            kernel_load_gpa: "0x00100000".to_string(),
            initramfs_load_gpa: None,
            kernel_path: "kernel".to_string(),
            kernel_bytes: 4096,
            kernel_sha256: "0".repeat(64),
            kernel_format: "linux-bzimage".to_string(),
            linux_boot_protocol: Some("0x020f".to_string()),
            linux_setup_sectors: Some(4),
            linux_setup_bytes: Some(2560),
            linux_protected_mode_offset: Some(2560),
            linux_protected_mode_bytes: Some(1536),
            linux_loadflags: Some(0x80),
            linux_preferred_load_address: Some("0x0000000001000000".to_string()),
            linux_entry_point_gpa: Some("0x00100000".to_string()),
            linux_boot_params_register: Some("rsi".to_string()),
            linux_expected_entry_mode: Some("x86-protected-mode-32".to_string()),
            guest_memory_map: default_linux_guest_memory_map(0),
            initramfs_path: None,
            initramfs_bytes: None,
            initramfs_sha256: None,
            cmdline: "console=ttyS0 panic=-1".to_string(),
            expected_serial_device: "ttyS0".to_string(),
            storage: None,
            framebuffer: Some(default_framebuffer_contract()),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };
        let kernel = fake_linux_bzimage();

        let (payload, entry_gpa, extra_regions) =
            kernel_layout_execution_image(&layout, &kernel).unwrap();
        assert_eq!(entry_gpa, 0x0010_0000);
        assert_eq!(payload, kernel[2560..].to_vec());
        assert!(extra_regions.iter().any(|region| {
            region.label == "linux-bzimage-setup"
                && region.guest_gpa == 0x0009_0000
                && region.bytes == kernel[..2560]
        }));

        layout.kernel_format = "controlled-serial-candidate".to_string();
        let (payload, entry_gpa, extra_regions) =
            kernel_layout_execution_image(&layout, &kernel).unwrap();
        assert_eq!(entry_gpa, 0x0010_0000);
        assert_eq!(payload, kernel);
        assert!(extra_regions.is_empty());
    }

    #[test]
    fn linux_guest_mapped_regions_include_ram_and_apic_stubs() {
        let framebuffer = default_framebuffer_contract();
        let input = default_input_contract();
        let mut guest_memory_map = default_linux_guest_memory_map(0);
        guest_memory_map
            .extend(runtime_contract_guest_memory_ranges(&framebuffer, &input).unwrap());
        let layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            boot_params_gpa: "0x00007000".to_string(),
            cmdline_gpa: "0x00020000".to_string(),
            kernel_load_gpa: "0x00100000".to_string(),
            initramfs_load_gpa: None,
            kernel_path: "kernel".to_string(),
            kernel_bytes: 4096,
            kernel_sha256: "0".repeat(64),
            kernel_format: "linux-bzimage".to_string(),
            linux_boot_protocol: Some("0x020f".to_string()),
            linux_setup_sectors: Some(4),
            linux_setup_bytes: Some(2560),
            linux_protected_mode_offset: Some(2560),
            linux_protected_mode_bytes: Some(1536),
            linux_loadflags: Some(0x80),
            linux_preferred_load_address: Some("0x0000000001000000".to_string()),
            linux_entry_point_gpa: Some("0x00100000".to_string()),
            linux_boot_params_register: Some("rsi".to_string()),
            linux_expected_entry_mode: Some("x86-protected-mode-32".to_string()),
            guest_memory_map,
            initramfs_path: None,
            initramfs_bytes: None,
            initramfs_sha256: None,
            cmdline: "console=ttyS0 panic=-1".to_string(),
            expected_serial_device: "ttyS0".to_string(),
            storage: None,
            framebuffer: Some(default_framebuffer_contract()),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };

        let regions = linux_guest_mapped_regions(&layout).unwrap();
        assert!(regions.iter().any(|region| {
            region.label == "linux-ram-high-ram"
                && region.guest_gpa == 0x0800_0000
                && region.writable
                && region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "linux-local-apic-mmio"
                && region.guest_gpa == 0xfee0_0000
                && region.writable
                && !region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "pane-framebuffer"
                && region.guest_gpa == 0x0e00_0000
                && region.bytes.len() == 0x0030_0000
                && region.writable
                && !region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "pane-input-queue"
                && region.guest_gpa == 0x0dff_0000
                && region.bytes.len() == 0x0000_1000
                && region.writable
                && !region.executable
        }));
    }

    #[test]
    fn parses_guest_physical_address_hex_contract() {
        assert_eq!(
            parse_guest_physical_address("0x00100000").unwrap(),
            0x0010_0000
        );
        assert!(parse_guest_physical_address("1048576").is_err());
        assert!(parse_guest_physical_address("0xnot-hex").is_err());
    }

    #[test]
    fn kernel_boot_layout_requires_verified_plan() {
        let paths = temp_runtime_paths("kernel-boot-layout-unverified");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();

        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            None,
            None,
            None,
            Some("console=ttyS0 root=/dev/pane0 rw"),
            false,
        )
        .unwrap();

        let error = build_kernel_boot_layout(&paths, "pane", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not hash-verified"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_verified_base_image_and_user_disk_descriptor() {
        let paths = temp_runtime_paths("runtime-artifacts");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch.img");
        std::fs::write(&source, b"pane arch image").unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        create_serial_boot_image_artifact(&paths, false).unwrap();
        super::write_runtime_config(&paths, "pane", &runtime_storage_budget(8)).unwrap();
        super::write_native_runtime_manifest(&paths, "pane").unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.base_os_image_exists);
        assert!(artifacts.base_os_image_verified);
        assert_eq!(
            artifacts.base_os_image_sha256.as_deref(),
            Some(expected.as_str())
        );
        assert!(artifacts.user_disk_exists);
        assert!(artifacts.user_disk_ready);
        assert_eq!(artifacts.user_disk_capacity_gib, Some(3));
        assert_eq!(
            artifacts.user_disk_format.as_deref(),
            Some("pane-sparse-user-disk-v1")
        );
        let user_disk_metadata =
            read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let user_disk_bytes = std::fs::read(&paths.user_disk).unwrap();
        assert!(user_disk_metadata.materialized_block_device);
        assert!(user_disk_metadata.sparse_backing);
        assert_eq!(
            user_disk_metadata.logical_size_bytes,
            3 * 1024 * 1024 * 1024
        );
        assert_eq!(
            user_disk_metadata.allocated_header_bytes,
            user_disk_bytes.len() as u64
        );
        assert!(user_disk_bytes.starts_with(b"PANE_USER_DISK_V1\n"));
        assert!(artifacts.framebuffer_contract_ready);
        assert!(artifacts.input_contract_ready);
        assert!(artifacts.serial_boot_image_exists);
        assert!(artifacts.serial_boot_image_ready);
        assert_eq!(
            artifacts.serial_boot_banner.as_deref(),
            Some(crate::native::SERIAL_BOOT_BANNER_TEXT)
        );

        let native_host = test_native_host_report(true);
        let native = build_native_runtime_report(true, &artifacts, &native_host);
        assert_eq!(native.state, NativeRuntimeState::EngineNotImplemented);
        assert!(native.ready_for_boot_spike);
        assert!(!native
            .blockers
            .iter()
            .any(|blocker| blocker.contains("No valid Pane-owned user disk")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn create_user_disk_upgrades_legacy_descriptor_to_sparse_artifact() {
        let paths = temp_runtime_paths("runtime-user-disk-upgrade");
        super::prepare_runtime_paths(&paths).unwrap();
        let legacy = serde_json::json!({
            "schema_version": 1,
            "format": "pane-user-disk-descriptor-v1",
            "disk_path": paths.user_disk.display().to_string(),
            "capacity_gib": 3,
            "materialized_block_device": false,
            "created_at_epoch_seconds": 1,
            "notes": []
        });
        write_json_file(&paths.user_disk, &legacy).unwrap();
        write_json_file(&paths.user_disk_metadata, &legacy).unwrap();

        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();

        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let user_disk_bytes = std::fs::read(&paths.user_disk).unwrap();
        assert_eq!(metadata.format, "pane-sparse-user-disk-v1");
        assert!(metadata.materialized_block_device);
        assert!(user_disk_bytes.starts_with(b"PANE_USER_DISK_V1\n"));
        assert!(user_disk_artifact_ready(&paths, &Some(metadata)));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_layout_attaches_verified_storage_and_display_contracts() {
        let paths = temp_runtime_paths("kernel-layout-storage-display");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-base.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&base, b"pane arch base image").unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("console=ttyS0 root=/dev/pane0 rw"),
            false,
        )
        .unwrap();

        let layout = build_kernel_boot_layout(&paths, "pane", true).unwrap();
        let storage = layout.storage.as_ref().expect("storage attachment");
        assert_eq!(storage.root_device, "/dev/pane0");
        assert_eq!(storage.user_device, "/dev/pane1");
        assert!(storage.readonly_base);
        assert!(storage.writable_user_disk);
        assert_eq!(storage.base_os_sha256, base_sha);
        assert_eq!(storage.user_disk_capacity_gib, 3);
        assert_eq!(
            layout
                .framebuffer
                .as_ref()
                .map(FramebufferContract::resolution_label)
                .as_deref(),
            Some("1024x768x32")
        );
        assert_eq!(
            layout
                .input
                .as_ref()
                .map(|contract| contract.pointer_device.as_str()),
            Some("pane-absolute-pointer-v1")
        );

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.kernel_boot_layout_ready);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn native_runtime_reports_host_blocker_after_artifacts_are_ready() {
        let paths = temp_runtime_paths("runtime-host-not-ready");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch.img");
        std::fs::write(&source, b"pane arch image").unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        create_serial_boot_image_artifact(&paths, false).unwrap();
        super::write_runtime_config(&paths, "pane", &runtime_storage_budget(8)).unwrap();
        super::write_native_runtime_manifest(&paths, "pane").unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        let native_host = test_native_host_report(false);
        let native = build_native_runtime_report(true, &artifacts, &native_host);

        assert_eq!(native.state, NativeRuntimeState::HostNotReady);
        assert!(!native.host_ready);
        assert!(!native.ready_for_boot_spike);
        assert!(native
            .blockers
            .iter()
            .any(|blocker| blocker.contains("Native host check")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn rejects_base_image_hash_mismatch() {
        let paths = temp_runtime_paths("runtime-hash-mismatch");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch.img");
        std::fs::write(&source, b"pane arch image").unwrap();

        let error = register_base_os_image(
            &paths,
            &source,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("SHA-256 mismatch"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[cfg(windows)]
    #[test]
    fn app_lifecycle_requires_wsl_before_onboarding() {
        let status = empty_status_report(false);
        let doctor = empty_doctor_report();

        let (phase, next_action, _, _) = determine_app_lifecycle(&status, &doctor, None);

        assert_eq!(phase, AppLifecyclePhase::HostNeedsWsl);
        assert_eq!(next_action, AppNextAction::InstallWsl);
    }

    #[cfg(not(windows))]
    #[test]
    fn app_lifecycle_reports_unsupported_host_on_non_windows() {
        let status = empty_status_report(false);
        let doctor = empty_doctor_report();

        let (phase, next_action, _, _) = determine_app_lifecycle(&status, &doctor, None);

        assert_eq!(phase, AppLifecyclePhase::UnsupportedHost);
        assert_eq!(next_action, AppNextAction::CollectSupportBundle);
    }

    #[cfg(windows)]
    #[test]
    fn app_lifecycle_launches_after_managed_arch_user_is_ready() {
        let mut status = empty_status_report(true);
        status.managed_environment = Some(ManagedEnvironmentState {
            environment_id: "arch".to_string(),
            distro_name: "pane-arch".to_string(),
            family: DistroFamily::Arch,
            ownership: ManagedEnvironmentOwnership::InstalledOnline,
            install_dir: None,
            source_rootfs: None,
            created_at_epoch_seconds: 1,
        });
        status.selected_distro = Some(DistroHealth {
            distro: DistroRecord {
                name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                default_user: Some("paneuser".to_string()),
                ..DistroRecord::default()
            },
            supported_for_mvp: true,
            present_in_inventory: true,
            checked_port: 3390,
            systemd_configured: Some(true),
            xrdp_installed: None,
            xrdp_service_active: None,
            xrdp_listening: None,
            localhost_reachable: None,
            pane_relay_available: None,
            preferred_transport: None,
            xsession_present: None,
            pane_session_assets_ready: None,
            user_home_ready: None,
            default_user_password_status: Some(crate::wsl::PasswordStatus::Usable),
        });
        let mut doctor = empty_doctor_report();
        doctor.supported_for_mvp = true;
        doctor.ready = true;

        let (phase, next_action, _, _) = determine_app_lifecycle(&status, &doctor, None);

        assert_eq!(phase, AppLifecyclePhase::ReadyToLaunch);
        assert_eq!(next_action, AppNextAction::LaunchArch);
    }

    #[test]
    fn preferred_transport_uses_direct_wsl_ip_when_localhost_is_unreachable() {
        assert_eq!(
            preferred_transport(Some(true), Some(false), Some(true), Some(true)),
            Some(LaunchTransport::DirectWslIp)
        );
    }

    #[test]
    fn preferred_transport_uses_pane_relay_when_only_relay_is_available() {
        assert_eq!(
            preferred_transport(Some(true), Some(false), Some(false), Some(true)),
            Some(LaunchTransport::PaneRelay)
        );
    }

    #[test]
    fn windows_transport_check_passes_when_pane_relay_can_bridge() {
        let health = DistroHealth {
            distro: DistroRecord::default(),
            supported_for_mvp: true,
            present_in_inventory: true,
            checked_port: 3390,
            systemd_configured: Some(true),
            xrdp_installed: Some(true),
            xrdp_service_active: Some(true),
            xrdp_listening: Some(true),
            localhost_reachable: Some(false),
            pane_relay_available: Some(true),
            preferred_transport: Some(LaunchTransport::PaneRelay),
            xsession_present: Some(true),
            pane_session_assets_ready: Some(true),
            user_home_ready: Some(true),
            default_user_password_status: None,
        };

        let check = windows_transport_check(3390, &health).unwrap();
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.summary.contains("pane-relay"));
    }

    #[test]
    fn resolve_saved_launch_rejects_mismatched_session() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: true,
            hypothetical: true,
            bootstrap_requested: true,
            connect_requested: false,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let error = resolve_saved_launch(Some("other"), Some(&state)).unwrap_err();
        assert!(error.to_string().contains("only tracks one active session"));
    }

    #[test]
    fn status_prefers_saved_port_for_matching_distro() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: true,
            hypothetical: true,
            bootstrap_requested: true,
            connect_requested: false,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        assert_eq!(status_port_for("archlinux", Some(&state)), 4489);
        assert_eq!(status_port_for("ubuntu", Some(&state)), 3390);
    }

    #[test]
    fn resolve_status_distro_prefers_managed_environment() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                install_dir: None,
                source_rootfs: None,
                created_at_epoch_seconds: 1,
            }),
            last_launch: Some(StoredLaunch {
                session_name: "pane".to_string(),
                distro: DistroRecord {
                    name: "archlinux".to_string(),
                    ..DistroRecord::default()
                },
                desktop_environment: DesktopEnvironment::Xfce,
                port: 4489,
                workspace: WorkspacePaths {
                    root: "root".into(),
                    bootstrap_script: "bootstrap".into(),
                    rdp_profile: "rdp".into(),
                    bootstrap_log: "bootstrap.log".into(),
                    transport_log: "transport.log".into(),
                    shared_dir: "shared".into(),
                },
                stage: LaunchStage::Planned,
                dry_run: false,
                hypothetical: false,
                bootstrap_requested: true,
                connect_requested: true,
                transport: None,
                generated_at_epoch_seconds: 1,
                bootstrapped_at_epoch_seconds: None,
                rdp_launched_at_epoch_seconds: None,
                last_error: None,
            }),
        };

        let resolved = resolve_status_distro(None, &WslInventory::default(), Some(&state)).unwrap();
        assert_eq!(resolved.as_deref(), Some("pane-arch"));
    }

    #[test]
    fn resolve_init_source_defaults_to_online_provisioning() {
        let args = InitArgs {
            distro_name: "pane-arch".to_string(),
            existing_distro: None,
            rootfs_tar: None,
            install_dir: None,
            dry_run: true,
            json: false,
        };

        match resolve_init_source(&args, &WslInventory::default()).unwrap() {
            InitSource::InstallOnline {
                distro_name,
                install_dir,
            } => {
                assert_eq!(distro_name, "pane-arch");
                assert!(install_dir.ends_with(std::path::Path::new("distros").join("pane-arch")));
            }
            _ => panic!("expected Pane-owned online provisioning source"),
        }
    }

    #[test]
    fn dry_run_launch_allows_explicit_missing_distro_for_package_certification() {
        let inventory = WslInventory {
            available: true,
            distros: Vec::new(),
            ..WslInventory::default()
        };

        let target = resolve_launch_target(Some("pane-arch"), &inventory, None, true).unwrap();
        assert!(target.hypothetical);
        assert_eq!(target.distro.name, "pane-arch");
        assert_eq!(target.distro.family, DistroFamily::Arch);
    }

    #[test]
    fn init_preserves_existing_managed_environment_ownership_for_same_distro() {
        let args = InitArgs {
            distro_name: "pane-arch".to_string(),
            existing_distro: None,
            rootfs_tar: None,
            install_dir: None,
            dry_run: true,
            json: false,
        };
        let inventory = WslInventory {
            available: true,
            distros: vec![DistroRecord {
                name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            }],
            ..WslInventory::default()
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::InstalledOnline,
                install_dir: Some("D:/Pane/distros/pane-arch".into()),
                source_rootfs: None,
                created_at_epoch_seconds: 42,
            }),
            last_launch: None,
        };

        let report = initialize_managed_arch_environment(&args, &inventory, Some(&state)).unwrap();
        assert_eq!(
            report.managed_environment.ownership,
            ManagedEnvironmentOwnership::InstalledOnline
        );
        assert_eq!(report.managed_environment.created_at_epoch_seconds, 42);
        assert_eq!(
            report.managed_environment.install_dir,
            Some(std::path::PathBuf::from("D:/Pane/distros/pane-arch"))
        );
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("Preserved Pane ownership metadata")));
    }
    #[test]
    fn factory_reset_rejects_adopted_managed_distro() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                install_dir: None,
                source_rootfs: None,
                created_at_epoch_seconds: 1,
            }),
            last_launch: None,
        };
        let args = ResetArgs {
            session_name: None,
            distro: None,
            purge_wsl: false,
            purge_shared: false,
            release_managed_environment: false,
            factory_reset: true,
            dry_run: false,
        };

        let error = resolve_managed_environment_for_reset(&args, Some(&state)).unwrap_err();
        assert!(error
            .to_string()
            .contains("Factory reset is only supported for Pane-provisioned distros"));
    }

    #[test]
    fn build_distro_health_prefers_managed_environment_family_when_inventory_is_missing() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::ImportedRootfs,
                install_dir: Some("D:/Pane/distros/pane-arch".into()),
                source_rootfs: Some("D:/Downloads/archlinux.tar".into()),
                created_at_epoch_seconds: 1,
            }),
            last_launch: None,
        };

        let health =
            build_distro_health("pane-arch", &WslInventory::default(), Some(&state), None).unwrap();
        assert_eq!(health.distro.family, DistroFamily::Arch);
        assert!(health.supported_for_mvp);
        assert!(!health.present_in_inventory);
    }

    #[test]
    fn inspects_workspace_asset_presence() {
        let temp = std::env::temp_dir().join("pane-workspace-health-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let bootstrap = temp.join("pane-bootstrap.sh");
        let rdp = temp.join("pane.rdp");
        let log = temp.join("bootstrap.log");
        let shared = temp.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(&bootstrap, "echo ok").unwrap();
        std::fs::write(&log, "ok").unwrap();

        let health = inspect_workspace(&WorkspacePaths {
            root: temp.clone(),
            bootstrap_script: bootstrap,
            rdp_profile: rdp,
            bootstrap_log: log.clone(),
            transport_log: log.with_file_name("transport.log"),
            shared_dir: shared,
        });

        assert!(health.root_exists);
        assert!(health.shared_dir_exists);
        assert!(health.bootstrap_script_exists);
        assert!(!health.rdp_profile_exists);
        assert!(health.bootstrap_log_exists);

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn planned_launch_summary_state_starts_at_planned() {
        let plan = LaunchPlan {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            connect_after_bootstrap: false,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            bootstrap_script: "script".to_string(),
            rdp_profile: "profile".to_string(),
            steps: vec!["one".to_string()],
        };

        let launch = StoredLaunch::planned_from_plan(&plan, false, false, true, false);
        assert_eq!(launch.stage, LaunchStage::Planned);
        assert!(launch.bootstrap_requested);
        assert!(!launch.connect_requested);
    }

    #[test]
    fn doctor_skipped_checks_are_visible_but_not_blocking() {
        let report = DoctorReport {
            target_distro: None,
            session_name: "pane".to_string(),
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            bootstrap_requested: true,
            connect_requested: false,
            write_probes_enabled: false,
            supported_for_mvp: false,
            ready: true,
            selected_distro: None,
            workspace: WorkspaceHealth {
                root_exists: false,
                shared_dir_exists: false,
                bootstrap_script_exists: false,
                rdp_profile_exists: false,
                bootstrap_log_exists: false,
                transport_log_exists: false,
            },
            checks: vec![DoctorCheck {
                id: "workspace-writable".to_string(),
                status: CheckStatus::Skipped,
                summary: "Write probe skipped.".to_string(),
                remediation: None,
            }],
        };

        assert_eq!(CheckStatus::Skipped.display_name(), "SKIP");
        assert!(!report.has_failures());
    }

    #[test]
    fn blocker_formatting_includes_remediation() {
        let report = DoctorReport {
            target_distro: Some("archlinux".to_string()),
            session_name: "pane".to_string(),
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            bootstrap_requested: true,
            connect_requested: true,
            write_probes_enabled: true,
            supported_for_mvp: false,
            ready: false,
            selected_distro: Some(DistroHealth {
                distro: DistroRecord::default(),
                supported_for_mvp: false,
                present_in_inventory: false,
                checked_port: 3390,
                systemd_configured: None,
                xrdp_installed: None,
                xrdp_service_active: None,
                xrdp_listening: None,
                localhost_reachable: None,
                pane_relay_available: None,
                preferred_transport: None,
                xsession_present: None,
                pane_session_assets_ready: None,
                user_home_ready: None,
                default_user_password_status: None,
            }),
            workspace: WorkspaceHealth {
                root_exists: true,
                shared_dir_exists: true,
                bootstrap_script_exists: true,
                rdp_profile_exists: true,
                bootstrap_log_exists: false,
                transport_log_exists: false,
            },
            checks: vec![DoctorCheck {
                id: "xrdp-active".to_string(),
                status: CheckStatus::Fail,
                summary: "XRDP is not active.".to_string(),
                remediation: Some("Run pane launch again.".to_string()),
            }],
        };

        let rendered = format_doctor_blockers("pane connect", &report);
        assert!(rendered.contains("XRDP is not active."));
        assert!(rendered.contains("Run pane launch again."));
    }

    #[test]
    fn resolve_session_context_prefers_saved_launch_workspace() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: false,
            hypothetical: false,
            bootstrap_requested: true,
            connect_requested: true,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let (session_name, saved_launch, workspace) = resolve_session_context(None, Some(&state));
        assert_eq!(session_name, "pane");
        assert_eq!(saved_launch.unwrap().session_name, "pane");
        assert_eq!(workspace.root, std::path::PathBuf::from("root"));
    }

    #[test]
    fn bundle_output_path_uses_directory_targets_and_zip_extension() {
        let base = std::env::temp_dir().join(format!("pane-bundle-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let from_dir = resolve_bundle_output_path(Some(base.as_path()), "pane");
        assert_eq!(from_dir.parent(), Some(base.as_path()));
        assert_eq!(
            from_dir.extension().and_then(|value| value.to_str()),
            Some("zip")
        );

        let stem = base.join("support-bundle");
        let from_stem = resolve_bundle_output_path(Some(stem.as_path()), "pane");
        assert_eq!(
            from_stem.extension().and_then(|value| value.to_str()),
            Some("zip")
        );
        assert!(from_stem.ends_with("support-bundle.zip"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bundle_doctor_request_switches_to_reconnect_after_bootstrap() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            stage: LaunchStage::Bootstrapped,
            dry_run: false,
            hypothetical: false,
            bootstrap_requested: true,
            connect_requested: true,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: Some(2),
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 2,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let request = build_bundle_doctor_request(
            "pane",
            None,
            state.last_launch.as_ref(),
            Some(&state),
            &WslInventory::default(),
        )
        .unwrap();

        assert_eq!(request.distro.as_deref(), Some("archlinux"));
        assert_eq!(request.port, 4489);
        assert!(!request.bootstrap_requested);
        assert!(request.connect_requested);
    }

    #[test]
    fn environment_catalog_report_reflects_first_three_managed_environments() {
        let report = build_environment_catalog_report();
        assert_eq!(report.environments.len(), 3);
        assert_eq!(report.environments[0].id, "arch");
        assert!(report.environments[0].launchable_now);
        assert_eq!(report.environments[1].id, "ubuntu-lts");
        assert_eq!(report.environments[2].id, "debian");
        assert!(report.notes.iter().any(|note| note.contains("Kali")));
    }

    #[test]
    fn inventory_contains_distro_is_case_insensitive() {
        let inventory = WslInventory {
            available: true,
            distros: vec![DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            }],
            ..WslInventory::default()
        };

        assert!(inventory_contains_distro(&inventory, "ARCHLINUX"));
    }

    #[test]
    fn ensure_wsl_conf_setting_replaces_existing_key_and_appends_missing_sections() {
        let raw = "[boot]\nsystemd=false\n[user]\ndefault=root\n";
        let updated = ensure_wsl_conf_setting(
            &ensure_wsl_conf_setting(raw, "boot", "systemd", "true"),
            "user",
            "default",
            "archuser",
        );
        assert!(updated.contains("[boot]\nsystemd=true\n"));
        assert!(updated.contains("[user]\ndefault=archuser\n"));

        let appended = ensure_wsl_conf_setting(
            "[network]\ngenerateResolvConf=false\n",
            "boot",
            "systemd",
            "true",
        );
        assert!(appended.contains("[network]\ngenerateResolvConf=false\n\n[boot]\nsystemd=true\n"));
    }

    #[test]
    fn setup_user_validation_rejects_unsafe_values() {
        assert!(validate_setup_username("root").is_err());
        assert!(validate_setup_username("ArchUser").is_err());
        assert!(validate_setup_password("bad:password").is_err());
        assert!(validate_setup_password("line\nbreak").is_err());
        assert!(validate_setup_username("arch-user").is_ok());
        assert!(validate_setup_password("safe-password").is_ok());
    }
}
