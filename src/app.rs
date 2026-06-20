#![allow(clippy::uninlined_format_args)]

use std::{
    env,
    fs::{self, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use clap::Parser;
use linux_loader::{
    cmdline::Cmdline,
    configurator::{linux::LinuxBootConfigurator, BootConfigurator, BootParams},
    loader::{bootparam, bzimage::BzImage, load_cmdline, KernelLoader},
};
use serde::{Deserialize, Serialize};
use vm_memory::{Bytes, GuestAddress, GuestMemoryMmap};

use crate::{
    bootstrap::{render_bootstrap_script, render_update_script},
    cli::{
        AppStatusArgs, BundleArgs, Cli, Commands, ConnectArgs, DoctorArgs, EnvironmentsArgs,
        InitArgs, LaunchArgs, LogsArgs, NativeBootSpikeArgs, NativeFoundationArgs,
        NativeKernelPlanArgs, NativePreflightArgs, OnboardArgs, RelayArgs, RepairArgs, ResetArgs,
        RuntimeArgs, SetupUserArgs, ShareArgs, StatusArgs, StopArgs, TerminalArgs, UpdateArgs,
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
    vmm_foundation: crate::vmm_foundation::VmmFoundationReport,
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
    device_loop: crate::native::NativeDeviceLoopReport,
    ready_for_boot_spike: bool,
    ready_for_arch_boot_attempt: bool,
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
    device_loop: crate::native::NativeDeviceLoopReport,
    ready_for_serial_kernel_spike: bool,
    ready_for_arch_boot_attempt: bool,
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
    initramfs_driver_dir: String,
    user_disk: String,
    base_os_metadata: String,
    serial_boot_metadata: String,
    boot_loader_metadata: String,
    kernel_boot_metadata: String,
    initramfs_driver_metadata: String,
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
    base_os_image_format: Option<String>,
    base_os_bootable_disk_hint: Option<bool>,
    base_os_root_partition_hint: Option<bool>,
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
    initramfs_driver_bundle_exists: bool,
    initramfs_driver_metadata_exists: bool,
    initramfs_driver_bundle_ready: bool,
    discovery_initramfs_matches_driver_bundle: bool,
    pane_block_module_exists: bool,
    pane_block_module_bytes: Option<u64>,
    pane_block_module_sha256: Option<String>,
    pane_block_module_verified: bool,
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
    user_disk_snapshot_count: usize,
    latest_user_disk_snapshot: Option<String>,
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
    #[serde(default = "unknown_base_os_image_format")]
    image_format: String,
    #[serde(default)]
    bootable_disk_hint: bool,
    #[serde(default)]
    partitions: Vec<BaseOsPartition>,
    #[serde(default)]
    root_partition_hint: Option<BaseOsPartition>,
    #[serde(default)]
    root_filesystem_hint: Option<String>,
    verified: bool,
    registered_at_epoch_seconds: u64,
    #[serde(default)]
    notes: Vec<String>,
}

fn unknown_base_os_image_format() -> String {
    "unknown".to_string()
}

#[derive(Clone, Debug)]
struct BaseOsImageInspection {
    image_format: String,
    bootable_disk_hint: bool,
    partitions: Vec<BaseOsPartition>,
    root_partition_hint: Option<BaseOsPartition>,
    root_filesystem_hint: Option<String>,
    notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BaseOsPartition {
    index: u32,
    scheme: String,
    partition_type: String,
    bootable: bool,
    start_lba: u64,
    sector_count: u64,
    byte_offset: u64,
    byte_length: u64,
    root_candidate: bool,
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

#[derive(Debug, Deserialize)]
struct NativeBootSetManifest {
    schema_version: u32,
    #[serde(default)]
    distro_family: Option<String>,
    base_image: PathBuf,
    base_image_sha256: String,
    kernel: PathBuf,
    kernel_sha256: String,
    initramfs: PathBuf,
    initramfs_sha256: String,
    kernel_cmdline: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PaneInitramfsDriverMetadata {
    schema_version: u32,
    bundle_kind: String,
    driver_dir: String,
    hook_path: String,
    header_path: String,
    init_source_path: String,
    probe_source_path: String,
    block_driver_source_path: String,
    block_driver_build_script_path: String,
    build_script_path: String,
    readme_path: String,
    hook_sha256: String,
    header_sha256: String,
    init_source_sha256: String,
    probe_source_sha256: String,
    block_driver_source_sha256: String,
    #[serde(default = "pane_block_driver_abi_sha256")]
    block_driver_abi_sha256: String,
    block_driver_build_script_sha256: String,
    build_script_sha256: String,
    readme_sha256: String,
    #[serde(default)]
    packaged_initramfs_path: Option<String>,
    #[serde(default)]
    packaged_initramfs_bytes: Option<u64>,
    #[serde(default)]
    packaged_initramfs_sha256: Option<String>,
    #[serde(default)]
    packaged_hook_sha256: Option<String>,
    #[serde(default)]
    packaged_init_source_sha256: Option<String>,
    #[serde(default)]
    packaged_probe_source_sha256: Option<String>,
    #[serde(default)]
    packaged_init_binary_sha256: Option<String>,
    #[serde(default)]
    packaged_probe_binary_sha256: Option<String>,
    #[serde(default)]
    packaged_binary_provenance: Option<String>,
    #[serde(default)]
    packaged_block_driver_source_sha256: Option<String>,
    #[serde(default)]
    packaged_block_driver_abi_sha256: Option<String>,
    #[serde(default)]
    packaged_block_module_sha256: Option<String>,
    block_io_protocol: String,
    block_io_port_base: String,
    block_io_port_count: u16,
    block_io_status_port_offset: u16,
    block_io_data_port_offset: u16,
    block_io_block_size_bytes: u64,
    generated_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct VirtioMmioModuleMetadata {
    schema_version: u32,
    module_kind: String,
    source_path: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    expected_sha256: Option<String>,
    verified: bool,
    #[serde(default)]
    target_kernel_path: Option<String>,
    #[serde(default)]
    target_kernel_sha256: Option<String>,
    registered_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PaneBlockModuleMetadata {
    schema_version: u32,
    module_kind: String,
    source_path: String,
    stored_path: String,
    bytes: u64,
    sha256: String,
    expected_sha256: Option<String>,
    verified: bool,
    #[serde(default)]
    target_kernel_path: Option<String>,
    #[serde(default)]
    target_kernel_bytes: Option<u64>,
    #[serde(default)]
    target_kernel_sha256: Option<String>,
    #[serde(default)]
    target_kernel_format: Option<String>,
    #[serde(default)]
    block_driver_source_sha256: Option<String>,
    #[serde(default)]
    block_driver_abi_sha256: Option<String>,
    registered_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KernelBootLayout {
    schema_version: u32,
    layout_kind: String,
    session_name: String,
    #[serde(default = "legacy_linux_loader_adapter_plan")]
    linux_loader: LinuxLoaderAdapterPlan,
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
    #[serde(default)]
    expected_serial_milestones: Vec<String>,
    storage: Option<KernelStorageAttachment>,
    initramfs_driver: Option<PaneInitramfsDriverMetadata>,
    framebuffer: Option<FramebufferContract>,
    input: Option<InputContract>,
    materialized_at_epoch_seconds: Option<u64>,
    notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LinuxLoaderAdapterPlan {
    schema_version: u32,
    adapter_kind: String,
    source_crate: String,
    candidate_crate_version: Option<String>,
    license: String,
    source_url: String,
    adoption_state: String,
    applicable: bool,
    kernel_format: String,
    linux_boot_protocol: Option<String>,
    kernel_loader: String,
    cmdline_loader: String,
    boot_params_writer: String,
    guest_memory_backend: String,
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
    #[serde(default = "default_base_os_block_size_bytes")]
    base_os_block_size_bytes: u64,
    #[serde(default = "default_pane_block_io_protocol")]
    block_io_protocol: String,
    #[serde(default = "default_pane_block_io_port_base")]
    block_io_port_base: String,
    #[serde(default = "default_pane_block_io_port_count")]
    block_io_port_count: u16,
    #[serde(default = "default_pane_block_io_status_port_offset")]
    block_io_status_port_offset: u16,
    #[serde(default = "default_pane_block_io_data_port_offset")]
    block_io_data_port_offset: u16,
    #[serde(default = "default_pane_block_io_block_size_bytes")]
    block_io_block_size_bytes: u64,
    #[serde(default = "default_pane_block_dma_gpa")]
    block_dma_gpa: String,
    #[serde(default = "default_pane_block_dma_size_bytes")]
    block_dma_size_bytes: u64,
    #[serde(default = "legacy_virtio_block_backend_plan")]
    virtio_block: VirtioBlockBackendPlan,
    base_os_image_format: String,
    base_os_bootable_disk_hint: bool,
    base_os_partitions: Vec<BaseOsPartition>,
    base_os_root_partition_hint: Option<BaseOsPartition>,
    #[serde(default = "default_kernel_root_handoff")]
    root_handoff: KernelRootHandoff,
    user_disk_path: String,
    user_disk_capacity_gib: u64,
    user_disk_logical_size_bytes: u64,
    user_disk_block_size_bytes: u64,
    user_disk_sparse_backing: bool,
    user_disk_header_sha256: String,
    user_disk_format: String,
    root_device: String,
    user_device: String,
    contract_gpa: String,
    readonly_base: bool,
    writable_user_disk: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct VirtioBlockBackendPlan {
    schema_version: u32,
    backend_kind: String,
    source_crate: String,
    candidate_crate_version: Option<String>,
    license: String,
    source_url: String,
    adoption_state: String,
    transport: String,
    #[serde(default = "default_virtio_mmio_base_gpa")]
    mmio_base_gpa: String,
    #[serde(default = "default_virtio_mmio_size_bytes")]
    mmio_size_bytes: u64,
    #[serde(default = "default_virtio_mmio_irq")]
    mmio_irq: u32,
    #[serde(default = "default_virtio_mmio_linux_kernel_parameter")]
    linux_kernel_parameter: String,
    queue_model: String,
    interrupt_model: String,
    sector_size_bytes: u64,
    root_device_hint: String,
    replaces: String,
    devices: Vec<VirtioBlockDevicePlan>,
    notes: Vec<String>,
}

impl VirtioBlockBackendPlan {
    fn boot_contract_matches(&self, expected: &Self) -> bool {
        let mut actual_contract = self.clone();
        let mut expected_contract = expected.clone();
        actual_contract.notes.clear();
        expected_contract.notes.clear();
        actual_contract == expected_contract
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct VirtioBlockDevicePlan {
    id: String,
    role: String,
    guest_device_hint: String,
    backend_path: String,
    readonly: bool,
    sparse_backing: bool,
    logical_size_bytes: u64,
    block_size_bytes: u64,
    root_partition_index: Option<u32>,
    root_partition_byte_offset: Option<u64>,
    root_partition_byte_length: Option<u64>,
    filesystem_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct KernelRootHandoff {
    schema_version: u32,
    mode: String,
    root_device: String,
    base_device: String,
    partition_index: Option<u32>,
    partition_byte_offset: Option<u64>,
    partition_byte_length: Option<u64>,
    filesystem_hint: Option<String>,
    requires_initramfs_driver: bool,
    notes: Vec<String>,
}

fn default_kernel_root_handoff() -> KernelRootHandoff {
    KernelRootHandoff {
        schema_version: 1,
        mode: "base-device".to_string(),
        root_device: "/dev/pane0".to_string(),
        base_device: "/dev/pane0".to_string(),
        partition_index: None,
        partition_byte_offset: None,
        partition_byte_length: None,
        filesystem_hint: None,
        requires_initramfs_driver: true,
        notes: vec![
            "Legacy layout without explicit root handoff; Pane will treat the whole base device as root."
                .to_string(),
        ],
    }
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
    #[serde(default = "default_input_queue_header_bytes")]
    queue_header_bytes: u32,
    #[serde(default = "default_input_queue_magic")]
    queue_magic: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
struct UserDiskSnapshotMetadata {
    schema_version: u32,
    snapshot_kind: String,
    snapshot_id: String,
    source_disk_path: String,
    source_metadata_path: String,
    snapshot_path: String,
    source_disk_bytes: u64,
    source_disk_sha256: String,
    user_disk_capacity_gib: u64,
    user_disk_logical_size_bytes: u64,
    user_disk_block_size_bytes: u64,
    created_at_epoch_seconds: u64,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserDiskExportManifest {
    schema_version: u32,
    export_kind: String,
    export_id: String,
    exported_disk_filename: String,
    exported_metadata_filename: String,
    source_disk_path: String,
    source_metadata_path: String,
    source_disk_bytes: u64,
    source_disk_sha256: String,
    user_disk_capacity_gib: u64,
    user_disk_logical_size_bytes: u64,
    user_disk_block_size_bytes: u64,
    exported_at_epoch_seconds: u64,
    notes: Vec<String>,
}

const PANE_USER_DISK_FORMAT: &str = "pane-sparse-user-disk-v1";
const PANE_USER_DISK_MAGIC: &str = "PANE_USER_DISK_V1\n";
const PANE_USER_DISK_BLOCK_SIZE_BYTES: u64 = 4096;
const PANE_BASE_OS_BLOCK_SIZE_BYTES: u64 = 4096;
const PANE_USER_DISK_EXPORT_MANIFEST_FILENAME: &str = "pane-user-disk-export.json";
const PANE_USER_DISK_EXPORT_DISK_FILENAME: &str = "user-data.panedisk";
const PANE_USER_DISK_EXPORT_METADATA_FILENAME: &str = "user-disk.json";

fn default_base_os_block_size_bytes() -> u64 {
    PANE_BASE_OS_BLOCK_SIZE_BYTES
}

fn default_pane_block_io_protocol() -> String {
    "pane-port-block-v1".to_string()
}

fn default_pane_block_io_port_base() -> String {
    format!("{:#06x}", crate::native::PANE_BLOCK_IO_BASE_PORT)
}

fn default_pane_block_io_port_count() -> u16 {
    crate::native::PANE_BLOCK_IO_PORT_COUNT
}

fn default_pane_block_io_status_port_offset() -> u16 {
    2
}

fn default_pane_block_io_data_port_offset() -> u16 {
    12
}

fn default_pane_block_io_block_size_bytes() -> u64 {
    u64::from(crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES)
}

const COMPATIBLE_PANE_BLOCK_DRIVER_SOURCE_SHA256_BY_ABI: &[&str] = &[
    // Shared-DMA, 4096-byte block ABI before successful status logging was capped.
    "dc1a49843850c2122003f0cbd467285f2a469a266a7209c214f4e5bc7053381f",
];

fn pane_block_driver_abi_sha256() -> String {
    sha256_bytes(pane_block_driver_abi_contract().as_bytes())
}

fn pane_block_driver_abi_contract() -> String {
    format!(
        "pane-linux-block-module-abi-v1\nprotocol={}\nbase_port={}\nport_count={}\nstatus_offset={}\ndata_offset={}\nblock_size_bytes={}\nsector_size_bytes=512\ndevice_count=2\nminors_per_disk=16\nbase_device=pane0\nuser_device=pane1\noperations=read,write,flush,discard\nshared_buffer=optional-memremap\n",
        default_pane_block_io_protocol(),
        default_pane_block_io_port_base(),
        default_pane_block_io_port_count(),
        default_pane_block_io_status_port_offset(),
        default_pane_block_io_data_port_offset(),
        default_pane_block_io_block_size_bytes(),
    )
}

fn pane_block_module_matches_current_driver_abi(
    module_metadata: &PaneBlockModuleMetadata,
    initramfs_driver_metadata: &PaneInitramfsDriverMetadata,
) -> bool {
    let expected_abi_sha256 = pane_block_driver_abi_sha256();
    if module_metadata.block_driver_abi_sha256.as_deref() == Some(expected_abi_sha256.as_str()) {
        return true;
    }

    let Some(source_sha256) = module_metadata.block_driver_source_sha256.as_deref() else {
        return false;
    };

    source_sha256 == initramfs_driver_metadata.block_driver_source_sha256
        || COMPATIBLE_PANE_BLOCK_DRIVER_SOURCE_SHA256_BY_ABI.contains(&source_sha256)
}

fn pane_discovery_initramfs_matches_current_driver_bundle(
    paths: &RuntimePaths,
    initramfs_driver_metadata: Option<&PaneInitramfsDriverMetadata>,
    pane_block_module_metadata: Option<&PaneBlockModuleMetadata>,
    initramfs_image_bytes: Option<u64>,
    initramfs_actual_sha256: Option<&String>,
) -> bool {
    let Some(metadata) = initramfs_driver_metadata else {
        return false;
    };
    metadata.packaged_initramfs_path.as_deref()
        == Some(paths.initramfs_image.display().to_string().as_str())
        && metadata.packaged_initramfs_bytes == initramfs_image_bytes
        && metadata.packaged_initramfs_sha256.as_ref() == initramfs_actual_sha256
        && metadata.packaged_hook_sha256.as_deref() == Some(metadata.hook_sha256.as_str())
        && metadata.packaged_init_source_sha256.as_deref()
            == Some(metadata.init_source_sha256.as_str())
        && metadata.packaged_probe_source_sha256.as_deref()
            == Some(metadata.probe_source_sha256.as_str())
        && metadata.packaged_binary_provenance.as_deref() == Some("compiled-from-current-source")
        && metadata.packaged_block_driver_source_sha256.as_deref()
            == Some(metadata.block_driver_source_sha256.as_str())
        && metadata.packaged_block_driver_abi_sha256.as_deref()
            == Some(metadata.block_driver_abi_sha256.as_str())
        && metadata.packaged_block_module_sha256.as_deref()
            == pane_block_module_metadata.map(|metadata| metadata.sha256.as_str())
}

fn pane_initramfs_driver_metadata_matches_current_sources(
    metadata: &PaneInitramfsDriverMetadata,
) -> bool {
    metadata.hook_sha256 == sha256_bytes(pane_initramfs_hook_source().as_bytes())
        && metadata.header_sha256 == sha256_bytes(pane_port_block_header_source().as_bytes())
        && metadata.init_source_sha256 == sha256_bytes(pane_init_source().as_bytes())
        && metadata.probe_source_sha256 == sha256_bytes(pane_port_probe_source().as_bytes())
        && metadata.block_driver_source_sha256
            == sha256_bytes(pane_block_driver_source().as_bytes())
        && metadata.block_driver_abi_sha256 == pane_block_driver_abi_sha256()
        && metadata.block_driver_build_script_sha256
            == sha256_bytes(pane_block_driver_build_script().as_bytes())
        && metadata.build_script_sha256 == sha256_bytes(pane_initramfs_build_script().as_bytes())
        && metadata.readme_sha256 == sha256_bytes(pane_initramfs_driver_readme().as_bytes())
}

fn default_pane_block_dma_gpa() -> String {
    "0x0dfd0000".to_string()
}

fn default_pane_block_dma_size_bytes() -> u64 {
    0x00001000
}

fn storage_contract_size_bytes() -> u64 {
    0x00002000
}

fn default_virtio_mmio_base_gpa() -> String {
    crate::virtio::format_guest_physical_address(crate::virtio::PANE_VIRTIO_MMIO_BASE_GPA)
}

fn default_virtio_mmio_size_bytes() -> u64 {
    crate::virtio::PANE_VIRTIO_MMIO_SIZE_BYTES
}

fn default_virtio_mmio_irq() -> u32 {
    crate::virtio::PANE_VIRTIO_MMIO_IRQ
}

fn default_virtio_mmio_linux_kernel_parameter() -> String {
    crate::virtio::pane_virtio_mmio_kernel_arg()
}

fn virtio_block_backend_plan(storage: &KernelStorageAttachment) -> VirtioBlockBackendPlan {
    let root_device_hint = storage
        .root_handoff
        .partition_index
        .map(|index| format!("/dev/vda{index}"))
        .unwrap_or_else(|| "/dev/vda".to_string());

    VirtioBlockBackendPlan {
        schema_version: 1,
        backend_kind: "pane-virtio-blk-backend-plan-v1".to_string(),
        source_crate: "rust-vmm/virtio-queue".to_string(),
        candidate_crate_version: Some("0.17.0".to_string()),
        license: "Apache-2.0 AND BSD-3-Clause".to_string(),
        source_url: "https://github.com/rust-vmm/vm-virtio".to_string(),
        adoption_state: "live-whp-mmio-execution-and-irq-request-ready-guest-ack-pending"
            .to_string(),
        transport: "virtio-mmio".to_string(),
        mmio_base_gpa: default_virtio_mmio_base_gpa(),
        mmio_size_bytes: default_virtio_mmio_size_bytes(),
        mmio_irq: default_virtio_mmio_irq(),
        linux_kernel_parameter: default_virtio_mmio_linux_kernel_parameter(),
        queue_model: "rust-vmm-virtio-queue-split-ring-batch-drain-ready".to_string(),
        interrupt_model: "WHP interrupt injection through Pane device loop".to_string(),
        sector_size_bytes: 512,
        root_device_hint,
        replaces: "pane-port-block-v1 plus generated pane-block.ko root storage".to_string(),
        devices: vec![
            VirtioBlockDevicePlan {
                id: "vda".to_string(),
                role: "read-only-arch-base-os".to_string(),
                guest_device_hint: "/dev/vda".to_string(),
                backend_path: storage.base_os_path.clone(),
                readonly: true,
                sparse_backing: false,
                logical_size_bytes: storage.base_os_bytes,
                block_size_bytes: storage.base_os_block_size_bytes,
                root_partition_index: storage.root_handoff.partition_index,
                root_partition_byte_offset: storage.root_handoff.partition_byte_offset,
                root_partition_byte_length: storage.root_handoff.partition_byte_length,
                filesystem_hint: storage.root_handoff.filesystem_hint.clone(),
            },
            VirtioBlockDevicePlan {
                id: "vdb".to_string(),
                role: "writable-pane-user-disk".to_string(),
                guest_device_hint: "/dev/vdb".to_string(),
                backend_path: storage.user_disk_path.clone(),
                readonly: false,
                sparse_backing: storage.user_disk_sparse_backing,
                logical_size_bytes: storage.user_disk_logical_size_bytes,
                block_size_bytes: storage.user_disk_block_size_bytes,
                root_partition_index: None,
                root_partition_byte_offset: None,
                root_partition_byte_length: None,
                filesystem_hint: None,
            },
        ],
        notes: vec![
            "This is the standard Linux block-device target for Pane-owned Arch boot; Pane routes live WHP virtio-MMIO exits through rust-vmm queue parsing into the Pane-owned block backend."
                .to_string(),
            "Pane reserves a virtio-MMIO aperture, advertises it with Linux's virtio_mmio.device kernel parameter, executes WHP MMIO exits through WinHvEmulation.dll, negotiates Linux-compatible modern virtio-blk features, masks unsupported driver feature bits, refuses FEATURES_OK until negotiation is valid, blocks DRIVER_OK until FEATURES_OK is accepted, reports non-existent selected queues as absent, resets queue runtime state when QueueReady is cleared, ignores queue notifications until DRIVER_OK and QueueReady are both set, drains batched split-virtqueue notifications into the verified native block handler, consumes malformed descriptor chains with used-ring error completion, accepts empty kicks and guest-visible block errors without aborting WHP emulation, reports actual used-ring transfer lengths, denies read-only base-disk writes at the device boundary, records callback details, and requests the virtio IRQ after queue completion; guest IRQ acknowledgement and root-mount proof are the remaining storage milestone."
                .to_string(),
            "The existing Pane block-port contract remains only as the current diagnostic bridge until the virtio device loop is implemented."
                .to_string(),
        ],
    }
}

fn legacy_virtio_block_backend_plan() -> VirtioBlockBackendPlan {
    VirtioBlockBackendPlan {
        schema_version: 0,
        backend_kind: "legacy-no-virtio-block-contract".to_string(),
        source_crate: "none".to_string(),
        candidate_crate_version: None,
        license: "not-applicable".to_string(),
        source_url: String::new(),
        adoption_state: "legacy-layout-rematerialize-required".to_string(),
        transport: "none".to_string(),
        mmio_base_gpa: String::new(),
        mmio_size_bytes: 0,
        mmio_irq: 0,
        linux_kernel_parameter: String::new(),
        queue_model: "none".to_string(),
        interrupt_model: "none".to_string(),
        sector_size_bytes: 0,
        root_device_hint: String::new(),
        replaces: "none".to_string(),
        devices: Vec::new(),
        notes: vec![
            "This layout predates the virtio-blk backend contract and must be rematerialized before the native storage path can progress."
                .to_string(),
        ],
    }
}

#[derive(Debug, Serialize)]
struct NativeRuntimeReport {
    state: NativeRuntimeState,
    state_label: &'static str,
    bootable: bool,
    host_ready: bool,
    ready_for_boot_spike: bool,
    ready_for_arch_boot_attempt: bool,
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
        Commands::Runtime(args) => runtime(*args),
        Commands::NativePreflight(args) => native_preflight(args),
        Commands::NativeBootSpike(args) => native_boot_spike(args),
        Commands::NativeKernelPlan(args) => native_kernel_plan(args),
        Commands::NativeFoundation(args) => native_foundation(args),
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
        "  Arch Boot Try  {}",
        yes_no(report.native_runtime.ready_for_arch_boot_attempt)
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
    let registering_native_boot_set =
        args.register_native_boot_set || args.register_native_boot_set_manifest.is_some();
    if !args.build_discovery_initramfs
        && (args.discovery_init_binary.is_some() || args.discovery_probe_binary.is_some())
    {
        return Err(AppError::message(
            "--discovery-init-binary and --discovery-probe-binary require --build-discovery-initramfs.",
        ));
    }
    if args.build_discovery_initramfs
        && (args.discovery_init_binary.is_some() != args.discovery_probe_binary.is_some())
    {
        return Err(AppError::message(
            "--discovery-init-binary and --discovery-probe-binary must be provided together.",
        ));
    }
    let has_runtime_mutation = args.register_base_image.is_some()
        || registering_native_boot_set
        || args.write_native_boot_set_manifest_template.is_some()
        || args.register_boot_loader.is_some()
        || args.register_kernel.is_some()
        || args.register_initramfs.is_some()
        || args.kernel_cmdline.is_some()
        || args.write_initramfs_driver
        || args.build_discovery_initramfs
        || args.build_pane_block_module
        || args.register_pane_block_module.is_some()
        || args.register_virtio_mmio_module.is_some()
        || args.create_user_disk
        || args.snapshot_user_disk
        || args.restore_user_disk_snapshot.is_some()
        || args.import_user_disk.is_some()
        || args.resize_user_disk_gib.is_some()
        || args.repair_user_disk
        || args.create_serial_boot_image
        || args.prepare;

    if has_runtime_mutation {
        prepare_runtime_paths(&paths)?;
        write_runtime_config(&paths, &session_name, &budget)?;
        write_native_runtime_manifest(&paths, &session_name)?;
        write_framebuffer_contract(&paths)?;
        write_input_contract(&paths)?;
    }

    if let Some(manifest) = args.register_native_boot_set_manifest.as_deref() {
        register_native_boot_set_from_manifest(&paths, manifest, &args)?;
    } else if args.register_native_boot_set {
        register_native_boot_set(&paths, &args)?;
    } else if let Some(source_image) = args.register_base_image.as_deref() {
        register_base_os_image(
            &paths,
            source_image,
            args.expected_sha256.as_deref(),
            args.require_native_root_disk,
            args.force,
        )?;
    } else if args.require_native_root_disk {
        return Err(AppError::message(
            "--require-native-root-disk requires --register-base-image.",
        ));
    } else if args.expected_sha256.is_some() {
        return Err(AppError::message(
            "--expected-sha256 requires --register-base-image.",
        ));
    }

    if let Some(template_path) = args.write_native_boot_set_manifest_template.as_deref() {
        write_native_boot_set_manifest_template(template_path, args.force)?;
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

    if !registering_native_boot_set
        && (args.register_kernel.is_some()
            || args.register_initramfs.is_some()
            || args.kernel_cmdline.is_some())
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
    } else if !registering_native_boot_set
        && (args.kernel_expected_sha256.is_some() || args.initramfs_expected_sha256.is_some())
    {
        return Err(AppError::message(
            "--kernel-expected-sha256 requires --register-kernel, and --initramfs-expected-sha256 requires --register-initramfs.",
        ));
    }

    let refresh_initramfs_driver_for_discovery = args.build_discovery_initramfs
        && (args.force
            || load_verified_pane_initramfs_driver_metadata(&paths)
                .map(|metadata| !pane_initramfs_driver_metadata_matches_current_sources(&metadata))
                .unwrap_or(true));
    if args.write_initramfs_driver || refresh_initramfs_driver_for_discovery {
        write_pane_initramfs_driver_bundle(&paths)?;
    }

    if args.build_pane_block_module && args.register_pane_block_module.is_some() {
        return Err(AppError::message(
            "--build-pane-block-module and --register-pane-block-module are mutually exclusive.",
        ));
    }
    if !args.build_pane_block_module && args.kernel_build_dir.is_some() {
        return Err(AppError::message(
            "--kernel-build-dir requires --build-pane-block-module.",
        ));
    }

    if args.build_pane_block_module {
        build_and_register_pane_block_module(&paths, args.kernel_build_dir.as_deref(), args.force)?;
    } else if let Some(module) = args.register_pane_block_module.as_deref() {
        register_pane_block_module(
            &paths,
            module,
            args.pane_block_module_expected_sha256.as_deref(),
            args.force,
        )?;
    } else if args.pane_block_module_expected_sha256.is_some() {
        return Err(AppError::message(
            "--pane-block-module-expected-sha256 requires --register-pane-block-module.",
        ));
    }

    if let Some(module) = args.register_virtio_mmio_module.as_deref() {
        register_virtio_mmio_module(
            &paths,
            module,
            args.virtio_mmio_module_expected_sha256.as_deref(),
            args.force,
        )?;
    } else if args.virtio_mmio_module_expected_sha256.is_some() {
        return Err(AppError::message(
            "--virtio-mmio-module-expected-sha256 requires --register-virtio-mmio-module.",
        ));
    }

    if args.build_discovery_initramfs {
        build_and_register_pane_discovery_initramfs(
            &paths,
            args.discovery_init_binary.as_deref(),
            args.discovery_probe_binary.as_deref(),
            args.force,
        )?;
    }

    if args.create_user_disk {
        create_user_disk_descriptor(&paths, &budget, args.force)?;
    }

    if args.snapshot_user_disk {
        create_user_disk_snapshot(&paths)?;
    }

    if let Some(snapshot_metadata) = args.restore_user_disk_snapshot.as_deref() {
        restore_user_disk_snapshot(&paths, snapshot_metadata)?;
    }

    if let Some(import_package) = args.import_user_disk.as_deref() {
        import_user_disk_package(&paths, import_package)?;
    }

    if let Some(new_capacity_gib) = args.resize_user_disk_gib {
        resize_user_disk(&paths, new_capacity_gib)?;
    }

    if args.repair_user_disk {
        repair_user_disk_metadata(&paths)?;
    }

    if args.create_serial_boot_image {
        create_serial_boot_image_artifact(&paths, args.force)?;
    }

    if let Some(export_dir) = args.export_user_disk.as_deref() {
        export_user_disk_package(&paths, export_dir, args.force)?;
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
    if args.prepare_runtime {
        prepare_native_runtime_boundary(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB)?;
    }
    let runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let host = runtime.native_host.clone();
    let ready_for_boot_spike = runtime.native_runtime.ready_for_boot_spike;
    let ready_for_arch_boot_attempt = runtime.native_runtime.ready_for_arch_boot_attempt;
    let blockers = runtime.native_runtime.blockers.clone();
    let mut next_steps = host.next_steps.clone();
    next_steps.extend(runtime.next_steps.clone());
    next_steps.dedup();

    let report = NativePreflightReport {
        product_shape: "Pane native-runtime preflight for moving from runtime artifacts to a WHP boot/display engine.",
        session_name,
        host,
        runtime,
        device_loop: crate::native::native_device_loop_report(),
        ready_for_boot_spike,
        ready_for_arch_boot_attempt,
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
    if args.prepare_runtime {
        prepare_native_runtime_boundary(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB)?;
    }
    let mut runtime = build_runtime_report(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB, false)?;
    let arch_boot_attempt_requested = args.execute && args.run_kernel_layout;
    let arch_boot_attempt_ready =
        !arch_boot_attempt_requested || runtime.native_runtime.ready_for_arch_boot_attempt;
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
    } else if arch_boot_attempt_requested && arch_boot_attempt_ready {
        Some(load_kernel_layout_boot_image_artifact(&runtime_paths)?)
    } else {
        None
    };
    let host = runtime.native_host.clone();
    let runtime_block_io_handler =
        |command: &crate::native::NativeBlockIoCommand, write_payload: Option<&[u8]>| {
            execute_native_block_io_command(&runtime_paths, command, write_payload)
                .map_err(|error| error.to_string())
        };
    let block_io_handler: Option<&crate::native::NativeBlockIoHandler<'_>> =
        if arch_boot_attempt_requested && arch_boot_attempt_ready {
            Some(&runtime_block_io_handler)
        } else {
            None
        };
    if let Some(path) = &args.trace_checkpoint {
        initialize_native_boot_trace_checkpoint(path, &session_name)?;
    }
    let partition_smoke = crate::native::run_partition_smoke(
        args.execute,
        run_guest_image,
        boot_image.as_ref(),
        &host,
        block_io_handler,
        args.trace_checkpoint.as_deref(),
    );
    let protected_linux_entry_requested =
        partition_smoke.entry_mode.as_deref() == Some("linux-protected-mode-32");
    let ready_for_arch_boot_attempt = runtime.native_runtime.ready_for_arch_boot_attempt;
    let ready_for_serial_kernel_spike = args.execute
        && run_guest_image
        && partition_smoke.status == crate::native::NativePartitionSmokeStatus::Pass
        && host.ready_for_boot_spike
        && runtime.prepared
        && runtime.artifacts.runtime_config_exists
        && runtime.artifacts.native_manifest_exists
        && (!args.run_fixture || runtime.artifacts.serial_boot_image_ready)
        && (!args.run_boot_loader || runtime.artifacts.boot_loader_image_verified)
        && (!args.run_kernel_layout || runtime.native_runtime.ready_for_arch_boot_attempt)
        && (!protected_linux_entry_requested
            || if partition_smoke.serial_expected_markers.is_empty() {
                partition_smoke.serial_text.is_some()
            } else {
                partition_smoke.serial_markers_observed
            });

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
            "No materialized Pane kernel boot layout exists. Run `pane native-kernel-plan --prepare-runtime --materialize` after registering a verified kernel plan."
                .to_string(),
        );
    }
    if arch_boot_attempt_requested && !runtime.native_runtime.ready_for_arch_boot_attempt {
        blockers.extend(runtime.native_runtime.blockers.clone());
    }
    if run_guest_image {
        if !runtime.prepared {
            blockers.push(
                "Dedicated runtime directories have not been prepared. Run `pane native-boot-spike --prepare-runtime`, or `pane runtime --prepare`."
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
        && args.run_kernel_layout
        && protected_linux_entry_requested
        && !partition_smoke.serial_expected_markers.is_empty()
        && !partition_smoke.serial_markers_observed
    {
        blockers.push(format!(
            "Kernel layout has not yet produced the required initramfs serial milestones: {}.",
            partition_smoke.serial_expected_markers.join(", ")
        ));
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
        device_loop: crate::native::native_device_loop_report(),
        ready_for_serial_kernel_spike,
        ready_for_arch_boot_attempt,
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

fn initialize_native_boot_trace_checkpoint(path: &Path, session_name: &str) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let checkpoint = serde_json::json!({
        "schema_version": 1,
        "kind": "pane-native-boot-trace-checkpoint",
        "reason": "requested",
        "session_name": session_name,
        "status": "pending",
    });
    fs::write(path, serde_json::to_string_pretty(&checkpoint)?)?;
    Ok(())
}

fn prepare_native_runtime_boundary(session_name: &str, capacity_gib: u64) -> AppResult<()> {
    let paths = crate::plan::runtime_for(session_name);
    let budget = runtime_storage_budget(capacity_gib);
    prepare_runtime_paths(&paths)?;
    write_runtime_config(&paths, session_name, &budget)?;
    write_native_runtime_manifest(&paths, session_name)?;
    write_framebuffer_contract(&paths)?;
    write_input_contract(&paths)?;
    create_user_disk_descriptor(&paths, &budget, false)?;
    create_serial_boot_image_artifact(&paths, false)?;
    let report = build_runtime_report(session_name, capacity_gib, false)?;
    write_runtime_manifest(&paths, &report)
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
                "Rerun with `--prepare-runtime --execute --run-kernel-layout` to prepare the runtime boundary and consume the materialized kernel layout in WHP."
                    .to_string(),
                "Use only a controlled small serial/HALT candidate until the Linux boot-protocol runner exists."
                    .to_string(),
            ];
        }
        return vec![
            "Rerun with `--execute` to create and tear down the guarded WHP partition/vCPU."
                .to_string(),
            "Then rerun with `--prepare-runtime --execute --run-fixture` to prepare runtime contracts and prove guest memory, register setup, vCPU execution, and serial I/O exit handling."
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
            "Rerun with `--prepare-runtime --execute --run-fixture` to prepare runtime contracts, map guest memory, and execute the deterministic serial test image."
                .to_string(),
            "Only after the serial test image passes, replace it with a boot-to-serial kernel or loader."
                .to_string(),
        ];
    }

    if !run_boot_loader {
        return vec![
            "Register a controlled boot-to-serial loader with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256>`."
                .to_string(),
            "Run `pane native-boot-spike --prepare-runtime --execute --run-boot-loader` to prove Pane can execute a runtime-provided boot candidate."
                .to_string(),
            "Materialize a kernel boot layout with `pane native-kernel-plan --prepare-runtime --materialize`, then run `pane native-boot-spike --prepare-runtime --execute --run-kernel-layout` with a controlled small candidate."
                .to_string(),
            "Connect that loader to Pane's verified Arch base image and user disk only after its serial contract is deterministic."
                .to_string(),
        ];
    }

    vec![
        "Register a controlled boot-to-serial loader with `pane runtime --register-boot-loader <path> --boot-loader-expected-sha256 <sha256>`."
            .to_string(),
        "Run `pane native-boot-spike --prepare-runtime --execute --run-boot-loader` to prove Pane can execute a runtime-provided boot candidate."
            .to_string(),
        "Materialize a kernel boot layout with `pane native-kernel-plan --prepare-runtime --materialize`, then run `pane native-boot-spike --prepare-runtime --execute --run-kernel-layout` with a controlled small candidate."
            .to_string(),
        "Connect that loader to Pane's verified Arch base image and user disk only after its serial contract is deterministic."
            .to_string(),
    ]
}

fn native_kernel_plan(args: NativeKernelPlanArgs) -> AppResult<()> {
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let paths = crate::plan::runtime_for(&session_name);
    if args.prepare_runtime {
        prepare_native_runtime_boundary(&session_name, DEFAULT_RUNTIME_CAPACITY_GIB)?;
    }
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
    if runtime.artifacts.base_os_image_verified
        && runtime.artifacts.user_disk_ready
        && !runtime.artifacts.initramfs_driver_bundle_ready
    {
        blockers.push(
            "No valid Pane initramfs driver source bundle exists. Run `pane runtime --write-initramfs-driver` before materializing a storage-backed kernel layout."
                .to_string(),
        );
    }
    if runtime.artifacts.base_os_image_verified
        && runtime.artifacts.user_disk_ready
        && runtime.artifacts.initramfs_driver_bundle_ready
        && !runtime.artifacts.initramfs_image_verified
    {
        blockers.push(
            "No verified Pane discovery initramfs artifact exists. Run `pane runtime --build-discovery-initramfs`, or register an externally built initramfs with `pane runtime --register-initramfs <path> --initramfs-expected-sha256 <sha256>`."
                .to_string(),
        );
    }
    if runtime.artifacts.base_os_image_verified
        && runtime.artifacts.user_disk_ready
        && runtime.artifacts.initramfs_driver_bundle_ready
        && runtime.artifacts.initramfs_image_verified
        && !runtime.artifacts.discovery_initramfs_matches_driver_bundle
    {
        blockers.push(
            "Verified discovery initramfs artifact was not packaged from the current Pane initramfs driver bundle. Rebuild it with `pane runtime --build-discovery-initramfs` before materializing a storage-backed kernel layout."
                .to_string(),
        );
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

fn native_foundation(args: NativeFoundationArgs) -> AppResult<()> {
    let report = crate::vmm_foundation::build_vmm_foundation_report();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_native_foundation_report(&report);
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
            initramfs_driver_dir: paths.initramfs_driver_dir.display().to_string(),
            user_disk: paths.user_disk.display().to_string(),
            base_os_metadata: paths.base_os_metadata.display().to_string(),
            serial_boot_metadata: paths.serial_boot_metadata.display().to_string(),
            boot_loader_metadata: paths.boot_loader_metadata.display().to_string(),
            kernel_boot_metadata: paths.kernel_boot_metadata.display().to_string(),
            initramfs_driver_metadata: paths.initramfs_driver_metadata.display().to_string(),
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
        vmm_foundation: crate::vmm_foundation::build_vmm_foundation_report(),
        current_limitation: "Pane owns the runtime storage layout, config, manifests, and host preflight now. It still cannot boot a Pane-owned OS image or draw a Pane-owned desktop window without the current WSL/XRDP bridge.",
        next_steps: vec![
            "Run `pane native-preflight --prepare-runtime --json` to prepare the Pane-owned runtime boundary and prove the host can support the first WHP boot-to-serial spike."
                .to_string(),
            "Register a Pane-approved native Arch boot set with `pane runtime --register-native-boot-set-manifest <pane-native-boot-set.json>`, or use `pane runtime --register-native-boot-set --register-base-image <arch.img> --expected-sha256 <sha256> --register-kernel <vmlinuz-linux> --kernel-expected-sha256 <sha256> --register-initramfs <initramfs.img> --initramfs-expected-sha256 <sha256> --kernel-cmdline \"console=ttyS0 panic=-1\"` for manual intake."
                .to_string(),
            "Keep the Pane-owned sparse user disk from `--prepare-runtime`, or recreate it explicitly with `pane runtime --create-user-disk` if metadata repair is needed."
                .to_string(),
            "Keep the runtime-backed serial boot image from `--prepare-runtime`, or recreate it explicitly with `pane runtime --create-serial-boot-image` if fixture metadata is stale."
                .to_string(),
            "Run `pane native-boot-spike --prepare-runtime --execute --run-fixture` to prove WHP guest memory, register setup, vCPU execution, and serial I/O."
                .to_string(),
            "Register and run a controlled boot-to-serial loader candidate with `pane runtime --register-boot-loader` and `pane native-boot-spike --prepare-runtime --execute --run-boot-loader`."
                .to_string(),
            "Register a verified kernel/initramfs boot plan with `pane runtime --register-kernel` and an explicit serial console cmdline."
                .to_string(),
            "Materialize the WHP kernel boot layout with `pane native-kernel-plan --prepare-runtime --materialize`."
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
        queue_header_bytes: default_input_queue_header_bytes(),
        queue_magic: default_input_queue_magic(),
    }
}

fn default_input_queue_header_bytes() -> u32 {
    64
}

fn default_input_queue_magic() -> String {
    "PANEINQ1".to_string()
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

fn register_native_boot_set(paths: &RuntimePaths, args: &RuntimeArgs) -> AppResult<()> {
    let base_image = args.register_base_image.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --register-base-image.")
    })?;
    let base_expected_sha256 = args.expected_sha256.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --expected-sha256.")
    })?;
    let kernel = args.register_kernel.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --register-kernel.")
    })?;
    let kernel_expected_sha256 = args.kernel_expected_sha256.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --kernel-expected-sha256.")
    })?;
    let initramfs = args.register_initramfs.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --register-initramfs.")
    })?;
    let initramfs_expected_sha256 = args.initramfs_expected_sha256.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --initramfs-expected-sha256.")
    })?;
    let cmdline = args.kernel_cmdline.as_deref().ok_or_else(|| {
        AppError::message("--register-native-boot-set requires --kernel-cmdline.")
    })?;

    register_native_boot_set_artifacts(
        paths,
        base_image,
        base_expected_sha256,
        kernel,
        kernel_expected_sha256,
        initramfs,
        initramfs_expected_sha256,
        cmdline,
        args.force,
    )
}

fn register_native_boot_set_from_manifest(
    paths: &RuntimePaths,
    manifest_path: &Path,
    args: &RuntimeArgs,
) -> AppResult<()> {
    if args.register_native_boot_set
        || args.register_base_image.is_some()
        || args.expected_sha256.is_some()
        || args.require_native_root_disk
        || args.register_kernel.is_some()
        || args.kernel_expected_sha256.is_some()
        || args.register_initramfs.is_some()
        || args.initramfs_expected_sha256.is_some()
        || args.kernel_cmdline.is_some()
    {
        return Err(AppError::message(
            "--register-native-boot-set-manifest cannot be combined with individual native boot-set artifact flags. Use the manifest or the explicit flags, not both.",
        ));
    }

    let manifest = read_json_file::<NativeBootSetManifest>(manifest_path)?;
    if manifest.schema_version != 1 {
        return Err(AppError::message(format!(
            "Native boot-set manifest schema_version must be 1, got {}.",
            manifest.schema_version
        )));
    }
    if manifest
        .distro_family
        .as_deref()
        .is_some_and(|family| !family.eq_ignore_ascii_case("arch"))
    {
        return Err(AppError::message(
            "Pane native boot-set manifests currently support only distro_family `arch`.",
        ));
    }

    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let base_image = resolve_manifest_path(manifest_dir, &manifest.base_image);
    let kernel = resolve_manifest_path(manifest_dir, &manifest.kernel);
    let initramfs = resolve_manifest_path(manifest_dir, &manifest.initramfs);

    register_native_boot_set_artifacts(
        paths,
        &base_image,
        &manifest.base_image_sha256,
        &kernel,
        &manifest.kernel_sha256,
        &initramfs,
        &manifest.initramfs_sha256,
        &manifest.kernel_cmdline,
        args.force,
    )
}

fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

fn write_native_boot_set_manifest_template(path: &Path, force: bool) -> AppResult<()> {
    if path.exists() && !force {
        return Err(AppError::message(format!(
            "Native boot-set manifest template already exists at {}. Pass --force to replace it.",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let template = serde_json::json!({
        "schema_version": 1,
        "distro_family": "arch",
        "base_image": "artifacts/arch-root.img",
        "base_image_sha256": "replace-with-64-character-sha256",
        "kernel": "artifacts/vmlinuz-linux",
        "kernel_sha256": "replace-with-64-character-sha256",
        "initramfs": "artifacts/initramfs-linux.img",
        "initramfs_sha256": "replace-with-64-character-sha256",
        "kernel_cmdline": "console=ttyS0 panic=-1"
    });
    write_json_file(path, &template)
}

#[allow(clippy::too_many_arguments)]
fn register_native_boot_set_artifacts(
    paths: &RuntimePaths,
    base_image: &Path,
    base_expected_sha256: &str,
    kernel: &Path,
    kernel_expected_sha256: &str,
    initramfs: &Path,
    initramfs_expected_sha256: &str,
    cmdline: &str,
    force: bool,
) -> AppResult<()> {
    validate_native_boot_set_inputs(
        paths,
        base_image,
        base_expected_sha256,
        kernel,
        kernel_expected_sha256,
        initramfs,
        initramfs_expected_sha256,
        cmdline,
        force,
    )?;

    register_base_os_image(paths, base_image, Some(base_expected_sha256), true, force)?;
    register_kernel_boot_plan(
        paths,
        Some(kernel),
        Some(kernel_expected_sha256),
        Some(initramfs),
        Some(initramfs_expected_sha256),
        Some(cmdline),
        force,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_native_boot_set_inputs(
    paths: &RuntimePaths,
    base_image: &Path,
    base_expected_sha256: &str,
    kernel: &Path,
    kernel_expected_sha256: &str,
    initramfs: &Path,
    initramfs_expected_sha256: &str,
    cmdline: &str,
    force: bool,
) -> AppResult<()> {
    validate_expected_artifact_sha256(base_image, base_expected_sha256, "Base OS image")?;
    validate_expected_artifact_sha256(kernel, kernel_expected_sha256, "Kernel")?;
    validate_expected_artifact_sha256(initramfs, initramfs_expected_sha256, "Initramfs")?;

    let base_inspection = inspect_base_os_image_artifact(base_image)?;
    validate_native_root_disk_image(base_image, &base_inspection)?;

    let kernel_inspection = inspect_kernel_image_artifact(kernel)?;
    if kernel_inspection.format != "linux-bzimage" {
        return Err(AppError::message(format!(
            "Pane native Arch boot sets require a Linux bzImage kernel. `{}` was inspected as `{}`.",
            kernel.display(),
            kernel_inspection.format
        )));
    }

    if fs::metadata(initramfs)?.len() == 0 {
        return Err(AppError::message(format!(
            "Pane native Arch boot sets require a non-empty initramfs artifact: {}",
            initramfs.display()
        )));
    }
    validate_kernel_cmdline(cmdline)?;

    validate_runtime_artifact_target_available(
        &paths.base_os_image,
        base_image,
        force,
        "base OS image",
    )?;
    validate_runtime_artifact_target_available(&paths.kernel_image, kernel, force, "Kernel")?;
    validate_runtime_artifact_target_available(
        &paths.initramfs_image,
        initramfs,
        force,
        "Initramfs",
    )
}

fn validate_expected_artifact_sha256(
    source: &Path,
    expected_sha256: &str,
    label: &str,
) -> AppResult<()> {
    if !source.is_file() {
        return Err(AppError::message(format!(
            "{label} source does not exist or is not a file: {}",
            source.display()
        )));
    }
    let expected_sha256 = normalize_sha256_hex(expected_sha256)?;
    let actual_sha256 = sha256_file(source)?;
    if expected_sha256 != actual_sha256 {
        return Err(AppError::message(format!(
            "{label} SHA-256 mismatch. expected {expected_sha256}, got {actual_sha256}."
        )));
    }
    Ok(())
}

fn validate_runtime_artifact_target_available(
    destination: &Path,
    source: &Path,
    force: bool,
    label: &str,
) -> AppResult<()> {
    let same_target =
        destination.exists() && source.canonicalize().ok() == destination.canonicalize().ok();
    if destination.exists() && !force && !same_target {
        return Err(AppError::message(format!(
            "A registered {label} artifact already exists at {}. Pass --force to replace it.",
            destination.display()
        )));
    }
    Ok(())
}

fn register_base_os_image(
    paths: &RuntimePaths,
    source_image: &Path,
    expected_sha256: Option<&str>,
    require_native_root_disk: bool,
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

    let source_inspection = inspect_base_os_image_artifact(source_image)?;
    if require_native_root_disk {
        validate_native_root_disk_image(source_image, &source_inspection)?;
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
    let inspection = if same_target {
        source_inspection
    } else {
        inspect_base_os_image_artifact(&paths.base_os_image)?
    };
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
        image_format: inspection.image_format,
        bootable_disk_hint: inspection.bootable_disk_hint,
        partitions: inspection.partitions,
        root_partition_hint: inspection.root_partition_hint,
        root_filesystem_hint: inspection.root_filesystem_hint,
        verified,
        registered_at_epoch_seconds: current_epoch_seconds(),
        notes: inspection.notes,
    };
    write_json_file(&paths.base_os_metadata, &metadata)
}

fn validate_native_root_disk_image(
    path: &Path,
    inspection: &BaseOsImageInspection,
) -> AppResult<()> {
    if !inspection.bootable_disk_hint {
        return Err(AppError::message(format!(
            "Pane native boot requires a bootable raw disk image with a detectable Linux root partition. `{}` was registered as `{}`. Convert the Arch rootfs to a raw disk image or register a bootable Arch disk image, then retry.",
            path.display(),
            inspection.image_format
        )));
    }

    if inspection.root_partition_hint.is_none() {
        return Err(AppError::message(format!(
            "Pane native boot found a raw disk image at {}, but no Linux root partition was detected. Register an Arch raw disk image with an MBR Linux partition or GPT Linux root partition before continuing.",
            path.display()
        )));
    }

    Ok(())
}

fn inspect_base_os_image_artifact(path: &Path) -> AppResult<BaseOsImageInspection> {
    let bytes = fs::metadata(path)?.len();
    let read_len = usize::try_from(bytes.min(4096)).map_err(|_| {
        AppError::message("Base OS image header length is too large for this host.")
    })?;
    let mut header = vec![0_u8; read_len];
    let mut file = OpenOptions::new().read(true).open(path)?;
    if read_len > 0 {
        file.read_exact(&mut header)?;
    }

    let has_mbr_signature = header.get(510..512) == Some(&[0x55, 0xaa]);
    let has_gpt_header = header.get(512..520) == Some(b"EFI PART");
    let has_nonempty_mbr_partition = header
        .get(446..510)
        .map(|entries| entries.chunks_exact(16).any(|entry| entry[4] != 0))
        .unwrap_or(false);
    let has_tar_magic = header.get(257..263) == Some(b"ustar\0");
    let has_zstd_magic = header.get(0..4) == Some(&[0x28, 0xb5, 0x2f, 0xfd]);
    let has_gzip_magic = header.get(0..2) == Some(&[0x1f, 0x8b]);
    let partitions = if has_gpt_header {
        inspect_gpt_partitions(&mut file, bytes)?
    } else if has_mbr_signature {
        inspect_mbr_partitions(&header)
    } else {
        Vec::new()
    };
    let root_partition_hint = partitions
        .iter()
        .find(|partition| partition.root_candidate)
        .cloned();
    let root_filesystem_hint = root_partition_hint.as_ref().and_then(|root| {
        detect_partition_filesystem(path, root.byte_offset)
            .ok()
            .flatten()
    });

    let (image_format, bootable_disk_hint) = if has_gpt_header {
        ("raw-gpt-disk", true)
    } else if has_mbr_signature && has_nonempty_mbr_partition {
        ("raw-mbr-disk", true)
    } else if has_tar_magic {
        ("tar-rootfs", false)
    } else if has_zstd_magic {
        ("zstd-compressed-image", false)
    } else if has_gzip_magic {
        ("gzip-compressed-image", false)
    } else {
        ("unknown", false)
    };

    let mut notes = Vec::new();
    if bootable_disk_hint {
        notes.push("Base OS image looks like a raw disk image with a partition table.".to_string());
        if let Some(root) = &root_partition_hint {
            notes.push(format!(
                "Pane found a likely Linux root partition at index {} offset {} bytes.",
                root.index, root.byte_offset
            ));
            if let Some(filesystem) = &root_filesystem_hint {
                notes.push(format!(
                    "Pane detected the likely root filesystem as {filesystem}."
                ));
            }
        } else {
            notes.push(
                "Pane did not find an obvious Linux root partition; native boot may need an explicit root handoff."
                    .to_string(),
            );
        }
    } else {
        notes.push(
            "Base OS image does not look like a directly bootable raw disk yet; future native boot may require conversion or an initramfs root handoff."
                .to_string(),
        );
    }
    if bytes < 16 * 1024 * 1024 {
        notes.push("Base OS image is smaller than 16 MiB; this is probably a test artifact, not a real Arch image.".to_string());
    }

    Ok(BaseOsImageInspection {
        image_format: image_format.to_string(),
        bootable_disk_hint,
        partitions,
        root_partition_hint,
        root_filesystem_hint,
        notes,
    })
}

fn detect_partition_filesystem(path: &Path, partition_offset: u64) -> AppResult<Option<String>> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    let mut probe = vec![0_u8; 128 * 1024];
    file.seek(SeekFrom::Start(partition_offset))?;
    let bytes_read = file.read(&mut probe)?;
    probe.truncate(bytes_read);

    if probe.get(0x438..0x43a) == Some(&[0x53, 0xef]) {
        return Ok(Some("ext4".to_string()));
    }
    if probe.get(0x1_0000 + 0x40..0x1_0000 + 0x48) == Some(b"_BHRfS_M") {
        return Ok(Some("btrfs".to_string()));
    }
    if probe.get(0..4) == Some(b"XFSB") {
        return Ok(Some("xfs".to_string()));
    }
    if probe.get(0x400..0x404) == Some(&[0x10, 0x20, 0xf5, 0xf2]) {
        return Ok(Some("f2fs".to_string()));
    }

    Ok(None)
}

fn refresh_base_os_metadata_inspection(
    paths: &RuntimePaths,
    mut metadata: BaseOsImageMetadata,
) -> AppResult<BaseOsImageMetadata> {
    if !metadata.verified || metadata.stored_path != paths.base_os_image.display().to_string() {
        return Ok(metadata);
    }

    let actual_bytes = fs::metadata(&paths.base_os_image)?.len();
    if actual_bytes != metadata.bytes || sha256_file(&paths.base_os_image)? != metadata.sha256 {
        return Ok(metadata);
    }

    let inspection = inspect_base_os_image_artifact(&paths.base_os_image)?;
    let changed = metadata.image_format != inspection.image_format
        || metadata.bootable_disk_hint != inspection.bootable_disk_hint
        || metadata.partitions != inspection.partitions
        || metadata.root_partition_hint != inspection.root_partition_hint
        || metadata.root_filesystem_hint != inspection.root_filesystem_hint
        || metadata.notes != inspection.notes;

    if changed {
        metadata.image_format = inspection.image_format;
        metadata.bootable_disk_hint = inspection.bootable_disk_hint;
        metadata.partitions = inspection.partitions;
        metadata.root_partition_hint = inspection.root_partition_hint;
        metadata.root_filesystem_hint = inspection.root_filesystem_hint;
        metadata.notes = inspection.notes;
        write_json_file(&paths.base_os_metadata, &metadata)?;
    }

    Ok(metadata)
}

fn inspect_mbr_partitions(header: &[u8]) -> Vec<BaseOsPartition> {
    header
        .get(446..510)
        .into_iter()
        .flat_map(|entries| entries.chunks_exact(16).enumerate())
        .filter_map(|(index, entry)| {
            let partition_type = entry[4];
            let start_lba = read_u32_le_slice(&entry[8..12])? as u64;
            let sector_count = read_u32_le_slice(&entry[12..16])? as u64;
            if partition_type == 0 || sector_count == 0 {
                return None;
            }
            let root_candidate = matches!(partition_type, 0x83 | 0x8e);
            Some(BaseOsPartition {
                index: (index + 1) as u32,
                scheme: "mbr".to_string(),
                partition_type: format!("0x{partition_type:02x}"),
                bootable: entry[0] == 0x80,
                start_lba,
                sector_count,
                byte_offset: start_lba.saturating_mul(512),
                byte_length: sector_count.saturating_mul(512),
                root_candidate,
            })
        })
        .collect()
}

fn inspect_gpt_partitions(
    file: &mut fs::File,
    image_bytes: u64,
) -> AppResult<Vec<BaseOsPartition>> {
    if image_bytes < 1024 {
        return Ok(Vec::new());
    }

    let mut gpt_header = [0_u8; 92];
    file.seek(SeekFrom::Start(512))?;
    file.read_exact(&mut gpt_header)?;
    let entries_lba = read_u64_le_at(&gpt_header, 72).unwrap_or(2);
    let entry_count = read_u32_le_at(&gpt_header, 80).unwrap_or(0).min(128);
    let entry_size = read_u32_le_at(&gpt_header, 84)
        .unwrap_or(128)
        .clamp(128, 4096);
    if entry_count == 0 {
        return Ok(Vec::new());
    }

    let entries_offset = entries_lba.saturating_mul(512);
    let entries_bytes = u64::from(entry_count).saturating_mul(u64::from(entry_size));
    if entries_offset.saturating_add(entries_bytes) > image_bytes {
        return Ok(Vec::new());
    }

    let mut partitions = Vec::new();
    file.seek(SeekFrom::Start(entries_offset))?;
    for index in 0..entry_count {
        let mut entry = vec![0_u8; entry_size as usize];
        file.read_exact(&mut entry)?;
        if entry
            .get(0..16)
            .map(|guid| guid.iter().all(|byte| *byte == 0))
            .unwrap_or(true)
        {
            continue;
        }
        let first_lba = read_u64_le_at(&entry, 32).unwrap_or(0);
        let last_lba = read_u64_le_at(&entry, 40).unwrap_or(0);
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }
        let partition_type = gpt_partition_type_label(&entry[0..16]);
        let root_candidate = partition_type.contains("linux");
        let sector_count = last_lba - first_lba + 1;
        partitions.push(BaseOsPartition {
            index: index + 1,
            scheme: "gpt".to_string(),
            partition_type,
            bootable: false,
            start_lba: first_lba,
            sector_count,
            byte_offset: first_lba.saturating_mul(512),
            byte_length: sector_count.saturating_mul(512),
            root_candidate,
        });
    }

    Ok(partitions)
}

fn gpt_partition_type_label(guid: &[u8]) -> String {
    const LINUX_FILESYSTEM_GUID_LE: [u8; 16] = [
        0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d,
        0xe4,
    ];
    const LINUX_ROOT_X86_64_GUID_LE: [u8; 16] = [
        0xe3, 0xbc, 0x68, 0x4f, 0xcd, 0xe8, 0xb1, 0x4d, 0x96, 0xe7, 0xfb, 0xca, 0xf9, 0x84, 0xb7,
        0x09,
    ];
    if guid == LINUX_FILESYSTEM_GUID_LE {
        "linux-filesystem".to_string()
    } else if guid == LINUX_ROOT_X86_64_GUID_LE {
        "linux-root-x86_64".to_string()
    } else {
        format!("gpt-guid-{}", hex_lower(guid))
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_u32_le_slice(bytes: &[u8]) -> Option<u32> {
    let slice = bytes.get(0..4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP read-only base block-device handler.
fn read_base_os_block(paths: &RuntimePaths, block_index: u64) -> AppResult<Vec<u8>> {
    read_base_os_io_block(paths, block_index, PANE_BASE_OS_BLOCK_SIZE_BYTES)
}

fn read_base_os_io_block(
    paths: &RuntimePaths,
    block_index: u64,
    block_size_bytes: u64,
) -> AppResult<Vec<u8>> {
    if block_size_bytes == 0 {
        return Err(AppError::message(
            "Pane base OS block I/O requires a non-zero block size.",
        ));
    }
    let metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata)?;
    if !metadata.verified {
        return Err(AppError::message(
            "Pane base OS image is not verified for block I/O.",
        ));
    }
    if metadata.stored_path != paths.base_os_image.display().to_string() {
        return Err(AppError::message(
            "Pane base OS image metadata points at a different artifact path.",
        ));
    }
    let actual_bytes = fs::metadata(&paths.base_os_image)?.len();
    if actual_bytes != metadata.bytes || sha256_file(&paths.base_os_image)? != metadata.sha256 {
        return Err(AppError::message(
            "Pane base OS image changed after registration; re-register it before block I/O.",
        ));
    }

    let block_size: usize = block_size_bytes
        .try_into()
        .map_err(|_| AppError::message("Pane base OS block size is too large for this host."))?;
    let offset = block_index
        .checked_mul(block_size_bytes)
        .ok_or_else(|| AppError::message("Pane base OS block offset overflowed."))?;
    let mut block = vec![0_u8; block_size];
    if offset >= metadata.bytes {
        return Ok(block);
    }

    let mut file = OpenOptions::new().read(true).open(&paths.base_os_image)?;
    file.seek(SeekFrom::Start(offset))?;
    let remaining = metadata.bytes - offset;
    let to_read = remaining.min(block_size_bytes) as usize;
    file.read_exact(&mut block[..to_read])?;
    Ok(block)
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

    let Ok(mut file) = OpenOptions::new().read(true).open(&paths.user_disk) else {
        return false;
    };
    let Ok(header_len) = usize::try_from(metadata.allocated_header_bytes) else {
        return false;
    };
    let mut header = vec![0_u8; header_len];
    if file.read_exact(&mut header).is_err() {
        return false;
    }

    header.starts_with(PANE_USER_DISK_MAGIC.as_bytes())
        && sha256_bytes(&header) == metadata.header_sha256
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP block-device handler.
fn user_disk_data_offset(metadata: &UserDiskMetadata) -> u64 {
    page_align_guest_range(metadata.allocated_header_bytes)
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP block-device handler.
fn user_disk_total_blocks(metadata: &UserDiskMetadata) -> u64 {
    metadata.logical_size_bytes / metadata.block_size_bytes
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP block-device handler.
fn user_disk_block_offset(metadata: &UserDiskMetadata, block_index: u64) -> AppResult<u64> {
    if block_index >= user_disk_total_blocks(metadata) {
        return Err(AppError::message(format!(
            "Pane user disk block {block_index} is outside the logical disk size."
        )));
    }

    user_disk_data_offset(metadata)
        .checked_add(block_index.saturating_mul(metadata.block_size_bytes))
        .ok_or_else(|| AppError::message("Pane user disk block offset overflowed."))
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP block-device handler.
fn read_user_disk_block(
    paths: &RuntimePaths,
    metadata: &UserDiskMetadata,
    block_index: u64,
) -> AppResult<Vec<u8>> {
    read_user_disk_io_block(paths, metadata, block_index, metadata.block_size_bytes)
}

fn user_disk_io_block_offset(
    metadata: &UserDiskMetadata,
    block_index: u64,
    block_size_bytes: u64,
) -> AppResult<u64> {
    if block_size_bytes == 0 {
        return Err(AppError::message(
            "Pane user disk block I/O requires a non-zero block size.",
        ));
    }
    let byte_offset = block_index
        .checked_mul(block_size_bytes)
        .ok_or_else(|| AppError::message("Pane user disk I/O block offset overflowed."))?;
    if byte_offset >= metadata.logical_size_bytes {
        return Err(AppError::message(format!(
            "Pane user disk I/O block {block_index} is outside the logical disk size."
        )));
    }
    user_disk_data_offset(metadata)
        .checked_add(byte_offset)
        .ok_or_else(|| AppError::message("Pane user disk I/O block data offset overflowed."))
}

fn read_user_disk_io_block(
    paths: &RuntimePaths,
    metadata: &UserDiskMetadata,
    block_index: u64,
    block_size_bytes: u64,
) -> AppResult<Vec<u8>> {
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane sparse user disk is not ready for block I/O.",
        ));
    }

    let block_size: usize = block_size_bytes
        .try_into()
        .map_err(|_| AppError::message("Pane user disk block size is too large for this host."))?;
    let offset = user_disk_io_block_offset(metadata, block_index, block_size_bytes)?;
    let mut block = vec![0_u8; block_size];
    let mut file = OpenOptions::new().read(true).open(&paths.user_disk)?;
    let file_len = file.metadata()?.len();
    if offset >= file_len {
        return Ok(block);
    }

    file.seek(SeekFrom::Start(offset))?;
    let bytes_read = file.read(&mut block)?;
    if bytes_read < block.len() {
        block[bytes_read..].fill(0);
    }
    Ok(block)
}

#[allow(dead_code)] // Covered by tests; used by the upcoming WHP block-device handler.
fn write_user_disk_block(
    paths: &RuntimePaths,
    metadata: &UserDiskMetadata,
    block_index: u64,
    block: &[u8],
) -> AppResult<()> {
    write_user_disk_io_block(paths, metadata, block_index, block)
}

fn write_user_disk_io_block(
    paths: &RuntimePaths,
    metadata: &UserDiskMetadata,
    block_index: u64,
    block: &[u8],
) -> AppResult<()> {
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane sparse user disk is not ready for block I/O.",
        ));
    }

    if block.is_empty() {
        return Err(AppError::message(
            "Pane user disk writes must contain at least one byte.",
        ));
    }

    let block_size_bytes = block.len() as u64;
    let offset = user_disk_io_block_offset(metadata, block_index, block_size_bytes)?;
    let mut file = OpenOptions::new().write(true).open(&paths.user_disk)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(block)?;
    file.flush()?;
    Ok(())
}

fn execute_native_block_io_command(
    paths: &RuntimePaths,
    command: &crate::native::NativeBlockIoCommand,
    write_payload: Option<&[u8]>,
) -> AppResult<crate::native::NativeBlockIoServiceResult> {
    let decision = crate::native::evaluate_native_block_io(command);
    if !decision.allowed {
        return Ok(crate::native::NativeBlockIoServiceResult {
            decision,
            bytes: Vec::new(),
        });
    }

    let block_size: usize = command.block_size_bytes.try_into().map_err(|_| {
        AppError::message("Pane native block I/O block size is too large for this host.")
    })?;

    let bytes = match (command.device, command.operation) {
        (crate::native::NativeBlockDeviceId::BaseOs, crate::native::NativeBlockOperation::Read) => {
            read_base_os_io_block(
                paths,
                command.block_index,
                u64::from(command.block_size_bytes),
            )?
        }
        (
            crate::native::NativeBlockDeviceId::UserDisk,
            crate::native::NativeBlockOperation::Read,
        ) => {
            let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
            read_user_disk_io_block(
                paths,
                &metadata,
                command.block_index,
                u64::from(command.block_size_bytes),
            )?
        }
        (
            crate::native::NativeBlockDeviceId::UserDisk,
            crate::native::NativeBlockOperation::Write,
        ) => {
            let payload = write_payload.ok_or_else(|| {
                AppError::message("Pane native user-disk write command is missing a payload.")
            })?;
            if payload.len() != block_size {
                return Err(AppError::message(format!(
                    "Pane native user-disk write payload must be exactly {block_size} bytes."
                )));
            }
            let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
            write_user_disk_io_block(paths, &metadata, command.block_index, payload)?;
            Vec::new()
        }
        (
            crate::native::NativeBlockDeviceId::BaseOs,
            crate::native::NativeBlockOperation::Write,
        ) => Vec::new(),
    };

    Ok(crate::native::NativeBlockIoServiceResult { decision, bytes })
}

fn write_pane_initramfs_driver_bundle(
    paths: &RuntimePaths,
) -> AppResult<PaneInitramfsDriverMetadata> {
    fs::create_dir_all(&paths.initramfs_driver_dir)?;

    let hook_path = paths.initramfs_driver_dir.join("pane-initramfs-hook.sh");
    let header_path = paths.initramfs_driver_dir.join("pane-port-block.h");
    let init_source_path = paths.initramfs_driver_dir.join("pane-init.c");
    let probe_source_path = paths.initramfs_driver_dir.join("pane-port-probe.c");
    let block_driver_source_path = paths.initramfs_driver_dir.join("pane-block.c");
    let block_driver_build_script_path = paths
        .initramfs_driver_dir
        .join("build-pane-block-module.sh");
    let build_script_path = paths.initramfs_driver_dir.join("build-pane-initramfs.sh");
    let readme_path = paths.initramfs_driver_dir.join("README.md");

    fs::write(&hook_path, pane_initramfs_hook_source())?;
    fs::write(&header_path, pane_port_block_header_source())?;
    fs::write(&init_source_path, pane_init_source())?;
    fs::write(&probe_source_path, pane_port_probe_source())?;
    fs::write(&block_driver_source_path, pane_block_driver_source())?;
    fs::write(
        &block_driver_build_script_path,
        pane_block_driver_build_script(),
    )?;
    fs::write(&build_script_path, pane_initramfs_build_script())?;
    fs::write(&readme_path, pane_initramfs_driver_readme())?;

    let metadata = PaneInitramfsDriverMetadata {
        schema_version: 1,
        bundle_kind: "pane-initramfs-driver-source-v1".to_string(),
        driver_dir: paths.initramfs_driver_dir.display().to_string(),
        hook_path: hook_path.display().to_string(),
        header_path: header_path.display().to_string(),
        init_source_path: init_source_path.display().to_string(),
        probe_source_path: probe_source_path.display().to_string(),
        block_driver_source_path: block_driver_source_path.display().to_string(),
        block_driver_build_script_path: block_driver_build_script_path.display().to_string(),
        build_script_path: build_script_path.display().to_string(),
        readme_path: readme_path.display().to_string(),
        hook_sha256: sha256_file(&hook_path)?,
        header_sha256: sha256_file(&header_path)?,
        init_source_sha256: sha256_file(&init_source_path)?,
        probe_source_sha256: sha256_file(&probe_source_path)?,
        block_driver_source_sha256: sha256_file(&block_driver_source_path)?,
        block_driver_abi_sha256: pane_block_driver_abi_sha256(),
        block_driver_build_script_sha256: sha256_file(&block_driver_build_script_path)?,
        build_script_sha256: sha256_file(&build_script_path)?,
        readme_sha256: sha256_file(&readme_path)?,
        packaged_initramfs_path: None,
        packaged_initramfs_bytes: None,
        packaged_initramfs_sha256: None,
        packaged_hook_sha256: None,
        packaged_init_source_sha256: None,
        packaged_probe_source_sha256: None,
        packaged_init_binary_sha256: None,
        packaged_probe_binary_sha256: None,
        packaged_binary_provenance: None,
        packaged_block_driver_source_sha256: None,
        packaged_block_driver_abi_sha256: None,
        packaged_block_module_sha256: None,
        block_io_protocol: default_pane_block_io_protocol(),
        block_io_port_base: default_pane_block_io_port_base(),
        block_io_port_count: default_pane_block_io_port_count(),
        block_io_status_port_offset: default_pane_block_io_status_port_offset(),
        block_io_data_port_offset: default_pane_block_io_data_port_offset(),
        block_io_block_size_bytes: default_pane_block_io_block_size_bytes(),
        generated_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This bundle is the reproducible guest-side source contract for Pane's native storage ABI.".to_string(),
            "It includes a Linux block-driver source/build contract for /dev/pane0 and /dev/pane1, but Pane does not yet compile or load the module automatically.".to_string(),
            "The next native boot milestone must compile/package this into a verified initramfs and prove the root handoff against a target Arch kernel.".to_string(),
        ],
    };
    write_json_file(&paths.initramfs_driver_metadata, &metadata)?;
    Ok(metadata)
}

fn load_verified_pane_initramfs_driver_metadata(
    paths: &RuntimePaths,
) -> AppResult<PaneInitramfsDriverMetadata> {
    let metadata = read_json_file::<PaneInitramfsDriverMetadata>(&paths.initramfs_driver_metadata)
        .map_err(|error| {
            AppError::message(format!(
                "Pane initramfs driver metadata is missing or invalid. Run `pane runtime --write-initramfs-driver`: {error}"
            ))
        })?;
    if metadata.schema_version != 1
        || metadata.bundle_kind != "pane-initramfs-driver-source-v1"
        || metadata.driver_dir != paths.initramfs_driver_dir.display().to_string()
        || metadata.block_io_protocol != default_pane_block_io_protocol()
        || metadata.block_io_port_base != default_pane_block_io_port_base()
        || metadata.block_io_port_count != default_pane_block_io_port_count()
        || metadata.block_io_status_port_offset != default_pane_block_io_status_port_offset()
        || metadata.block_io_data_port_offset != default_pane_block_io_data_port_offset()
        || metadata.block_io_block_size_bytes != default_pane_block_io_block_size_bytes()
        || metadata.block_driver_abi_sha256 != pane_block_driver_abi_sha256()
    {
        return Err(AppError::message(
            "Pane initramfs driver metadata does not match the native block I/O ABI. Regenerate it with `pane runtime --write-initramfs-driver`.",
        ));
    }

    for (label, path, expected_sha256) in [
        ("hook", &metadata.hook_path, &metadata.hook_sha256),
        ("header", &metadata.header_path, &metadata.header_sha256),
        (
            "init source",
            &metadata.init_source_path,
            &metadata.init_source_sha256,
        ),
        (
            "probe source",
            &metadata.probe_source_path,
            &metadata.probe_source_sha256,
        ),
        (
            "block driver source",
            &metadata.block_driver_source_path,
            &metadata.block_driver_source_sha256,
        ),
        (
            "block driver build script",
            &metadata.block_driver_build_script_path,
            &metadata.block_driver_build_script_sha256,
        ),
        (
            "build script",
            &metadata.build_script_path,
            &metadata.build_script_sha256,
        ),
        ("readme", &metadata.readme_path, &metadata.readme_sha256),
    ] {
        let actual = sha256_file(Path::new(path)).map_err(|error| {
            AppError::message(format!(
                "Pane initramfs driver {label} artifact at {path} is missing or unreadable: {error}"
            ))
        })?;
        if actual != *expected_sha256 {
            return Err(AppError::message(format!(
                "Pane initramfs driver {label} artifact at {path} no longer matches metadata. Regenerate it with `pane runtime --write-initramfs-driver`."
            )));
        }
    }

    Ok(metadata)
}

fn pane_block_module_path(paths: &RuntimePaths) -> PathBuf {
    paths.initramfs_driver_dir.join("pane-block.ko")
}

fn virtio_mmio_module_path(paths: &RuntimePaths) -> PathBuf {
    paths.initramfs_driver_dir.join("virtio_mmio.ko")
}

fn virtio_mmio_module_metadata_path(paths: &RuntimePaths) -> PathBuf {
    paths.state.join("virtio-mmio-module.json")
}

fn pane_block_module_metadata_path(paths: &RuntimePaths) -> PathBuf {
    paths.state.join("pane-block-module.json")
}

fn register_virtio_mmio_module(
    paths: &RuntimePaths,
    source_module: &Path,
    expected_sha256: Option<&str>,
    force: bool,
) -> AppResult<()> {
    let kernel_metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata)
        .map_err(|error| {
            AppError::message(format!(
                "virtio_mmio module registration requires an existing verified kernel boot plan. Register the target Arch kernel first with `pane runtime --register-kernel ... --kernel-expected-sha256 ...`: {error}"
            ))
        })?;
    if !kernel_metadata.kernel_verified {
        return Err(AppError::message(
            "virtio_mmio module registration requires a hash-verified target kernel. Re-register the kernel with --kernel-expected-sha256.",
        ));
    }
    if expected_sha256.is_none() {
        return Err(AppError::message(
            "virtio_mmio module registration requires --virtio-mmio-module-expected-sha256 so Pane can verify the module before including it in the initramfs.",
        ));
    }
    let destination = virtio_mmio_module_path(paths);
    let registration = copy_verified_runtime_artifact(
        source_module,
        &destination,
        expected_sha256,
        "virtio_mmio module",
        force,
    )?;
    if !registration.verified {
        return Err(AppError::message(
            "virtio_mmio module must be registered with --virtio-mmio-module-expected-sha256 before it can be included in a native boot initramfs.",
        ));
    }
    let metadata = VirtioMmioModuleMetadata {
        schema_version: 1,
        module_kind: "linux-virtio-mmio-module".to_string(),
        source_path: registration.source_path,
        stored_path: registration.stored_path,
        bytes: registration.bytes,
        sha256: registration.sha256,
        expected_sha256: registration.expected_sha256,
        verified: registration.verified,
        target_kernel_path: Some(kernel_metadata.kernel_stored_path),
        target_kernel_sha256: Some(kernel_metadata.kernel_sha256),
        registered_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "Stock Arch builds CONFIG_VIRTIO_MMIO=m, so the virtio-mmio bus driver must be loaded before /dev/vda appears.".to_string(),
            "Pane copies this module into the discovery initramfs as /lib/modules/virtio_mmio.ko so /init loads it before waiting for the virtio root device.".to_string(),
            "The module's vermagic must match the registered target kernel; a mismatched module will fail to insmod inside the guest.".to_string(),
        ],
    };
    write_json_file(&virtio_mmio_module_metadata_path(paths), &metadata)
}

fn register_pane_block_module(
    paths: &RuntimePaths,
    source_module: &Path,
    expected_sha256: Option<&str>,
    force: bool,
) -> AppResult<()> {
    let initramfs_driver = load_verified_pane_initramfs_driver_metadata(paths)?;
    let kernel_metadata =
        read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).map_err(|error| {
            AppError::message(format!(
                "Pane block module registration requires an existing verified kernel boot plan. Register the target Arch kernel first with `pane runtime --register-kernel ... --kernel-expected-sha256 ...`: {error}"
            ))
        })?;
    if !kernel_metadata.kernel_verified {
        return Err(AppError::message(
            "Pane block module registration requires a hash-verified target kernel. Re-register the kernel with --kernel-expected-sha256.",
        ));
    }
    if kernel_metadata.kernel_stored_path != paths.kernel_image.display().to_string()
        || kernel_metadata.kernel_bytes != fs::metadata(&paths.kernel_image)?.len()
        || kernel_metadata.kernel_sha256 != sha256_file(&paths.kernel_image)?
    {
        return Err(AppError::message(
            "Pane block module registration requires the current target kernel to match kernel-boot metadata. Re-register the kernel first.",
        ));
    }
    if expected_sha256.is_none() {
        return Err(AppError::message(
            "Pane block module registration requires --pane-block-module-expected-sha256 so Pane can verify the module before including it in the initramfs.",
        ));
    }
    let destination = pane_block_module_path(paths);
    let registration = copy_verified_runtime_artifact(
        source_module,
        &destination,
        expected_sha256,
        "Pane block module",
        force,
    )?;
    if !registration.verified {
        return Err(AppError::message(
            "Pane block module must be registered with --pane-block-module-expected-sha256 before it can be included in a native boot initramfs.",
        ));
    }
    let metadata = PaneBlockModuleMetadata {
        schema_version: 1,
        module_kind: "pane-linux-block-module-v1".to_string(),
        source_path: registration.source_path,
        stored_path: registration.stored_path,
        bytes: registration.bytes,
        sha256: registration.sha256,
        expected_sha256: registration.expected_sha256,
        verified: registration.verified,
        target_kernel_path: Some(kernel_metadata.kernel_stored_path),
        target_kernel_bytes: Some(kernel_metadata.kernel_bytes),
        target_kernel_sha256: Some(kernel_metadata.kernel_sha256),
        target_kernel_format: Some(kernel_metadata.kernel_format),
        block_driver_source_sha256: Some(initramfs_driver.block_driver_source_sha256),
        block_driver_abi_sha256: Some(initramfs_driver.block_driver_abi_sha256),
        registered_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This kernel module is bound to the currently registered target kernel artifact and Pane block-driver ABI hash.".to_string(),
            "Pane copies this module into the discovery initramfs as /lib/modules/pane-block.ko so /init can load it before waiting for /dev/pane0.".to_string(),
        ],
    };
    write_json_file(&pane_block_module_metadata_path(paths), &metadata)
}

fn load_verified_pane_block_module_metadata(
    paths: &RuntimePaths,
) -> AppResult<PaneBlockModuleMetadata> {
    let module_path = pane_block_module_path(paths);
    let metadata_path = pane_block_module_metadata_path(paths);
    let initramfs_driver = load_verified_pane_initramfs_driver_metadata(paths)?;
    let kernel_metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata)
        .map_err(|error| {
            AppError::message(format!(
                "Pane block module verification requires a current kernel boot plan: {error}"
            ))
        })?;
    let metadata = read_json_file::<PaneBlockModuleMetadata>(&metadata_path).map_err(|error| {
        AppError::message(format!(
            "Pane block module metadata is missing or invalid. Register the module with `pane runtime --register-pane-block-module ... --pane-block-module-expected-sha256 ...`: {error}"
        ))
    })?;
    if metadata.schema_version != 1
        || metadata.module_kind != "pane-linux-block-module-v1"
        || metadata.stored_path != module_path.display().to_string()
        || !metadata.verified
    {
        return Err(AppError::message(
            "Pane block module metadata is not verified for this runtime. Re-register it with the expected SHA-256.",
        ));
    }
    let actual_sha256 = sha256_file(&module_path).map_err(|error| {
        AppError::message(format!(
            "Registered Pane block module is missing or unreadable at {}: {error}",
            module_path.display()
        ))
    })?;
    let actual_bytes = fs::metadata(&module_path)?.len();
    if metadata.sha256 != actual_sha256 || metadata.bytes != actual_bytes {
        return Err(AppError::message(
            "Registered Pane block module no longer matches its verified metadata. Re-register it before building the initramfs.",
        ));
    }
    if metadata.target_kernel_path.as_deref()
        != Some(paths.kernel_image.display().to_string().as_str())
        || metadata.target_kernel_bytes != Some(kernel_metadata.kernel_bytes)
        || metadata.target_kernel_sha256.as_deref() != Some(kernel_metadata.kernel_sha256.as_str())
        || metadata.target_kernel_format.as_deref() != Some(kernel_metadata.kernel_format.as_str())
        || !pane_block_module_matches_current_driver_abi(&metadata, &initramfs_driver)
    {
        return Err(AppError::message(
            "Registered Pane block module was not built for the current verified kernel and Pane block-driver ABI. Rebuild and re-register pane-block.ko.",
        ));
    }
    Ok(metadata)
}

fn build_and_register_pane_block_module(
    paths: &RuntimePaths,
    kernel_build_dir: Option<&Path>,
    force: bool,
) -> AppResult<()> {
    let kernel_build_dir = kernel_build_dir.ok_or_else(|| {
        AppError::message(
            "--build-pane-block-module requires --kernel-build-dir pointing at the target Arch kernel build directory.",
        )
    })?;
    if !kernel_build_dir.is_dir() {
        return Err(AppError::message(format!(
            "Pane block module kernel build directory does not exist or is not a directory: {}",
            kernel_build_dir.display()
        )));
    }
    load_verified_pane_initramfs_driver_metadata(paths)?;
    let build_script = paths
        .initramfs_driver_dir
        .join("build-pane-block-module.sh");
    if !build_script.is_file() {
        return Err(AppError::message(format!(
            "Pane block module build script is missing: {}. Regenerate it with `pane runtime --write-initramfs-driver`.",
            build_script.display()
        )));
    }
    let output = pane_block_module_path(paths);
    if output.exists() && !force {
        return Err(AppError::message(format!(
            "A Pane block module already exists at {}. Pass --force to rebuild and replace it.",
            output.display()
        )));
    }
    let status = Command::new("sh")
        .arg("./build-pane-block-module.sh")
        .arg("pane-block.ko")
        .env("KERNEL_BUILD_DIR", kernel_build_dir)
        .current_dir(&paths.initramfs_driver_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            AppError::message(format!(
                "Failed to run Pane block module build script with `sh`: {error}. Build pane-block.ko externally from the generated bundle, then register it with `pane runtime --register-pane-block-module <path> --pane-block-module-expected-sha256 <sha256>`."
            ))
        })?;

    if !status.status.success() {
        return Err(AppError::message(format!(
            "Pane block module build failed with status {}.\nstdout:\n{}\nstderr:\n{}",
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        )));
    }

    let sha256 = sha256_file(&output)?;
    register_pane_block_module(paths, &output, Some(&sha256), true)
}

fn pane_initramfs_hook_source() -> String {
    format!(
        r#"#!/bin/sh
set -eu

# Pane initramfs discovery hook.
# This runs before root mount and records the native storage contract that a
# Pane-aware block driver must consume.

pane_cmdline_value() {{
    key="$1"
    for token in $(cat /proc/cmdline); do
        case "$token" in
            "$key="*) printf '%s\n' "${{token#*=}}"; return 0 ;;
        esac
    done
    return 1
}}

pane_storage_contract="$(pane_cmdline_value pane.storage_contract || true)"
pane_block_io="$(pane_cmdline_value pane.block_io || true)"
pane_block_dma="$(pane_cmdline_value pane.block_dma || true)"
pane_root="$(pane_cmdline_value pane.root || true)"
pane_user="$(pane_cmdline_value pane.user || true)"
pane_virtio_root="$(pane_cmdline_value pane.virtio_root || true)"
pane_block_devices="$(pane_cmdline_value pane.block_devices || true)"
pane_framebuffer="$(pane_cmdline_value pane.framebuffer || true)"
pane_input_queue="$(pane_cmdline_value pane.input_queue || true)"

mkdir -p /run/pane
cat > /run/pane/native-storage.env <<EOF
PANE_STORAGE_CONTRACT=$pane_storage_contract
PANE_BLOCK_IO=$pane_block_io
PANE_BLOCK_DMA=$pane_block_dma
PANE_ROOT=$pane_root
PANE_USER=$pane_user
PANE_VIRTIO_ROOT=$pane_virtio_root
PANE_BLOCK_DEVICES=$pane_block_devices
PANE_FRAMEBUFFER=$pane_framebuffer
PANE_INPUT_QUEUE=$pane_input_queue
PANE_BLOCK_IO_PROTOCOL={}
PANE_BLOCK_IO_STATUS_OFFSET={}
PANE_BLOCK_IO_DATA_OFFSET={}
EOF

if [ -z "$pane_storage_contract" ] || [ -z "$pane_block_io" ]; then
    echo "pane-initramfs: missing pane.storage_contract or pane.block_io kernel argument" >&2
    exit 1
fi

echo "pane-initramfs: discovered storage contract at $pane_storage_contract with block ABI $pane_block_io"
"#,
        default_pane_block_io_protocol(),
        default_pane_block_io_status_port_offset(),
        default_pane_block_io_data_port_offset()
    )
}

fn pane_port_block_header_source() -> String {
    format!(
        r#"#ifndef PANE_PORT_BLOCK_H
#define PANE_PORT_BLOCK_H

#ifdef __KERNEL__
#include <linux/types.h>
#else
#include <stdint.h>
#endif

#define PANE_BLOCK_IO_PROTOCOL "{}"
#define PANE_BLOCK_IO_BASE_PORT {}
#define PANE_BLOCK_IO_PORT_COUNT {}
#define PANE_BLOCK_IO_DEVICE_OFFSET 0
#define PANE_BLOCK_IO_OPERATION_OFFSET 1
#define PANE_BLOCK_IO_STATUS_OFFSET {}
#define PANE_BLOCK_IO_BLOCK_SIZE_UNITS_OFFSET 3
#define PANE_BLOCK_IO_BLOCK_INDEX_OFFSET 4
#define PANE_BLOCK_IO_DATA_OFFSET {}
#define PANE_BLOCK_IO_RESPONSE_SIZE_LOW_OFFSET 13
#define PANE_BLOCK_IO_RESPONSE_SIZE_MID_OFFSET 14
#define PANE_BLOCK_IO_RESPONSE_SIZE_HIGH_OFFSET 15
#define PANE_BLOCK_IO_BLOCK_SIZE_BYTES {}

#define PANE_BLOCK_DEVICE_BASE_OS 0
#define PANE_BLOCK_DEVICE_USER_DISK 1
#define PANE_BLOCK_OPERATION_READ 0
#define PANE_BLOCK_OPERATION_WRITE 1

#define PANE_BLOCK_STATUS_SUBMITTED 0x01
#define PANE_BLOCK_STATUS_SERVICED 0x02
#define PANE_BLOCK_STATUS_DENIED 0xfc
#define PANE_BLOCK_STATUS_FAILED 0xfd
#define PANE_BLOCK_STATUS_INVALID 0xfe

#endif
"#,
        default_pane_block_io_protocol(),
        crate::native::PANE_BLOCK_IO_BASE_PORT,
        crate::native::PANE_BLOCK_IO_PORT_COUNT,
        default_pane_block_io_status_port_offset(),
        default_pane_block_io_data_port_offset(),
        crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES
    )
}

fn pane_init_source() -> String {
    r#"#include "pane-port-block.h"

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sched.h>
#include <sys/io.h>
#include <sys/mount.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysmacros.h>
#include <unistd.h>

#define COM1_PORT 0x3f8
#define COM1_PORT_COUNT 8
#define COM1_LINE_STATUS 5
#define COM1_TRANSMIT_EMPTY 0x20

static int com1_enabled = 0;

static void com1_init(void) {
    if (ioperm(COM1_PORT, COM1_PORT_COUNT, 1) != 0) {
        return;
    }

    outb(0x00, COM1_PORT + 1);
    outb(0x80, COM1_PORT + 3);
    outb(0x03, COM1_PORT + 0);
    outb(0x00, COM1_PORT + 1);
    outb(0x03, COM1_PORT + 3);
    outb(0xc7, COM1_PORT + 2);
    outb(0x0b, COM1_PORT + 4);
    com1_enabled = 1;
}

static void com1_write_byte(char value) {
    if (!com1_enabled) {
        return;
    }
    for (unsigned int spin = 0; spin < 100000; spin++) {
        if ((inb(COM1_PORT + COM1_LINE_STATUS) & COM1_TRANSMIT_EMPTY) != 0) {
            break;
        }
    }
    outb((unsigned char)value, COM1_PORT);
}

static void log_line(const char *value) {
    write(STDOUT_FILENO, value, strlen(value));
    write(STDOUT_FILENO, "\n", 1);
    while (*value) {
        if (*value == '\n') {
            com1_write_byte('\r');
        }
        com1_write_byte(*value++);
    }
    com1_write_byte('\r');
    com1_write_byte('\n');
}

static void log_key_value(const char *key, const char *value) {
    char line[512];
    snprintf(line, sizeof(line), "%s=%s", key, value[0] ? value : "<missing>");
    log_line(line);
}

static int truthy_arg(const char *value) {
    return strcmp(value, "1") == 0 ||
           strcmp(value, "true") == 0 ||
           strcmp(value, "yes") == 0 ||
           strcmp(value, "readonly") == 0;
}

static void mkdir_if_missing(const char *path, mode_t mode) {
    if (mkdir(path, mode) != 0 && errno != EEXIST) {
        char line[256];
        snprintf(line, sizeof(line), "PANE_INITRAMFS_MKDIR_FAILED path=%s errno=%d", path, errno);
        log_line(line);
    }
}

static void write_native_storage_env(
    const char *storage_contract,
    const char *block_io,
    const char *block_dma,
    const char *root_device,
    const char *user_device,
    const char *virtio_root_device,
    const char *root_readonly,
    const char *root_fs,
    const char *framebuffer,
    const char *input_queue
) {
    mkdir_if_missing("/run", 0755);
    mkdir_if_missing("/run/pane", 0755);
    int fd = open("/run/pane/native-storage.env", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        log_line("PANE_INITRAMFS_ENV_WRITE_FAILED");
        return;
    }
    dprintf(fd, "PANE_STORAGE_CONTRACT=%s\n", storage_contract);
    dprintf(fd, "PANE_BLOCK_IO=%s\n", block_io);
    dprintf(fd, "PANE_BLOCK_DMA=%s\n", block_dma);
    dprintf(fd, "PANE_ROOT=%s\n", root_device);
    dprintf(fd, "PANE_USER=%s\n", user_device);
    dprintf(fd, "PANE_VIRTIO_ROOT=%s\n", virtio_root_device);
    dprintf(fd, "PANE_ROOT_READONLY=%s\n", root_readonly);
    dprintf(fd, "PANE_ROOT_FS=%s\n", root_fs);
    dprintf(fd, "PANE_FRAMEBUFFER=%s\n", framebuffer);
    dprintf(fd, "PANE_INPUT_QUEUE=%s\n", input_queue);
    dprintf(fd, "PANE_BLOCK_IO_PROTOCOL=%s\n", PANE_BLOCK_IO_PROTOCOL);
    dprintf(fd, "PANE_BLOCK_IO_BASE_PORT=0x%x\n", PANE_BLOCK_IO_BASE_PORT);
    dprintf(fd, "PANE_BLOCK_IO_BLOCK_SIZE_BYTES=%u\n", PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
    close(fd);
    log_line("PANE_INITRAMFS_ENV_READY");
}

static int read_cmdline(char *buffer, size_t size) {
    int fd = open("/proc/cmdline", O_RDONLY);
    if (fd < 0) {
        return -1;
    }
    ssize_t count = read(fd, buffer, size - 1);
    close(fd);
    if (count < 0) {
        return -1;
    }
    buffer[count] = '\0';
    return 0;
}

static int find_arg(const char *cmdline, const char *key, char *value, size_t value_size) {
    size_t key_len = strlen(key);
    const char *cursor = cmdline;
    while (*cursor) {
        while (*cursor == ' ') {
            cursor++;
        }
        if (strncmp(cursor, key, key_len) == 0 && cursor[key_len] == '=') {
            cursor += key_len + 1;
            size_t index = 0;
            while (*cursor && *cursor != ' ' && index + 1 < value_size) {
                value[index++] = *cursor++;
            }
            value[index] = '\0';
            return 0;
        }
        while (*cursor && *cursor != ' ') {
            cursor++;
        }
    }
    return -1;
}

static void wait_forever(void) {
    log_line("PANE_INITRAMFS_DISCOVERY_WAITING");
    for (;;) {
        pause();
    }
}

static void prepare_minimal_mounts(void) {
    mkdir_if_missing("/proc", 0555);
    mkdir_if_missing("/sys", 0555);
    mkdir_if_missing("/dev", 0755);
    mkdir_if_missing("/newroot", 0755);
    mount("proc", "/proc", "proc", 0, "");
    mount("sysfs", "/sys", "sysfs", 0, "");
    if (mount("devtmpfs", "/dev", "devtmpfs", 0, "mode=0755") != 0) {
        mknod("/dev/console", S_IFCHR | 0600, makedev(5, 1));
        mknod("/dev/null", S_IFCHR | 0666, makedev(1, 3));
    }
}

static int wait_for_device(const char *path) {
    for (unsigned int attempt = 0; attempt < 65536; attempt++) {
        if (access(path, F_OK) == 0) {
            return 0;
        }
        sched_yield();
    }
    return -1;
}

static int load_pane_block_module(const char *device_blocks, const char *root_offset, const char *block_dma) {
    const char *module_path = "/lib/modules/pane-block.ko";
    char params[256] = {0};
    char *dma_separator = NULL;
    char *dma_end = NULL;
    unsigned long long base_block_offset = 0;
    unsigned long long shared_buffer_gpa = 0;
    unsigned long long shared_buffer_bytes = 0;
    int fd = open(module_path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) {
            log_line("PANE_BLOCK_MODULE_NOT_PRESENT");
            return 0;
        }
        char line[160];
        snprintf(line, sizeof(line), "PANE_BLOCK_MODULE_OPEN_FAILED errno=%d", errno);
        log_line(line);
        return -1;
    }

    log_line("PANE_BLOCK_MODULE_LOAD_ATTEMPT");
    if (root_offset && root_offset[0] != '\0') {
        base_block_offset = strtoull(root_offset, NULL, 10) / PANE_BLOCK_IO_BLOCK_SIZE_BYTES;
    }
    if (block_dma && block_dma[0] != '\0') {
        errno = 0;
        shared_buffer_gpa = strtoull(block_dma, &dma_separator, 0);
        if (errno != 0 || dma_separator == block_dma || *dma_separator != ',') {
            log_line("PANE_BLOCK_DMA_CONTRACT_INVALID");
            shared_buffer_gpa = 0;
        } else {
            errno = 0;
            shared_buffer_bytes = strtoull(dma_separator + 1, &dma_end, 0);
            if (errno != 0 || dma_end == dma_separator + 1 || *dma_end != '\0' ||
                shared_buffer_bytes < PANE_BLOCK_IO_BLOCK_SIZE_BYTES) {
                log_line("PANE_BLOCK_DMA_CONTRACT_INVALID");
                shared_buffer_gpa = 0;
                shared_buffer_bytes = 0;
            } else {
                log_line("PANE_BLOCK_DMA_ENABLED_FOR_SHARED_BUFFER");
            }
        }
    }
    if (device_blocks && device_blocks[0] != '\0') {
        snprintf(params, sizeof(params),
                 "device_blocks=%s base_block_offset=%llu shared_buffer_gpa=%llu shared_buffer_bytes=%llu trust_submit_completion=1",
                 device_blocks, base_block_offset, shared_buffer_gpa, shared_buffer_bytes);
    } else {
        snprintf(params, sizeof(params),
                 "base_block_offset=%llu shared_buffer_gpa=%llu shared_buffer_bytes=%llu trust_submit_completion=1",
                 base_block_offset, shared_buffer_gpa, shared_buffer_bytes);
    }
#ifdef SYS_finit_module
    pid_t child = fork();
    if (child == 0) {
        int child_rc = syscall(SYS_finit_module, fd, params, 0);
        _exit(child_rc == 0 ? 0 : (errno == EEXIST ? 17 : 1));
    }
    if (child < 0) {
        char line[160];
        snprintf(line, sizeof(line), "PANE_BLOCK_MODULE_FORK_FAILED errno=%d", errno);
        log_line(line);
        close(fd);
        return -1;
    }
    for (unsigned int attempt = 0; attempt < 50000000; attempt++) {
        int status = 0;
        pid_t waited = waitpid(child, &status, WNOHANG);
        if (waited == child) {
            close(fd);
            if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
                log_line("PANE_BLOCK_MODULE_LOAD_OK");
                return 0;
            }
            if (WIFEXITED(status) && WEXITSTATUS(status) == 17) {
                log_line("PANE_BLOCK_MODULE_ALREADY_LOADED");
                return 0;
            }
            char line[160];
            snprintf(line, sizeof(line), "PANE_BLOCK_MODULE_LOAD_FAILED status=%d", status);
            log_line(line);
            return -1;
        }
        if (waited < 0) {
            char line[160];
            snprintf(line, sizeof(line), "PANE_BLOCK_MODULE_WAIT_FAILED errno=%d", errno);
            log_line(line);
            close(fd);
            return -1;
        }
        if ((attempt % 1000000U) == 999999U) {
            log_line("PANE_BLOCK_MODULE_LOAD_WAITING");
        }
        sched_yield();
    }
    log_line("PANE_BLOCK_MODULE_LOAD_TIMEOUT");
    close(fd);
    return -1;
#else
    errno = ENOSYS;
    char line[160];
    snprintf(line, sizeof(line), "PANE_BLOCK_MODULE_LOAD_FAILED errno=%d", errno);
    log_line(line);
    close(fd);
    return -1;
#endif
}

static int load_virtio_mmio_module(void) {
    const char *module_path = "/lib/modules/virtio_mmio.ko";
    int fd = open(module_path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) {
            log_line("PANE_VIRTIO_MMIO_MODULE_NOT_PRESENT");
            return 0;
        }
        char line[160];
        snprintf(line, sizeof(line), "PANE_VIRTIO_MMIO_MODULE_OPEN_FAILED errno=%d", errno);
        log_line(line);
        return -1;
    }

    log_line("PANE_VIRTIO_MMIO_MODULE_LOAD_ATTEMPT");
#ifdef SYS_finit_module
    /* virtio_mmio is built CONFIG_VIRTIO_MMIO=m on stock Arch. Loading it makes the
       module honor the virtio_mmio.device= directive already on the boot cmdline
       (CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y), which registers the platform device so
       the built-in virtio_blk driver can bind and create /dev/vda. Parameters are
       empty: the kernel applies the boot-cmdline module params at load time. */
    pid_t child = fork();
    if (child == 0) {
        int child_rc = syscall(SYS_finit_module, fd, "", 0);
        _exit(child_rc == 0 ? 0 : (errno == EEXIST ? 17 : 1));
    }
    if (child < 0) {
        char line[160];
        snprintf(line, sizeof(line), "PANE_VIRTIO_MMIO_MODULE_FORK_FAILED errno=%d", errno);
        log_line(line);
        close(fd);
        return -1;
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        char line[160];
        snprintf(line, sizeof(line), "PANE_VIRTIO_MMIO_MODULE_WAIT_FAILED errno=%d", errno);
        log_line(line);
        close(fd);
        return -1;
    }
    close(fd);
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
        log_line("PANE_VIRTIO_MMIO_MODULE_LOAD_OK");
        return 0;
    }
    if (WIFEXITED(status) && WEXITSTATUS(status) == 17) {
        log_line("PANE_VIRTIO_MMIO_MODULE_ALREADY_LOADED");
        return 0;
    }
    char line[160];
    snprintf(line, sizeof(line), "PANE_VIRTIO_MMIO_MODULE_LOAD_FAILED status=%d", status);
    log_line(line);
    return -1;
#else
    errno = ENOSYS;
    log_line("PANE_VIRTIO_MMIO_MODULE_LOAD_FAILED");
    close(fd);
    return -1;
#endif
}

static void drop_probe_caches_before_root_mount(void) {
    sync();
    int fd = open("/proc/sys/vm/drop_caches", O_WRONLY | O_CLOEXEC);
    if (fd < 0) {
        char line[160];
        snprintf(line, sizeof(line), "PANE_BLOCK_PROBE_CACHE_DROP_OPEN_FAILED errno=%d", errno);
        log_line(line);
        return;
    }
    if (write(fd, "3\n", 2) != 2) {
        char line[160];
        snprintf(line, sizeof(line), "PANE_BLOCK_PROBE_CACHE_DROP_WRITE_FAILED errno=%d", errno);
        log_line(line);
    } else {
        log_line("PANE_BLOCK_PROBE_CACHE_DROPPED");
    }
    close(fd);
}

static int supported_root_fs(const char *value) {
    return strcmp(value, "ext4") == 0 ||
           strcmp(value, "btrfs") == 0 ||
           strcmp(value, "xfs") == 0 ||
           strcmp(value, "f2fs") == 0;
}

#define PANE_ROOT_MOUNT_MAX_POLLS 65536U
#define PANE_ROOT_MOUNT_WAIT_LOG_INTERVAL 4096U

static int mount_root_with_fs(const char *root_device, const char *filesystem, int root_readonly) {
    unsigned long flags = MS_RELATIME | (root_readonly ? MS_RDONLY : 0);
    char line[192];
    snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_TRY fs=%s readonly=%s", filesystem, root_readonly ? "true" : "false");
    log_line(line);

    pid_t child = fork();
    if (child == 0) {
        int mount_rc = mount(root_device, "/newroot", filesystem, flags, "");
        _exit(mount_rc == 0 ? 0 : (errno & 0xff));
    }
    if (child < 0) {
        snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_FORK_FAILED fs=%s errno=%d", filesystem, errno);
        log_line(line);
        return -1;
    }

    for (unsigned int attempt = 0; attempt < PANE_ROOT_MOUNT_MAX_POLLS; attempt++) {
        int status = 0;
        pid_t waited = waitpid(child, &status, WNOHANG);
        if (waited == child) {
            if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
                if (root_readonly) {
                    snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_OK fs=%s readonly=true", filesystem);
                } else {
                    snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_OK fs=%s", filesystem);
                }
                log_line(line);
                return 0;
            }
            if (WIFEXITED(status)) {
                snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_FAIL fs=%s errno=%d", filesystem, WEXITSTATUS(status));
            } else {
                snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_FAIL fs=%s status=%d", filesystem, status);
            }
            log_line(line);
            return -1;
        }
        if (waited < 0) {
            snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_WAIT_FAILED fs=%s errno=%d", filesystem, errno);
            log_line(line);
            return -1;
        }
        if ((attempt % PANE_ROOT_MOUNT_WAIT_LOG_INTERVAL) == (PANE_ROOT_MOUNT_WAIT_LOG_INTERVAL - 1U)) {
            snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_WAITING fs=%s polls=%u", filesystem, attempt + 1U);
            log_line(line);
        }
        sched_yield();
    }
    kill(child, SIGKILL);
    snprintf(line, sizeof(line), "PANE_ROOT_MOUNT_TIMEOUT fs=%s polls=%u", filesystem, PANE_ROOT_MOUNT_MAX_POLLS);
    log_line(line);
    return -1;
}

static int try_mount_root(const char *root_device, int root_readonly, const char *root_fs) {
    const char *filesystems[] = {"ext4", "btrfs", "xfs", "f2fs", NULL};
    if (supported_root_fs(root_fs) && mount_root_with_fs(root_device, root_fs, root_readonly) == 0) {
        return 0;
    }

    if (root_readonly) {
        for (unsigned int index = 0; filesystems[index] != NULL; index++) {
            if (root_fs[0] != '\0' && strcmp(root_fs, filesystems[index]) == 0) {
                continue;
            }
            if (mount_root_with_fs(root_device, filesystems[index], 1) == 0) {
                return 0;
            }
        }
        return -1;
    }

    for (unsigned int index = 0; filesystems[index] != NULL; index++) {
        if (root_fs[0] != '\0' && strcmp(root_fs, filesystems[index]) == 0) {
            continue;
        }
        if (mount_root_with_fs(root_device, filesystems[index], 0) == 0) {
            return 0;
        }
    }
    if (mount_root_with_fs(root_device, "ext4", 1) == 0) {
        return 0;
    }
    return -1;
}

static void move_mount_if_present(const char *source, const char *target) {
    mkdir_if_missing(target, 0755);
    mount(source, target, NULL, MS_MOVE, NULL);
}

static void exec_real_init(void) {
    move_mount_if_present("/proc", "/newroot/proc");
    move_mount_if_present("/sys", "/newroot/sys");
    move_mount_if_present("/dev", "/newroot/dev");
    if (chdir("/newroot") != 0 || chroot(".") != 0 || chdir("/") != 0) {
        log_line("PANE_ROOT_SWITCH_FAILED");
        wait_forever();
    }
    log_line("PANE_INIT_EXEC");
    execl("/sbin/init", "init", NULL);
    execl("/usr/lib/systemd/systemd", "systemd", NULL);
    execl("/bin/init", "init", NULL);
    log_line("PANE_INIT_EXEC_FAILED");
    wait_forever();
}

int main(void) {
    char cmdline[4096];
    char storage_contract[128] = {0};
    char block_io[128] = {0};
    char block_dma[128] = {0};
    char root_device[128] = {0};
    char user_device[128] = {0};
    char virtio_root_device[128] = {0};
    char root_readonly[32] = {0};
    char root_fs[32] = {0};
    char root_offset[128] = {0};
    char block_devices[128] = {0};
    char framebuffer[128] = {0};
    char input_queue[128] = {0};

    com1_init();
    log_line("PANE_INITRAMFS_DISCOVERY_START");
    prepare_minimal_mounts();

    if (read_cmdline(cmdline, sizeof(cmdline)) != 0) {
        log_line("PANE_INITRAMFS_CMDLINE_FAILED");
        wait_forever();
    }
    find_arg(cmdline, "pane.storage_contract", storage_contract, sizeof(storage_contract));
    find_arg(cmdline, "pane.block_io", block_io, sizeof(block_io));
    find_arg(cmdline, "pane.block_dma", block_dma, sizeof(block_dma));
    find_arg(cmdline, "pane.root", root_device, sizeof(root_device));
    find_arg(cmdline, "pane.user", user_device, sizeof(user_device));
    find_arg(cmdline, "pane.virtio_root", virtio_root_device, sizeof(virtio_root_device));
    find_arg(cmdline, "pane.root_readonly", root_readonly, sizeof(root_readonly));
    find_arg(cmdline, "pane.root_fs", root_fs, sizeof(root_fs));
    find_arg(cmdline, "pane.root_offset", root_offset, sizeof(root_offset));
    find_arg(cmdline, "pane.block_devices", block_devices, sizeof(block_devices));
    find_arg(cmdline, "pane.framebuffer", framebuffer, sizeof(framebuffer));
    find_arg(cmdline, "pane.input_queue", input_queue, sizeof(input_queue));

    log_key_value("pane.storage_contract", storage_contract);
    log_key_value("pane.block_io", block_io);
    log_key_value("pane.block_dma", block_dma);
    log_key_value("pane.root", root_device);
    log_key_value("pane.user", user_device);
    log_key_value("pane.virtio_root", virtio_root_device);
    log_key_value("pane.root_readonly", root_readonly);
    log_key_value("pane.root_fs", root_fs);
    log_key_value("pane.root_offset", root_offset);
    log_key_value("pane.block_devices", block_devices);
    log_key_value("pane.framebuffer", framebuffer);
    log_key_value("pane.input_queue", input_queue);

    if (storage_contract[0] == '\0' || block_io[0] == '\0' || root_device[0] == '\0') {
        log_line("PANE_INITRAMFS_DISCOVERY_FAILED");
        wait_forever();
    }

    if (ioperm(PANE_BLOCK_IO_BASE_PORT, PANE_BLOCK_IO_PORT_COUNT, 1) != 0) {
        char line[128];
        snprintf(line, sizeof(line), "PANE_BLOCK_IO_PROBE_IOPERM_FAILED errno=%d", errno);
        log_line(line);
        wait_forever();
    }

    unsigned int units = inb(PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_BLOCK_SIZE_UNITS_OFFSET);
    unsigned int bytes = units * 512U;
    if (bytes != PANE_BLOCK_IO_BLOCK_SIZE_BYTES) {
        char line[128];
        snprintf(line, sizeof(line), "PANE_BLOCK_IO_PROBE_BAD_SIZE bytes=%u", bytes);
        log_line(line);
        wait_forever();
    }

    log_line("PANE_BLOCK_IO_PROBE_OK");
    load_pane_block_module(block_devices, root_offset, block_dma);
    drop_probe_caches_before_root_mount();
    log_line("PANE_INITRAMFS_DISCOVERY_DONE");
    if (framebuffer[0] != '\0' && input_queue[0] != '\0') {
        log_line("PANE_DISPLAY_CONTRACT_DISCOVERED");
    } else {
        log_line("PANE_DISPLAY_CONTRACT_MISSING");
    }
    write_native_storage_env(storage_contract, block_io, block_dma, root_device, user_device, virtio_root_device, root_readonly, root_fs, framebuffer, input_queue);

    if (virtio_root_device[0] != '\0') {
        log_line("PANE_VIRTIO_ROOT_MOUNT_ATTEMPT");
        load_virtio_mmio_module();
        if (wait_for_device(virtio_root_device) == 0) {
            if (try_mount_root(virtio_root_device, truthy_arg(root_readonly), root_fs) == 0) {
                exec_real_init();
            }
            log_line("PANE_VIRTIO_ROOT_MOUNT_FALLBACK");
        } else {
            log_line("PANE_VIRTIO_ROOT_DEVICE_WAIT_TIMEOUT");
        }
    }
    log_line("PANE_ROOT_MOUNT_ATTEMPT");
    if (wait_for_device(root_device) != 0) {
        log_line("PANE_ROOT_DEVICE_WAIT_TIMEOUT");
        wait_forever();
    }
    if (try_mount_root(root_device, truthy_arg(root_readonly), root_fs) != 0) {
        log_line("PANE_ROOT_MOUNT_FAILED");
        wait_forever();
    }
    exec_real_init();
    wait_forever();
}
"#
    .to_string()
}

fn pane_port_probe_source() -> String {
    r#"#include "pane-port-block.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/io.h>

/*
 * Pane native-storage probe.
 *
 * This is intentionally a small initramfs-side probe source, not the final
 * kernel block driver. It verifies that the packaged initramfs can reach the
 * Pane port ABI before the later milestone promotes the protocol into a real
 * root-mountable block device.
 */

int main(void) {
    if (ioperm(PANE_BLOCK_IO_BASE_PORT, PANE_BLOCK_IO_PORT_COUNT, 1) != 0) {
        fprintf(stderr, "pane-port-probe: ioperm failed: %d\n", errno);
        return 2;
    }

    unsigned int units = inb(PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_BLOCK_SIZE_UNITS_OFFSET);
    unsigned int bytes = units * 512U;
    printf("pane-port-probe: protocol=%s base=0x%x ports=%u block_size=%u\n",
           PANE_BLOCK_IO_PROTOCOL,
           PANE_BLOCK_IO_BASE_PORT,
           PANE_BLOCK_IO_PORT_COUNT,
           bytes);

    return bytes == PANE_BLOCK_IO_BLOCK_SIZE_BYTES ? 0 : 3;
}
"#
    .to_string()
}

fn pane_block_driver_source() -> String {
    r#"#include "pane-port-block.h"

#include <linux/blk-mq.h>
#include <linux/blkdev.h>
#include <linux/errno.h>
#include <linux/err.h>
#include <linux/hdreg.h>
#include <linux/io.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/processor.h>
#include <linux/string.h>
#include <linux/types.h>
#include <linux/vmalloc.h>

#define PANE_BLOCK_DRIVER_NAME "pane_block"
#define PANE_BLOCK_DEVICE_COUNT 2
#define PANE_BLOCK_MINORS_PER_DISK 16
#define PANE_BLOCK_DEFAULT_BLOCKS 2097152UL
#define PANE_BLOCK_SECTOR_SIZE 512U

static unsigned long device_blocks[PANE_BLOCK_DEVICE_COUNT] = {
    PANE_BLOCK_DEFAULT_BLOCKS,
    PANE_BLOCK_DEFAULT_BLOCKS,
};
static unsigned long base_block_offset;
static unsigned long shared_buffer_gpa;
static unsigned long shared_buffer_bytes;
static bool trust_submit_completion;
module_param_array(device_blocks, ulong, NULL, 0444);
MODULE_PARM_DESC(device_blocks, "Logical Pane I/O blocks for /dev/pane0 and /dev/pane1");
module_param(base_block_offset, ulong, 0444);
MODULE_PARM_DESC(base_block_offset, "Pane I/O block offset where /dev/pane0 starts inside the base OS image");
module_param(shared_buffer_gpa, ulong, 0444);
MODULE_PARM_DESC(shared_buffer_gpa, "Guest physical address of the Pane shared block transfer buffer");
module_param(shared_buffer_bytes, ulong, 0444);
MODULE_PARM_DESC(shared_buffer_bytes, "Bytes available in the Pane shared block transfer buffer");
module_param(trust_submit_completion, bool, 0444);
MODULE_PARM_DESC(trust_submit_completion, "Treat Pane's synchronous submit-port VM exit as I/O completion");

struct pane_block_disk {
    int pane_device_id;
    u64 block_offset;
    char name[DISK_NAME_LEN];
    struct gendisk *disk;
    struct blk_mq_tag_set tag_set;
};

static struct pane_block_disk pane_disks[PANE_BLOCK_DEVICE_COUNT];
static int pane_block_major;
static bool pane_block_initializing = true;
static void *pane_block_shared_buffer;
static unsigned char *pane_block_bounce_buffer;
static unsigned int pane_block_request_log_count;
static unsigned int pane_block_init_read_log_count;
static unsigned int pane_block_serial_log_count;

static const struct block_device_operations pane_block_fops = {
    .owner = THIS_MODULE,
};

static void pane_block_serial_log(const char *line)
{
    if (pane_block_serial_log_count >= 64)
        return;
    pane_block_serial_log_count++;
    while (*line) {
        outb((u8)*line, 0x3f8);
        line++;
    }
    outb('\r', 0x3f8);
    outb('\n', 0x3f8);
}

static void pane_block_write_index(u64 block_index)
{
    outl((u32)(block_index & 0xffffffff),
         PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_BLOCK_INDEX_OFFSET);
    outl((u32)(block_index >> 32),
         PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_BLOCK_INDEX_OFFSET + 4);
}

static int pane_block_wait_serviced(bool log_transfer)
{
    unsigned int attempt;
    u8 status = 0;

    for (attempt = 0; attempt < 1024; attempt++) {
        status = inb(PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_STATUS_OFFSET);
        if (log_transfer || status != PANE_BLOCK_STATUS_SERVICED) {
            pr_info(PANE_BLOCK_DRIVER_NAME
                    ": PANE_BLOCK_STATUS_READ attempt=%u status=0x%02x\n",
                    attempt, status);
        }
        if (status == PANE_BLOCK_STATUS_SERVICED) {
            if (log_transfer)
                pane_block_serial_log("PANE_BLOCK_STATUS_SERVICED");
            return 0;
        }
        if (status == PANE_BLOCK_STATUS_DENIED ||
            status == PANE_BLOCK_STATUS_FAILED ||
            status == PANE_BLOCK_STATUS_INVALID) {
            pane_block_serial_log("PANE_BLOCK_STATUS_ERROR");
            return -EIO;
        }
        cpu_relax();
    }

    pane_block_serial_log("PANE_BLOCK_STATUS_WAIT_TIMEOUT");
    return -EAGAIN;
}

static int pane_block_transfer(int device_id, int operation, u64 block_index, void *buffer)
{
    unsigned int word_index;
    unsigned char *bytes = buffer;
    bool log_transfer = false;

    outb(device_id, PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_DEVICE_OFFSET);
    outb(operation, PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_OPERATION_OFFSET);
    pane_block_write_index(block_index);

    if (operation == PANE_BLOCK_OPERATION_WRITE && pane_block_shared_buffer) {
        memcpy(pane_block_shared_buffer, bytes, PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
    } else if (operation == PANE_BLOCK_OPERATION_WRITE) {
        for (word_index = 0;
             word_index < PANE_BLOCK_IO_BLOCK_SIZE_BYTES / sizeof(u32);
             word_index++) {
            u32 word;

            memcpy(&word, bytes + word_index * sizeof(word), sizeof(word));
            outl(word, PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_DATA_OFFSET);
        }
    }

    pane_block_serial_log("PANE_BLOCK_SUBMIT_READY");
    outb(1, PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_STATUS_OFFSET);
    pane_block_serial_log("PANE_BLOCK_TRANSFER_SUBMITTED");
    if (!trust_submit_completion) {
        if (pane_block_wait_serviced(log_transfer) != 0) {
            pr_err(PANE_BLOCK_DRIVER_NAME
                   ": PANE_BLOCK_TRANSFER_WAIT_FAILED device=%d op=%d block=%llu trust_submit_completion=%d\n",
                   device_id, operation, block_index, trust_submit_completion);
            return -EIO;
        }
    }

    if (operation == PANE_BLOCK_OPERATION_READ) {
        if (pane_block_shared_buffer) {
            memcpy(bytes, pane_block_shared_buffer, PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
            pane_block_serial_log("PANE_BLOCK_READ_DMA_COPIED");
        } else {
            for (word_index = 0;
                 word_index < PANE_BLOCK_IO_BLOCK_SIZE_BYTES / sizeof(u32);
                 word_index++) {
                u32 word = inl(PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_DATA_OFFSET);

                memcpy(bytes + word_index * sizeof(word), &word, sizeof(word));
            }
            pane_block_serial_log("PANE_BLOCK_READ_PORT_COPIED");
        }
    }

    return 0;
}

static blk_status_t pane_block_queue_rq(struct blk_mq_hw_ctx *hctx,
                                        const struct blk_mq_queue_data *bd)
{
    struct request *rq = bd->rq;
    struct pane_block_disk *pane_disk = rq->q->queuedata;
    struct bio_vec bvec;
    struct req_iterator iter;
    blk_status_t status = BLK_STS_OK;
    unsigned char *bounce = pane_block_bounce_buffer;
    int operation;

    if (req_op(rq) == REQ_OP_READ)
        operation = PANE_BLOCK_OPERATION_READ;
    else if (req_op(rq) == REQ_OP_WRITE)
        operation = PANE_BLOCK_OPERATION_WRITE;
    else if (req_op(rq) == REQ_OP_FLUSH || req_op(rq) == REQ_OP_DISCARD) {
        blk_mq_start_request(rq);
        blk_mq_end_request(rq, BLK_STS_OK);
        return BLK_STS_OK;
    } else {
        blk_mq_start_request(rq);
        blk_mq_end_request(rq, BLK_STS_IOERR);
        return BLK_STS_IOERR;
    }

    if (pane_block_request_log_count < 16) {
        pane_block_request_log_count++;
        pr_info(PANE_BLOCK_DRIVER_NAME
                ": PANE_BLOCK_REQUEST_START disk=%s op=%d sector=%llu bytes=%u offset=%llu\n",
                pane_disk->name, operation, (u64)blk_rq_pos(rq), blk_rq_bytes(rq),
                pane_disk->block_offset);
    }

    blk_mq_start_request(rq);
    pane_block_serial_log("PANE_BLOCK_REQUEST_ACTIVE");
    if (pane_block_initializing && operation == PANE_BLOCK_OPERATION_READ) {
        /*
         * add_disk can synchronously probe the new gendisk before module init
         * returns. On Pane's current single-vCPU bootstrap path, routing those
         * early probe reads through the host port device can prevent the init
         * parent from reaching PANE_BLOCK_MODULE_LOAD_OK. The initramfs drops
         * these probe-time cache entries before attempting the real root mount.
         */
        if (pane_block_init_read_log_count < 16) {
            pane_block_init_read_log_count++;
            pr_info(PANE_BLOCK_DRIVER_NAME
                    ": PANE_BLOCK_INIT_READ_ZERO_FILL disk=%s sector=%llu bytes=%u\n",
                    pane_disk->name, (u64)blk_rq_pos(rq), blk_rq_bytes(rq));
        }
        rq_for_each_segment(bvec, rq, iter) {
            unsigned char *mapped = kmap_local_page(bvec.bv_page);
            memset(mapped + bvec.bv_offset, 0, bvec.bv_len);
            kunmap_local(mapped);
        }
        blk_mq_end_request(rq, BLK_STS_OK);
        return BLK_STS_OK;
    }

    if (!bounce) {
        pane_block_serial_log("PANE_BLOCK_BOUNCE_UNAVAILABLE");
        blk_mq_end_request(rq, BLK_STS_RESOURCE);
        return BLK_STS_RESOURCE;
    }
    pane_block_serial_log("PANE_BLOCK_BOUNCE_READY");

    rq_for_each_segment(bvec, rq, iter) {
        u64 absolute_byte = (u64)iter.iter.bi_sector * PANE_BLOCK_SECTOR_SIZE;
        unsigned int remaining = bvec.bv_len;
        unsigned int segment_offset = 0;
        unsigned char *mapped = kmap_local_page(bvec.bv_page);
        unsigned char *segment = mapped + bvec.bv_offset;

        while (remaining > 0) {
            u64 block_index = absolute_byte / PANE_BLOCK_IO_BLOCK_SIZE_BYTES;
            unsigned int block_offset =
                (unsigned int)(absolute_byte % PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
            unsigned int chunk = PANE_BLOCK_IO_BLOCK_SIZE_BYTES - block_offset;

            if (chunk > remaining)
                chunk = remaining;

            if (operation == PANE_BLOCK_OPERATION_READ) {
                if (pane_block_transfer(pane_disk->pane_device_id,
                                        PANE_BLOCK_OPERATION_READ,
                                        block_index + pane_disk->block_offset,
                                        bounce) != 0) {
                    status = BLK_STS_IOERR;
                    break;
                }
                memcpy(segment + segment_offset, bounce + block_offset, chunk);
            } else if (block_offset == 0 && chunk == PANE_BLOCK_IO_BLOCK_SIZE_BYTES) {
                if (pane_block_transfer(pane_disk->pane_device_id,
                                        PANE_BLOCK_OPERATION_WRITE,
                                        block_index + pane_disk->block_offset,
                                        segment + segment_offset) != 0) {
                    status = BLK_STS_IOERR;
                    break;
                }
            } else {
                /* Preserve untouched bytes for partial filesystem writes. */
                if (pane_block_transfer(pane_disk->pane_device_id,
                                        PANE_BLOCK_OPERATION_READ,
                                        block_index + pane_disk->block_offset,
                                        bounce) != 0) {
                    status = BLK_STS_IOERR;
                    break;
                }
                memcpy(bounce + block_offset, segment + segment_offset, chunk);
                if (pane_block_transfer(pane_disk->pane_device_id,
                                        PANE_BLOCK_OPERATION_WRITE,
                                        block_index + pane_disk->block_offset,
                                        bounce) != 0) {
                    status = BLK_STS_IOERR;
                    break;
                }
            }

            absolute_byte += chunk;
            segment_offset += chunk;
            remaining -= chunk;
        }

        kunmap_local(mapped);
        if (status != BLK_STS_OK)
            break;
    }

    pane_block_serial_log(status == BLK_STS_OK ?
                          "PANE_BLOCK_REQUEST_END_OK" :
                          "PANE_BLOCK_REQUEST_END_ERROR");
    blk_mq_end_request(rq, status);
    return status;
}

static const struct blk_mq_ops pane_block_mq_ops = {
    .queue_rq = pane_block_queue_rq,
};

static int pane_block_create_disk(int index, int pane_device_id, const char *name)
{
    struct pane_block_disk *pane_disk = &pane_disks[index];
    sector_t capacity_sectors = device_blocks[index] *
                                (PANE_BLOCK_IO_BLOCK_SIZE_BYTES / PANE_BLOCK_SECTOR_SIZE);
    struct queue_limits limits;
    int ret;

    memset(&limits, 0, sizeof(limits));
    limits.logical_block_size = PANE_BLOCK_IO_BLOCK_SIZE_BYTES;
    limits.physical_block_size = PANE_BLOCK_IO_BLOCK_SIZE_BYTES;
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_CREATE_DISK_START name=%s device=%d blocks=%lu sectors=%llu base_offset=%lu\n",
            name, pane_device_id, device_blocks[index], (u64)capacity_sectors,
            index == 0 ? base_block_offset : 0);

    pane_disk->pane_device_id = pane_device_id;
    pane_disk->block_offset = index == 0 ? base_block_offset : 0;
    strscpy(pane_disk->name, name, sizeof(pane_disk->name));
    pane_disk->tag_set.ops = &pane_block_mq_ops;
    pane_disk->tag_set.nr_hw_queues = 1;
    pane_disk->tag_set.queue_depth = 1;
    pane_disk->tag_set.numa_node = NUMA_NO_NODE;
    pane_disk->tag_set.cmd_size = 0;
    pane_disk->tag_set.driver_data = pane_disk;

    ret = blk_mq_alloc_tag_set(&pane_disk->tag_set);
    if (ret) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_ALLOC_TAG_SET_FAILED name=%s ret=%d\n",
               name, ret);
        return ret;
    }
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_ALLOC_TAG_SET_OK name=%s\n", name);

    pane_disk->disk = blk_mq_alloc_disk(&pane_disk->tag_set, &limits, pane_disk);
    if (IS_ERR(pane_disk->disk)) {
        ret = PTR_ERR(pane_disk->disk);
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_ALLOC_DISK_FAILED name=%s ret=%d\n",
               name, ret);
        pane_disk->disk = NULL;
        blk_mq_free_tag_set(&pane_disk->tag_set);
        return ret;
    }
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_ALLOC_DISK_OK name=%s\n", name);

    pane_disk->disk->major = pane_block_major;
    pane_disk->disk->first_minor = index * PANE_BLOCK_MINORS_PER_DISK;
    pane_disk->disk->minors = PANE_BLOCK_MINORS_PER_DISK;
    pane_disk->disk->fops = &pane_block_fops;
    pane_disk->disk->private_data = pane_disk;
#ifdef GENHD_FL_NO_PART
    pane_disk->disk->flags |= GENHD_FL_NO_PART;
#endif
    snprintf(pane_disk->disk->disk_name, DISK_NAME_LEN, "%s", name);
    set_capacity(pane_disk->disk, capacity_sectors);
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_ADD_DISK_START name=%s\n", name);
    ret = add_disk(pane_disk->disk);
    if (ret) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_ADD_DISK_FAILED name=%s ret=%d\n",
               name, ret);
        put_disk(pane_disk->disk);
        pane_disk->disk = NULL;
        blk_mq_free_tag_set(&pane_disk->tag_set);
        return ret;
    }
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_ADD_DISK_OK name=%s\n", name);
    return 0;
}

static void pane_block_destroy_disk(int index)
{
    struct pane_block_disk *pane_disk = &pane_disks[index];

    if (pane_disk->disk) {
        del_gendisk(pane_disk->disk);
        put_disk(pane_disk->disk);
    }
    blk_mq_free_tag_set(&pane_disk->tag_set);
}

static int __init pane_block_init(void)
{
    int ret;

    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_INIT_START protocol=%s io_block=%u device_blocks=%lu,%lu base_block_offset=%lu shared_buffer_gpa=%lu shared_buffer_bytes=%lu\n",
            PANE_BLOCK_IO_PROTOCOL, PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            device_blocks[0], device_blocks[1], base_block_offset,
            shared_buffer_gpa, shared_buffer_bytes);

    if (shared_buffer_gpa != 0 &&
        shared_buffer_bytes >= PANE_BLOCK_IO_BLOCK_SIZE_BYTES) {
        pane_block_shared_buffer = memremap((phys_addr_t)shared_buffer_gpa,
                                            shared_buffer_bytes,
                                            MEMREMAP_WB);
        if (pane_block_shared_buffer) {
            pr_info(PANE_BLOCK_DRIVER_NAME
                    ": PANE_BLOCK_SHARED_BUFFER_OK gpa=%lu bytes=%lu\n",
                    shared_buffer_gpa, shared_buffer_bytes);
            pane_block_serial_log("PANE_BLOCK_SHARED_BUFFER_OK");
        } else {
            pr_warn(PANE_BLOCK_DRIVER_NAME
                    ": PANE_BLOCK_SHARED_BUFFER_UNAVAILABLE gpa=%lu bytes=%lu\n",
                    shared_buffer_gpa, shared_buffer_bytes);
            pane_block_serial_log("PANE_BLOCK_SHARED_BUFFER_UNAVAILABLE");
        }
    }

    pane_block_bounce_buffer = vmalloc(PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
    if (!pane_block_bounce_buffer) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_BOUNCE_ALLOC_FAILED bytes=%u\n",
               PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
        if (pane_block_shared_buffer)
            memunmap(pane_block_shared_buffer);
        pane_block_shared_buffer = NULL;
        return -ENOMEM;
    }
    pane_block_serial_log("PANE_BLOCK_BOUNCE_ALLOC_OK");

    pane_block_major = register_blkdev(0, PANE_BLOCK_DRIVER_NAME);
    if (pane_block_major <= 0) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_REGISTER_BLKDEV_FAILED ret=%d\n",
               pane_block_major);
        vfree(pane_block_bounce_buffer);
        pane_block_bounce_buffer = NULL;
        if (pane_block_shared_buffer)
            memunmap(pane_block_shared_buffer);
        pane_block_shared_buffer = NULL;
        return -EBUSY;
    }
    pr_info(PANE_BLOCK_DRIVER_NAME
            ": PANE_BLOCK_REGISTER_BLKDEV_OK major=%d\n",
            pane_block_major);

    ret = pane_block_create_disk(0, PANE_BLOCK_DEVICE_BASE_OS, "pane0");
    if (ret) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_INIT_BASE_DISK_FAILED ret=%d\n", ret);
        unregister_blkdev(pane_block_major, PANE_BLOCK_DRIVER_NAME);
        vfree(pane_block_bounce_buffer);
        pane_block_bounce_buffer = NULL;
        if (pane_block_shared_buffer)
            memunmap(pane_block_shared_buffer);
        pane_block_shared_buffer = NULL;
        return ret;
    }

    ret = pane_block_create_disk(1, PANE_BLOCK_DEVICE_USER_DISK, "pane1");
    if (ret) {
        pr_err(PANE_BLOCK_DRIVER_NAME
               ": PANE_BLOCK_INIT_USER_DISK_FAILED ret=%d\n", ret);
        pane_block_destroy_disk(0);
        unregister_blkdev(pane_block_major, PANE_BLOCK_DRIVER_NAME);
        vfree(pane_block_bounce_buffer);
        pane_block_bounce_buffer = NULL;
        if (pane_block_shared_buffer)
            memunmap(pane_block_shared_buffer);
        pane_block_shared_buffer = NULL;
        return ret;
    }

    pane_block_initializing = false;
    pr_info(PANE_BLOCK_DRIVER_NAME ": PANE_BLOCK_INIT_OK registered /dev/pane0 and /dev/pane1 for %s\n",
            PANE_BLOCK_IO_PROTOCOL);
    return 0;
}

static void __exit pane_block_exit(void)
{
    pane_block_destroy_disk(1);
    pane_block_destroy_disk(0);
    if (pane_block_major > 0)
        unregister_blkdev(pane_block_major, PANE_BLOCK_DRIVER_NAME);
    vfree(pane_block_bounce_buffer);
    pane_block_bounce_buffer = NULL;
    if (pane_block_shared_buffer)
        memunmap(pane_block_shared_buffer);
    pane_block_shared_buffer = NULL;
}

module_init(pane_block_init);
module_exit(pane_block_exit);

MODULE_AUTHOR("Pane");
MODULE_DESCRIPTION("Pane native port-backed block device contract for /dev/pane0 and /dev/pane1");
MODULE_LICENSE("GPL");
"#
    .to_string()
}

fn pane_block_driver_build_script() -> String {
    r#"#!/bin/sh
set -eu

# Build the Pane Linux block module against a target kernel build tree.
# Example:
#   KERNEL_BUILD_DIR=/lib/modules/$(uname -r)/build sh build-pane-block-module.sh ./pane-block.ko

: "${KERNEL_BUILD_DIR:?set KERNEL_BUILD_DIR to the Linux kernel build directory}"

output="${1:-pane-block.ko}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

cp pane-block.c "$workdir/pane-block.c"
cp pane-port-block.h "$workdir/pane-port-block.h"
cat > "$workdir/Makefile" <<'EOF'
obj-m += pane-block.o
EOF

make -C "$KERNEL_BUILD_DIR" M="$workdir" modules
cp "$workdir/pane-block.ko" "$output"
printf 'wrote %s\n' "$output"
"#
    .to_string()
}

fn pane_initramfs_build_script() -> String {
    r#"#!/bin/sh
set -eu

# Build a minimal Pane native-storage discovery initramfs.
# Run from this directory on Linux with gcc and cpio available:
#   sh build-pane-initramfs.sh ./pane-storage-discovery.cpio

output="${1:-pane-storage-discovery.cpio}"
case "$output" in
    /*) output_path="$output" ;;
    *) output_path="$PWD/$output" ;;
esac
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

mkdir -p "$workdir/bin" "$workdir/dev" "$workdir/lib/modules" "$workdir/newroot" "$workdir/proc" "$workdir/run/pane" "$workdir/sys"
cc -Os -static -o "$workdir/init" pane-init.c
cc -Os -static -o "$workdir/bin/pane-port-probe" pane-port-probe.c
cp pane-initramfs-hook.sh "$workdir/bin/pane-initramfs-hook"
if [ -f pane-block.ko ]; then
    cp pane-block.ko "$workdir/lib/modules/pane-block.ko"
fi
chmod +x "$workdir/init" "$workdir/bin/pane-initramfs-hook" "$workdir/bin/pane-port-probe"

if command -v cpio >/dev/null 2>&1; then
    (cd "$workdir" && find . -print | cpio -o -H newc > "$output_path")
elif command -v bsdtar >/dev/null 2>&1; then
    (cd "$workdir" && bsdtar --format newc -cf "$output_path" .)
else
    echo "build-pane-initramfs: need cpio or bsdtar to write a newc initramfs" >&2
    exit 127
fi
printf 'wrote %s\n' "$output_path"
"#
    .to_string()
}

fn pane_initramfs_driver_readme() -> String {
    r#"# Pane Initramfs Driver Source

This bundle is generated by `pane runtime --write-initramfs-driver`.

It defines the guest-side discovery hook, C probe source, self-contained C
`/init` source, and Linux block-driver source contract for Pane's native
block-port ABI plus its shared-memory block transfer window. The generated `/init` discovers the Pane storage contract,
writes `/run/pane/native-storage.env`, waits for the declared root device,
attempts to mount it at `/newroot`, moves proc/sys/dev into the new root, and
executes the real Linux init once `/dev/pane0` exists. If `pane-block.ko` is
present in the initramfs, `/init` attempts to load it before waiting for the
root device. The generated `pane-block.c` is the early guest device contract
that maps Pane's read-only base OS to `/dev/pane0` and the writable user disk to
`/dev/pane1`.

Expected kernel arguments:

- `pane.storage_contract=<guest-physical-address>`
- `pane.block_io=<base-port>,<port-count>,<block-size-bytes>`
- `pane.block_dma=<guest-physical-address>,<bytes>`
- `pane.root=<root-device>`
- `pane.user=<user-device>`

To build and register the discovery initramfs through Pane:

```sh
pane runtime --write-initramfs-driver --build-discovery-initramfs
```

Pane compiles the generated guest `/init` and probe with a Linux-capable static
`cc`, verifies both outputs are ELF binaries, and packages the `newc` initramfs
archive itself. The archive writer is built into Pane, so the app path does not
depend on host `cpio` or `bsdtar`.

Compiler selection order for generated guest binaries:

- `PANE_LINUX_CC` plus optional whitespace-separated `PANE_LINUX_CC_ARGS`
- `cc`
- `zig cc -target x86_64-linux-musl`

If a reproducible artifact builder already produced the two guest binaries, Pane
can package them without invoking a compiler:

```sh
pane runtime --build-discovery-initramfs --discovery-init-binary ./init --discovery-probe-binary ./pane-port-probe
```

Both inputs must be Linux ELF binaries. Pane rejects Windows-host binaries before
registering the initramfs.

For external/manual builders, the generated shell script can still produce an
equivalent cpio archive:

```sh
sh build-pane-initramfs.sh ./pane-storage-discovery.cpio
```

To build the optional Pane block module against a target kernel tree:

```sh
KERNEL_BUILD_DIR=/lib/modules/$(uname -r)/build sh build-pane-block-module.sh ./pane-block.ko
sh build-pane-initramfs.sh ./pane-storage-discovery.cpio
```

The generated cpio is a discovery/probe initramfs, not the final Arch
root-mount initramfs until the Pane block module is compiled for the target Arch
kernel and loaded early enough to create the declared root device.

Expected serial-observable milestones:

- `PANE_INITRAMFS_DISCOVERY_START`
- `PANE_BLOCK_IO_PROBE_OK`
- `PANE_BLOCK_MODULE_NOT_PRESENT` or `PANE_BLOCK_MODULE_LOAD_OK`
- `PANE_INITRAMFS_DISCOVERY_DONE`
- `PANE_DISPLAY_CONTRACT_DISCOVERED`
- `PANE_ROOT_MOUNT_ATTEMPT`
- `PANE_ROOT_MOUNT_OK`
- `PANE_INIT_EXEC`
"#
    .to_string()
}

fn pane_initramfs_expected_serial_milestones() -> Vec<String> {
    vec![
        "PANE_INITRAMFS_DISCOVERY_START".to_string(),
        "PANE_BLOCK_IO_PROBE_OK".to_string(),
        "PANE_BLOCK_MODULE_LOAD_OK".to_string(),
        "PANE_INITRAMFS_DISCOVERY_DONE".to_string(),
        "PANE_DISPLAY_CONTRACT_DISCOVERED".to_string(),
        "PANE_ROOT_MOUNT_ATTEMPT".to_string(),
        "PANE_ROOT_MOUNT_OK".to_string(),
        "PANE_INIT_EXEC".to_string(),
    ]
}

struct NewcCpioEntry {
    name: String,
    mode: u32,
    data: Vec<u8>,
}

impl NewcCpioEntry {
    fn directory(name: &str) -> Self {
        Self {
            name: name.trim_matches('/').to_string(),
            mode: 0o040755,
            data: Vec::new(),
        }
    }

    fn file(name: &str, mode: u32, data: Vec<u8>) -> Self {
        Self {
            name: name.trim_matches('/').to_string(),
            mode: 0o100000 | mode,
            data,
        }
    }
}

fn write_newc_padding(file: &mut fs::File, written_len: usize) -> AppResult<()> {
    let padding = (4 - (written_len % 4)) % 4;
    if padding > 0 {
        file.write_all(&vec![0_u8; padding])?;
    }
    Ok(())
}

fn write_newc_cpio_entry(file: &mut fs::File, inode: u32, entry: &NewcCpioEntry) -> AppResult<()> {
    if entry.name.is_empty() {
        return Err(AppError::message(
            "newc cpio entries require a non-empty archive path.",
        ));
    }
    let data_len = u32::try_from(entry.data.len()).map_err(|_| {
        AppError::message(format!(
            "newc cpio entry {} is too large for the initramfs format.",
            entry.name
        ))
    })?;
    let name_size = u32::try_from(entry.name.len() + 1).map_err(|_| {
        AppError::message(format!(
            "newc cpio entry path is too long for {}.",
            entry.name
        ))
    })?;
    let header = format!(
        "070701{inode:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
        inode = inode,
        mode = entry.mode,
        uid = 0_u32,
        gid = 0_u32,
        nlink = 1_u32,
        mtime = 0_u32,
        filesize = data_len,
        devmajor = 0_u32,
        devminor = 0_u32,
        rdevmajor = 0_u32,
        rdevminor = 0_u32,
        namesize = name_size,
        check = 0_u32
    );
    file.write_all(header.as_bytes())?;
    file.write_all(entry.name.as_bytes())?;
    file.write_all(&[0])?;
    write_newc_padding(file, header.len() + entry.name.len() + 1)?;
    file.write_all(&entry.data)?;
    write_newc_padding(file, entry.data.len())?;
    Ok(())
}

fn write_newc_cpio_archive(output: &Path, entries: &[NewcCpioEntry]) -> AppResult<()> {
    let mut file = fs::File::create(output)?;
    for (index, entry) in entries.iter().enumerate() {
        write_newc_cpio_entry(
            &mut file,
            u32::try_from(index + 1).unwrap_or(u32::MAX),
            entry,
        )?;
    }
    write_newc_cpio_entry(
        &mut file,
        u32::try_from(entries.len() + 1).unwrap_or(u32::MAX),
        &NewcCpioEntry {
            name: "TRAILER!!!".to_string(),
            mode: 0,
            data: Vec::new(),
        },
    )?;
    Ok(())
}

struct InitramfsCompilerCandidate {
    label: String,
    program: String,
    base_args: Vec<String>,
}

struct DiscoveryInitramfsBuildOutput {
    init_binary_sha256: String,
    probe_binary_sha256: String,
    compiled_from_current_source: bool,
}

fn initramfs_compiler_candidates() -> Vec<InitramfsCompilerCandidate> {
    let mut candidates = Vec::new();
    if let Some(program) = env::var("PANE_LINUX_CC")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        let base_args = env::var("PANE_LINUX_CC_ARGS")
            .ok()
            .map(|args| {
                args.split_whitespace()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        candidates.push(InitramfsCompilerCandidate {
            label: "PANE_LINUX_CC".to_string(),
            program,
            base_args,
        });
    }
    candidates.push(InitramfsCompilerCandidate {
        label: "cc".to_string(),
        program: "cc".to_string(),
        base_args: Vec::new(),
    });
    candidates.push(InitramfsCompilerCandidate {
        label: "zig cc -target x86_64-linux-musl".to_string(),
        program: "zig".to_string(),
        base_args: vec![
            "cc".to_string(),
            "-target".to_string(),
            "x86_64-linux-musl".to_string(),
        ],
    });
    candidates
}

fn run_initramfs_compiler_candidate(
    candidate: &InitramfsCompilerCandidate,
    source: &Path,
    output: &Path,
    label: &str,
) -> Result<(), String> {
    let _ = fs::remove_file(output);
    let mut command = Command::new(&candidate.program);
    command.args(&candidate.base_args);
    command
        .arg("-Os")
        .arg("-static")
        .arg("-o")
        .arg(output)
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let status = command
        .output()
        .map_err(|error| format!("{} could not start: {error}", candidate.label))?;

    if !status.status.success() {
        return Err(format!(
            "{} exited with status {}.\nstdout:\n{}\nstderr:\n{}",
            candidate.label,
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        ));
    }

    verify_elf_binary(output, label)
        .map_err(|error| format!("{} produced unusable output: {error}", candidate.label))
}

fn windows_path_to_wsl_path(path: &Path) -> Option<String> {
    let path = path.canonicalize().ok()?;
    windows_absolute_path_to_wsl_path(&path)
}

fn windows_output_path_to_wsl_path(path: &Path) -> Option<String> {
    let parent = path.parent()?.canonicalize().ok()?;
    let file_name = path.file_name()?.to_string_lossy().replace('\\', "/");
    let parent_wsl = windows_absolute_path_to_wsl_path(&parent)?;
    Some(format!("{parent_wsl}/{file_name}"))
}

fn windows_absolute_path_to_wsl_path(path: &Path) -> Option<String> {
    let mut text = path.to_string_lossy().to_string();
    if let Some(stripped) = text.strip_prefix(r"\\?\") {
        text = stripped.to_string();
    }
    let mut chars = text.chars();
    let drive = chars.next()?.to_ascii_lowercase();
    if chars.next()? != ':' || chars.next()? != '\\' {
        return None;
    }
    Some(format!(
        "/mnt/{}/{}",
        drive,
        chars.as_str().replace('\\', "/")
    ))
}

fn run_initramfs_wsl_cc(source: &Path, output: &Path, label: &str) -> Result<(), String> {
    let source_wsl = windows_path_to_wsl_path(source)
        .ok_or_else(|| format!("could not convert {} to a WSL path", source.display()))?;
    let output_parent = output
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", output.display()))?;
    fs::create_dir_all(output_parent).map_err(|error| {
        format!(
            "could not create output directory {}: {error}",
            output_parent.display()
        )
    })?;
    let output_wsl = windows_output_path_to_wsl_path(output)
        .ok_or_else(|| format!("could not convert {} to a WSL path", output.display()))?;
    let distro = env::var("PANE_LINUX_WSL_DISTRO")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let mut command = Command::new("wsl.exe");
    if let Some(distro) = distro.as_deref() {
        command.arg("-d").arg(distro);
    }
    command
        .arg("--exec")
        .arg("sh")
        .arg("-lc")
        .arg("cc -Os -static -o \"$1\" \"$2\"")
        .arg("pane-initramfs-cc")
        .arg(&output_wsl)
        .arg(&source_wsl)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let status = command
        .output()
        .map_err(|error| format!("wsl.exe cc could not start: {error}"))?;
    let distro_label = distro.as_deref().unwrap_or("default");
    if !status.status.success() {
        return Err(format!(
            "wsl.exe ({distro_label}) cc exited with status {}.\nstdout:\n{}\nstderr:\n{}",
            status.status,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    verify_elf_binary(output, label)
        .map_err(|error| format!("wsl.exe ({distro_label}) cc produced unusable output: {error}"))
}

fn run_initramfs_cc(source: &Path, output: &Path, label: &str) -> AppResult<()> {
    let mut failures = Vec::new();
    for candidate in initramfs_compiler_candidates() {
        match run_initramfs_compiler_candidate(&candidate, source, output, label) {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(error),
        }
    }
    if cfg!(windows) {
        match run_initramfs_wsl_cc(source, output, label) {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(error),
        }
    }
    Err(AppError::message(format!(
        "Failed to compile {label} as a static Linux ELF binary. Pane now packages the initramfs archive itself, but the guest `/init` and probe still require a Linux-capable C compiler. Install `cc`, install Zig, set `PANE_LINUX_CC`/`PANE_LINUX_CC_ARGS`, set `PANE_LINUX_WSL_DISTRO` to a WSL distro with `cc`, or pass prebuilt ELF binaries with `--discovery-init-binary` and `--discovery-probe-binary`.\nCompiler attempts:\n- {}",
        failures.join("\n- ")
    )))
}

fn verify_elf_binary(path: &Path, label: &str) -> AppResult<()> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic).map_err(|error| {
        AppError::message(format!(
            "Failed to read compiled {label} at {}: {error}",
            path.display()
        ))
    })?;
    if magic != *b"\x7fELF" {
        return Err(AppError::message(format!(
            "Compiled {label} at {} is not an ELF binary. Pane initramfs artifacts must be built for Linux, not the Windows host ABI.",
            path.display()
        )));
    }
    Ok(())
}

fn build_pane_discovery_initramfs_with_native_packager(
    paths: &RuntimePaths,
    metadata: &PaneInitramfsDriverMetadata,
    output: &Path,
    prebuilt_init_binary: Option<&Path>,
    prebuilt_probe_binary: Option<&Path>,
) -> AppResult<DiscoveryInitramfsBuildOutput> {
    if prebuilt_init_binary.is_some() != prebuilt_probe_binary.is_some() {
        return Err(AppError::message(
            "--discovery-init-binary and --discovery-probe-binary must be provided together.",
        ));
    }

    let staging = paths
        .initramfs_driver_dir
        .join("pane-storage-discovery-root");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join("bin"))?;
    fs::create_dir_all(staging.join("dev"))?;
    fs::create_dir_all(staging.join("lib/modules"))?;
    fs::create_dir_all(staging.join("newroot"))?;
    fs::create_dir_all(staging.join("proc"))?;
    fs::create_dir_all(staging.join("run/pane"))?;
    fs::create_dir_all(staging.join("sys"))?;

    let init_binary = staging.join("init");
    let probe_binary = staging.join("bin/pane-port-probe");
    if let (Some(prebuilt_init), Some(prebuilt_probe)) =
        (prebuilt_init_binary, prebuilt_probe_binary)
    {
        verify_elf_binary(prebuilt_init, "prebuilt Pane discovery /init")?;
        verify_elf_binary(prebuilt_probe, "prebuilt Pane port probe")?;
        fs::copy(prebuilt_init, &init_binary)?;
        fs::copy(prebuilt_probe, &probe_binary)?;
    } else {
        run_initramfs_cc(
            Path::new(&metadata.init_source_path),
            &init_binary,
            "Pane discovery /init",
        )?;
        run_initramfs_cc(
            Path::new(&metadata.probe_source_path),
            &probe_binary,
            "Pane port probe",
        )?;
    }
    let compiled_from_current_source = prebuilt_init_binary.is_none();
    let init_binary_sha256 = sha256_file(&init_binary)?;
    let probe_binary_sha256 = sha256_file(&probe_binary)?;

    let hook = fs::read(Path::new(&metadata.hook_path))?;
    let init = fs::read(&init_binary)?;
    let probe = fs::read(&probe_binary)?;
    let mut entries = vec![
        NewcCpioEntry::directory("bin"),
        NewcCpioEntry::directory("dev"),
        NewcCpioEntry::directory("lib"),
        NewcCpioEntry::directory("lib/modules"),
        NewcCpioEntry::directory("newroot"),
        NewcCpioEntry::directory("proc"),
        NewcCpioEntry::directory("run"),
        NewcCpioEntry::directory("run/pane"),
        NewcCpioEntry::directory("sys"),
        NewcCpioEntry::file("init", 0o755, init),
        NewcCpioEntry::file("bin/pane-port-probe", 0o755, probe),
        NewcCpioEntry::file("bin/pane-initramfs-hook", 0o755, hook),
    ];

    let module = pane_block_module_path(paths);
    if module.is_file() {
        entries.push(NewcCpioEntry::file(
            "lib/modules/pane-block.ko",
            0o644,
            fs::read(module)?,
        ));
    }

    let virtio_mmio_module = virtio_mmio_module_path(paths);
    if virtio_mmio_module.is_file() {
        entries.push(NewcCpioEntry::file(
            "lib/modules/virtio_mmio.ko",
            0o644,
            fs::read(virtio_mmio_module)?,
        ));
    }

    write_newc_cpio_archive(output, &entries)?;
    Ok(DiscoveryInitramfsBuildOutput {
        init_binary_sha256,
        probe_binary_sha256,
        compiled_from_current_source,
    })
}

fn build_and_register_pane_discovery_initramfs(
    paths: &RuntimePaths,
    prebuilt_init_binary: Option<&Path>,
    prebuilt_probe_binary: Option<&Path>,
    force: bool,
) -> AppResult<()> {
    let metadata = load_verified_pane_initramfs_driver_metadata(paths)?;
    if !paths.kernel_boot_metadata.is_file() {
        return Err(AppError::message(
            "`pane runtime --build-discovery-initramfs` requires an existing verified kernel boot plan. Register a kernel first with `pane runtime --register-kernel ... --kernel-expected-sha256 ...`.",
        ));
    }

    let output = paths
        .initramfs_driver_dir
        .join("pane-storage-discovery.cpio");
    let build_script = Path::new(&metadata.build_script_path);
    if !build_script.is_file() {
        return Err(AppError::message(format!(
            "Pane discovery initramfs build script is missing: {}. Regenerate it with `pane runtime --write-initramfs-driver`.",
            build_script.display()
        )));
    }
    if pane_block_module_path(paths).exists() {
        load_verified_pane_block_module_metadata(paths)?;
    }
    let build_output = build_pane_discovery_initramfs_with_native_packager(
        paths,
        &metadata,
        &output,
        prebuilt_init_binary,
        prebuilt_probe_binary,
    )?;

    register_pane_discovery_initramfs_artifact(paths, &output, force)?;
    record_pane_discovery_initramfs_package_metadata(paths, &build_output)
}

fn register_pane_discovery_initramfs_artifact(
    paths: &RuntimePaths,
    source: &Path,
    force: bool,
) -> AppResult<()> {
    let sha256 = sha256_file(source)?;
    register_kernel_boot_plan(paths, None, None, Some(source), Some(&sha256), None, force)
}

fn record_pane_discovery_initramfs_package_metadata(
    paths: &RuntimePaths,
    build_output: &DiscoveryInitramfsBuildOutput,
) -> AppResult<()> {
    let mut metadata = load_verified_pane_initramfs_driver_metadata(paths)?;
    let module_metadata = if pane_block_module_path(paths).is_file() {
        Some(load_verified_pane_block_module_metadata(paths)?)
    } else {
        None
    };
    metadata.packaged_initramfs_path = Some(paths.initramfs_image.display().to_string());
    metadata.packaged_initramfs_bytes = Some(fs::metadata(&paths.initramfs_image)?.len());
    metadata.packaged_initramfs_sha256 = Some(sha256_file(&paths.initramfs_image)?);
    metadata.packaged_hook_sha256 = Some(metadata.hook_sha256.clone());
    if build_output.compiled_from_current_source {
        metadata.packaged_init_source_sha256 = Some(metadata.init_source_sha256.clone());
        metadata.packaged_probe_source_sha256 = Some(metadata.probe_source_sha256.clone());
        metadata.packaged_binary_provenance = Some("compiled-from-current-source".to_string());
    } else {
        metadata.packaged_init_source_sha256 = None;
        metadata.packaged_probe_source_sha256 = None;
        metadata.packaged_binary_provenance = Some("external-prebuilt-elf".to_string());
    }
    metadata.packaged_init_binary_sha256 = Some(build_output.init_binary_sha256.clone());
    metadata.packaged_probe_binary_sha256 = Some(build_output.probe_binary_sha256.clone());
    metadata.packaged_block_driver_source_sha256 =
        Some(metadata.block_driver_source_sha256.clone());
    metadata.packaged_block_driver_abi_sha256 = Some(metadata.block_driver_abi_sha256.clone());
    metadata.packaged_block_module_sha256 = module_metadata.map(|metadata| metadata.sha256);
    write_json_file(&paths.initramfs_driver_metadata, &metadata)
}

fn create_user_disk_snapshot(paths: &RuntimePaths) -> AppResult<UserDiskSnapshotMetadata> {
    let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane sparse user disk is not ready to snapshot. Run `pane runtime --create-user-disk` first.",
        ));
    }

    fs::create_dir_all(&paths.snapshots)?;
    let source_disk_bytes = fs::metadata(&paths.user_disk)?.len();
    let source_disk_sha256 = sha256_file(&paths.user_disk)?;
    let snapshot_id = next_user_disk_snapshot_id(paths)?;
    let snapshot_path = paths.snapshots.join(format!("{snapshot_id}.panedisk"));
    let snapshot_metadata_path = paths.snapshots.join(format!("{snapshot_id}.json"));

    let copied_bytes = fs::copy(&paths.user_disk, &snapshot_path)?;
    if copied_bytes != source_disk_bytes {
        return Err(AppError::message(
            "Pane user disk snapshot copied an unexpected number of bytes.",
        ));
    }
    let copied_sha256 = sha256_file(&snapshot_path)?;
    if copied_sha256 != source_disk_sha256 {
        return Err(AppError::message(
            "Pane user disk snapshot verification failed after copy.",
        ));
    }

    let snapshot = UserDiskSnapshotMetadata {
        schema_version: 1,
        snapshot_kind: "pane-user-disk-snapshot-v1".to_string(),
        snapshot_id,
        source_disk_path: paths.user_disk.display().to_string(),
        source_metadata_path: paths.user_disk_metadata.display().to_string(),
        snapshot_path: snapshot_path.display().to_string(),
        source_disk_bytes,
        source_disk_sha256,
        user_disk_capacity_gib: metadata.capacity_gib,
        user_disk_logical_size_bytes: metadata.logical_size_bytes,
        user_disk_block_size_bytes: metadata.block_size_bytes,
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This snapshot captures the current Pane sparse user disk artifact for recovery experiments."
                .to_string(),
            "Restore, export, import, and resize semantics are separate milestones and must verify this metadata before mutation."
                .to_string(),
        ],
    };
    write_json_file(&snapshot_metadata_path, &snapshot)?;
    Ok(snapshot)
}

fn next_user_disk_snapshot_id(paths: &RuntimePaths) -> AppResult<String> {
    let base = format!("user-disk-{}", current_epoch_seconds());
    for suffix in 0..1000 {
        let candidate = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !paths
            .snapshots
            .join(format!("{candidate}.panedisk"))
            .exists()
            && !paths.snapshots.join(format!("{candidate}.json")).exists()
        {
            return Ok(candidate);
        }
    }
    Err(AppError::message(
        "Pane could not allocate a unique user disk snapshot name.",
    ))
}

fn user_disk_snapshot_metadata_files(paths: &RuntimePaths) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(&paths.snapshots) else {
        return Vec::new();
    };
    let mut metadata_files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .filter(|path| {
            read_json_file::<UserDiskSnapshotMetadata>(path)
                .map(|metadata| metadata.snapshot_kind == "pane-user-disk-snapshot-v1")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    metadata_files.sort();
    metadata_files
}

fn restore_user_disk_snapshot(
    paths: &RuntimePaths,
    snapshot_metadata_path: &Path,
) -> AppResult<UserDiskSnapshotMetadata> {
    let snapshot = read_json_file::<UserDiskSnapshotMetadata>(snapshot_metadata_path)?;
    if snapshot.schema_version != 1 || snapshot.snapshot_kind != "pane-user-disk-snapshot-v1" {
        return Err(AppError::message(
            "Pane user disk snapshot metadata is not a supported snapshot contract.",
        ));
    }

    let mut snapshot_path = PathBuf::from(&snapshot.snapshot_path);
    if !snapshot_path.is_file() {
        let sibling = snapshot_metadata_path.with_extension("panedisk");
        if sibling.is_file() {
            snapshot_path = sibling;
        }
    }
    if !snapshot_path.is_file() {
        return Err(AppError::message(format!(
            "Pane user disk snapshot artifact is missing for metadata {}.",
            snapshot_metadata_path.display()
        )));
    }

    let snapshot_bytes = fs::metadata(&snapshot_path)?.len();
    if snapshot_bytes != snapshot.source_disk_bytes {
        return Err(AppError::message(
            "Pane user disk snapshot byte length does not match its metadata.",
        ));
    }
    let snapshot_sha256 = sha256_file(&snapshot_path)?;
    if snapshot_sha256 != snapshot.source_disk_sha256 {
        return Err(AppError::message(
            "Pane user disk snapshot SHA-256 does not match its metadata.",
        ));
    }

    let logical_size_bytes = user_disk_logical_size_bytes(snapshot.user_disk_capacity_gib)?;
    if logical_size_bytes != snapshot.user_disk_logical_size_bytes
        || snapshot.user_disk_block_size_bytes != PANE_USER_DISK_BLOCK_SIZE_BYTES
    {
        return Err(AppError::message(
            "Pane user disk snapshot geometry is not compatible with this Pane version.",
        ));
    }

    let expected_header = validate_user_disk_artifact_header(
        &snapshot_path,
        snapshot.user_disk_capacity_gib,
        snapshot.user_disk_logical_size_bytes,
        snapshot.user_disk_block_size_bytes,
    )?;

    if let Some(parent) = paths.user_disk.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.user_disk_metadata.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_restore = paths.user_disk.with_extension("panedisk.restore.tmp");
    if temp_restore.exists() {
        fs::remove_file(&temp_restore)?;
    }
    let copied_bytes = fs::copy(&snapshot_path, &temp_restore)?;
    if copied_bytes != snapshot.source_disk_bytes || sha256_file(&temp_restore)? != snapshot_sha256
    {
        let _ = fs::remove_file(&temp_restore);
        return Err(AppError::message(
            "Pane user disk restore verification failed after staging copy.",
        ));
    }

    if paths.user_disk.exists() {
        fs::remove_file(&paths.user_disk)?;
    }
    fs::rename(&temp_restore, &paths.user_disk)?;

    let metadata = UserDiskMetadata {
        schema_version: 1,
        format: PANE_USER_DISK_FORMAT.to_string(),
        disk_path: paths.user_disk.display().to_string(),
        capacity_gib: snapshot.user_disk_capacity_gib,
        logical_size_bytes,
        block_size_bytes: PANE_USER_DISK_BLOCK_SIZE_BYTES,
        sparse_backing: true,
        allocated_header_bytes: expected_header.len() as u64,
        header_sha256: sha256_bytes(&expected_header),
        materialized_block_device: true,
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            format!(
                "Restored from Pane user disk snapshot `{}`.",
                snapshot.snapshot_id
            ),
            "The restored artifact was verified by byte count, SHA-256, and Pane disk header before replacing the active user disk."
                .to_string(),
        ],
    };
    write_json_file(&paths.user_disk_metadata, &metadata)?;
    Ok(snapshot)
}

fn export_user_disk_package(
    paths: &RuntimePaths,
    export_dir: &Path,
    force: bool,
) -> AppResult<UserDiskExportManifest> {
    let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane sparse user disk is not ready to export. Run `pane runtime --create-user-disk` first.",
        ));
    }
    if export_dir.is_file() {
        return Err(AppError::message(
            "Pane user disk export target must be a directory.",
        ));
    }

    fs::create_dir_all(export_dir)?;
    let manifest_path = export_dir.join(PANE_USER_DISK_EXPORT_MANIFEST_FILENAME);
    let disk_path = export_dir.join(PANE_USER_DISK_EXPORT_DISK_FILENAME);
    let metadata_path = export_dir.join(PANE_USER_DISK_EXPORT_METADATA_FILENAME);
    if !force && (manifest_path.exists() || disk_path.exists() || metadata_path.exists()) {
        return Err(AppError::message(format!(
            "Pane user disk export package already exists at {}. Pass --force to replace the package files.",
            export_dir.display()
        )));
    }

    let source_disk_bytes = fs::metadata(&paths.user_disk)?.len();
    let source_disk_sha256 = sha256_file(&paths.user_disk)?;
    let copied_bytes = fs::copy(&paths.user_disk, &disk_path)?;
    if copied_bytes != source_disk_bytes || sha256_file(&disk_path)? != source_disk_sha256 {
        return Err(AppError::message(
            "Pane user disk export verification failed after copy.",
        ));
    }
    fs::copy(&paths.user_disk_metadata, &metadata_path)?;

    let manifest = UserDiskExportManifest {
        schema_version: 1,
        export_kind: "pane-user-disk-export-v1".to_string(),
        export_id: format!("pane-user-disk-export-{}", current_epoch_seconds()),
        exported_disk_filename: PANE_USER_DISK_EXPORT_DISK_FILENAME.to_string(),
        exported_metadata_filename: PANE_USER_DISK_EXPORT_METADATA_FILENAME.to_string(),
        source_disk_path: paths.user_disk.display().to_string(),
        source_metadata_path: paths.user_disk_metadata.display().to_string(),
        source_disk_bytes,
        source_disk_sha256,
        user_disk_capacity_gib: metadata.capacity_gib,
        user_disk_logical_size_bytes: metadata.logical_size_bytes,
        user_disk_block_size_bytes: metadata.block_size_bytes,
        exported_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "This package is a portable Pane user disk export for backup or migration."
                .to_string(),
            "Import verifies the manifest, byte count, SHA-256, and Pane disk header before replacing the active user disk."
                .to_string(),
        ],
    };
    write_json_file(&manifest_path, &manifest)?;
    Ok(manifest)
}

fn import_user_disk_package(
    paths: &RuntimePaths,
    package_or_manifest: &Path,
) -> AppResult<UserDiskExportManifest> {
    let manifest_path = if package_or_manifest.is_dir() {
        package_or_manifest.join(PANE_USER_DISK_EXPORT_MANIFEST_FILENAME)
    } else {
        package_or_manifest.to_path_buf()
    };
    let manifest = read_json_file::<UserDiskExportManifest>(&manifest_path)?;
    if manifest.schema_version != 1 || manifest.export_kind != "pane-user-disk-export-v1" {
        return Err(AppError::message(
            "Pane user disk export manifest is not a supported export contract.",
        ));
    }
    let package_dir = manifest_path.parent().ok_or_else(|| {
        AppError::message("Pane user disk export manifest must live inside a package directory.")
    })?;
    let disk_path = package_dir.join(&manifest.exported_disk_filename);
    let exported_metadata_path = package_dir.join(&manifest.exported_metadata_filename);
    if !disk_path.is_file() || !exported_metadata_path.is_file() {
        return Err(AppError::message(
            "Pane user disk export package is missing its disk or metadata file.",
        ));
    }

    let disk_bytes = fs::metadata(&disk_path)?.len();
    if disk_bytes != manifest.source_disk_bytes {
        return Err(AppError::message(
            "Pane user disk export byte length does not match its manifest.",
        ));
    }
    let disk_sha256 = sha256_file(&disk_path)?;
    if disk_sha256 != manifest.source_disk_sha256 {
        return Err(AppError::message(
            "Pane user disk export SHA-256 does not match its manifest.",
        ));
    }
    let expected_header = validate_user_disk_artifact_header(
        &disk_path,
        manifest.user_disk_capacity_gib,
        manifest.user_disk_logical_size_bytes,
        manifest.user_disk_block_size_bytes,
    )?;
    let exported_metadata = read_json_file::<UserDiskMetadata>(&exported_metadata_path)?;
    if exported_metadata.format != PANE_USER_DISK_FORMAT
        || exported_metadata.capacity_gib != manifest.user_disk_capacity_gib
        || exported_metadata.logical_size_bytes != manifest.user_disk_logical_size_bytes
        || exported_metadata.block_size_bytes != PANE_USER_DISK_BLOCK_SIZE_BYTES
        || exported_metadata.header_sha256 != sha256_bytes(&expected_header)
    {
        return Err(AppError::message(
            "Pane user disk export metadata does not match its manifest and disk header.",
        ));
    }

    if let Some(parent) = paths.user_disk.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.user_disk_metadata.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_import = paths.user_disk.with_extension("panedisk.import.tmp");
    if temp_import.exists() {
        fs::remove_file(&temp_import)?;
    }
    let copied_bytes = fs::copy(&disk_path, &temp_import)?;
    if copied_bytes != manifest.source_disk_bytes || sha256_file(&temp_import)? != disk_sha256 {
        let _ = fs::remove_file(&temp_import);
        return Err(AppError::message(
            "Pane user disk import verification failed after staging copy.",
        ));
    }

    if paths.user_disk.exists() {
        fs::remove_file(&paths.user_disk)?;
    }
    fs::rename(&temp_import, &paths.user_disk)?;

    let metadata = UserDiskMetadata {
        schema_version: 1,
        format: PANE_USER_DISK_FORMAT.to_string(),
        disk_path: paths.user_disk.display().to_string(),
        capacity_gib: manifest.user_disk_capacity_gib,
        logical_size_bytes: manifest.user_disk_logical_size_bytes,
        block_size_bytes: PANE_USER_DISK_BLOCK_SIZE_BYTES,
        sparse_backing: true,
        allocated_header_bytes: expected_header.len() as u64,
        header_sha256: sha256_bytes(&expected_header),
        materialized_block_device: true,
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            format!("Imported from Pane user disk export `{}`.", manifest.export_id),
            "The imported artifact was verified by manifest, byte count, SHA-256, metadata, and Pane disk header before replacing the active user disk."
                .to_string(),
        ],
    };
    write_json_file(&paths.user_disk_metadata, &metadata)?;
    Ok(manifest)
}

fn resize_user_disk(paths: &RuntimePaths, new_capacity_gib: u64) -> AppResult<UserDiskMetadata> {
    let mut metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata)?;
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane sparse user disk is not ready to resize. Run `pane runtime --create-user-disk` first.",
        ));
    }
    if new_capacity_gib < metadata.capacity_gib {
        return Err(AppError::message(format!(
            "Pane user disk resize is grow-only. Current capacity is {} GiB; requested {} GiB.",
            metadata.capacity_gib, new_capacity_gib
        )));
    }
    if new_capacity_gib == metadata.capacity_gib {
        return Ok(metadata);
    }

    let new_logical_size_bytes = user_disk_logical_size_bytes(new_capacity_gib)?;
    if new_logical_size_bytes <= metadata.logical_size_bytes {
        return Err(AppError::message(
            "Pane user disk resize must increase logical size.",
        ));
    }
    let new_header = user_disk_header_bytes(new_logical_size_bytes);
    if new_header.len() as u64 != metadata.allocated_header_bytes {
        return Err(AppError::message(
            "Pane user disk resize would move existing block offsets because the disk header size changed. Export the disk and recreate/import with the larger size after the layout-migration milestone.",
        ));
    }

    let mut file = OpenOptions::new().write(true).open(&paths.user_disk)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&new_header)?;
    file.flush()?;

    metadata.capacity_gib = new_capacity_gib;
    metadata.logical_size_bytes = new_logical_size_bytes;
    metadata.header_sha256 = sha256_bytes(&new_header);
    metadata.notes.push(format!(
        "Grew Pane sparse user disk logical capacity to {new_capacity_gib} GiB without moving existing block offsets."
    ));
    write_json_file(&paths.user_disk_metadata, &metadata)?;
    Ok(metadata)
}

#[derive(Debug)]
struct UserDiskHeaderInspection {
    capacity_gib: u64,
    logical_size_bytes: u64,
    block_size_bytes: u64,
    allocated_header_bytes: u64,
    header_sha256: String,
}

fn repair_user_disk_metadata(paths: &RuntimePaths) -> AppResult<UserDiskMetadata> {
    if !paths.user_disk.is_file() {
        return Err(AppError::message(
            "Pane user disk is missing; nothing can be repaired.",
        ));
    }
    let header = inspect_user_disk_header(&paths.user_disk)?;
    let metadata = UserDiskMetadata {
        schema_version: 1,
        format: PANE_USER_DISK_FORMAT.to_string(),
        disk_path: paths.user_disk.display().to_string(),
        capacity_gib: header.capacity_gib,
        logical_size_bytes: header.logical_size_bytes,
        block_size_bytes: header.block_size_bytes,
        sparse_backing: true,
        allocated_header_bytes: header.allocated_header_bytes,
        header_sha256: header.header_sha256,
        materialized_block_device: true,
        created_at_epoch_seconds: current_epoch_seconds(),
        notes: vec![
            "Repaired Pane user disk metadata from a valid disk header.".to_string(),
            "Only metadata was rebuilt; Pane did not infer or modify guest filesystem contents."
                .to_string(),
        ],
    };
    write_json_file(&paths.user_disk_metadata, &metadata)?;
    if !user_disk_artifact_ready(paths, &Some(metadata.clone())) {
        return Err(AppError::message(
            "Pane user disk metadata repair did not produce a ready artifact.",
        ));
    }
    Ok(metadata)
}

fn inspect_user_disk_header(disk_path: &Path) -> AppResult<UserDiskHeaderInspection> {
    let mut file = OpenOptions::new().read(true).open(disk_path)?;
    let mut buffer = vec![0_u8; 512];
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    let header_end = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| index + 2)
        .ok_or_else(|| AppError::message("Pane user disk header terminator is missing."))?;
    let header = &buffer[..header_end];
    let header_text = std::str::from_utf8(header)
        .map_err(|_| AppError::message("Pane user disk header is not valid UTF-8."))?;
    if !header_text.starts_with(PANE_USER_DISK_MAGIC) {
        return Err(AppError::message(
            "Pane user disk header magic is missing or invalid.",
        ));
    }
    let format = user_disk_header_field(header_text, "format")?;
    if format != PANE_USER_DISK_FORMAT {
        return Err(AppError::message(
            "Pane user disk header format is not supported.",
        ));
    }
    let logical_size_bytes = user_disk_header_field(header_text, "logical_size_bytes")?
        .parse::<u64>()
        .map_err(|_| AppError::message("Pane user disk logical size is invalid."))?;
    let block_size_bytes = user_disk_header_field(header_text, "block_size_bytes")?
        .parse::<u64>()
        .map_err(|_| AppError::message("Pane user disk block size is invalid."))?;
    if block_size_bytes != PANE_USER_DISK_BLOCK_SIZE_BYTES {
        return Err(AppError::message(
            "Pane user disk block size is not compatible with this Pane version.",
        ));
    }
    let gib = 1024_u64 * 1024 * 1024;
    if logical_size_bytes == 0 || logical_size_bytes % gib != 0 {
        return Err(AppError::message(
            "Pane user disk logical size is not an exact GiB value.",
        ));
    }
    let capacity_gib = logical_size_bytes / gib;
    let expected_header = user_disk_header_bytes(logical_size_bytes);
    if expected_header != header {
        return Err(AppError::message(
            "Pane user disk header fields are not in the canonical Pane format.",
        ));
    }
    Ok(UserDiskHeaderInspection {
        capacity_gib,
        logical_size_bytes,
        block_size_bytes,
        allocated_header_bytes: header.len() as u64,
        header_sha256: sha256_bytes(header),
    })
}

fn user_disk_header_field<'a>(header: &'a str, key: &str) -> AppResult<&'a str> {
    header
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .ok_or_else(|| AppError::message(format!("Pane user disk header is missing `{key}`.")))
}

fn validate_user_disk_artifact_header(
    disk_path: &Path,
    capacity_gib: u64,
    logical_size_bytes: u64,
    block_size_bytes: u64,
) -> AppResult<Vec<u8>> {
    let expected_logical_size = user_disk_logical_size_bytes(capacity_gib)?;
    if expected_logical_size != logical_size_bytes
        || block_size_bytes != PANE_USER_DISK_BLOCK_SIZE_BYTES
    {
        return Err(AppError::message(
            "Pane user disk artifact geometry is not compatible with this Pane version.",
        ));
    }

    let expected_header = user_disk_header_bytes(logical_size_bytes);
    let mut file = OpenOptions::new().read(true).open(disk_path)?;
    let mut actual_header = vec![0_u8; expected_header.len()];
    file.read_exact(&mut actual_header)?;
    if actual_header != expected_header {
        return Err(AppError::message(
            "Pane user disk artifact header does not match its recorded geometry.",
        ));
    }
    Ok(expected_header)
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
        expected_serial_markers: Vec::new(),
        guest_entry_gpa: 0x1000,
        entry_mode: crate::native::NativeGuestEntryMode::RealModeSerial,
        boot_params_gpa: None,
        virtio_block_logical_size_bytes: None,
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
        expected_serial_markers: Vec::new(),
        guest_entry_gpa: 0x1000,
        entry_mode: crate::native::NativeGuestEntryMode::RealModeSerial,
        boot_params_gpa: None,
        virtio_block_logical_size_bytes: None,
        extra_regions: Vec::new(),
    })
}

fn load_kernel_layout_boot_image_artifact(
    paths: &RuntimePaths,
) -> AppResult<crate::native::NativeSerialBootImage> {
    let artifacts = build_runtime_artifact_report(paths);
    if !artifacts.kernel_boot_layout_ready {
        return Err(AppError::message(
            "Pane kernel boot layout is missing or stale. Run `pane native-kernel-plan --prepare-runtime --materialize` after registering a verified kernel plan.",
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
            "Kernel layout no longer matches the registered kernel artifact. Re-run `pane native-kernel-plan --prepare-runtime --materialize`.",
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

    let cmdline_bytes = linux_cmdline_bytes(&layout.cmdline)?;
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
        expected_serial_markers: layout.expected_serial_milestones,
        guest_entry_gpa: kernel_entry_gpa,
        entry_mode: if layout.kernel_format == "linux-bzimage" {
            crate::native::NativeGuestEntryMode::LinuxProtectedMode32
        } else {
            crate::native::NativeGuestEntryMode::RealModeSerial
        },
        boot_params_gpa: (layout.kernel_format == "linux-bzimage")
            .then(|| parse_guest_physical_address(&layout.boot_params_gpa))
            .transpose()?,
        virtio_block_logical_size_bytes: layout.storage.as_ref().map(|storage| {
            storage
                .root_handoff
                .partition_byte_length
                .unwrap_or(storage.base_os_bytes)
        }),
        extra_regions,
    })
}

fn linux_cmdline_bytes(value: &str) -> AppResult<Vec<u8>> {
    let capacity = value
        .len()
        .checked_add(1)
        .ok_or_else(|| AppError::message("Linux kernel command line length overflowed."))?;
    let mut cmdline = Cmdline::new(capacity)
        .map_err(|error| AppError::message(format!("Invalid Linux command line: {error}")))?;
    cmdline
        .insert_str(value)
        .map_err(|error| AppError::message(format!("Invalid Linux command line: {error}")))?;
    let guest_address = GuestAddress(0x1000);
    let memory =
        GuestMemoryMmap::<()>::from_ranges(&[(guest_address, capacity)]).map_err(|error| {
            AppError::message(format!("Could not allocate cmdline memory: {error}"))
        })?;
    load_cmdline(&memory, guest_address, &cmdline).map_err(|error| {
        AppError::message(format!("Could not load Linux command line: {error}"))
    })?;
    let mut bytes = vec![0_u8; capacity];
    memory
        .read_slice(&mut bytes, guest_address)
        .map_err(|error| {
            AppError::message(format!("Could not read Linux command line: {error}"))
        })?;
    Ok(bytes)
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

    let setup_region_size = layout
        .guest_memory_map
        .iter()
        .find(|range| range.label == "bzimage-setup")
        .map(|range| range.size_bytes)
        .unwrap_or(setup_bytes as u64)
        .try_into()
        .map_err(|_| {
            AppError::message("Linux bzImage setup region is too large to map on this host.")
        })?;
    if setup_region_size < setup_bytes {
        return Err(AppError::message(
            "Linux bzImage setup region is smaller than the setup payload.",
        ));
    }
    let mut setup_region_bytes = vec![0_u8; setup_region_size];
    setup_region_bytes[..setup_bytes].copy_from_slice(&kernel_bytes[..setup_bytes]);
    let setup_region = crate::native::NativeGuestMemoryRegion {
        label: "linux-bzimage-setup".to_string(),
        guest_gpa: 0x0009_0000,
        bytes: setup_region_bytes,
        writable: true,
        executable: false,
    };
    let protected_mode_payload =
        &kernel_bytes[protected_mode_offset..protected_mode_offset + protected_mode_bytes];
    let protected_mode_region_size = layout
        .guest_memory_map
        .iter()
        .find(|range| range.label == "kernel-payload")
        .map(|range| range.size_bytes)
        .unwrap_or(protected_mode_bytes as u64)
        .try_into()
        .map_err(|_| {
            AppError::message("Linux kernel payload region is too large to map on this host.")
        })?;
    if protected_mode_region_size < protected_mode_payload.len() {
        return Err(AppError::message(
            "Linux kernel payload region is smaller than the protected-mode payload.",
        ));
    }
    let mut protected_mode_region = vec![0_u8; protected_mode_region_size];
    protected_mode_region[..protected_mode_payload.len()].copy_from_slice(protected_mode_payload);

    Ok((protected_mode_region, kernel_load_gpa, vec![setup_region]))
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
                "usable"
                    | "bios-data"
                    | "bios-rom"
                    | "legacy-rom"
                    | "storage-contract"
                    | "block-dma"
                    | "framebuffer"
                    | "input-queue"
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
                "bios-data" | "bios-rom" | "legacy-rom" => format!("linux-{}", range.label),
                "storage-contract" | "block-dma" | "framebuffer" | "input-queue" => {
                    range.label.clone()
                }
                _ => format!("linux-{}", range.label),
            };
            let bytes = match range.region_type.as_str() {
                "bios-data" => linux_bios_data_area_page_bytes(size),
                "bios-rom" | "legacy-rom" => vec![0_u8; size],
                "storage-contract" => layout
                    .storage
                    .as_ref()
                    .map(storage_contract_page_bytes)
                    .transpose()?
                    .unwrap_or_else(|| vec![0_u8; size]),
                "input-queue" => layout
                    .input
                    .as_ref()
                    .map(|input| input_queue_page_bytes(input, size))
                    .transpose()?
                    .ok_or_else(|| {
                        AppError::message(
                            "Kernel layout maps a Pane input queue but has no input contract.",
                        )
                    })?,
                _ => vec![0_u8; size],
            };
            Ok(crate::native::NativeGuestMemoryRegion {
                label,
                guest_gpa: parse_guest_physical_address(&range.start_gpa)?,
                bytes,
                writable: true,
                executable: range.region_type == "usable",
            })
        })
        .collect()
}

fn linux_bios_data_area_page_bytes(size: usize) -> Vec<u8> {
    let mut page = vec![0_u8; size];
    if size >= 0x415 {
        page[0x400..0x402].copy_from_slice(&0x03f8_u16.to_le_bytes());
        page[0x40e..0x410].copy_from_slice(&0_u16.to_le_bytes());
        page[0x413..0x415].copy_from_slice(&640_u16.to_le_bytes());
    }
    page
}

fn storage_contract_page_bytes(storage: &KernelStorageAttachment) -> AppResult<Vec<u8>> {
    let mut page = vec![0_u8; storage_contract_size_bytes() as usize];
    let mut payload = serde_json::to_vec(storage)?;
    payload.push(0);
    if payload.len() > page.len() {
        return Err(AppError::message(
            "Pane storage contract is too large for its guest discovery page.",
        ));
    }
    page[..payload.len()].copy_from_slice(&payload);
    Ok(page)
}

fn input_queue_page_bytes(input: &InputContract, size: usize) -> AppResult<Vec<u8>> {
    let header_bytes = input.queue_header_bytes as usize;
    let queue_size: usize = input
        .queue_size_bytes
        .try_into()
        .map_err(|_| AppError::message("Pane input queue is too large to map on this host."))?;
    let record_bytes = input.event_record_bytes as usize;
    let magic = input.queue_magic.as_bytes();

    if magic.len() != 8 {
        return Err(AppError::message(
            "Pane input queue magic must be exactly 8 bytes.",
        ));
    }
    if header_bytes < 64 {
        return Err(AppError::message(
            "Pane input queue header must be at least 64 bytes.",
        ));
    }
    if record_bytes == 0 {
        return Err(AppError::message(
            "Pane input queue event record size must be greater than zero.",
        ));
    }
    if queue_size > size {
        return Err(AppError::message(
            "Pane input queue contract is larger than the mapped guest page.",
        ));
    }
    if queue_size < header_bytes + record_bytes {
        return Err(AppError::message(
            "Pane input queue must have room for its header and at least one event record.",
        ));
    }

    let capacity_records = ((queue_size - header_bytes) / record_bytes) as u64;
    let mut page = vec![0_u8; size];
    page[0..8].copy_from_slice(magic);
    page[8..12].copy_from_slice(&input.schema_version.to_le_bytes());
    page[12..16].copy_from_slice(&input.queue_header_bytes.to_le_bytes());
    page[16..24].copy_from_slice(&input.queue_size_bytes.to_le_bytes());
    page[24..28].copy_from_slice(&input.event_record_bytes.to_le_bytes());
    page[32..40].copy_from_slice(&0_u64.to_le_bytes());
    page[40..48].copy_from_slice(&0_u64.to_le_bytes());
    page[48..56].copy_from_slice(&capacity_records.to_le_bytes());
    Ok(page)
}

fn augment_kernel_cmdline_for_runtime_contracts(
    base_cmdline: &str,
    storage: Option<&KernelStorageAttachment>,
    framebuffer: &FramebufferContract,
    input: &InputContract,
) -> AppResult<String> {
    let mut cmdline = base_cmdline.trim().to_string();
    append_kernel_arg(
        &mut cmdline,
        "earlycon=uart8250,io,0x3f8,115200n8".to_string(),
    );
    append_kernel_arg(&mut cmdline, "earlyprintk=serial,ttyS0,115200".to_string());
    append_kernel_arg(&mut cmdline, "quiet".to_string());
    append_kernel_arg(&mut cmdline, "loglevel=4".to_string());
    append_kernel_arg(&mut cmdline, "panic=-1".to_string());
    append_kernel_arg(&mut cmdline, "nomodeset".to_string());
    append_kernel_arg(&mut cmdline, "lpj=1000000".to_string());
    append_kernel_arg(&mut cmdline, "tsc=reliable".to_string());
    append_kernel_arg(&mut cmdline, "clocksource=tsc".to_string());
    append_kernel_arg(&mut cmdline, "no_timer_check".to_string());
    append_kernel_arg(&mut cmdline, "i8042.noaux".to_string());
    append_kernel_arg(&mut cmdline, "acpi=off".to_string());
    append_kernel_arg(&mut cmdline, "pci=off".to_string());
    if let Some(storage) = storage {
        if !storage.virtio_block.linux_kernel_parameter.is_empty() {
            append_kernel_arg(
                &mut cmdline,
                storage.virtio_block.linux_kernel_parameter.clone(),
            );
        }
        append_kernel_arg(
            &mut cmdline,
            format!("pane.storage_contract={}", storage.contract_gpa),
        );
        append_kernel_arg(
            &mut cmdline,
            format!("pane.root={}", storage.root_handoff.root_device),
        );
        append_kernel_arg(
            &mut cmdline,
            format!("pane.root_mode={}", storage.root_handoff.mode),
        );
        append_kernel_arg(
            &mut cmdline,
            format!(
                "pane.root_readonly={}",
                if storage.readonly_base { 1 } else { 0 }
            ),
        );
        if let Some(filesystem) = storage
            .root_handoff
            .filesystem_hint
            .as_deref()
            .and_then(supported_root_filesystem_hint)
        {
            append_kernel_arg(&mut cmdline, format!("pane.root_fs={filesystem}"));
        }
        if let Some(index) = storage.root_handoff.partition_index {
            append_kernel_arg(&mut cmdline, format!("pane.root_partition={index}"));
        }
        if let Some(offset) = storage.root_handoff.partition_byte_offset {
            append_kernel_arg(&mut cmdline, format!("pane.root_offset={offset}"));
        }
        if let Some(length) = storage.root_handoff.partition_byte_length {
            append_kernel_arg(&mut cmdline, format!("pane.root_length={length}"));
        }
        append_kernel_arg(&mut cmdline, format!("pane.user={}", storage.user_device));
        if !storage.virtio_block.root_device_hint.is_empty() {
            append_kernel_arg(
                &mut cmdline,
                format!("pane.virtio_root={}", storage.virtio_block.root_device_hint),
            );
        }
        append_kernel_arg(
            &mut cmdline,
            format!(
                "pane.block_io={},{},{}",
                storage.block_io_port_base,
                storage.block_io_port_count,
                storage.block_io_block_size_bytes
            ),
        );
        append_kernel_arg(
            &mut cmdline,
            format!(
                "pane.block_devices={},{}",
                pane_block_device_blocks(
                    storage
                        .root_handoff
                        .partition_byte_length
                        .unwrap_or(storage.base_os_bytes),
                    storage.block_io_block_size_bytes
                )?,
                pane_block_device_blocks(
                    storage.user_disk_logical_size_bytes,
                    storage.block_io_block_size_bytes
                )?
            ),
        );
        append_kernel_arg(
            &mut cmdline,
            format!(
                "pane.block_dma={},{}",
                storage.block_dma_gpa, storage.block_dma_size_bytes
            ),
        );
    }
    append_kernel_arg(
        &mut cmdline,
        format!(
            "pane.framebuffer={},{},{},{},{}",
            framebuffer.guest_gpa,
            framebuffer.width,
            framebuffer.height,
            framebuffer.bytes_per_pixel * 8,
            framebuffer.format
        ),
    );
    append_kernel_arg(
        &mut cmdline,
        format!(
            "pane.input_queue={},{},{},{}",
            input.guest_queue_gpa,
            input.queue_size_bytes,
            input.event_record_bytes,
            input.queue_header_bytes
        ),
    );
    validate_kernel_cmdline(&cmdline)?;
    Ok(cmdline)
}

fn supported_root_filesystem_hint(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case("ext4") {
        Some("ext4")
    } else if value.eq_ignore_ascii_case("btrfs") {
        Some("btrfs")
    } else if value.eq_ignore_ascii_case("xfs") {
        Some("xfs")
    } else if value.eq_ignore_ascii_case("f2fs") {
        Some("f2fs")
    } else {
        None
    }
}

fn pane_block_device_blocks(bytes: u64, block_size_bytes: u64) -> AppResult<u64> {
    if block_size_bytes == 0 {
        return Err(AppError::message(
            "Pane block device geometry requires a non-zero block size.",
        ));
    }
    Ok(bytes.saturating_add(block_size_bytes - 1) / block_size_bytes)
}

fn append_kernel_arg(cmdline: &mut String, arg: String) {
    let key = arg.split_once('=').map(|(key, _)| key).unwrap_or(&arg);
    if cmdline.split_whitespace().any(|existing| {
        existing == key
            || existing
                .strip_prefix(key)
                .is_some_and(|suffix| suffix.starts_with('='))
    }) {
        return;
    }
    if !cmdline.is_empty() {
        cmdline.push(' ');
    }
    cmdline.push_str(&arg);
}

fn build_linux_boot_params_page(
    layout: &KernelBootLayout,
    kernel_bytes: Option<&[u8]>,
) -> AppResult<Vec<u8>> {
    if layout.kernel_format != "linux-bzimage" {
        return Ok(vec![0_u8; 4096]);
    }

    let kernel_bytes = kernel_bytes.ok_or_else(|| {
        AppError::message("Linux boot params require the original bzImage bytes.")
    })?;
    let kernel_load_gpa = parse_guest_physical_address(&layout.kernel_load_gpa)?;
    let guest_memory = GuestMemoryMmap::<()>::from_ranges(&[(
        GuestAddress(kernel_load_gpa),
        kernel_bytes.len().max(4096),
    )])
    .map_err(|error| {
        AppError::message(format!("Could not allocate Linux loader memory: {error}"))
    })?;
    let loader_result = BzImage::load(
        &guest_memory,
        Some(GuestAddress(kernel_load_gpa)),
        &mut Cursor::new(kernel_bytes),
        None,
    )
    .map_err(|error| AppError::message(format!("linux-loader rejected the bzImage: {error}")))?;
    let setup_header = loader_result
        .setup_header
        .ok_or_else(|| AppError::message("linux-loader did not return a bzImage setup header."))?;
    let mut params = bootparam::boot_params {
        hdr: setup_header,
        ..Default::default()
    };

    let cmdline_gpa = checked_u32_gpa(&layout.cmdline_gpa, "kernel cmdline")?;
    params.hdr.boot_flag = 0xaa55;
    params.hdr.header = 0x5372_6448;
    params.hdr.version = layout
        .linux_boot_protocol
        .as_deref()
        .and_then(parse_hex_u16)
        .unwrap_or(0x020f);
    params.hdr.type_of_loader = 0xff;
    params.hdr.loadflags |= 0x80;
    params.hdr.code32_start = checked_u32_gpa(&layout.kernel_load_gpa, "kernel entry")?;
    params.hdr.cmd_line_ptr = cmdline_gpa;
    params.hdr.initrd_addr_max = 0x7fff_ffff;
    params.hdr.cmdline_size = layout.cmdline.len().saturating_add(1) as u32;

    if layout.initramfs_path.is_some() {
        let initramfs_gpa = layout
            .initramfs_load_gpa
            .as_deref()
            .ok_or_else(|| AppError::message("Initramfs layout is missing a load GPA."))?;
        params.hdr.ramdisk_image = checked_u32_gpa(initramfs_gpa, "initramfs")?;
        params.hdr.ramdisk_size = layout
            .initramfs_bytes
            .ok_or_else(|| AppError::message("Initramfs layout is missing its byte length."))?
            .try_into()
            .map_err(|_| {
                AppError::message("Initramfs is too large for the 32-bit boot protocol field.")
            })?;
    }

    if let Some(framebuffer) = &layout.framebuffer {
        params.screen_info = linux_screen_info(framebuffer)?;
    }
    let e820_entries = linux_e820_entries(&layout.guest_memory_map)?;
    params.e820_entries = e820_entries.len() as u8;
    params.e820_table[..e820_entries.len()].copy_from_slice(&e820_entries);

    let boot_params_gpa = parse_guest_physical_address(&layout.boot_params_gpa)?;
    let boot_memory = GuestMemoryMmap::<()>::from_ranges(&[(GuestAddress(boot_params_gpa), 8192)])
        .map_err(|error| {
            AppError::message(format!(
                "Could not allocate Linux boot-params memory: {error}"
            ))
        })?;
    let boot_params = BootParams::new(&params, GuestAddress(boot_params_gpa));
    LinuxBootConfigurator::write_bootparams(&boot_params, &boot_memory).map_err(|error| {
        AppError::message(format!("Could not write Linux boot parameters: {error}"))
    })?;
    let mut bytes = vec![0_u8; 4096];
    boot_memory
        .read_slice(&mut bytes, GuestAddress(boot_params_gpa))
        .map_err(|error| {
            AppError::message(format!("Could not read Linux boot parameters: {error}"))
        })?;
    Ok(bytes)
}

fn linux_loader_adapter_plan(
    kernel_format: &str,
    linux_boot_protocol: Option<&str>,
) -> LinuxLoaderAdapterPlan {
    let applicable = kernel_format == "linux-bzimage";
    LinuxLoaderAdapterPlan {
        schema_version: 1,
        adapter_kind: "pane-linux-loader-adapter-v1".to_string(),
        source_crate: "rust-vmm/linux-loader".to_string(),
        candidate_crate_version: Some("0.13.2".to_string()),
        license: "Apache-2.0 OR BSD-3-Clause".to_string(),
        source_url: "https://github.com/rust-vmm/linux-loader".to_string(),
        adoption_state: if applicable {
            "linked-linux-loader-0.13.2"
        } else {
            "not-applicable"
        }
        .to_string(),
        applicable,
        kernel_format: kernel_format.to_string(),
        linux_boot_protocol: linux_boot_protocol.map(ToOwned::to_owned),
        kernel_loader: if applicable {
            "bzImage loader semantics"
        } else {
            "not-applicable"
        }
        .to_string(),
        cmdline_loader: if applicable {
            "load_cmdline-compatible placement"
        } else {
            "not-applicable"
        }
        .to_string(),
        boot_params_writer: if applicable {
            "LinuxBootConfigurator-compatible boot_params page"
        } else {
            "not-applicable"
        }
        .to_string(),
        guest_memory_backend: if applicable {
            "Pane vm-memory 0.17.1 adapter over WHP guest mappings"
        } else {
            "Pane controlled serial candidate memory"
        }
        .to_string(),
        notes: vec![
            "Pane validates and loads bzImage payloads with BzImage, serializes generated boot_params through LinuxBootConfigurator, and materializes command lines with load_cmdline."
                .to_string(),
            "Layouts without this current adapter record are stale and must be rematerialized before WHP execution."
                .to_string(),
        ],
    }
}

fn legacy_linux_loader_adapter_plan() -> LinuxLoaderAdapterPlan {
    LinuxLoaderAdapterPlan {
        schema_version: 0,
        adapter_kind: "legacy-manual-linux-boot-layout".to_string(),
        source_crate: "none".to_string(),
        candidate_crate_version: None,
        license: "not-applicable".to_string(),
        source_url: String::new(),
        adoption_state: "legacy-layout-rematerialize-required".to_string(),
        applicable: false,
        kernel_format: "unknown".to_string(),
        linux_boot_protocol: None,
        kernel_loader: "manual".to_string(),
        cmdline_loader: "manual".to_string(),
        boot_params_writer: "manual".to_string(),
        guest_memory_backend: "Pane WHP guest memory".to_string(),
        notes: vec![
            "This layout predates the linux-loader adapter boundary and is intentionally not ready for WHP execution."
                .to_string(),
        ],
    }
}

fn linux_screen_info(framebuffer: &FramebufferContract) -> AppResult<bootparam::screen_info> {
    if framebuffer.format != "x8r8g8b8" {
        return Err(AppError::message(format!(
            "Framebuffer format `{}` cannot be advertised through Linux screen_info yet.",
            framebuffer.format
        )));
    }

    let width: u16 = framebuffer
        .width
        .try_into()
        .map_err(|_| AppError::message("Framebuffer width is too large for Linux screen_info."))?;
    let height: u16 = framebuffer
        .height
        .try_into()
        .map_err(|_| AppError::message("Framebuffer height is too large for Linux screen_info."))?;
    let depth: u16 = (framebuffer.bytes_per_pixel * 8)
        .try_into()
        .map_err(|_| AppError::message("Framebuffer depth is too large for Linux screen_info."))?;
    let stride: u16 = framebuffer
        .stride_bytes
        .try_into()
        .map_err(|_| AppError::message("Framebuffer stride is too large for Linux screen_info."))?;
    let lfb_size: u32 = framebuffer
        .size_bytes
        .try_into()
        .map_err(|_| AppError::message("Framebuffer size is too large for Linux screen_info."))?;

    Ok(bootparam::screen_info {
        orig_video_isVGA: 0x23,
        lfb_width: width,
        lfb_height: height,
        lfb_depth: depth,
        lfb_base: checked_u32_gpa(&framebuffer.guest_gpa, "framebuffer")?,
        lfb_size,
        lfb_linelength: stride,
        red_size: 8,
        red_pos: 16,
        green_size: 8,
        green_pos: 8,
        blue_size: 8,
        blue_pos: 0,
        rsvd_size: 8,
        rsvd_pos: 24,
        ..Default::default()
    })
}

fn linux_e820_entries(
    ranges: &[KernelGuestMemoryRange],
) -> AppResult<Vec<bootparam::boot_e820_entry>> {
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

    let mut entries = Vec::with_capacity(ranges.len());
    for range in ranges {
        let start = parse_guest_physical_address(&range.start_gpa)?;
        let region_type = match range.region_type.as_str() {
            "usable" => 1,
            "reserved" | "bios-data" | "bios-rom" | "legacy-rom" | "mmio-stub" | "virtio-mmio"
            | "storage-contract" | "block-dma" | "framebuffer" | "input-queue" => 2,
            other => {
                return Err(AppError::message(format!(
                    "Unsupported Linux E820 range type `{other}` for `{}`.",
                    range.label
                )))
            }
        };
        entries.push(bootparam::boot_e820_entry {
            addr: start,
            size: range.size_bytes,
            type_: region_type,
        });
    }
    Ok(entries)
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

fn read_u16_le_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
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
        initramfs_load_gpa: initramfs_record.as_ref().map(|_| "0x0c000000".to_string()),
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
    let linux_loader = linux_loader_adapter_plan(
        &metadata.kernel_format,
        metadata.linux_boot_protocol.as_deref(),
    );
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
    let initramfs_driver = if storage.is_some() {
        Some(load_verified_pane_initramfs_driver_metadata(paths)?)
    } else {
        None
    };
    if storage.is_some() {
        load_verified_pane_block_module_metadata(paths)?;
    }
    if storage.is_some() && !metadata.initramfs_verified {
        return Err(AppError::message(
            "Storage-backed kernel layouts require a verified initramfs artifact containing Pane discovery support. Run `pane runtime --write-initramfs-driver --build-discovery-initramfs`, or register an externally built initramfs with `pane runtime --register-initramfs <path> --initramfs-expected-sha256 <sha256>`.",
        ));
    }
    let expected_serial_milestones = if initramfs_driver.is_some() {
        pane_initramfs_expected_serial_milestones()
    } else {
        Vec::new()
    };
    let framebuffer = read_json_file::<FramebufferContract>(&paths.framebuffer_contract)
        .unwrap_or_else(|_| default_framebuffer_contract());
    let input = read_json_file::<InputContract>(&paths.input_contract)
        .unwrap_or_else(|_| default_input_contract());
    let cmdline = augment_kernel_cmdline_for_runtime_contracts(
        &metadata.cmdline,
        storage.as_ref(),
        &framebuffer,
        &input,
    )?;
    if is_linux_bzimage {
        if let Some(storage) = &storage {
            guest_memory_map.push(virtio_mmio_guest_memory_range(storage)?);
            guest_memory_map.push(storage_contract_guest_memory_range(storage)?);
            guest_memory_map.push(block_dma_guest_memory_range(storage)?);
        }
        guest_memory_map.extend(runtime_contract_guest_memory_ranges(&framebuffer, &input)?);
        validate_guest_memory_ranges_do_not_overlap(&guest_memory_map)?;
    }

    let layout = KernelBootLayout {
        schema_version: 1,
        layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
        session_name: session_name.to_string(),
        linux_loader,
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
        cmdline,
        expected_serial_device: metadata.expected_serial_device,
        expected_serial_milestones,
        storage,
        initramfs_driver,
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
    let base_metadata = refresh_base_os_metadata_inspection(
        paths,
        read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata)?,
    )?;
    if !base_metadata.bootable_disk_hint {
        return Err(AppError::message(format!(
            "Pane native storage requires a verified raw disk base image with a Linux root partition hint; `{}` was registered as `{}`. Convert the Arch rootfs to a raw disk image or register a bootable raw Arch image before materializing a storage-backed kernel layout.",
            paths.base_os_image.display(),
            base_metadata.image_format
        )));
    }
    if base_metadata.root_partition_hint.is_none() {
        return Err(AppError::message(format!(
            "Pane native storage found a verified raw disk base image at {}, but no Linux root partition hint. Register an Arch raw disk image with a detectable Linux root partition before materializing the kernel layout.",
            paths.base_os_image.display()
        )));
    }
    let root_handoff = build_kernel_root_handoff(&base_metadata);
    let root_device = root_handoff.root_device.clone();

    let mut storage = KernelStorageAttachment {
        schema_version: 1,
        base_os_path: paths.base_os_image.display().to_string(),
        base_os_sha256: base_sha256,
        base_os_bytes: base_bytes,
        base_os_block_size_bytes: PANE_BASE_OS_BLOCK_SIZE_BYTES,
        block_io_protocol: default_pane_block_io_protocol(),
        block_io_port_base: default_pane_block_io_port_base(),
        block_io_port_count: default_pane_block_io_port_count(),
        block_io_status_port_offset: default_pane_block_io_status_port_offset(),
        block_io_data_port_offset: default_pane_block_io_data_port_offset(),
        block_io_block_size_bytes: default_pane_block_io_block_size_bytes(),
        block_dma_gpa: default_pane_block_dma_gpa(),
        block_dma_size_bytes: default_pane_block_dma_size_bytes(),
        virtio_block: legacy_virtio_block_backend_plan(),
        base_os_image_format: base_metadata.image_format,
        base_os_bootable_disk_hint: base_metadata.bootable_disk_hint,
        base_os_partitions: base_metadata.partitions,
        base_os_root_partition_hint: base_metadata.root_partition_hint,
        root_handoff,
        user_disk_path: user_disk_metadata.disk_path,
        user_disk_capacity_gib: user_disk_metadata.capacity_gib,
        user_disk_logical_size_bytes: user_disk_metadata.logical_size_bytes,
        user_disk_block_size_bytes: user_disk_metadata.block_size_bytes,
        user_disk_sparse_backing: user_disk_metadata.sparse_backing,
        user_disk_header_sha256: user_disk_metadata.header_sha256,
        user_disk_format: user_disk_metadata.format,
        root_device,
        user_device: "/dev/pane1".to_string(),
        contract_gpa: "0x0dfe0000".to_string(),
        readonly_base: true,
        writable_user_disk: true,
    };
    storage.virtio_block = virtio_block_backend_plan(&storage);
    Ok(Some(storage))
}

fn build_kernel_root_handoff(base_metadata: &BaseOsImageMetadata) -> KernelRootHandoff {
    const BASE_DEVICE: &str = "/dev/pane0";

    if let Some(root) = &base_metadata.root_partition_hint {
        return KernelRootHandoff {
            schema_version: 1,
            mode: "base-partition-direct".to_string(),
            root_device: BASE_DEVICE.to_string(),
            base_device: BASE_DEVICE.to_string(),
            partition_index: Some(root.index),
            partition_byte_offset: Some(root.byte_offset),
            partition_byte_length: Some(root.byte_length),
            filesystem_hint: base_metadata
                .root_filesystem_hint
                .clone()
                .or_else(|| Some(root.partition_type.clone())),
            requires_initramfs_driver: true,
            notes: vec![
                "Pane found a likely Linux root partition and exposes that partition directly as /dev/pane0 to avoid guest partition-scan I/O during native boot."
                    .to_string(),
                "The Pane block driver applies the partition byte offset internally so the guest mounts a root device, not the outer raw disk."
                    .to_string(),
            ],
        };
    }

    let (mode, note) = if base_metadata.bootable_disk_hint {
        (
            "base-disk",
            "Pane found a bootable raw disk but no obvious Linux root partition; the whole base disk is the root candidate.",
        )
    } else if base_metadata.image_format == "tar-rootfs" {
        (
            "rootfs-archive",
            "Pane found a rootfs archive; native boot needs an initramfs extraction or conversion path before mounting root.",
        )
    } else {
        (
            "base-device",
            "Pane did not find a stronger root hint; the whole base device remains the conservative root candidate.",
        )
    };

    KernelRootHandoff {
        schema_version: 1,
        mode: mode.to_string(),
        root_device: BASE_DEVICE.to_string(),
        base_device: BASE_DEVICE.to_string(),
        partition_index: None,
        partition_byte_offset: None,
        partition_byte_length: None,
        filesystem_hint: base_metadata
            .root_filesystem_hint
            .clone()
            .or_else(|| Some(base_metadata.image_format.clone())),
        requires_initramfs_driver: true,
        notes: vec![note.to_string()],
    }
}

fn storage_contract_guest_memory_range(
    storage: &KernelStorageAttachment,
) -> AppResult<KernelGuestMemoryRange> {
    let storage_gpa = parse_guest_physical_address(&storage.contract_gpa)?;
    Ok(KernelGuestMemoryRange {
        label: "pane-storage-contract".to_string(),
        start_gpa: format_guest_physical_address(storage_gpa),
        size_bytes: storage_contract_size_bytes(),
        region_type: "storage-contract".to_string(),
    })
}

fn virtio_mmio_guest_memory_range(
    storage: &KernelStorageAttachment,
) -> AppResult<KernelGuestMemoryRange> {
    let virtio_mmio_gpa = parse_guest_physical_address(&storage.virtio_block.mmio_base_gpa)?;
    Ok(KernelGuestMemoryRange {
        label: "pane-virtio-mmio".to_string(),
        start_gpa: format_guest_physical_address(virtio_mmio_gpa),
        size_bytes: page_align_guest_range(storage.virtio_block.mmio_size_bytes),
        region_type: "virtio-mmio".to_string(),
    })
}

fn block_dma_guest_memory_range(
    storage: &KernelStorageAttachment,
) -> AppResult<KernelGuestMemoryRange> {
    let block_dma_gpa = parse_guest_physical_address(&storage.block_dma_gpa)?;
    Ok(KernelGuestMemoryRange {
        label: "pane-block-dma".to_string(),
        start_gpa: format_guest_physical_address(block_dma_gpa),
        size_bytes: page_align_guest_range(storage.block_dma_size_bytes),
        region_type: "block-dma".to_string(),
    })
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
    const INITRAMFS_GPA: u64 = 0x0c00_0000;
    const HIGH_RAM_GPA: u64 = 0x0800_0000;

    let mut ranges = vec![
        KernelGuestMemoryRange {
            label: "bios-data-area".to_string(),
            start_gpa: "0x00000000".to_string(),
            size_bytes: 0x00001000,
            region_type: "bios-data".to_string(),
        },
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
            label: "legacy-rom".to_string(),
            start_gpa: "0x000a0000".to_string(),
            size_bytes: 0x00040000,
            region_type: "legacy-rom".to_string(),
        },
        KernelGuestMemoryRange {
            label: "bios-rom".to_string(),
            start_gpa: "0x000e0000".to_string(),
            size_bytes: 0x00020000,
            region_type: "bios-rom".to_string(),
        },
        KernelGuestMemoryRange {
            label: "kernel-payload".to_string(),
            start_gpa: "0x00100000".to_string(),
            size_bytes: 0x02000000,
            region_type: "reserved".to_string(),
        },
        KernelGuestMemoryRange {
            label: "kernel-work-ram".to_string(),
            start_gpa: "0x02100000".to_string(),
            size_bytes: 0x05f00000,
            region_type: "usable".to_string(),
        },
    ];

    if initramfs_bytes > 0 {
        let initramfs_size = page_align_guest_range(initramfs_bytes);
        ranges.push(KernelGuestMemoryRange {
            label: "initramfs".to_string(),
            start_gpa: format_guest_physical_address(INITRAMFS_GPA),
            size_bytes: initramfs_size,
            region_type: "reserved".to_string(),
        });
        let initramfs_end_gpa = INITRAMFS_GPA.saturating_add(initramfs_size);
        if initramfs_end_gpa < HIGH_RAM_GPA {
            ranges.push(KernelGuestMemoryRange {
                label: "mid-ram".to_string(),
                start_gpa: format_guest_physical_address(initramfs_end_gpa),
                size_bytes: HIGH_RAM_GPA - initramfs_end_gpa,
                region_type: "usable".to_string(),
            });
        }
    }

    ranges.extend([
        KernelGuestMemoryRange {
            label: "high-ram".to_string(),
            start_gpa: format_guest_physical_address(HIGH_RAM_GPA),
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
        "initramfs_driver_dir": paths.initramfs_driver_dir.display().to_string(),
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
        "foundation": crate::vmm_foundation::build_vmm_foundation_report(),
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
            "initramfs_driver_dir": paths.initramfs_driver_dir.display().to_string(),
            "kernel_boot_metadata": paths.kernel_boot_metadata.display().to_string(),
            "initramfs_driver_metadata": paths.initramfs_driver_metadata.display().to_string(),
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
            "Pane initramfs driver bundle must exist before root storage can be mounted natively",
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
    let initramfs_driver_metadata =
        read_json_file::<PaneInitramfsDriverMetadata>(&paths.initramfs_driver_metadata).ok();
    let initramfs_driver_bundle_exists = paths.initramfs_driver_dir.is_dir();
    let initramfs_driver_bundle_ready = initramfs_driver_metadata
        .as_ref()
        .map(|metadata| {
            metadata.schema_version == 1
                && metadata.bundle_kind == "pane-initramfs-driver-source-v1"
                && metadata.driver_dir == paths.initramfs_driver_dir.display().to_string()
                && metadata.block_io_protocol == default_pane_block_io_protocol()
                && metadata.block_io_port_base == default_pane_block_io_port_base()
                && metadata.block_io_port_count == default_pane_block_io_port_count()
                && metadata.block_io_status_port_offset
                    == default_pane_block_io_status_port_offset()
                && metadata.block_io_data_port_offset == default_pane_block_io_data_port_offset()
                && metadata.block_io_block_size_bytes == default_pane_block_io_block_size_bytes()
                && metadata.block_driver_abi_sha256 == pane_block_driver_abi_sha256()
                && sha256_file(Path::new(&metadata.hook_path)).ok().as_deref()
                    == Some(metadata.hook_sha256.as_str())
                && sha256_file(Path::new(&metadata.header_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.header_sha256.as_str())
                && sha256_file(Path::new(&metadata.init_source_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.init_source_sha256.as_str())
                && sha256_file(Path::new(&metadata.probe_source_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.probe_source_sha256.as_str())
                && sha256_file(Path::new(&metadata.block_driver_source_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.block_driver_source_sha256.as_str())
                && sha256_file(Path::new(&metadata.block_driver_build_script_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.block_driver_build_script_sha256.as_str())
                && sha256_file(Path::new(&metadata.build_script_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.build_script_sha256.as_str())
                && sha256_file(Path::new(&metadata.readme_path))
                    .ok()
                    .as_deref()
                    == Some(metadata.readme_sha256.as_str())
        })
        .unwrap_or(false);
    let pane_block_module_path = pane_block_module_path(paths);
    let pane_block_module_metadata_path = pane_block_module_metadata_path(paths);
    let pane_block_module_metadata =
        read_json_file::<PaneBlockModuleMetadata>(&pane_block_module_metadata_path).ok();
    let pane_block_module_bytes = fs::metadata(&pane_block_module_path)
        .ok()
        .map(|metadata| metadata.len());
    let pane_block_module_actual_sha256 = sha256_file(&pane_block_module_path).ok();
    let pane_block_module_verified = pane_block_module_metadata
        .as_ref()
        .zip(pane_block_module_bytes)
        .zip(pane_block_module_actual_sha256.as_ref())
        .map(|((metadata, bytes), actual_sha256)| {
            metadata.schema_version == 1
                && metadata.module_kind == "pane-linux-block-module-v1"
                && metadata.stored_path == pane_block_module_path.display().to_string()
                && metadata.bytes == bytes
                && metadata.sha256 == *actual_sha256
                && metadata.verified
                && metadata.target_kernel_path.as_deref()
                    == Some(paths.kernel_image.display().to_string().as_str())
                && metadata.target_kernel_bytes
                    == kernel_boot_metadata
                        .as_ref()
                        .map(|metadata| metadata.kernel_bytes)
                && metadata.target_kernel_sha256.as_deref()
                    == kernel_boot_metadata
                        .as_ref()
                        .map(|metadata| metadata.kernel_sha256.as_str())
                && metadata.target_kernel_format.as_deref()
                    == kernel_boot_metadata
                        .as_ref()
                        .map(|metadata| metadata.kernel_format.as_str())
                && initramfs_driver_metadata
                    .as_ref()
                    .is_some_and(|initramfs_driver| {
                        pane_block_module_matches_current_driver_abi(metadata, initramfs_driver)
                    })
        })
        .unwrap_or(false);
    let discovery_initramfs_matches_driver_bundle =
        pane_discovery_initramfs_matches_current_driver_bundle(
            paths,
            initramfs_driver_metadata.as_ref(),
            pane_block_module_metadata.as_ref(),
            initramfs_image_bytes,
            initramfs_actual_sha256.as_ref(),
        );

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
                && contract.format == "x8r8g8b8"
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
                && contract.queue_header_bytes >= 64
                && contract.queue_magic == default_input_queue_magic()
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
                        if let Some(storage) = &layout.storage {
                            ranges.push(virtio_mmio_guest_memory_range(storage).ok()?);
                            ranges.push(storage_contract_guest_memory_range(storage).ok()?);
                            ranges.push(block_dma_guest_memory_range(storage).ok()?);
                        }
                        ranges
                            .extend(runtime_contract_guest_memory_ranges(framebuffer, input).ok()?);
                        Some(ranges)
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let expected_cmdline = framebuffer_contract
                .as_ref()
                .zip(input_contract.as_ref())
                .and_then(|(framebuffer, input)| {
                    augment_kernel_cmdline_for_runtime_contracts(
                        &metadata.cmdline,
                        layout.storage.as_ref(),
                        framebuffer,
                        input,
                    )
                    .ok()
                });
            let expected_serial_milestones = if layout.storage.is_some() {
                pane_initramfs_expected_serial_milestones()
            } else {
                Vec::new()
            };
            let expected_linux_loader = linux_loader_adapter_plan(
                &metadata.kernel_format,
                metadata.linux_boot_protocol.as_deref(),
            );

            kernel_boot_plan_ready
                && layout.schema_version == 1
                && layout.layout_kind == "pane-linux-kernel-boot-layout-v1"
                && layout.linux_loader == expected_linux_loader
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
                && Some(&layout.cmdline) == expected_cmdline.as_ref()
                && layout.cmdline.contains("console=ttyS0")
                && (layout.storage.is_none() || initramfs_image_verified)
                && (layout.storage.is_none() || initramfs_driver_bundle_ready)
                && (layout.storage.is_none() || discovery_initramfs_matches_driver_bundle)
                && (layout.storage.is_none() || pane_block_module_verified)
                && layout
                    .storage
                    .as_ref()
                    .map(|storage| {
                        storage
                            .virtio_block
                            .boot_contract_matches(&virtio_block_backend_plan(storage))
                    })
                    .unwrap_or(true)
                && layout.initramfs_driver
                    == layout
                        .storage
                        .as_ref()
                        .and_then(|_| initramfs_driver_metadata.clone())
                && layout.expected_serial_device == "ttyS0"
                && layout.expected_serial_milestones == expected_serial_milestones
                && layout.framebuffer.as_ref().is_some_and(|contract| {
                    contract.schema_version == 1
                        && contract.device == "pane-linear-framebuffer-v1"
                        && contract.format == "x8r8g8b8"
                        && contract.size_bytes
                            == u64::from(contract.stride_bytes) * u64::from(contract.height)
                })
                && layout.input.as_ref().is_some_and(|contract| {
                    contract.schema_version == 1
                        && contract.keyboard_device == "pane-ps2-keyboard-v1"
                        && contract.pointer_device == "pane-absolute-pointer-v1"
                        && contract.guest_queue_gpa == "0x0dff0000"
                        && contract.queue_size_bytes == 0x00001000
                        && contract.event_record_bytes == 32
                        && contract.queue_header_bytes == default_input_queue_header_bytes()
                        && contract.queue_magic == default_input_queue_magic()
                })
        })
        .unwrap_or(false);

    let user_disk_metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).ok();
    let user_disk_ready = user_disk_artifact_ready(paths, &user_disk_metadata);
    let user_disk_snapshots = user_disk_snapshot_metadata_files(paths);

    RuntimeArtifactReport {
        base_os_image_exists: paths.base_os_image.is_file(),
        base_os_image_bytes: base_image_bytes,
        base_os_image_sha256: base_metadata
            .as_ref()
            .map(|metadata| metadata.sha256.clone()),
        base_os_image_format: base_metadata
            .as_ref()
            .map(|metadata| metadata.image_format.clone()),
        base_os_bootable_disk_hint: base_metadata
            .as_ref()
            .map(|metadata| metadata.bootable_disk_hint),
        base_os_root_partition_hint: base_metadata
            .as_ref()
            .map(|metadata| metadata.root_partition_hint.is_some()),
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
        initramfs_driver_bundle_exists,
        initramfs_driver_metadata_exists: paths.initramfs_driver_metadata.is_file(),
        initramfs_driver_bundle_ready,
        discovery_initramfs_matches_driver_bundle,
        pane_block_module_exists: pane_block_module_path.is_file(),
        pane_block_module_bytes,
        pane_block_module_sha256: pane_block_module_metadata
            .as_ref()
            .map(|metadata| metadata.sha256.clone()),
        pane_block_module_verified,
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
        user_disk_snapshot_count: user_disk_snapshots.len(),
        latest_user_disk_snapshot: user_disk_snapshots
            .last()
            .map(|path| path.display().to_string()),
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
        } else if artifacts.base_os_bootable_disk_hint != Some(true)
            || artifacts.base_os_root_partition_hint != Some(true)
        {
            blockers.push(
                "Pane has a verified base OS image, but native Arch boot requires a raw disk with a detectable Linux root partition. Re-register it with `pane runtime --register-base-image <path> --expected-sha256 <sha256> --require-native-root-disk`."
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
        let native_storage_ready = artifacts.base_os_image_verified
            && artifacts.base_os_bootable_disk_hint == Some(true)
            && artifacts.base_os_root_partition_hint == Some(true)
            && artifacts.user_disk_ready;
        if native_storage_ready && !artifacts.kernel_boot_plan_ready {
            blockers.push(
                "No verified Linux kernel boot plan exists for the native Arch boot path. Register a complete boot set with `pane runtime --register-native-boot-set-manifest <pane-native-boot-set.json>`."
                    .to_string(),
            );
        }
        if native_storage_ready && !artifacts.initramfs_driver_bundle_ready {
            blockers.push(
                "No valid Pane initramfs driver source bundle exists for native root storage discovery. Run `pane runtime --write-initramfs-driver`."
                    .to_string(),
            );
        }
        if native_storage_ready
            && artifacts.initramfs_driver_bundle_ready
            && !artifacts.pane_block_module_verified
        {
            blockers.push(
                "No verified Pane block module exists for native root storage. Build pane-block.ko from the generated bundle against the target Arch kernel, then run `pane runtime --register-pane-block-module <path> --pane-block-module-expected-sha256 <sha256>`."
                    .to_string(),
            );
        }
        if native_storage_ready
            && artifacts.initramfs_driver_bundle_ready
            && artifacts.pane_block_module_verified
            && !artifacts.initramfs_image_verified
        {
            blockers.push(
                "No verified Pane discovery initramfs artifact exists for native root storage discovery. Run `pane runtime --build-discovery-initramfs`, or register an externally built initramfs with `pane runtime --register-initramfs <path> --initramfs-expected-sha256 <sha256>`."
                    .to_string(),
            );
        }
        if native_storage_ready
            && artifacts.initramfs_driver_bundle_ready
            && artifacts.pane_block_module_verified
            && artifacts.initramfs_image_verified
            && !artifacts.discovery_initramfs_matches_driver_bundle
        {
            blockers.push(
                "Verified discovery initramfs artifact was not packaged from the current Pane initramfs driver bundle. Rebuild it with `pane runtime --build-discovery-initramfs` before attempting native Arch boot."
                    .to_string(),
            );
        }
        if native_storage_ready
            && artifacts.kernel_boot_plan_ready
            && artifacts.initramfs_driver_bundle_ready
            && artifacts.pane_block_module_verified
            && artifacts.initramfs_image_verified
            && artifacts.discovery_initramfs_matches_driver_bundle
            && !artifacts.kernel_boot_layout_ready
        {
            blockers.push(
                "No materialized native kernel boot layout exists. Run `pane native-kernel-plan --prepare-runtime --materialize` after registering the verified boot artifacts."
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
        && artifacts.base_os_bootable_disk_hint == Some(true)
        && artifacts.base_os_root_partition_hint == Some(true)
        && artifacts.user_disk_ready
        && artifacts.serial_boot_image_ready
        && artifacts.runtime_config_exists
        && artifacts.native_manifest_exists
        && artifacts.framebuffer_contract_ready
        && artifacts.input_contract_ready
        && native_host.ready_for_boot_spike;
    let ready_for_arch_boot_attempt = prepared
        && artifacts.base_os_image_verified
        && artifacts.base_os_bootable_disk_hint == Some(true)
        && artifacts.base_os_root_partition_hint == Some(true)
        && artifacts.user_disk_ready
        && artifacts.runtime_config_exists
        && artifacts.native_manifest_exists
        && artifacts.framebuffer_contract_ready
        && artifacts.input_contract_ready
        && native_host.ready_for_boot_spike
        && artifacts.kernel_boot_plan_ready
        && artifacts.initramfs_driver_bundle_ready
        && artifacts.pane_block_module_verified
        && artifacts.initramfs_image_verified
        && artifacts.discovery_initramfs_matches_driver_bundle
        && artifacts.kernel_boot_layout_ready;

    NativeRuntimeReport {
        state,
        state_label: state.display_name(),
        bootable: false,
        host_ready: native_host.ready_for_boot_spike,
        ready_for_boot_spike,
        ready_for_arch_boot_attempt,
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
    println!(
        "  Initramfs Driver {}",
        report.directories.initramfs_driver_dir
    );
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
    println!(
        "  Driver Metadata {}",
        report.directories.initramfs_driver_metadata
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
    if let Some(format) = &report.artifacts.base_os_image_format {
        println!("  Base Format    {}", format);
    }
    if let Some(bootable) = report.artifacts.base_os_bootable_disk_hint {
        println!("  Base Boot Hint {}", yes_no(bootable));
    }
    if let Some(root_partition) = report.artifacts.base_os_root_partition_hint {
        println!("  Base Root Hint {}", yes_no(root_partition));
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
        "  Initramfs Driver {}",
        yes_no(report.artifacts.initramfs_driver_bundle_ready)
    );
    println!(
        "  Pane Block Module {}",
        yes_no(report.artifacts.pane_block_module_verified)
    );
    if let Some(sha256) = &report.artifacts.pane_block_module_sha256 {
        println!("  Pane Block Module SHA-256 {}", sha256);
    }
    if let Some(bytes) = report.artifacts.pane_block_module_bytes {
        println!("  Pane Block Module Bytes {}", bytes);
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
    println!(
        "  Disk Snapshots {}",
        report.artifacts.user_disk_snapshot_count
    );
    if let Some(snapshot) = &report.artifacts.latest_user_disk_snapshot {
        println!("  Latest Snapshot {}", snapshot);
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
    println!("VMM Foundation");
    println!(
        "  Strategy       {}",
        report.vmm_foundation.selected_strategy
    );
    println!(
        "  Reference      {}",
        report.vmm_foundation.reference_vmm.name
    );
    println!("  Boot Adapter   rust-vmm/linux-loader");
    println!("  Device Model   rust-vmm/vm-virtio + virtio semantics");
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
    println!("  Boot Spike     {}", yes_no(report.ready_for_boot_spike));
    println!(
        "  Arch Boot Try  {}",
        yes_no(report.ready_for_arch_boot_attempt)
    );
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
    println!("Device Loop");
    println!("  Strategy       {}", report.device_loop.strategy);
    println!("  Boundary       {}", report.device_loop.active_boundary);
    println!("  State          {}", report.device_loop.adoption_state);
    println!(
        "  MMIO Base      {}",
        report.device_loop.mmio_window.base_gpa
    );
    println!(
        "  MMIO Size      {}",
        report.device_loop.mmio_window.size_bytes
    );
    println!(
        "  MMIO Handshake {}",
        report.device_loop.mmio_window.handshake_smoke.status
    );
    println!(
        "  Queue Exec     {}",
        report.device_loop.mmio_window.execution_smoke.status
    );
    println!("  Devices        {}", report.device_loop.devices.len());
    println!("  Routes         {}", report.device_loop.routes.len());
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
        println!("  Loader Adapter {}", layout.linux_loader.source_crate);
        println!("  Loader State   {}", layout.linux_loader.adoption_state);
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
        if !layout.expected_serial_milestones.is_empty() {
            println!(
                "  Serial Milestones {}",
                layout.expected_serial_milestones.join(", ")
            );
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
            println!("  Base Format    {}", storage.base_os_image_format);
            println!("  Base Block     {}", storage.base_os_block_size_bytes);
            println!(
                "  Base Boot Hint {}",
                yes_no(storage.base_os_bootable_disk_hint)
            );
            if let Some(root) = &storage.base_os_root_partition_hint {
                println!("  Base Root Part {}", root.index);
                println!("  Root Offset    {}", root.byte_offset);
            }
            println!("  Root Mode      {}", storage.root_handoff.mode);
            println!("  Root Handoff   {}", storage.root_handoff.root_device);
            println!("  User Device    {}", storage.user_device);
            println!("  User Disk      {}", storage.user_disk_path);
            println!("  Contract GPA   {}", storage.contract_gpa);
            println!("  User Disk GiB  {}", storage.user_disk_capacity_gib);
            println!("  Block Size     {}", storage.user_disk_block_size_bytes);
            println!(
                "  Sparse Backing {}",
                yes_no(storage.user_disk_sparse_backing)
            );
            println!("  Virtio Backend {}", storage.virtio_block.backend_kind);
            println!("  Virtio State   {}", storage.virtio_block.adoption_state);
            println!("  Virtio Root    {}", storage.virtio_block.root_device_hint);
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
            println!("  Queue Header   {}", input.queue_header_bytes);
            println!("  Queue Magic    {}", input.queue_magic);
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

fn print_native_foundation_report(report: &crate::vmm_foundation::VmmFoundationReport) {
    println!("Pane Native Foundation");
    println!("  Strategy       {}", report.selected_strategy);
    println!("  Rule           {}", report.implementation_rule);
    println!("Reference VMM");
    println!(
        "  {} ({})",
        report.reference_vmm.name, report.reference_vmm.license
    );
    println!("  {}", report.reference_vmm.role);
    println!("Adopted Components");
    for component in &report.adopted_crates {
        println!("  - {} ({})", component.name, component.license);
        println!("    {}", component.role);
    }
    println!("Rejected Paths");
    for path in &report.rejected_paths {
        println!("  - {}: {}", path.name, path.reason);
    }
    println!("Migration Milestones");
    for milestone in &report.migration_milestones {
        println!("  - {}: {}", milestone.id, milestone.title);
        println!("    {}", milestone.objective);
        println!("    gate: {}", milestone.acceptance_gate);
    }
    println!("Immediate Next Steps");
    for step in &report.immediate_next_steps {
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
    println!(
        "  Arch Boot Try  {}",
        yes_no(report.ready_for_arch_boot_attempt)
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
        "  Arch Ready     {}",
        yes_no(report.runtime.native_runtime.ready_for_arch_boot_attempt)
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
    if report.partition_smoke.guest_exit_budget > 0 {
        println!(
            "  Guest Exits    {}/{}",
            report.partition_smoke.guest_exit_count, report.partition_smoke.guest_exit_budget
        );
    }
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
    if !report.partition_smoke.serial_expected_markers.is_empty() {
        println!(
            "  Serial Markers {}",
            report.partition_smoke.serial_expected_markers.join(", ")
        );
        println!(
            "  Markers Seen   {}",
            yes_no(report.partition_smoke.serial_markers_observed)
        );
    }
    if let Some(text) = &report.partition_smoke.serial_text {
        println!("  Serial Text    {:?}", text);
    }
    println!("Device Loop");
    println!("  Strategy       {}", report.device_loop.strategy);
    println!("  Boundary       {}", report.device_loop.active_boundary);
    println!("  State          {}", report.device_loop.adoption_state);
    println!(
        "  MMIO Base      {}",
        report.device_loop.mmio_window.base_gpa
    );
    println!(
        "  MMIO Size      {}",
        report.device_loop.mmio_window.size_bytes
    );
    println!(
        "  MMIO Handshake {}",
        report.device_loop.mmio_window.handshake_smoke.status
    );
    println!(
        "  Queue Exec     {}",
        report.device_loop.mmio_window.execution_smoke.status
    );
    println!("  Devices        {}", report.device_loop.devices.len());
    for device in &report.device_loop.devices {
        println!(
            "  Device         {} [{}] {}",
            device.id, device.status, device.role
        );
    }
    println!("  Routes         {}", report.device_loop.routes.len());
    for route in &report.device_loop.routes {
        println!(
            "  Route          {} {} -> {}",
            route.exit_reason, route.selector, route.handler
        );
    }
    if let Some(snapshot) = &report.partition_smoke.framebuffer_snapshot {
        println!("  Framebuffer    {}", snapshot.label);
        println!("  FB Guest GPA   {}", snapshot.guest_gpa);
        println!("  FB Bytes       {}", snapshot.bytes);
        println!("  FB Nonzero     {}", snapshot.nonzero_bytes);
        println!("  FB All Zero    {}", yes_no(snapshot.all_zero));
    }
    if let Some(snapshot) = &report.partition_smoke.input_queue_snapshot {
        println!("  Input Queue    {}", snapshot.label);
        println!("  Input GPA      {}", snapshot.guest_gpa);
        println!("  Input Bytes    {}", snapshot.bytes);
        println!("  Input Nonzero  {}", snapshot.nonzero_bytes);
        println!("  Input All Zero {}", yes_no(snapshot.all_zero));
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
    use std::path::PathBuf;

    use crate::{
        cli::{InitArgs, ResetArgs, RuntimeArgs},
        model::{DesktopEnvironment, DistroFamily, DistroRecord},
        native::test_native_host_report,
        plan::{LaunchPlan, RuntimePaths, WorkspacePaths, DEFAULT_RUNTIME_CAPACITY_GIB},
        state::{
            LaunchStage, LaunchTransport, ManagedEnvironmentOwnership, ManagedEnvironmentState,
            StoredLaunch,
        },
    };

    use super::{
        build_and_register_pane_block_module, build_bundle_doctor_request, build_distro_health,
        build_environment_catalog_report, build_kernel_boot_layout, build_linux_boot_params_page,
        build_native_runtime_report, build_runtime_artifact_report, build_steps,
        build_update_steps, create_serial_boot_image_artifact, create_user_disk_descriptor,
        create_user_disk_snapshot, default_framebuffer_contract, default_input_contract,
        default_linux_guest_memory_map, determine_app_lifecycle, ensure_wsl_conf_setting,
        execute_native_block_io_command, export_user_disk_package, format_doctor_blockers,
        import_user_disk_package, initialize_managed_arch_environment, input_queue_page_bytes,
        inspect_kernel_image_artifact, inspect_workspace, inventory_contains_distro,
        kernel_layout_execution_image, legacy_linux_loader_adapter_plan,
        legacy_virtio_block_backend_plan, linux_guest_mapped_regions, linux_loader_adapter_plan,
        load_kernel_layout_boot_image_artifact, load_verified_pane_block_module_metadata,
        pane_block_device_blocks, pane_block_driver_abi_sha256,
        pane_initramfs_expected_serial_milestones, parse_guest_physical_address,
        preferred_transport, read_base_os_block, read_json_file, read_user_disk_block,
        register_base_os_image, register_boot_loader_image, register_kernel_boot_plan,
        register_native_boot_set_artifacts, register_native_boot_set_from_manifest,
        register_pane_block_module, register_pane_discovery_initramfs_artifact,
        register_virtio_mmio_module, repair_user_disk_metadata, resize_user_disk,
        resolve_bundle_output_path,
        resolve_init_source, resolve_launch_target, resolve_managed_environment_for_reset,
        resolve_saved_launch, resolve_session_context, resolve_status_distro,
        restore_user_disk_snapshot, runtime_contract_guest_memory_ranges, runtime_storage_budget,
        sha256_file, status_port_for, user_disk_artifact_ready, validate_setup_password,
        validate_setup_username, virtio_mmio_module_metadata_path, virtio_mmio_module_path,
        windows_transport_check, write_json_file, write_native_boot_set_manifest_template,
        write_pane_initramfs_driver_bundle, write_user_disk_block, AppLifecyclePhase,
        AppNextAction, BaseOsImageMetadata, CheckStatus,
        DistroHealth, DoctorCheck, DoctorReport, FramebufferContract, InitSource, KernelBootLayout,
        KernelBootMetadata, NativeRuntimeState, PaneBlockModuleMetadata,
        PaneInitramfsDriverMetadata, StatusReport, UserDiskExportManifest, UserDiskMetadata,
        UserDiskSnapshotMetadata, VirtioMmioModuleMetadata, WorkspaceHealth, WslInventory,
        COMPATIBLE_PANE_BLOCK_DRIVER_SOURCE_SHA256_BY_ABI, EMBEDDED_APP_ASSETS,
        PANE_USER_DISK_EXPORT_DISK_FILENAME, PANE_USER_DISK_EXPORT_MANIFEST_FILENAME,
        PANE_USER_DISK_EXPORT_METADATA_FILENAME,
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
            initramfs_driver_dir: engines.join("pane-initramfs-driver"),
            user_disk: disks.join("user-data.panedisk"),
            base_os_metadata: state.join("base-os-image.json"),
            serial_boot_metadata: state.join("serial-boot-image.json"),
            boot_loader_metadata: state.join("boot-to-serial-loader.json"),
            kernel_boot_metadata: state.join("kernel-boot.json"),
            initramfs_driver_metadata: state.join("pane-initramfs-driver.json"),
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

    fn default_runtime_args() -> RuntimeArgs {
        RuntimeArgs {
            session_name: "pane".to_string(),
            capacity_gib: DEFAULT_RUNTIME_CAPACITY_GIB,
            prepare: false,
            register_base_image: None,
            expected_sha256: None,
            require_native_root_disk: false,
            register_native_boot_set: false,
            register_native_boot_set_manifest: None,
            write_native_boot_set_manifest_template: None,
            register_boot_loader: None,
            boot_loader_expected_sha256: None,
            boot_loader_expected_serial: None,
            register_kernel: None,
            kernel_expected_sha256: None,
            register_initramfs: None,
            initramfs_expected_sha256: None,
            kernel_cmdline: None,
            write_initramfs_driver: false,
            build_discovery_initramfs: false,
            discovery_init_binary: None,
            discovery_probe_binary: None,
            build_pane_block_module: false,
            kernel_build_dir: None,
            register_pane_block_module: None,
            pane_block_module_expected_sha256: None,
            register_virtio_mmio_module: None,
            virtio_mmio_module_expected_sha256: None,
            create_user_disk: false,
            snapshot_user_disk: false,
            restore_user_disk_snapshot: None,
            export_user_disk: None,
            import_user_disk: None,
            resize_user_disk_gib: None,
            repair_user_disk: false,
            create_serial_boot_image: false,
            force: false,
            json: false,
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

    fn fake_mbr_linux_root_disk_image() -> Vec<u8> {
        let mut image = vec![0_u8; 4 * 1024 * 1024];
        image[446] = 0x80;
        image[450] = 0x83;
        image[454..458].copy_from_slice(&2048_u32.to_le_bytes());
        image[458..462].copy_from_slice(&4096_u32.to_le_bytes());
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        let root_offset = 2048 * 512;
        image[root_offset + 0x438..root_offset + 0x43a].copy_from_slice(&[0x53, 0xef]);
        image
    }

    fn register_fake_discovery_initramfs(paths: &RuntimePaths) {
        let discovery_initramfs = paths
            .initramfs_driver_dir
            .join("pane-storage-discovery.cpio");
        std::fs::create_dir_all(&paths.initramfs_driver_dir).unwrap();
        std::fs::write(
            &discovery_initramfs,
            b"PANE_INITRAMFS_DISCOVERY_START\nPANE_BLOCK_IO_PROBE_OK\nPANE_INITRAMFS_DISCOVERY_DONE\n",
        )
        .unwrap();
        register_pane_discovery_initramfs_artifact(paths, &discovery_initramfs, false).unwrap();
        super::record_pane_discovery_initramfs_package_metadata(
            paths,
            &super::DiscoveryInitramfsBuildOutput {
                init_binary_sha256: "fake-init-binary-sha256".to_string(),
                probe_binary_sha256: "fake-probe-binary-sha256".to_string(),
                compiled_from_current_source: true,
            },
        )
        .unwrap();
    }

    fn register_fake_pane_block_module(paths: &RuntimePaths) {
        let module = paths.downloads.join("pane-block.ko");
        std::fs::write(&module, b"fake pane block kernel module").unwrap();
        let module_sha = sha256_file(&module).unwrap();
        register_pane_block_module(paths, &module, Some(&module_sha), false).unwrap();
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
    fn runtime_artifacts_reject_unsupported_framebuffer_format() {
        let paths = temp_runtime_paths("unsupported-framebuffer-format");
        super::prepare_runtime_paths(&paths).unwrap();
        let mut framebuffer = default_framebuffer_contract();
        framebuffer.format = "r5g6b5".to_string();
        write_json_file(&paths.framebuffer_contract, &framebuffer).unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.framebuffer_contract_exists);
        assert!(!artifacts.framebuffer_contract_ready);

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
        assert_eq!(layout.linux_loader.source_crate, "rust-vmm/linux-loader");
        assert_eq!(
            layout.linux_loader.candidate_crate_version.as_deref(),
            Some("0.13.2")
        );
        assert_eq!(
            layout.linux_loader.adoption_state,
            "linked-linux-loader-0.13.2"
        );
        assert!(layout.linux_loader.applicable);
        assert_eq!(layout.boot_params_gpa, "0x00007000");
        assert_eq!(layout.cmdline_gpa, "0x00020000");
        assert_eq!(layout.kernel_load_gpa, "0x00100000");
        assert_eq!(layout.initramfs_load_gpa.as_deref(), Some("0x0c000000"));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "bios-data-area"
                && range.start_gpa == "0x00000000"
                && range.size_bytes == 0x00001000
                && range.region_type == "bios-data"
        }));
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
            range.label == "bios-rom"
                && range.start_gpa == "0x000e0000"
                && range.size_bytes == 0x00020000
                && range.region_type == "bios-rom"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "legacy-rom"
                && range.start_gpa == "0x000a0000"
                && range.size_bytes == 0x00040000
                && range.region_type == "legacy-rom"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "kernel-work-ram"
                && range.start_gpa == "0x02100000"
                && range.size_bytes == 0x05f00000
                && range.region_type == "usable"
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
                && range.start_gpa == "0x0c000000"
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

        let mut stale_layout = layout;
        stale_layout.linux_loader = legacy_linux_loader_adapter_plan();
        write_json_file(&paths.kernel_boot_layout, &stale_layout).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.kernel_boot_layout_exists);
        assert!(!artifacts.kernel_boot_layout_ready);

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
                && region.guest_gpa == 0x0c00_0000
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
            linux_loader: linux_loader_adapter_plan("linux-bzimage", Some("0x020f")),
            boot_params_gpa: "0x00007000".to_string(),
            cmdline_gpa: "0x00020000".to_string(),
            kernel_load_gpa: "0x00100000".to_string(),
            initramfs_load_gpa: Some("0x0c000000".to_string()),
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
            expected_serial_milestones: Vec::new(),
            storage: None,
            initramfs_driver: None,
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
        assert_eq!(page[0x0f], 0x23);
        assert_eq!(&page[0x12..0x14], &1024_u16.to_le_bytes());
        assert_eq!(&page[0x14..0x16], &768_u16.to_le_bytes());
        assert_eq!(&page[0x16..0x18], &32_u16.to_le_bytes());
        assert_eq!(&page[0x18..0x1c], &0x0e00_0000_u32.to_le_bytes());
        assert_eq!(&page[0x1c..0x20], &0x0030_0000_u32.to_le_bytes());
        assert_eq!(&page[0x24..0x26], &4096_u16.to_le_bytes());
        assert_eq!(page[0x26], 8);
        assert_eq!(page[0x27], 16);
        assert_eq!(page[0x28], 8);
        assert_eq!(page[0x29], 8);
        assert_eq!(page[0x2a], 8);
        assert_eq!(page[0x2b], 0);
        assert_eq!(page[0x2c], 8);
        assert_eq!(page[0x2d], 24);
        assert_eq!(&page[0x228..0x22c], &0x0002_0000_u32.to_le_bytes());
        assert_eq!(&page[0x218..0x21c], &0x0c00_0000_u32.to_le_bytes());
        assert_eq!(&page[0x21c..0x220], &1234_u32.to_le_bytes());
        assert_eq!(page[0x1e8], 14);
        assert_eq!(&page[0x2d0..0x2d8], &0_u64.to_le_bytes());
        assert_eq!(&page[0x2d0 + 16..0x2d0 + 20], &2_u32.to_le_bytes());
        let boot_params_offset = 0x2d0 + 1 * 20;
        assert_eq!(
            &page[boot_params_offset..boot_params_offset + 8],
            &0x0000_7000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[boot_params_offset + 16..boot_params_offset + 20],
            &2_u32.to_le_bytes()
        );
        let boot_gdt_offset = 0x2d0 + 2 * 20;
        assert_eq!(
            &page[boot_gdt_offset..boot_gdt_offset + 8],
            &0x0000_8000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[boot_gdt_offset + 16..boot_gdt_offset + 20],
            &2_u32.to_le_bytes()
        );
        let work_ram_offset = 0x2d0 + 9 * 20;
        assert_eq!(
            &page[work_ram_offset..work_ram_offset + 8],
            &0x0210_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[work_ram_offset + 16..work_ram_offset + 20],
            &1_u32.to_le_bytes()
        );
        let initramfs_offset = 0x2d0 + 10 * 20;
        assert_eq!(
            &page[initramfs_offset..initramfs_offset + 8],
            &0x0c00_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[initramfs_offset + 8..initramfs_offset + 16],
            &0x0000_1000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[initramfs_offset + 16..initramfs_offset + 20],
            &2_u32.to_le_bytes()
        );
        let high_ram_offset = 0x2d0 + 11 * 20;
        assert_eq!(
            &page[high_ram_offset..high_ram_offset + 8],
            &0x0800_0000_u64.to_le_bytes()
        );
        assert_eq!(
            &page[high_ram_offset + 16..high_ram_offset + 20],
            &1_u32.to_le_bytes()
        );
        let local_apic_offset = 0x2d0 + 13 * 20;
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
    fn linux_boot_params_rejects_unsupported_framebuffer_format() {
        let mut framebuffer = default_framebuffer_contract();
        framebuffer.format = "r5g6b5".to_string();
        let layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            linux_loader: linux_loader_adapter_plan("linux-bzimage", Some("0x020f")),
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
            expected_serial_milestones: Vec::new(),
            storage: None,
            initramfs_driver: None,
            framebuffer: Some(framebuffer),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };

        let kernel = fake_linux_bzimage();
        let error = build_linux_boot_params_page(&layout, Some(&kernel)).unwrap_err();
        assert!(error
            .to_string()
            .contains("cannot be advertised through Linux screen_info"));
    }

    #[test]
    fn linux_bzimage_execution_image_splits_setup_and_payload() {
        let mut layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            linux_loader: linux_loader_adapter_plan("linux-bzimage", Some("0x020f")),
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
            expected_serial_milestones: Vec::new(),
            storage: None,
            initramfs_driver: None,
            framebuffer: Some(default_framebuffer_contract()),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };
        let kernel = fake_linux_bzimage();

        let (payload, entry_gpa, extra_regions) =
            kernel_layout_execution_image(&layout, &kernel).unwrap();
        assert_eq!(entry_gpa, 0x0010_0000);
        assert_eq!(payload.len(), 0x0200_0000);
        assert_eq!(&payload[..kernel.len() - 2560], &kernel[2560..]);
        assert!(payload[kernel.len() - 2560..].iter().all(|byte| *byte == 0));
        assert!(extra_regions.iter().any(|region| {
            region.label == "linux-bzimage-setup"
                && region.guest_gpa == 0x0009_0000
                && region.bytes.len() == 0x0001_0000
                && region.bytes[..2560] == kernel[..2560]
                && region.bytes[2560..].iter().all(|byte| *byte == 0)
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
        let mut guest_memory_map = default_linux_guest_memory_map(1234);
        guest_memory_map
            .extend(runtime_contract_guest_memory_ranges(&framebuffer, &input).unwrap());
        let layout = KernelBootLayout {
            schema_version: 1,
            layout_kind: "pane-linux-kernel-boot-layout-v1".to_string(),
            session_name: "pane".to_string(),
            linux_loader: linux_loader_adapter_plan("linux-bzimage", Some("0x020f")),
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
            expected_serial_milestones: Vec::new(),
            storage: None,
            initramfs_driver: None,
            framebuffer: Some(default_framebuffer_contract()),
            input: Some(default_input_contract()),
            materialized_at_epoch_seconds: Some(1),
            notes: Vec::new(),
        };

        let regions = linux_guest_mapped_regions(&layout).unwrap();
        let bios_data = regions
            .iter()
            .find(|region| region.label == "linux-bios-data-area")
            .expect("bios data area mapping");
        assert_eq!(bios_data.guest_gpa, 0x0000_0000);
        assert_eq!(&bios_data.bytes[0x400..0x402], &0x03f8_u16.to_le_bytes());
        assert_eq!(&bios_data.bytes[0x40e..0x410], &0_u16.to_le_bytes());
        assert_eq!(&bios_data.bytes[0x413..0x415], &640_u16.to_le_bytes());
        assert!(bios_data.writable);
        assert!(!bios_data.executable);
        assert!(regions.iter().any(|region| {
            region.label == "linux-bios-rom"
                && region.guest_gpa == 0x000e_0000
                && region.bytes.len() == 0x0002_0000
                && region.writable
                && !region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "linux-legacy-rom"
                && region.guest_gpa == 0x000a_0000
                && region.bytes.len() == 0x0004_0000
                && region.writable
                && !region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "linux-ram-kernel-work-ram"
                && region.guest_gpa == 0x0210_0000
                && region.bytes.len() == 0x05f0_0000
                && region.writable
                && region.executable
        }));
        assert!(regions.iter().any(|region| {
            region.label == "linux-ram-high-ram"
                && region.guest_gpa == 0x0800_0000
                && region.writable
                && region.executable
        }));
        assert!(!regions
            .iter()
            .any(|region| region.label == "linux-local-apic-mmio"));
        assert!(!regions
            .iter()
            .any(|region| region.label == "linux-io-apic-mmio"));
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
                && &region.bytes[0..8] == b"PANEINQ1"
                && &region.bytes[8..12] == &1_u32.to_le_bytes()
                && &region.bytes[12..16] == &64_u32.to_le_bytes()
                && &region.bytes[16..24] == &0x1000_u64.to_le_bytes()
                && &region.bytes[24..28] == &32_u32.to_le_bytes()
                && region.writable
                && !region.executable
        }));
    }

    #[test]
    fn input_queue_page_bytes_writes_stable_abi_header() {
        let input = default_input_contract();
        let page = input_queue_page_bytes(&input, input.queue_size_bytes as usize).unwrap();

        assert_eq!(&page[0..8], b"PANEINQ1");
        assert_eq!(
            u32::from_le_bytes(page[8..12].try_into().unwrap()),
            input.schema_version
        );
        assert_eq!(
            u32::from_le_bytes(page[12..16].try_into().unwrap()),
            input.queue_header_bytes
        );
        assert_eq!(
            u64::from_le_bytes(page[16..24].try_into().unwrap()),
            input.queue_size_bytes
        );
        assert_eq!(
            u32::from_le_bytes(page[24..28].try_into().unwrap()),
            input.event_record_bytes
        );
        assert_eq!(u64::from_le_bytes(page[32..40].try_into().unwrap()), 0);
        assert_eq!(u64::from_le_bytes(page[40..48].try_into().unwrap()), 0);
        assert_eq!(u64::from_le_bytes(page[48..56].try_into().unwrap()), 126);
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
        std::fs::write(&source, fake_mbr_linux_root_disk_image()).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), true, false).unwrap();
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
        assert_eq!(
            artifacts.base_os_image_format.as_deref(),
            Some("raw-mbr-disk")
        );
        assert_eq!(artifacts.base_os_bootable_disk_hint, Some(true));
        assert_eq!(artifacts.base_os_root_partition_hint, Some(true));
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
        assert!(!native.ready_for_arch_boot_attempt);
        assert!(native
            .blockers
            .iter()
            .any(|blocker| blocker.contains("No verified Linux kernel boot plan")));
        assert!(!native
            .blockers
            .iter()
            .any(|blocker| blocker.contains("No valid Pane-owned user disk")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn base_os_registration_records_raw_disk_boot_hint() {
        let paths = temp_runtime_paths("runtime-base-image-gpt");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-gpt.img");
        let mut image = vec![0_u8; 4096];
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        image[512..520].copy_from_slice(b"EFI PART");
        std::fs::write(&source, image).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), false, false).unwrap();

        let metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert_eq!(metadata.image_format, "raw-gpt-disk");
        assert!(metadata.bootable_disk_hint);
        assert_eq!(
            artifacts.base_os_image_format.as_deref(),
            Some("raw-gpt-disk")
        );
        assert_eq!(artifacts.base_os_bootable_disk_hint, Some(true));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn base_os_registration_records_linux_root_partition_hint() {
        let paths = temp_runtime_paths("runtime-base-image-mbr-root");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-mbr.img");
        std::fs::write(&source, fake_mbr_linux_root_disk_image()).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), false, false).unwrap();

        let metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();
        assert_eq!(metadata.image_format, "raw-mbr-disk");
        assert_eq!(metadata.partitions.len(), 1);
        let root = metadata.root_partition_hint.expect("root partition hint");
        assert_eq!(root.index, 1);
        assert_eq!(root.partition_type, "0x83");
        assert!(root.bootable);
        assert_eq!(root.byte_offset, 2048 * 512);
        assert_eq!(root.byte_length, 4096 * 512);
        assert!(root.root_candidate);
        assert_eq!(metadata.root_filesystem_hint.as_deref(), Some("ext4"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn base_os_registration_accepts_required_native_root_disk() {
        let paths = temp_runtime_paths("runtime-base-image-required-root");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-root.img");
        std::fs::write(&source, fake_mbr_linux_root_disk_image()).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), true, false).unwrap();

        let metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();
        assert_eq!(metadata.image_format, "raw-mbr-disk");
        assert!(metadata.bootable_disk_hint);
        assert!(metadata.root_partition_hint.is_some());
        assert!(paths.base_os_image.is_file());

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn base_os_registration_rejects_required_native_root_disk_without_mutating_store() {
        let paths = temp_runtime_paths("runtime-base-image-required-reject");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-rootfs.tar");
        std::fs::write(&source, b"not a bootable disk").unwrap();
        let expected = sha256_file(&source).unwrap();

        let error = register_base_os_image(&paths, &source, Some(&expected), true, false)
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires a bootable raw disk image"));
        assert!(!paths.base_os_image.exists());
        assert!(!paths.base_os_metadata.exists());

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_native_arch_boot_set_artifacts() {
        let paths = temp_runtime_paths("runtime-native-boot-set");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-root.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        std::fs::write(&base, fake_mbr_linux_root_disk_image()).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(&initramfs, b"fake pane discovery initramfs").unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();

        register_native_boot_set_artifacts(
            &paths,
            &base,
            &base_sha,
            &kernel,
            &kernel_sha,
            &initramfs,
            &initramfs_sha,
            "console=ttyS0 earlyprintk=serial panic=-1",
            false,
        )
        .unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.base_os_image_verified);
        assert_eq!(artifacts.base_os_bootable_disk_hint, Some(true));
        assert_eq!(artifacts.base_os_root_partition_hint, Some(true));
        assert!(artifacts.kernel_image_verified);
        assert!(artifacts.initramfs_image_verified);
        assert!(artifacts.kernel_boot_plan_ready);

        let metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).unwrap();
        assert_eq!(metadata.kernel_format, "linux-bzimage");
        assert_eq!(
            metadata.initramfs_sha256.as_deref(),
            Some(initramfs_sha.as_str())
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_native_arch_boot_set_from_manifest() {
        let paths = temp_runtime_paths("runtime-native-boot-set-manifest");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-root.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        let manifest = paths.downloads.join("pane-native-boot-set.json");
        std::fs::write(&base, fake_mbr_linux_root_disk_image()).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(&initramfs, b"fake pane discovery initramfs").unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();
        std::fs::write(
            &manifest,
            format!(
                r#"{{
  "schema_version": 1,
  "distro_family": "arch",
  "base_image": "arch-root.img",
  "base_image_sha256": "{base_sha}",
  "kernel": "vmlinuz-linux",
  "kernel_sha256": "{kernel_sha}",
  "initramfs": "initramfs-linux.img",
  "initramfs_sha256": "{initramfs_sha}",
  "kernel_cmdline": "console=ttyS0 earlyprintk=serial panic=-1"
}}"#
            ),
        )
        .unwrap();
        let args = default_runtime_args();

        register_native_boot_set_from_manifest(&paths, &manifest, &args).unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.base_os_image_verified);
        assert_eq!(artifacts.base_os_root_partition_hint, Some(true));
        assert!(artifacts.kernel_image_verified);
        assert!(artifacts.initramfs_image_verified);
        assert!(artifacts.kernel_boot_plan_ready);

        let metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).unwrap();
        assert_eq!(metadata.kernel_format, "linux-bzimage");
        assert_eq!(
            metadata.initramfs_sha256.as_deref(),
            Some(initramfs_sha.as_str())
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn native_arch_boot_set_manifest_rejects_mixed_explicit_flags() {
        let paths = temp_runtime_paths("runtime-native-boot-set-manifest-conflict");
        super::prepare_runtime_paths(&paths).unwrap();
        let manifest = paths.downloads.join("pane-native-boot-set.json");
        std::fs::write(
            &manifest,
            r#"{
  "schema_version": 1,
  "distro_family": "arch",
  "base_image": "arch-root.img",
  "base_image_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "kernel": "vmlinuz-linux",
  "kernel_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "initramfs": "initramfs-linux.img",
  "initramfs_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
  "kernel_cmdline": "console=ttyS0 panic=-1"
}"#,
        )
        .unwrap();
        let mut args = default_runtime_args();
        args.register_kernel = Some(paths.downloads.join("vmlinuz-linux"));

        let error = register_native_boot_set_from_manifest(&paths, &manifest, &args)
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot be combined"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn writes_native_boot_set_manifest_template_without_overwriting_by_default() {
        let paths = temp_runtime_paths("runtime-native-boot-set-template");
        let template = paths.downloads.join("pane-native-boot-set.json");

        write_native_boot_set_manifest_template(&template, false).unwrap();

        let manifest = read_json_file::<serde_json::Value>(&template).unwrap();
        assert_eq!(manifest["schema_version"], serde_json::json!(1));
        assert_eq!(manifest["distro_family"], serde_json::json!("arch"));
        assert_eq!(
            manifest["base_image"],
            serde_json::json!("artifacts/arch-root.img")
        );
        assert_eq!(
            manifest["kernel_cmdline"],
            serde_json::json!("console=ttyS0 panic=-1")
        );

        let error = write_native_boot_set_manifest_template(&template, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--force"));

        write_native_boot_set_manifest_template(&template, true).unwrap();

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn native_arch_boot_set_rejects_bad_root_disk_before_mutating_store() {
        let paths = temp_runtime_paths("runtime-native-boot-set-reject");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-rootfs.tar");
        let kernel = paths.downloads.join("vmlinuz-linux");
        let initramfs = paths.downloads.join("initramfs-linux.img");
        std::fs::write(&base, b"not a raw disk").unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(&initramfs, b"fake pane discovery initramfs").unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let initramfs_sha = sha256_file(&initramfs).unwrap();

        let error = register_native_boot_set_artifacts(
            &paths,
            &base,
            &base_sha,
            &kernel,
            &kernel_sha,
            &initramfs,
            &initramfs_sha,
            "console=ttyS0 earlyprintk=serial panic=-1",
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("requires a bootable raw disk image"));
        assert!(!paths.base_os_image.exists());
        assert!(!paths.kernel_image.exists());
        assert!(!paths.initramfs_image.exists());
        assert!(!paths.base_os_metadata.exists());
        assert!(!paths.kernel_boot_metadata.exists());

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn base_os_registration_records_gpt_linux_root_partition_hint() {
        let paths = temp_runtime_paths("runtime-base-image-gpt-root");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-gpt.img");
        let mut image = vec![0_u8; 4096 * 512];
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        image[512..520].copy_from_slice(b"EFI PART");
        image[512 + 72..512 + 80].copy_from_slice(&2_u64.to_le_bytes());
        image[512 + 80..512 + 84].copy_from_slice(&1_u32.to_le_bytes());
        image[512 + 84..512 + 88].copy_from_slice(&128_u32.to_le_bytes());
        let entry = 1024;
        image[entry..entry + 16].copy_from_slice(&[
            0xe3, 0xbc, 0x68, 0x4f, 0xcd, 0xe8, 0xb1, 0x4d, 0x96, 0xe7, 0xfb, 0xca, 0xf9, 0x84,
            0xb7, 0x09,
        ]);
        image[entry + 32..entry + 40].copy_from_slice(&2048_u64.to_le_bytes());
        image[entry + 40..entry + 48].copy_from_slice(&4095_u64.to_le_bytes());
        std::fs::write(&source, image).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), false, false).unwrap();

        let metadata = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();
        assert_eq!(metadata.image_format, "raw-gpt-disk");
        assert_eq!(metadata.partitions.len(), 1);
        let root = metadata.root_partition_hint.expect("root partition hint");
        assert_eq!(root.index, 1);
        assert_eq!(root.partition_type, "linux-root-x86_64");
        assert_eq!(root.byte_offset, 2048 * 512);
        assert_eq!(root.byte_length, 2048 * 512);
        assert!(root.root_candidate);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn read_base_os_block_zero_fills_tail_and_beyond_eof() {
        let paths = temp_runtime_paths("runtime-base-image-block-read");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch-base.img");
        let mut image = vec![0_u8; 5000];
        image[..16].copy_from_slice(b"PANE_BASE_BLOCK_");
        image[4096..4112].copy_from_slice(b"PANE_TAIL_BLOCK_");
        std::fs::write(&source, image).unwrap();
        let expected = sha256_file(&source).unwrap();
        register_base_os_image(&paths, &source, Some(&expected), false, false).unwrap();

        let first = read_base_os_block(&paths, 0).unwrap();
        let second = read_base_os_block(&paths, 1).unwrap();
        let beyond = read_base_os_block(&paths, 2).unwrap();

        assert_eq!(first.len(), 4096);
        assert_eq!(&first[..16], b"PANE_BASE_BLOCK_");
        assert_eq!(second.len(), 4096);
        assert_eq!(&second[..16], b"PANE_TAIL_BLOCK_");
        assert!(second[904..].iter().all(|byte| *byte == 0));
        assert!(beyond.iter().all(|byte| *byte == 0));

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
    fn sparse_user_disk_block_io_zero_fills_and_persists_blocks() {
        let paths = temp_runtime_paths("runtime-user-disk-block-io");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();

        let zero_block = read_user_disk_block(&paths, &metadata, 2).unwrap();
        assert_eq!(zero_block.len(), 4096);
        assert!(zero_block.iter().all(|byte| *byte == 0));

        let mut written = vec![0_u8; 4096];
        written[..16].copy_from_slice(b"PANE_BLOCK_IO_V1");
        write_user_disk_block(&paths, &metadata, 2, &written).unwrap();

        let read_back = read_user_disk_block(&paths, &metadata, 2).unwrap();
        assert_eq!(read_back, written);
        let still_zero = read_user_disk_block(&paths, &metadata, 3).unwrap();
        assert!(still_zero.iter().all(|byte| *byte == 0));
        assert!(user_disk_artifact_ready(&paths, &Some(metadata)));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn native_block_io_adapter_routes_base_reads_and_user_writes() {
        let paths = temp_runtime_paths("runtime-native-block-io-adapter");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-base.img");
        let mut image = vec![0_u8; 5000];
        image[..16].copy_from_slice(b"PANE_BASE_ADAPT_");
        image[4096..4112].copy_from_slice(b"PANE_BASE_4096__");
        std::fs::write(&base, image).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();

        let base_read = execute_native_block_io_command(
            &paths,
            &crate::native::NativeBlockIoCommand {
                device: crate::native::NativeBlockDeviceId::BaseOs,
                operation: crate::native::NativeBlockOperation::Read,
                block_index: 0,
                block_size_bytes: crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            },
            None,
        )
        .unwrap();
        assert!(base_read.decision.allowed);
        assert_eq!(&base_read.bytes[..16], b"PANE_BASE_ADAPT_");

        let base_second_sector = execute_native_block_io_command(
            &paths,
            &crate::native::NativeBlockIoCommand {
                device: crate::native::NativeBlockDeviceId::BaseOs,
                operation: crate::native::NativeBlockOperation::Read,
                block_index: 1,
                block_size_bytes: crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            },
            None,
        )
        .unwrap();
        assert_eq!(&base_second_sector.bytes[..16], b"PANE_BASE_4096__");

        let denied_base_write = execute_native_block_io_command(
            &paths,
            &crate::native::NativeBlockIoCommand {
                device: crate::native::NativeBlockDeviceId::BaseOs,
                operation: crate::native::NativeBlockOperation::Write,
                block_index: 0,
                block_size_bytes: crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            },
            Some(&vec![
                1_u8;
                crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize
            ]),
        )
        .unwrap();
        assert!(!denied_base_write.decision.allowed);
        assert_eq!(denied_base_write.decision.status, "readonly-device");

        let payload = vec![0x5a_u8; crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize];
        let user_write = execute_native_block_io_command(
            &paths,
            &crate::native::NativeBlockIoCommand {
                device: crate::native::NativeBlockDeviceId::UserDisk,
                operation: crate::native::NativeBlockOperation::Write,
                block_index: 3,
                block_size_bytes: crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            },
            Some(&payload),
        )
        .unwrap();
        assert!(user_write.decision.allowed);
        assert!(user_write.bytes.is_empty());

        let user_read = execute_native_block_io_command(
            &paths,
            &crate::native::NativeBlockIoCommand {
                device: crate::native::NativeBlockDeviceId::UserDisk,
                operation: crate::native::NativeBlockOperation::Read,
                block_index: 3,
                block_size_bytes: crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            },
            None,
        )
        .unwrap();
        assert_eq!(user_read.bytes, payload);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn pane_block_device_blocks_rounds_up_to_io_blocks() {
        assert_eq!(pane_block_device_blocks(0, 4096).unwrap(), 0);
        assert_eq!(pane_block_device_blocks(1, 4096).unwrap(), 1);
        assert_eq!(pane_block_device_blocks(4096, 4096).unwrap(), 1);
        assert_eq!(pane_block_device_blocks(4097, 4096).unwrap(), 2);
        assert!(pane_block_device_blocks(4096, 0).is_err());
    }

    #[test]
    fn writes_pane_initramfs_driver_source_bundle() {
        let paths = temp_runtime_paths("runtime-pane-initramfs-driver");
        super::prepare_runtime_paths(&paths).unwrap();

        let metadata = write_pane_initramfs_driver_bundle(&paths).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);

        assert!(super::pane_initramfs_driver_metadata_matches_current_sources(&metadata));
        assert_eq!(metadata.bundle_kind, "pane-initramfs-driver-source-v1");
        assert_eq!(metadata.block_io_protocol, "pane-port-block-v1");
        assert_eq!(metadata.block_io_port_base, "0x0d00");
        assert_eq!(metadata.block_io_port_count, 16);
        assert_eq!(
            metadata.block_io_block_size_bytes,
            u64::from(crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES)
        );
        assert_eq!(
            metadata.block_driver_abi_sha256,
            pane_block_driver_abi_sha256()
        );
        assert!(paths
            .initramfs_driver_dir
            .join("pane-initramfs-hook.sh")
            .is_file());
        assert!(paths
            .initramfs_driver_dir
            .join("pane-port-block.h")
            .is_file());
        assert!(paths.initramfs_driver_dir.join("pane-init.c").is_file());
        assert!(paths
            .initramfs_driver_dir
            .join("pane-port-probe.c")
            .is_file());
        assert!(paths.initramfs_driver_dir.join("pane-block.c").is_file());
        assert!(paths
            .initramfs_driver_dir
            .join("build-pane-block-module.sh")
            .is_file());
        assert!(paths
            .initramfs_driver_dir
            .join("build-pane-initramfs.sh")
            .is_file());
        assert!(paths.initramfs_driver_metadata.is_file());
        let hook =
            std::fs::read_to_string(paths.initramfs_driver_dir.join("pane-initramfs-hook.sh"))
                .unwrap();
        assert!(hook.contains("pane.storage_contract"));
        assert!(hook.contains("pane.block_io"));
        assert!(hook.contains("pane.block_dma"));
        assert!(hook.contains("pane.block_devices"));
        assert!(hook.contains("pane.virtio_root"));
        assert!(hook.contains("pane.framebuffer"));
        assert!(hook.contains("pane.input_queue"));
        let header =
            std::fs::read_to_string(paths.initramfs_driver_dir.join("pane-port-block.h")).unwrap();
        assert!(header.contains("#ifdef __KERNEL__"));
        assert!(header.contains("#include <linux/types.h>"));
        assert!(header.contains("#define PANE_BLOCK_IO_BASE_PORT 3328"));
        assert!(header.contains("#define PANE_BLOCK_IO_DATA_OFFSET 12"));
        let init_source =
            std::fs::read_to_string(paths.initramfs_driver_dir.join("pane-init.c")).unwrap();
        assert!(init_source.contains("PANE_INITRAMFS_DISCOVERY_START"));
        assert!(init_source.contains("PANE_BLOCK_IO_PROBE_OK"));
        assert!(init_source.contains("PANE_BLOCK_MODULE_LOAD_ATTEMPT"));
        assert!(init_source.contains("PANE_BLOCK_MODULE_LOAD_OK"));
        assert!(init_source.contains("PANE_BLOCK_MODULE_LOAD_TIMEOUT"));
        assert!(init_source.contains("PANE_BLOCK_MODULE_NOT_PRESENT"));
        assert!(init_source.contains("PANE_BLOCK_MODULE_LOAD_WAITING"));
        assert!(init_source.contains("waitpid(child, &status, WNOHANG)"));
        assert!(init_source.contains("sched_yield();"));
        assert!(init_source.contains("attempt < 50000000"));
        assert!(init_source.contains("drop_probe_caches_before_root_mount"));
        assert!(init_source.contains("/proc/sys/vm/drop_caches"));
        assert!(init_source.contains("PANE_BLOCK_PROBE_CACHE_DROPPED"));
        assert!(init_source.contains("shared_buffer_gpa=%llu shared_buffer_bytes=%llu"));
        assert!(init_source.contains("pane.root_offset"));
        assert!(init_source.contains("pane.virtio_root"));
        assert!(init_source.contains("pane.root_readonly"));
        assert!(init_source.contains("pane.root_fs"));
        assert!(init_source.contains("PANE_VIRTIO_ROOT"));
        assert!(init_source.contains("PANE_ROOT_READONLY"));
        assert!(init_source.contains("PANE_ROOT_FS"));
        assert!(init_source.contains("PANE_VIRTIO_ROOT_MOUNT_ATTEMPT"));
        assert!(init_source.contains("PANE_VIRTIO_ROOT_DEVICE_WAIT_TIMEOUT"));
        assert!(init_source.contains("PANE_VIRTIO_ROOT_MOUNT_FALLBACK"));
        assert!(init_source.contains("load_virtio_mmio_module"));
        assert!(init_source.contains("/lib/modules/virtio_mmio.ko"));
        assert!(init_source.contains("PANE_VIRTIO_MMIO_MODULE_LOAD_OK"));
        assert!(init_source.contains("PANE_VIRTIO_MMIO_MODULE_NOT_PRESENT"));
        // The bus driver must be loaded before the virtio root device wait, otherwise
        // /dev/vda never appears on a stock Arch kernel (CONFIG_VIRTIO_MMIO=m).
        assert!(
            init_source.find("load_virtio_mmio_module();").unwrap()
                < init_source.find("wait_for_device(virtio_root_device)").unwrap()
        );
        assert!(init_source.contains("attempt < 65536"));
        assert!(!init_source.contains("usleep(100000)"));
        assert!(init_source.contains("supported_root_fs"));
        assert!(init_source.contains("MS_RELATIME | (root_readonly ? MS_RDONLY : 0)"));
        assert!(init_source.contains("pane.block_devices"));
        assert!(init_source.contains("pane.block_dma"));
        assert!(init_source.contains("PANE_BLOCK_DMA_ENABLED_FOR_SHARED_BUFFER"));
        assert!(init_source.contains("PANE_BLOCK_DMA_CONTRACT_INVALID"));
        assert!(init_source.contains("strtoull(block_dma, &dma_separator, 0)"));
        assert!(init_source.contains("PANE_DISPLAY_CONTRACT_DISCOVERED"));
        assert!(init_source.contains("PANE_FRAMEBUFFER"));
        assert!(init_source.contains("PANE_INPUT_QUEUE"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_ATTEMPT"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_TRY"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_FAIL"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_TIMEOUT"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_WAITING"));
        assert!(init_source.contains("kill(child, SIGKILL)"));
        assert!(init_source.contains("#define PANE_ROOT_MOUNT_MAX_POLLS 65536U"));
        assert!(init_source.contains("#define PANE_ROOT_MOUNT_WAIT_LOG_INTERVAL 4096U"));
        assert!(init_source.contains("attempt < PANE_ROOT_MOUNT_MAX_POLLS"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_WAITING fs=%s polls=%u"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_TIMEOUT fs=%s polls=%u"));
        assert!(init_source.contains("PANE_ROOT_MOUNT_OK"));
        assert!(init_source.contains("execl(\"/sbin/init\""));
        assert!(init_source.contains("#define COM1_PORT 0x3f8"));
        let build_script =
            std::fs::read_to_string(paths.initramfs_driver_dir.join("build-pane-initramfs.sh"))
                .unwrap();
        assert!(build_script.contains("cpio -o -H newc"));
        assert!(build_script.contains("bsdtar --format newc"));
        assert!(build_script.contains("$workdir/newroot"));
        assert!(build_script.contains("$workdir/lib/modules"));
        assert!(build_script.contains("output_path="));
        assert!(build_script.contains("/*) output_path=\"$output\""));
        assert!(build_script.contains("*) output_path=\"$PWD/$output\""));
        assert!(build_script.contains("-o \"$workdir/init\" pane-init.c"));
        assert!(build_script.contains("pane-port-probe"));
        assert!(build_script.contains("pane-block.ko"));
        assert!(init_source.contains("trust_submit_completion=1"));
        let block_driver =
            std::fs::read_to_string(paths.initramfs_driver_dir.join("pane-block.c")).unwrap();
        assert!(block_driver.contains("PANE_BLOCK_DRIVER_NAME \"pane_block\""));
        assert!(block_driver.contains("PANE_BLOCK_DEVICE_BASE_OS"));
        assert!(block_driver.contains("PANE_BLOCK_DEVICE_USER_DISK"));
        assert!(block_driver.contains("PANE_BLOCK_OPERATION_READ"));
        assert!(block_driver.contains("PANE_BLOCK_OPERATION_WRITE"));
        assert!(block_driver.contains("PANE_BLOCK_IO_BASE_PORT"));
        assert!(block_driver.contains("PANE_BLOCK_STATUS_SERVICED"));
        assert!(block_driver.contains("shared_buffer_gpa"));
        assert!(block_driver.contains("pane_block_shared_buffer"));
        assert!(block_driver.contains("PANE_BLOCK_SHARED_BUFFER_OK"));
        assert!(block_driver.contains("pane_block_serial_log(\"PANE_BLOCK_SHARED_BUFFER_OK\")"));
        assert!(block_driver
            .contains("pane_block_serial_log(\"PANE_BLOCK_SHARED_BUFFER_UNAVAILABLE\")"));
        assert!(block_driver.contains("trust_submit_completion"));
        assert!(block_driver.contains("module_param(trust_submit_completion, bool, 0444)"));
        assert!(block_driver.contains("static bool trust_submit_completion;"));
        assert!(block_driver.contains("if (!trust_submit_completion)"));
        assert!(block_driver.contains("PANE_BLOCK_SUBMIT_READY"));
        assert!(block_driver.contains("PANE_BLOCK_STATUS_SERVICED"));
        assert!(block_driver.contains("PANE_BLOCK_STATUS_WAIT_TIMEOUT"));
        assert!(block_driver.contains("pane_block_serial_log"));
        assert!(block_driver.contains("PANE_BLOCK_TRANSFER_SUBMITTED"));
        assert!(block_driver.contains("PANE_BLOCK_READ_DMA_COPIED"));
        assert!(block_driver.contains("PANE_BLOCK_REQUEST_ACTIVE"));
        assert!(block_driver.contains("PANE_BLOCK_BOUNCE_ALLOC_OK"));
        assert!(block_driver.contains("PANE_BLOCK_BOUNCE_READY"));
        assert!(block_driver.contains("pane_block_bounce_buffer"));
        assert!(block_driver.contains("queue_depth = 1"));
        assert!(block_driver.contains("PANE_BLOCK_REQUEST_END_OK"));
        assert!(block_driver.contains("memremap((phys_addr_t)shared_buffer_gpa"));
        assert!(block_driver.contains("outl((u32)(block_index & 0xffffffff)"));
        assert!(block_driver
            .contains("outl(word, PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_DATA_OFFSET)"));
        assert!(block_driver
            .contains("u32 word = inl(PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_DATA_OFFSET)"));
        assert!(block_driver.contains("REQ_OP_FLUSH"));
        assert!(block_driver.contains("REQ_OP_DISCARD"));
        assert!(block_driver.contains("blk_mq"));
        assert!(block_driver.contains("struct queue_limits limits"));
        assert!(block_driver.contains("blk_mq_alloc_disk(&pane_disk->tag_set, &limits, pane_disk)"));
        assert!(block_driver.contains("vmalloc(PANE_BLOCK_IO_BLOCK_SIZE_BYTES)"));
        assert!(block_driver.contains("absolute_byte"));
        assert!(block_driver.contains("base_block_offset"));
        assert!(block_driver.contains("GENHD_FL_NO_PART"));
        assert!(block_driver.contains("block_index + pane_disk->block_offset"));
        assert!(block_driver.contains("PANE_BLOCK_INIT_READ_ZERO_FILL"));
        assert!(block_driver.contains("pane_block_initializing = false"));
        assert!(block_driver.contains("probe-time cache entries"));
        assert!(block_driver.contains("partial filesystem writes"));
        assert!(block_driver.contains("PANE_BLOCK_MINORS_PER_DISK 16"));
        assert!(block_driver.contains("index * PANE_BLOCK_MINORS_PER_DISK"));
        assert!(block_driver.contains("add_disk"));
        assert!(block_driver.contains("PANE_BLOCK_INIT_START"));
        assert!(block_driver.contains("PANE_BLOCK_REGISTER_BLKDEV_OK"));
        assert!(block_driver.contains("PANE_BLOCK_CREATE_DISK_START"));
        assert!(block_driver.contains("PANE_BLOCK_ADD_DISK_START"));
        assert!(block_driver.contains("PANE_BLOCK_STATUS_READ"));
        assert!(block_driver.contains("pane_block_wait_serviced(log_transfer)"));
        assert!(block_driver.contains("log_transfer || status != PANE_BLOCK_STATUS_SERVICED"));
        assert!(block_driver.contains("attempt < 1024"));
        assert!(block_driver.contains("cpu_relax()"));
        assert!(block_driver.contains("bool log_transfer = false"));
        assert!(block_driver.contains("PANE_BLOCK_TRANSFER_WAIT_FAILED"));
        assert!(block_driver.contains("/dev/pane0"));
        assert!(block_driver.contains("/dev/pane1"));
        let block_build_script = std::fs::read_to_string(
            paths
                .initramfs_driver_dir
                .join("build-pane-block-module.sh"),
        )
        .unwrap();
        assert!(block_build_script.contains("KERNEL_BUILD_DIR"));
        assert!(block_build_script.contains("pane-block.ko"));
        assert!(block_build_script.contains("make -C"));
        let readme = std::fs::read_to_string(paths.initramfs_driver_dir.join("README.md")).unwrap();
        assert!(readme.contains("Pane compiles the generated guest `/init`"));
        assert!(readme.contains("PANE_LINUX_CC"));
        assert!(readme.contains("zig cc -target x86_64-linux-musl"));
        assert!(readme.contains("packages the `newc` initramfs"));
        assert!(readme.contains("depend on host `cpio` or `bsdtar`"));
        assert!(artifacts.initramfs_driver_bundle_exists);
        assert!(artifacts.initramfs_driver_metadata_exists);
        assert!(artifacts.initramfs_driver_bundle_ready);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn initramfs_driver_source_freshness_rejects_stale_metadata() {
        let paths = temp_runtime_paths("runtime-pane-initramfs-driver-stale-source");
        super::prepare_runtime_paths(&paths).unwrap();
        let mut metadata = write_pane_initramfs_driver_bundle(&paths).unwrap();

        metadata.init_source_sha256 = "0".repeat(64);

        assert!(!super::pane_initramfs_driver_metadata_matches_current_sources(&metadata));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn writes_newc_cpio_archive_with_trailer_and_payloads() {
        let paths = temp_runtime_paths("runtime-newc-cpio");
        super::prepare_runtime_paths(&paths).unwrap();
        let output = paths.downloads.join("test-initramfs.cpio");

        super::write_newc_cpio_archive(
            &output,
            &[
                super::NewcCpioEntry::directory("bin"),
                super::NewcCpioEntry::file("init", 0o755, b"pane init".to_vec()),
                super::NewcCpioEntry::file("bin/pane-port-probe", 0o755, b"pane probe".to_vec()),
            ],
        )
        .unwrap();

        let archive = std::fs::read(&output).unwrap();
        assert!(archive.starts_with(b"070701"));
        assert_eq!(archive.len() % 4, 0);
        assert!(archive
            .windows("TRAILER!!!".len())
            .any(|window| window == b"TRAILER!!!"));
        assert!(archive
            .windows("bin/pane-port-probe".len())
            .any(|window| window == b"bin/pane-port-probe"));
        assert!(archive
            .windows("pane init".len())
            .any(|window| window == b"pane init"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn initramfs_compiler_candidates_include_portable_fallbacks() {
        let labels = super::initramfs_compiler_candidates()
            .into_iter()
            .map(|candidate| candidate.label)
            .collect::<Vec<_>>();

        assert!(labels.iter().any(|label| label == "cc"));
        assert!(labels
            .iter()
            .any(|label| label == "zig cc -target x86_64-linux-musl"));
    }

    #[test]
    fn rejects_non_elf_discovery_binary_before_packaging() {
        let paths = temp_runtime_paths("runtime-non-elf-discovery-binary");
        super::prepare_runtime_paths(&paths).unwrap();
        let binary = paths.downloads.join("init.exe");
        std::fs::write(&binary, b"MZnot-linux").unwrap();

        let error = super::verify_elf_binary(&binary, "prebuilt Pane discovery /init")
            .unwrap_err()
            .to_string();

        assert!(error.contains("not an ELF binary"));
        assert!(error.contains("Windows host ABI"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn packages_discovery_initramfs_from_prebuilt_elfs_without_compiler() {
        let paths = temp_runtime_paths("runtime-prebuilt-discovery-elfs");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("console=ttyS0 panic=-1 root=/dev/pane0"),
            false,
        )
        .unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
        let init = paths.downloads.join("pane-init.elf");
        let probe = paths.downloads.join("pane-port-probe.elf");
        std::fs::write(&init, b"\x7fELFpane-prebuilt-init").unwrap();
        std::fs::write(&probe, b"\x7fELFpane-prebuilt-probe").unwrap();

        super::build_and_register_pane_discovery_initramfs(
            &paths,
            Some(&init),
            Some(&probe),
            false,
        )
        .unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        let archive = std::fs::read(
            paths
                .initramfs_driver_dir
                .join("pane-storage-discovery.cpio"),
        )
        .unwrap();
        assert!(artifacts.initramfs_image_exists);
        assert!(artifacts.initramfs_image_verified);
        assert!(!artifacts.discovery_initramfs_matches_driver_bundle);
        let metadata =
            read_json_file::<PaneInitramfsDriverMetadata>(&paths.initramfs_driver_metadata)
                .unwrap();
        assert_eq!(
            metadata.packaged_binary_provenance.as_deref(),
            Some("external-prebuilt-elf")
        );
        assert!(metadata.packaged_init_binary_sha256.is_some());
        assert!(metadata.packaged_probe_binary_sha256.is_some());
        assert!(archive
            .windows("pane-prebuilt-init".len())
            .any(|window| window == b"pane-prebuilt-init"));
        assert!(archive
            .windows("pane-prebuilt-probe".len())
            .any(|window| window == b"pane-prebuilt-probe"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn stale_registered_discovery_initramfs_does_not_match_current_driver_bundle() {
        let paths = temp_runtime_paths("runtime-stale-discovery-initramfs");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("console=ttyS0 panic=-1 root=/dev/pane0"),
            false,
        )
        .unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
        let stale_initramfs = paths.downloads.join("stale-discovery.cpio");
        std::fs::write(&stale_initramfs, b"stale pane initramfs").unwrap();

        register_pane_discovery_initramfs_artifact(&paths, &stale_initramfs, false).unwrap();

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.initramfs_image_verified);
        assert!(!artifacts.discovery_initramfs_matches_driver_bundle);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_verified_pane_block_module_for_initramfs_packaging() {
        let paths = temp_runtime_paths("runtime-pane-block-module");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
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
        let error = build_and_register_pane_block_module(&paths, None, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--kernel-build-dir"));

        let source = paths.downloads.join("pane-block.ko");
        std::fs::write(&source, b"fake pane block kernel module").unwrap();
        let sha256 = sha256_file(&source).unwrap();

        register_pane_block_module(&paths, &source, Some(&sha256), false).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);

        assert!(paths.initramfs_driver_dir.join("pane-block.ko").is_file());
        assert!(paths.state.join("pane-block-module.json").is_file());
        assert!(artifacts.pane_block_module_exists);
        assert!(artifacts.pane_block_module_verified);
        assert_eq!(
            artifacts.pane_block_module_sha256.as_deref(),
            Some(sha256.as_str())
        );
        let metadata =
            read_json_file::<PaneBlockModuleMetadata>(&paths.state.join("pane-block-module.json"))
                .unwrap();
        assert_eq!(
            metadata.target_kernel_sha256.as_deref(),
            Some(kernel_sha.as_str())
        );
        assert_eq!(
            metadata.target_kernel_path.as_deref(),
            Some(paths.kernel_image.display().to_string().as_str())
        );
        assert_eq!(
            metadata.block_driver_source_sha256.as_deref(),
            Some(
                read_json_file::<PaneInitramfsDriverMetadata>(&paths.initramfs_driver_metadata)
                    .unwrap()
                    .block_driver_source_sha256
                    .as_str()
            )
        );
        assert_eq!(
            metadata.block_driver_abi_sha256.as_deref(),
            Some(pane_block_driver_abi_sha256().as_str())
        );

        let mut legacy_metadata = metadata.clone();
        legacy_metadata.block_driver_abi_sha256 = None;
        legacy_metadata.block_driver_source_sha256 =
            Some(COMPATIBLE_PANE_BLOCK_DRIVER_SOURCE_SHA256_BY_ABI[0].to_string());
        write_json_file(
            &paths.state.join("pane-block-module.json"),
            &legacy_metadata,
        )
        .unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.pane_block_module_verified);
        assert!(load_verified_pane_block_module_metadata(&paths).is_ok());

        let replacement_kernel = paths.downloads.join("vmlinuz-linux-replacement");
        let mut replacement_bytes = fake_linux_bzimage();
        replacement_bytes[3000] = 0x7f;
        std::fs::write(&replacement_kernel, replacement_bytes).unwrap();
        let replacement_sha = sha256_file(&replacement_kernel).unwrap();
        register_kernel_boot_plan(
            &paths,
            Some(&replacement_kernel),
            Some(&replacement_sha),
            None,
            None,
            Some("console=ttyS0 root=/dev/pane0 rw"),
            true,
        )
        .unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.pane_block_module_exists);
        assert!(!artifacts.pane_block_module_verified);
        let error = load_verified_pane_block_module_metadata(&paths)
            .unwrap_err()
            .to_string();
        assert!(error.contains("current verified kernel"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_virtio_mmio_module_and_packs_it_into_initramfs() {
        let paths = temp_runtime_paths("runtime-virtio-mmio-module");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
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

        let source = paths.downloads.join("virtio_mmio.ko");
        std::fs::write(&source, b"fake virtio_mmio kernel module").unwrap();
        let sha256 = sha256_file(&source).unwrap();

        // Registration without the expected SHA is rejected.
        let error = register_virtio_mmio_module(&paths, &source, None, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--virtio-mmio-module-expected-sha256"));

        register_virtio_mmio_module(&paths, &source, Some(&sha256), false).unwrap();

        let stored = virtio_mmio_module_path(&paths);
        assert!(stored.is_file());
        assert_eq!(sha256_file(&stored).unwrap(), sha256);
        let metadata =
            read_json_file::<VirtioMmioModuleMetadata>(&virtio_mmio_module_metadata_path(&paths))
                .unwrap();
        assert!(metadata.verified);
        assert_eq!(metadata.sha256, sha256);
        assert_eq!(
            metadata.target_kernel_sha256.as_deref(),
            Some(kernel_sha.as_str())
        );

        // Re-registering over an existing module requires --force.
        let error = register_virtio_mmio_module(&paths, &source, Some(&sha256), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--force"));
        register_virtio_mmio_module(&paths, &source, Some(&sha256), true).unwrap();

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn registers_generated_discovery_initramfs_into_kernel_plan() {
        let paths = temp_runtime_paths("runtime-pane-discovery-initramfs");
        super::prepare_runtime_paths(&paths).unwrap();
        let kernel = paths.downloads.join("vmlinuz-linux");
        let discovery_initramfs = paths
            .initramfs_driver_dir
            .join("pane-storage-discovery.cpio");
        std::fs::create_dir_all(&paths.initramfs_driver_dir).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        std::fs::write(
            &discovery_initramfs,
            b"PANE_INITRAMFS_DISCOVERY_START\nPANE_BLOCK_IO_PROBE_OK\nPANE_INITRAMFS_DISCOVERY_DONE\n",
        )
        .unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();
        let discovery_sha = sha256_file(&discovery_initramfs).unwrap();

        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("console=ttyS0 panic=-1"),
            false,
        )
        .unwrap();

        register_pane_discovery_initramfs_artifact(&paths, &discovery_initramfs, false).unwrap();
        let metadata = read_json_file::<KernelBootMetadata>(&paths.kernel_boot_metadata).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        let stored_initramfs_path = paths.initramfs_image.display().to_string();

        assert_eq!(
            metadata.initramfs_stored_path.as_deref(),
            Some(stored_initramfs_path.as_str())
        );
        assert_eq!(
            metadata.initramfs_sha256.as_deref(),
            Some(discovery_sha.as_str())
        );
        assert_eq!(
            metadata.initramfs_expected_sha256.as_deref(),
            Some(discovery_sha.as_str())
        );
        assert!(metadata.initramfs_verified);
        assert!(artifacts.initramfs_image_exists);
        assert!(artifacts.initramfs_image_verified);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_snapshot_copies_and_verifies_sparse_artifact() {
        let paths = temp_runtime_paths("runtime-user-disk-snapshot");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let mut written = vec![0_u8; 4096];
        written[..16].copy_from_slice(b"PANE_SNAPSHOT_V1");
        write_user_disk_block(&paths, &metadata, 2, &written).unwrap();
        let source_sha = sha256_file(&paths.user_disk).unwrap();

        let snapshot = create_user_disk_snapshot(&paths).unwrap();

        let snapshot_path = PathBuf::from(&snapshot.snapshot_path);
        assert!(snapshot_path.is_file());
        assert_eq!(sha256_file(&snapshot_path).unwrap(), source_sha);
        assert_eq!(snapshot.snapshot_kind, "pane-user-disk-snapshot-v1");
        assert_eq!(snapshot.source_disk_sha256, source_sha);
        assert_eq!(snapshot.user_disk_capacity_gib, 3);
        assert_eq!(snapshot.user_disk_block_size_bytes, 4096);
        let snapshot_metadata_path = paths
            .snapshots
            .join(format!("{}.json", snapshot.snapshot_id));
        let persisted =
            read_json_file::<UserDiskSnapshotMetadata>(&snapshot_metadata_path).unwrap();
        assert_eq!(persisted.snapshot_id, snapshot.snapshot_id);

        let artifacts = build_runtime_artifact_report(&paths);
        assert_eq!(artifacts.user_disk_snapshot_count, 1);
        let expected_snapshot_metadata = snapshot_metadata_path.display().to_string();
        assert_eq!(
            artifacts.latest_user_disk_snapshot.as_deref(),
            Some(expected_snapshot_metadata.as_str())
        );

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_restore_replaces_active_disk_from_verified_snapshot() {
        let paths = temp_runtime_paths("runtime-user-disk-restore");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let mut original = vec![0_u8; 4096];
        original[..16].copy_from_slice(b"PANE_RESTORE_OLD");
        write_user_disk_block(&paths, &metadata, 2, &original).unwrap();
        let snapshot = create_user_disk_snapshot(&paths).unwrap();
        let snapshot_metadata_path = paths
            .snapshots
            .join(format!("{}.json", snapshot.snapshot_id));
        let mut mutated = vec![0_u8; 4096];
        mutated[..16].copy_from_slice(b"PANE_RESTORE_NEW");
        write_user_disk_block(&paths, &metadata, 2, &mutated).unwrap();
        assert_eq!(
            read_user_disk_block(&paths, &metadata, 2).unwrap()[..16],
            mutated[..16]
        );

        let restored = restore_user_disk_snapshot(&paths, &snapshot_metadata_path).unwrap();

        assert_eq!(restored.snapshot_id, snapshot.snapshot_id);
        let restored_metadata =
            read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        assert!(user_disk_artifact_ready(
            &paths,
            &Some(restored_metadata.clone())
        ));
        assert_eq!(
            read_user_disk_block(&paths, &restored_metadata, 2).unwrap()[..16],
            original[..16]
        );
        assert!(restored_metadata
            .notes
            .iter()
            .any(|note| note.contains("Restored from Pane user disk snapshot")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_export_and_import_round_trip_verified_package() {
        let paths = temp_runtime_paths("runtime-user-disk-export-source");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let mut original = vec![0_u8; 4096];
        original[..16].copy_from_slice(b"PANE_EXPORT_V1__");
        write_user_disk_block(&paths, &metadata, 2, &original).unwrap();
        let source_sha = sha256_file(&paths.user_disk).unwrap();
        let export_dir = paths.root.join("portable-export");

        let manifest = export_user_disk_package(&paths, &export_dir, false).unwrap();

        assert_eq!(manifest.export_kind, "pane-user-disk-export-v1");
        assert_eq!(manifest.source_disk_sha256, source_sha);
        assert!(export_dir
            .join(PANE_USER_DISK_EXPORT_MANIFEST_FILENAME)
            .is_file());
        assert!(export_dir
            .join(PANE_USER_DISK_EXPORT_DISK_FILENAME)
            .is_file());
        assert!(export_dir
            .join(PANE_USER_DISK_EXPORT_METADATA_FILENAME)
            .is_file());
        let persisted = read_json_file::<UserDiskExportManifest>(
            &export_dir.join(PANE_USER_DISK_EXPORT_MANIFEST_FILENAME),
        )
        .unwrap();
        assert_eq!(persisted.export_id, manifest.export_id);

        let target = temp_runtime_paths("runtime-user-disk-export-target");
        super::prepare_runtime_paths(&target).unwrap();
        let imported = import_user_disk_package(&target, &export_dir).unwrap();

        assert_eq!(imported.export_id, manifest.export_id);
        let imported_metadata =
            read_json_file::<UserDiskMetadata>(&target.user_disk_metadata).unwrap();
        assert!(user_disk_artifact_ready(
            &target,
            &Some(imported_metadata.clone())
        ));
        assert_eq!(sha256_file(&target.user_disk).unwrap(), source_sha);
        assert_eq!(
            read_user_disk_block(&target, &imported_metadata, 2).unwrap()[..16],
            original[..16]
        );
        assert!(imported_metadata
            .notes
            .iter()
            .any(|note| note.contains("Imported from Pane user disk export")));

        let _ = std::fs::remove_dir_all(&paths.root);
        let _ = std::fs::remove_dir_all(&target.root);
    }

    #[test]
    fn user_disk_resize_grows_logical_capacity_without_moving_blocks() {
        let paths = temp_runtime_paths("runtime-user-disk-resize");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        let metadata = read_json_file::<UserDiskMetadata>(&paths.user_disk_metadata).unwrap();
        let mut written = vec![0_u8; 4096];
        written[..16].copy_from_slice(b"PANE_RESIZE_V1__");
        write_user_disk_block(&paths, &metadata, 2, &written).unwrap();

        let resized = resize_user_disk(&paths, 4).unwrap();

        assert_eq!(resized.capacity_gib, 4);
        assert_eq!(resized.logical_size_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(
            resized.allocated_header_bytes,
            metadata.allocated_header_bytes
        );
        assert!(resized
            .notes
            .iter()
            .any(|note| note.contains("Grew Pane sparse user disk")));
        assert!(user_disk_artifact_ready(&paths, &Some(resized.clone())));
        assert_eq!(
            read_user_disk_block(&paths, &resized, 2).unwrap()[..16],
            written[..16]
        );
        let artifacts = build_runtime_artifact_report(&paths);
        assert_eq!(artifacts.user_disk_capacity_gib, Some(4));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_resize_rejects_shrink() {
        let paths = temp_runtime_paths("runtime-user-disk-resize-shrink");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();

        let error = resize_user_disk(&paths, 2).unwrap_err().to_string();

        assert!(error.contains("grow-only"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_repair_rebuilds_metadata_from_valid_header() {
        let paths = temp_runtime_paths("runtime-user-disk-repair");
        super::prepare_runtime_paths(&paths).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        std::fs::remove_file(&paths.user_disk_metadata).unwrap();

        let repaired = repair_user_disk_metadata(&paths).unwrap();

        assert_eq!(repaired.capacity_gib, 3);
        assert_eq!(repaired.logical_size_bytes, 3 * 1024 * 1024 * 1024);
        assert_eq!(repaired.block_size_bytes, 4096);
        assert!(user_disk_artifact_ready(&paths, &Some(repaired.clone())));
        assert!(repaired
            .notes
            .iter()
            .any(|note| note.contains("Repaired Pane user disk metadata")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn user_disk_repair_rejects_corrupt_header() {
        let paths = temp_runtime_paths("runtime-user-disk-repair-corrupt");
        super::prepare_runtime_paths(&paths).unwrap();
        std::fs::write(&paths.user_disk, b"not a pane disk\n\n").unwrap();

        let error = repair_user_disk_metadata(&paths).unwrap_err().to_string();

        assert!(error.contains("header magic"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_layout_attaches_verified_storage_and_display_contracts() {
        let paths = temp_runtime_paths("kernel-layout-storage-display");
        super::prepare_runtime_paths(&paths).unwrap();
        super::write_runtime_config(&paths, "pane", &runtime_storage_budget(8)).unwrap();
        super::write_native_runtime_manifest(&paths, "pane").unwrap();
        super::write_framebuffer_contract(&paths).unwrap();
        super::write_input_contract(&paths).unwrap();
        let base = paths.downloads.join("arch-base.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&base, fake_mbr_linux_root_disk_image()).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
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
        register_fake_pane_block_module(&paths);
        register_fake_discovery_initramfs(&paths);

        let layout = build_kernel_boot_layout(&paths, "pane", true).unwrap();
        let storage = layout.storage.as_ref().expect("storage attachment");
        let driver = layout
            .initramfs_driver
            .as_ref()
            .expect("initramfs driver attachment");
        assert_eq!(driver.bundle_kind, "pane-initramfs-driver-source-v1");
        assert_eq!(driver.block_io_protocol, "pane-port-block-v1");
        assert_eq!(
            layout.expected_serial_milestones,
            pane_initramfs_expected_serial_milestones()
        );
        assert_eq!(storage.root_device, "/dev/pane0");
        assert_eq!(storage.user_device, "/dev/pane1");
        assert_eq!(storage.contract_gpa, "0x0dfe0000");
        assert_eq!(storage.base_os_block_size_bytes, 4096);
        assert_eq!(storage.block_io_protocol, "pane-port-block-v1");
        assert_eq!(storage.block_io_port_base, "0x0d00");
        assert_eq!(storage.block_io_port_count, 16);
        assert_eq!(storage.block_io_status_port_offset, 2);
        assert_eq!(storage.block_io_data_port_offset, 12);
        assert_eq!(
            storage.block_io_block_size_bytes,
            u64::from(crate::native::PANE_BLOCK_IO_BLOCK_SIZE_BYTES)
        );
        assert_eq!(storage.block_dma_gpa, "0x0dfd0000");
        assert_eq!(storage.block_dma_size_bytes, 4096);
        assert_eq!(
            storage.virtio_block.backend_kind,
            "pane-virtio-blk-backend-plan-v1"
        );
        assert_eq!(storage.virtio_block.source_crate, "rust-vmm/virtio-queue");
        assert_eq!(
            storage.virtio_block.candidate_crate_version.as_deref(),
            Some("0.17.0")
        );
        assert_eq!(
            storage.virtio_block.adoption_state,
            "live-whp-mmio-execution-and-irq-request-ready-guest-ack-pending"
        );
        assert_eq!(storage.virtio_block.transport, "virtio-mmio");
        assert_eq!(storage.virtio_block.mmio_base_gpa, "0x0dfc0000");
        assert_eq!(storage.virtio_block.mmio_size_bytes, 4096);
        assert_eq!(
            storage.virtio_block.mmio_irq,
            crate::virtio::PANE_VIRTIO_MMIO_IRQ
        );
        assert_eq!(
            storage.virtio_block.linux_kernel_parameter,
            "virtio_mmio.device=4K@0xdfc0000:5"
        );
        assert_eq!(
            storage.virtio_block.queue_model,
            "rust-vmm-virtio-queue-split-ring-batch-drain-ready"
        );
        assert_eq!(storage.virtio_block.root_device_hint, "/dev/vda1");
        assert_eq!(storage.virtio_block.devices.len(), 2);
        assert_eq!(storage.virtio_block.devices[0].id, "vda");
        assert_eq!(
            storage.virtio_block.devices[0].guest_device_hint,
            "/dev/vda"
        );
        assert!(storage.virtio_block.devices[0].readonly);
        assert_eq!(
            storage.virtio_block.devices[0].root_partition_byte_offset,
            Some(2048 * 512)
        );
        assert_eq!(storage.virtio_block.devices[1].id, "vdb");
        assert_eq!(
            storage.virtio_block.devices[1].guest_device_hint,
            "/dev/vdb"
        );
        assert!(!storage.virtio_block.devices[1].readonly);
        assert!(storage.virtio_block.devices[1].sparse_backing);
        assert!(layout
            .cmdline
            .contains("earlycon=uart8250,io,0x3f8,115200n8"));
        assert!(layout.cmdline.contains("earlyprintk=serial,ttyS0,115200"));
        assert!(layout.cmdline.contains("quiet"));
        assert!(layout.cmdline.contains("loglevel=4"));
        assert!(!layout.cmdline.contains("ignore_loglevel"));
        assert!(layout.cmdline.contains("nomodeset"));
        assert!(layout.cmdline.contains("lpj=1000000"));
        assert!(layout.cmdline.contains("tsc=reliable"));
        assert!(layout.cmdline.contains("clocksource=tsc"));
        assert!(layout.cmdline.contains("no_timer_check"));
        assert!(layout.cmdline.contains("i8042.noaux"));
        assert!(layout.cmdline.contains("acpi=off"));
        assert!(layout.cmdline.contains("pci=off"));
        assert!(!layout.cmdline.contains("noapic"));
        assert!(!layout.cmdline.contains("nolapic"));
        assert_eq!(
            layout
                .cmdline
                .split_whitespace()
                .filter(|arg| *arg == "panic=-1")
                .count(),
            1
        );
        assert!(layout.cmdline.contains("pane.storage_contract=0x0dfe0000"));
        assert!(layout.cmdline.contains("pane.root=/dev/pane0"));
        assert!(layout
            .cmdline
            .contains("pane.root_mode=base-partition-direct"));
        assert!(layout.cmdline.contains("pane.root_readonly=1"));
        assert!(layout.cmdline.contains("pane.root_fs=ext4"));
        assert!(layout.cmdline.contains("pane.root_partition=1"));
        assert!(layout.cmdline.contains("pane.user=/dev/pane1"));
        assert!(layout.cmdline.contains("pane.virtio_root=/dev/vda1"));
        assert!(layout.cmdline.contains("virtio_mmio.device=4K@0xdfc0000:5"));
        assert!(layout.cmdline.contains("pane.block_io=0x0d00,16,4096"));
        assert!(layout.cmdline.contains("pane.block_devices=512,786432"));
        assert!(layout.cmdline.contains("pane.block_dma=0x0dfd0000,4096"));
        assert!(layout
            .cmdline
            .contains("pane.framebuffer=0x0e000000,1024,768,32,x8r8g8b8"));
        assert!(layout
            .cmdline
            .contains("pane.input_queue=0x0dff0000,4096,32,64"));
        assert!(storage.readonly_base);
        assert!(storage.writable_user_disk);
        assert_eq!(storage.root_handoff.mode, "base-partition-direct");
        assert_eq!(storage.root_handoff.root_device, "/dev/pane0");
        assert_eq!(storage.root_handoff.partition_index, Some(1));
        assert_eq!(
            storage.root_handoff.filesystem_hint.as_deref(),
            Some("ext4")
        );
        assert!(storage.root_handoff.requires_initramfs_driver);
        assert_eq!(storage.base_os_sha256, base_sha);
        assert_eq!(storage.user_disk_capacity_gib, 3);
        assert_eq!(storage.user_disk_logical_size_bytes, 3 * 1024 * 1024 * 1024);
        assert_eq!(storage.user_disk_block_size_bytes, 4096);
        assert!(storage.user_disk_sparse_backing);
        assert_eq!(storage.user_disk_format, "pane-sparse-user-disk-v1");
        assert_eq!(storage.user_disk_header_sha256.len(), 64);
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "pane-virtio-mmio"
                && range.start_gpa == "0x0dfc0000"
                && range.size_bytes == 4096
                && range.region_type == "virtio-mmio"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "pane-storage-contract"
                && range.start_gpa == "0x0dfe0000"
                && range.region_type == "storage-contract"
        }));
        assert!(layout.guest_memory_map.iter().any(|range| {
            range.label == "pane-block-dma"
                && range.start_gpa == "0x0dfd0000"
                && range.size_bytes == 4096
                && range.region_type == "block-dma"
        }));
        let mapped_regions = linux_guest_mapped_regions(&layout).unwrap();
        assert!(!mapped_regions
            .iter()
            .any(|region| region.label == "pane-virtio-mmio" || region.guest_gpa == 0x0dfc_0000));
        assert!(mapped_regions
            .iter()
            .any(|region| region.label == "pane-block-dma" && region.guest_gpa == 0x0dfd_0000));
        assert!(!mapped_regions
            .iter()
            .any(|region| region.label.contains("local-apic-mmio")));
        assert!(!mapped_regions
            .iter()
            .any(|region| region.label.contains("io-apic-mmio")));
        let storage_contract = mapped_regions
            .iter()
            .find(|region| region.label == "pane-storage-contract")
            .expect("storage contract region");
        assert_eq!(storage_contract.guest_gpa, 0x0dfe_0000);
        assert!(storage_contract.bytes.starts_with(b"{\"schema_version\":1"));
        assert!(storage_contract
            .bytes
            .windows("pane-port-block-v1".len())
            .any(|window| window == b"pane-port-block-v1"));
        assert!(storage_contract
            .bytes
            .windows("rust-vmm/vm-virtio".len())
            .any(|window| window == b"rust-vmm/vm-virtio"));
        assert!(storage_contract
            .bytes
            .windows("/dev/vda1".len())
            .any(|window| window == b"/dev/vda1"));
        assert!(storage_contract
            .bytes
            .windows("/dev/pane1".len())
            .any(|window| window == b"/dev/pane1"));
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
        let native_runtime =
            build_native_runtime_report(true, &artifacts, &test_native_host_report(true));
        assert!(native_runtime.ready_for_arch_boot_attempt);

        let mut note_only_layout = layout.clone();
        note_only_layout
            .storage
            .as_mut()
            .unwrap()
            .virtio_block
            .notes
            .push("Non-operative diagnostic wording changed.".to_string());
        write_json_file(&paths.kernel_boot_layout, &note_only_layout).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert!(artifacts.kernel_boot_layout_ready);

        let mut stale_layout = layout;
        stale_layout.storage.as_mut().unwrap().virtio_block = legacy_virtio_block_backend_plan();
        write_json_file(&paths.kernel_boot_layout, &stale_layout).unwrap();
        let artifacts = build_runtime_artifact_report(&paths);
        assert!(!artifacts.kernel_boot_layout_ready);

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_layout_refreshes_stale_base_os_filesystem_hint() {
        let paths = temp_runtime_paths("kernel-layout-refreshes-base-fs-hint");
        super::prepare_runtime_paths(&paths).unwrap();
        super::write_runtime_config(&paths, "pane", &runtime_storage_budget(8)).unwrap();
        super::write_native_runtime_manifest(&paths, "pane").unwrap();
        super::write_framebuffer_contract(&paths).unwrap();
        super::write_input_contract(&paths).unwrap();
        let base = paths.downloads.join("arch-base.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&base, fake_mbr_linux_root_disk_image()).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
        let mut stale_metadata =
            read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();
        stale_metadata.root_filesystem_hint = None;
        stale_metadata.notes = vec!["stale metadata from older Pane build".to_string()];
        write_json_file(&paths.base_os_metadata, &stale_metadata).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
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
        register_fake_pane_block_module(&paths);
        register_fake_discovery_initramfs(&paths);

        let layout = build_kernel_boot_layout(&paths, "pane", true).unwrap();
        let refreshed = read_json_file::<BaseOsImageMetadata>(&paths.base_os_metadata).unwrap();

        assert!(layout.cmdline.contains("pane.root_fs=ext4"));
        assert_eq!(
            layout
                .storage
                .as_ref()
                .unwrap()
                .root_handoff
                .filesystem_hint
                .as_deref(),
            Some("ext4")
        );
        assert_eq!(refreshed.root_filesystem_hint.as_deref(), Some("ext4"));
        assert!(refreshed
            .notes
            .iter()
            .any(|note| note.contains("root filesystem as ext4")));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn storage_backed_kernel_layout_requires_initramfs_driver_bundle() {
        let paths = temp_runtime_paths("kernel-layout-requires-driver");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-base.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        let mut base_image = vec![0_u8; 4 * 1024 * 1024];
        base_image[446] = 0x80;
        base_image[450] = 0x83;
        base_image[454..458].copy_from_slice(&2048_u32.to_le_bytes());
        base_image[458..462].copy_from_slice(&4096_u32.to_le_bytes());
        base_image[510..512].copy_from_slice(&[0x55, 0xaa]);
        std::fs::write(&base, base_image).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
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
        let error = build_kernel_boot_layout(&paths, "pane", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--write-initramfs-driver"));

        let artifacts = build_runtime_artifact_report(&paths);
        assert!(!artifacts.kernel_boot_layout_exists);
        assert!(!artifacts.initramfs_driver_bundle_ready);
        assert!(!artifacts.kernel_boot_layout_ready);
        let native_runtime =
            build_native_runtime_report(true, &artifacts, &test_native_host_report(true));
        assert!(native_runtime
            .blockers
            .iter()
            .any(|blocker| blocker.contains("--write-initramfs-driver")));

        write_pane_initramfs_driver_bundle(&paths).unwrap();
        let error = build_kernel_boot_layout(&paths, "pane", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--register-pane-block-module"));
        let artifacts = build_runtime_artifact_report(&paths);
        let native_runtime =
            build_native_runtime_report(true, &artifacts, &test_native_host_report(true));
        assert!(native_runtime
            .blockers
            .iter()
            .any(|blocker| blocker.contains("--register-pane-block-module")));

        register_fake_pane_block_module(&paths);
        let error = build_kernel_boot_layout(&paths, "pane", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--build-discovery-initramfs"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_layout_rejects_non_disk_base_image_for_native_storage() {
        let paths = temp_runtime_paths("kernel-layout-nondisk-base");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-rootfs.tar");
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&base, b"pane arch rootfs archive placeholder").unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
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
        register_fake_pane_block_module(&paths);
        register_fake_discovery_initramfs(&paths);

        let error = build_kernel_boot_layout(&paths, "pane", true)
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires a verified raw disk base image"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn kernel_boot_layout_uses_partition_root_handoff_when_available() {
        let paths = temp_runtime_paths("kernel-layout-root-handoff");
        super::prepare_runtime_paths(&paths).unwrap();
        let base = paths.downloads.join("arch-root-partition.img");
        let kernel = paths.downloads.join("vmlinuz-linux");
        std::fs::write(&base, fake_mbr_linux_root_disk_image()).unwrap();
        std::fs::write(&kernel, fake_linux_bzimage()).unwrap();
        let base_sha = sha256_file(&base).unwrap();
        let kernel_sha = sha256_file(&kernel).unwrap();

        register_base_os_image(&paths, &base, Some(&base_sha), false, false).unwrap();
        create_user_disk_descriptor(&paths, &runtime_storage_budget(8), false).unwrap();
        write_pane_initramfs_driver_bundle(&paths).unwrap();
        register_kernel_boot_plan(
            &paths,
            Some(&kernel),
            Some(&kernel_sha),
            None,
            None,
            Some("console=ttyS0 rw"),
            false,
        )
        .unwrap();
        register_fake_pane_block_module(&paths);
        register_fake_discovery_initramfs(&paths);

        let layout = build_kernel_boot_layout(&paths, "pane", true).unwrap();
        let storage = layout.storage.as_ref().expect("storage attachment");

        assert_eq!(storage.root_device, "/dev/pane0");
        assert_eq!(storage.root_handoff.mode, "base-partition-direct");
        assert_eq!(storage.root_handoff.root_device, "/dev/pane0");
        assert_eq!(storage.root_handoff.partition_index, Some(1));
        assert_eq!(storage.root_handoff.partition_byte_offset, Some(2048 * 512));
        assert_eq!(storage.root_handoff.partition_byte_length, Some(4096 * 512));
        assert_eq!(
            storage.root_handoff.filesystem_hint.as_deref(),
            Some("ext4")
        );
        assert!(storage.root_handoff.requires_initramfs_driver);
        assert_eq!(
            layout.expected_serial_milestones,
            pane_initramfs_expected_serial_milestones()
        );
        assert!(layout.cmdline.contains("pane.root=/dev/pane0"));
        assert!(layout
            .cmdline
            .contains("pane.root_mode=base-partition-direct"));
        assert!(layout.cmdline.contains("pane.root_fs=ext4"));
        assert!(layout.cmdline.contains("pane.root_partition=1"));
        assert!(layout.cmdline.contains("pane.root_offset=1048576"));
        assert!(layout.cmdline.contains("pane.root_length=2097152"));

        let mapped_regions = linux_guest_mapped_regions(&layout).unwrap();
        let storage_contract = mapped_regions
            .iter()
            .find(|region| region.label == "pane-storage-contract")
            .expect("storage contract region");
        assert!(storage_contract
            .bytes
            .windows("base-partition-direct".len())
            .any(|window| window == b"base-partition-direct"));

        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn native_runtime_reports_host_blocker_after_artifacts_are_ready() {
        let paths = temp_runtime_paths("runtime-host-not-ready");
        super::prepare_runtime_paths(&paths).unwrap();
        let source = paths.downloads.join("arch.img");
        std::fs::write(&source, fake_mbr_linux_root_disk_image()).unwrap();
        let expected = sha256_file(&source).unwrap();

        register_base_os_image(&paths, &source, Some(&expected), true, false).unwrap();
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
    fn native_boot_trace_checkpoint_is_initialized_before_whp_execution() {
        let paths = temp_runtime_paths("native-trace-checkpoint-init");
        let checkpoint = paths.root.join("traces").join("native-boot.json");

        super::initialize_native_boot_trace_checkpoint(&checkpoint, "pane-test").unwrap();

        let value = read_json_file::<serde_json::Value>(&checkpoint).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["kind"], "pane-native-boot-trace-checkpoint");
        assert_eq!(value["reason"], "requested");
        assert_eq!(value["session_name"], "pane-test");
        assert_eq!(value["status"], "pending");

        let _ = std::fs::remove_dir_all(&paths.root);
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
