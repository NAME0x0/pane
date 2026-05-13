use serde::Serialize;

const REQUIRED_WHP_EXPORTS: &[&str] = &[
    "WHvGetCapability",
    "WHvCreatePartition",
    "WHvSetPartitionProperty",
    "WHvSetupPartition",
    "WHvDeletePartition",
    "WHvCreateVirtualProcessor",
    "WHvDeleteVirtualProcessor",
    "WHvSetVirtualProcessorRegisters",
    "WHvRunVirtualProcessor",
    "WHvMapGpaRange",
    "WHvUnmapGpaRange",
];
pub(crate) const SERIAL_BOOT_BANNER_TEXT: &str = "PANE_BOOT_OK\n";
pub(crate) const SERIAL_BOOT_TEST_IMAGE_SIZE: usize = 4096;
pub(crate) const LINUX_BOOT_GDT_GPA: u64 = 0x0000_8000;
pub(crate) const LINUX_BOOT_STACK_GPA: u64 = 0x0008_0000;
pub(crate) const LINUX_BOOT_CODE_SELECTOR: u16 = 0x10;
pub(crate) const LINUX_BOOT_DATA_SELECTOR: u16 = 0x18;
pub(crate) const PANE_BLOCK_IO_BASE_PORT: u16 = 0x0d00;
pub(crate) const PANE_BLOCK_IO_PORT_COUNT: u16 = 0x0010;
pub(crate) const PANE_BLOCK_IO_LAST_PORT: u16 =
    PANE_BLOCK_IO_BASE_PORT + PANE_BLOCK_IO_PORT_COUNT - 1;
pub(crate) const PANE_BLOCK_IO_BLOCK_SIZE_BYTES: u32 = 4096;
pub(crate) const PANE_BLOCK_IO_STATUS_SUBMITTED: u8 = 0x01;
pub(crate) const PANE_BLOCK_IO_STATUS_SERVICED: u8 = 0x02;
pub(crate) const PANE_BLOCK_IO_STATUS_DENIED: u8 = 0xfc;
pub(crate) const PANE_BLOCK_IO_STATUS_FAILED: u8 = 0xfd;
pub(crate) const PANE_BLOCK_IO_STATUS_INVALID: u8 = 0xfe;

#[derive(Clone, Debug)]
pub(crate) struct NativeSerialBootImage {
    pub(crate) source_label: String,
    pub(crate) path: Option<String>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) expected_serial_text: String,
    pub(crate) expected_serial_markers: Vec<String>,
    pub(crate) guest_entry_gpa: u64,
    pub(crate) entry_mode: NativeGuestEntryMode,
    pub(crate) boot_params_gpa: Option<u64>,
    pub(crate) extra_regions: Vec<NativeGuestMemoryRegion>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeGuestEntryMode {
    RealModeSerial,
    LinuxProtectedMode32,
}

impl NativeGuestEntryMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RealModeSerial => "real-mode-serial",
            Self::LinuxProtectedMode32 => "linux-protected-mode-32",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NativeGuestMemoryRegion {
    pub(crate) label: String,
    pub(crate) guest_gpa: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) writable: bool,
    pub(crate) executable: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeBlockDeviceId {
    BaseOs,
    UserDisk,
}

impl NativeBlockDeviceId {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::BaseOs => "pane-base-os",
            Self::UserDisk => "pane-user-disk",
        }
    }

    fn writable(self) -> bool {
        matches!(self, Self::UserDisk)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeBlockOperation {
    Read,
    Write,
}

impl NativeBlockOperation {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBlockIoCommand {
    pub(crate) device: NativeBlockDeviceId,
    pub(crate) operation: NativeBlockOperation,
    pub(crate) block_index: u64,
    pub(crate) block_size_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBlockIoSubmission {
    pub(crate) command: NativeBlockIoCommand,
    pub(crate) write_payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBlockIoDecision {
    pub(crate) allowed: bool,
    pub(crate) status: &'static str,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBlockIoServiceResult {
    pub(crate) decision: NativeBlockIoDecision,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) type NativeBlockIoHandler<'a> =
    dyn Fn(&NativeBlockIoCommand, Option<&[u8]>) -> Result<NativeBlockIoServiceResult, String> + 'a;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBlockIoServiceOutcome {
    pub(crate) report: NativeWhpCallReport,
    pub(crate) status_code: u8,
    pub(crate) response_bytes: Vec<u8>,
}

pub(crate) fn evaluate_native_block_io(command: &NativeBlockIoCommand) -> NativeBlockIoDecision {
    if command.block_size_bytes != PANE_BLOCK_IO_BLOCK_SIZE_BYTES {
        return NativeBlockIoDecision {
            allowed: false,
            status: "unsupported-block-size",
            detail: format!(
                "Pane block I/O requires {} byte blocks; guest requested {} bytes.",
                PANE_BLOCK_IO_BLOCK_SIZE_BYTES, command.block_size_bytes
            ),
        };
    }

    if command.operation == NativeBlockOperation::Write && !command.device.writable() {
        return NativeBlockIoDecision {
            allowed: false,
            status: "readonly-device",
            detail: format!(
                "Pane rejected write to read-only {} block {}.",
                command.device.label(),
                command.block_index
            ),
        };
    }

    NativeBlockIoDecision {
        allowed: true,
        status: "allowed",
        detail: format!(
            "Pane accepted {} for {} block {}.",
            command.operation.label(),
            command.device.label(),
            command.block_index
        ),
    }
}

pub(crate) fn service_native_block_io_command(
    command: &NativeBlockIoCommand,
    handler: Option<&NativeBlockIoHandler<'_>>,
    write_payload: Option<&[u8]>,
) -> NativeBlockIoServiceOutcome {
    let decision = evaluate_native_block_io(command);
    if !decision.allowed {
        return NativeBlockIoServiceOutcome {
            report: NativeWhpCallReport {
                name: "PaneBlockIoPolicyDenied",
                hresult: None,
                ok: false,
                detail: format!(
                    "Pane denied {} for {} block {}; policy={} detail={}.",
                    command.operation.label(),
                    command.device.label(),
                    command.block_index,
                    decision.status,
                    decision.detail
                ),
            },
            status_code: PANE_BLOCK_IO_STATUS_DENIED,
            response_bytes: Vec::new(),
        };
    }

    let Some(handler) = handler else {
        return NativeBlockIoServiceOutcome {
            report: NativeWhpCallReport {
                name: "PaneBlockIoExitPending",
                hresult: None,
                ok: true,
                detail: format!(
                    "Linux submitted Pane block I/O command for {} {} block {}; policy={} detail={}. The next milestone must attach the runtime disk service to this exit.",
                    command.operation.label(),
                    command.device.label(),
                    command.block_index,
                    decision.status,
                    decision.detail
                ),
            },
            status_code: PANE_BLOCK_IO_STATUS_SUBMITTED,
            response_bytes: Vec::new(),
        };
    };

    match handler(command, write_payload) {
        Ok(result) if !result.decision.allowed => NativeBlockIoServiceOutcome {
            report: NativeWhpCallReport {
                name: "PaneBlockIoPolicyDenied",
                hresult: None,
                ok: false,
                detail: format!(
                    "Pane runtime storage denied {} for {} block {}; policy={} detail={}.",
                    command.operation.label(),
                    command.device.label(),
                    command.block_index,
                    result.decision.status,
                    result.decision.detail
                ),
            },
            status_code: PANE_BLOCK_IO_STATUS_DENIED,
            response_bytes: Vec::new(),
        },
        Ok(result) => {
            let expected_len = match command.operation {
                NativeBlockOperation::Read => command.block_size_bytes as usize,
                NativeBlockOperation::Write => 0,
            };
            let ok = result.bytes.len() == expected_len;
            let response_len = result.bytes.len();
            NativeBlockIoServiceOutcome {
                report: NativeWhpCallReport {
                    name: if ok {
                        "PaneBlockIoServiced"
                    } else {
                        "PaneBlockIoServiceFailed"
                    },
                    hresult: None,
                    ok,
                    detail: if ok {
                        format!(
                            "Pane runtime storage serviced {} for {} block {} with {} response bytes.",
                            command.operation.label(),
                            command.device.label(),
                            command.block_index,
                            response_len
                        )
                    } else {
                        format!(
                            "Pane runtime storage returned {} response bytes for {} {} block {}; expected {}.",
                            response_len,
                            command.operation.label(),
                            command.device.label(),
                            command.block_index,
                            expected_len
                        )
                    },
                },
                status_code: if ok {
                    PANE_BLOCK_IO_STATUS_SERVICED
                } else {
                    PANE_BLOCK_IO_STATUS_FAILED
                },
                response_bytes: if ok { result.bytes } else { Vec::new() },
            }
        }
        Err(error) => NativeBlockIoServiceOutcome {
            report: NativeWhpCallReport {
                name: "PaneBlockIoServiceFailed",
                hresult: None,
                ok: false,
                detail: format!(
                    "Pane runtime storage failed {} for {} block {}: {error}",
                    command.operation.label(),
                    command.device.label(),
                    command.block_index
                ),
            },
            status_code: PANE_BLOCK_IO_STATUS_FAILED,
            response_bytes: Vec::new(),
        },
    }
}

pub(crate) fn native_block_io_exit_can_resume(status_code: u8) -> bool {
    status_code == PANE_BLOCK_IO_STATUS_SERVICED
}

pub(crate) fn pane_block_io_port_offset(port: u16) -> Option<u16> {
    if (PANE_BLOCK_IO_BASE_PORT..=PANE_BLOCK_IO_LAST_PORT).contains(&port) {
        Some(port - PANE_BLOCK_IO_BASE_PORT)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NativeBlockIoPortState {
    device: NativeBlockDeviceId,
    operation: NativeBlockOperation,
    block_index_bytes: [u8; 8],
    status: u8,
    response_bytes: Vec<u8>,
    response_cursor: usize,
    write_payload: Vec<u8>,
}

impl Default for NativeBlockIoPortState {
    fn default() -> Self {
        Self {
            device: NativeBlockDeviceId::BaseOs,
            operation: NativeBlockOperation::Read,
            block_index_bytes: [0; 8],
            status: 0,
            response_bytes: Vec::new(),
            response_cursor: 0,
            write_payload: Vec::new(),
        }
    }
}

impl NativeBlockIoPortState {
    fn clear_response(&mut self) {
        self.response_bytes.clear();
        self.response_cursor = 0;
    }

    fn clear_transfer_buffers(&mut self) {
        self.clear_response();
        self.write_payload.clear();
    }

    pub(crate) fn write(&mut self, port: u16, value: u8) -> Option<NativeBlockIoSubmission> {
        match pane_block_io_port_offset(port)? {
            0 => {
                self.clear_transfer_buffers();
                self.device = match value {
                    0 => NativeBlockDeviceId::BaseOs,
                    1 => NativeBlockDeviceId::UserDisk,
                    _ => {
                        self.status = PANE_BLOCK_IO_STATUS_INVALID;
                        return None;
                    }
                };
                self.status = 0;
                None
            }
            1 => {
                self.clear_transfer_buffers();
                self.operation = match value {
                    0 => NativeBlockOperation::Read,
                    1 => NativeBlockOperation::Write,
                    _ => {
                        self.status = PANE_BLOCK_IO_STATUS_INVALID;
                        return None;
                    }
                };
                self.status = 0;
                None
            }
            2 => {
                self.clear_response();
                if value != 1 {
                    self.status = PANE_BLOCK_IO_STATUS_INVALID;
                    return None;
                }
                self.status = PANE_BLOCK_IO_STATUS_SUBMITTED;
                let command = NativeBlockIoCommand {
                    device: self.device,
                    operation: self.operation,
                    block_index: u64::from_le_bytes(self.block_index_bytes),
                    block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
                };
                let write_payload = (self.operation == NativeBlockOperation::Write)
                    .then(|| self.write_payload.clone());
                Some(NativeBlockIoSubmission {
                    command,
                    write_payload,
                })
            }
            4..=11 => {
                self.clear_transfer_buffers();
                self.block_index_bytes[usize::from(pane_block_io_port_offset(port)? - 4)] = value;
                self.status = 0;
                None
            }
            12 => {
                self.clear_response();
                if self.operation != NativeBlockOperation::Write {
                    self.status = PANE_BLOCK_IO_STATUS_INVALID;
                    return None;
                }
                if self.write_payload.len() >= PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize {
                    self.status = PANE_BLOCK_IO_STATUS_INVALID;
                    return None;
                }
                self.write_payload.push(value);
                self.status = 0;
                None
            }
            _ => None,
        }
    }

    pub(crate) fn set_service_result(&mut self, status: u8, response_bytes: Vec<u8>) {
        self.status = status;
        self.response_cursor = 0;
        self.response_bytes = if status == PANE_BLOCK_IO_STATUS_SERVICED {
            response_bytes
        } else {
            Vec::new()
        };
    }

    pub(crate) fn read(&mut self, port: u16) -> u8 {
        match pane_block_io_port_offset(port) {
            Some(0) => match self.device {
                NativeBlockDeviceId::BaseOs => 0,
                NativeBlockDeviceId::UserDisk => 1,
            },
            Some(1) => match self.operation {
                NativeBlockOperation::Read => 0,
                NativeBlockOperation::Write => 1,
            },
            Some(2) => self.status,
            Some(3) => (PANE_BLOCK_IO_BLOCK_SIZE_BYTES / 512) as u8,
            Some(offset @ 4..=11) => self.block_index_bytes[usize::from(offset - 4)],
            Some(12) => {
                let value = self
                    .response_bytes
                    .get(self.response_cursor)
                    .copied()
                    .unwrap_or(0);
                self.response_cursor = self
                    .response_cursor
                    .saturating_add(1)
                    .min(self.response_bytes.len());
                value
            }
            Some(13) => (self.response_bytes.len() & 0xff) as u8,
            Some(14) => ((self.response_bytes.len() >> 8) & 0xff) as u8,
            Some(15) => ((self.response_bytes.len() >> 16) & 0xff) as u8,
            _ => 0,
        }
    }
}

pub(crate) fn serial_boot_test_image_bytes() -> Vec<u8> {
    let mut image = vec![0_u8; SERIAL_BOOT_TEST_IMAGE_SIZE];
    write_serial_boot_test_image(&mut image);
    image
}

pub(crate) fn linux_boot_gdt_page_bytes() -> Vec<u8> {
    let mut page = vec![0_u8; SERIAL_BOOT_TEST_IMAGE_SIZE];
    page[usize::from(LINUX_BOOT_CODE_SELECTOR)..usize::from(LINUX_BOOT_CODE_SELECTOR) + 8]
        .copy_from_slice(&0x00cf_9a00_0000_ffff_u64.to_le_bytes());
    page[usize::from(LINUX_BOOT_DATA_SELECTOR)..usize::from(LINUX_BOOT_DATA_SELECTOR) + 8]
        .copy_from_slice(&0x00cf_9200_0000_ffff_u64.to_le_bytes());
    page
}

fn write_serial_boot_test_image(page: &mut [u8]) {
    let mut offset = 0;
    for byte in SERIAL_BOOT_BANNER_TEXT.as_bytes() {
        let instruction = [
            0xba, 0xf8, 0x03, // mov dx, 0x03f8
            0xb0, *byte, // mov al, byte
            0xee,  // out dx, al
        ];
        page[offset..offset + instruction.len()].copy_from_slice(&instruction);
        offset += instruction.len();
    }
    page[offset] = 0xf4; // hlt after the whole serial banner has been emitted
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeHostPreflightReport {
    pub(crate) product_shape: &'static str,
    pub(crate) host_os: String,
    pub(crate) host_arch: String,
    pub(crate) windows_host: bool,
    pub(crate) supported_arch: bool,
    pub(crate) whp: WhpPreflightReport,
    pub(crate) ready_for_boot_spike: bool,
    pub(crate) checks: Vec<NativePreflightCheck>,
    pub(crate) next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WhpPreflightReport {
    pub(crate) dll_loaded: bool,
    pub(crate) get_capability_available: bool,
    pub(crate) hypervisor_present: Option<bool>,
    pub(crate) get_capability_hresult: Option<String>,
    pub(crate) required_exports: Vec<NativeExportCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeExportCheck {
    pub(crate) symbol: &'static str,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativePreflightCheck {
    pub(crate) id: &'static str,
    pub(crate) status: NativePreflightStatus,
    pub(crate) summary: String,
    pub(crate) remediation: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NativePreflightStatus {
    Pass,
    Fail,
    Skipped,
}

impl NativePreflightStatus {
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativePartitionSmokeReport {
    pub(crate) product_shape: &'static str,
    pub(crate) execute_requested: bool,
    pub(crate) attempted: bool,
    pub(crate) status: NativePartitionSmokeStatus,
    pub(crate) status_label: &'static str,
    pub(crate) partition_created: bool,
    pub(crate) processor_count_configured: bool,
    pub(crate) partition_setup: bool,
    pub(crate) virtual_processor_created: bool,
    pub(crate) virtual_processor_deleted: bool,
    pub(crate) partition_deleted: bool,
    pub(crate) fixture_requested: bool,
    pub(crate) memory_mapped: bool,
    pub(crate) registers_configured: bool,
    pub(crate) virtual_processor_ran: bool,
    pub(crate) memory_unmapped: bool,
    pub(crate) exit_reason: Option<u32>,
    pub(crate) exit_reason_label: Option<String>,
    pub(crate) boot_image_source: Option<String>,
    pub(crate) boot_image_path: Option<String>,
    pub(crate) boot_image_bytes: Option<u64>,
    pub(crate) entry_mode: Option<String>,
    pub(crate) boot_params_gpa: Option<String>,
    pub(crate) guest_regions: Vec<NativeGuestRegionReport>,
    pub(crate) serial_port: Option<u16>,
    pub(crate) serial_byte: Option<u8>,
    pub(crate) serial_bytes: Vec<u8>,
    pub(crate) serial_text: Option<String>,
    pub(crate) serial_expected_text: Option<String>,
    pub(crate) serial_expected_markers: Vec<String>,
    pub(crate) serial_markers_observed: bool,
    pub(crate) serial_io_exit_count: u32,
    pub(crate) guest_exit_count: u32,
    pub(crate) guest_exit_budget: u32,
    pub(crate) framebuffer_snapshot: Option<NativeFramebufferSnapshotReport>,
    pub(crate) input_queue_snapshot: Option<NativeInputQueueSnapshotReport>,
    pub(crate) halt_observed: bool,
    pub(crate) calls: Vec<NativeWhpCallReport>,
    pub(crate) blocker: Option<String>,
    pub(crate) next_step: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeGuestRegionReport {
    pub(crate) label: String,
    pub(crate) guest_gpa: String,
    pub(crate) source_bytes: u64,
    pub(crate) mapped_bytes: u64,
    pub(crate) writable: bool,
    pub(crate) executable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeFramebufferSnapshotReport {
    pub(crate) label: String,
    pub(crate) guest_gpa: String,
    pub(crate) bytes: u64,
    pub(crate) nonzero_bytes: u64,
    pub(crate) first_nonzero_offset: Option<u64>,
    pub(crate) all_zero: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NativeInputQueueSnapshotReport {
    pub(crate) label: String,
    pub(crate) guest_gpa: String,
    pub(crate) bytes: u64,
    pub(crate) nonzero_bytes: u64,
    pub(crate) first_nonzero_offset: Option<u64>,
    pub(crate) all_zero: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NativePartitionSmokeStatus {
    Planned,
    Skipped,
    Pass,
    Fail,
}

impl NativePartitionSmokeStatus {
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Skipped => "skipped",
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct NativeWhpCallReport {
    pub(crate) name: &'static str,
    pub(crate) hresult: Option<String>,
    pub(crate) ok: bool,
    pub(crate) detail: String,
}

pub(crate) fn probe_native_host() -> NativeHostPreflightReport {
    build_native_host_preflight_report(
        std::env::consts::OS.to_string(),
        std::env::consts::ARCH.to_string(),
        cfg!(windows),
        supported_host_arch(std::env::consts::ARCH),
        probe_whp(),
    )
}

pub(crate) fn run_partition_smoke(
    execute: bool,
    run_fixture: bool,
    boot_image: Option<&NativeSerialBootImage>,
    host: &NativeHostPreflightReport,
    block_io_handler: Option<&NativeBlockIoHandler<'_>>,
) -> NativePartitionSmokeReport {
    if !execute {
        return planned_partition_smoke_report(run_fixture);
    }

    if !host.ready_for_boot_spike {
        return skipped_partition_smoke_report(
            true,
            run_fixture,
            boot_image,
            "Native host preflight is not ready; run `pane native-preflight` and resolve failures first.",
        );
    }

    if run_fixture && boot_image.is_none() {
        return skipped_partition_smoke_report(
            true,
            run_fixture,
            None,
            "No runtime-backed boot image was available; resolve the requested runtime artifact blockers before executing WHP.",
        );
    }

    run_whp_partition_smoke(run_fixture, boot_image, block_io_handler)
}

fn supported_host_arch(arch: &str) -> bool {
    matches!(arch, "x86_64" | "aarch64")
}

fn build_native_host_preflight_report(
    host_os: String,
    host_arch: String,
    windows_host: bool,
    supported_arch: bool,
    whp: WhpPreflightReport,
) -> NativeHostPreflightReport {
    let mut checks = Vec::new();

    checks.push(NativePreflightCheck {
        id: "host-os",
        status: if windows_host {
            NativePreflightStatus::Pass
        } else {
            NativePreflightStatus::Fail
        },
        summary: if windows_host {
            "Host OS is Windows, which is the native runtime target.".to_string()
        } else {
            format!("Host OS is {host_os}; the native runtime target is Windows.")
        },
        remediation: if windows_host {
            None
        } else {
            Some(
                "Run Pane native-runtime work on Windows 10/11 with Hyper-V capability."
                    .to_string(),
            )
        },
    });

    checks.push(NativePreflightCheck {
        id: "host-architecture",
        status: if supported_arch {
            NativePreflightStatus::Pass
        } else {
            NativePreflightStatus::Fail
        },
        summary: if supported_arch {
            format!("Host architecture {host_arch} is supported for the native runtime spike.")
        } else {
            format!("Host architecture {host_arch} is not supported for the native runtime spike.")
        },
        remediation: if supported_arch {
            None
        } else {
            Some(
                "Use x86_64 or aarch64 Windows hardware for the Pane-owned runtime path."
                    .to_string(),
            )
        },
    });

    checks.push(NativePreflightCheck {
        id: "whp-library",
        status: if windows_host {
            if whp.dll_loaded {
                NativePreflightStatus::Pass
            } else {
                NativePreflightStatus::Fail
            }
        } else {
            NativePreflightStatus::Skipped
        },
        summary: if !windows_host {
            "Windows Hypervisor Platform is only probed on Windows hosts.".to_string()
        } else if whp.dll_loaded {
            "WinHvPlatform.dll is loadable.".to_string()
        } else {
            "WinHvPlatform.dll could not be loaded.".to_string()
        },
        remediation: if windows_host && !whp.dll_loaded {
            Some(
                "Enable Windows Hypervisor Platform and Virtual Machine Platform, then reboot."
                    .to_string(),
            )
        } else {
            None
        },
    });

    let missing_exports = whp
        .required_exports
        .iter()
        .filter(|export| !export.available)
        .map(|export| export.symbol)
        .collect::<Vec<_>>();

    checks.push(NativePreflightCheck {
        id: "whp-exports",
        status: if !windows_host || !whp.dll_loaded {
            NativePreflightStatus::Skipped
        } else if missing_exports.is_empty() {
            NativePreflightStatus::Pass
        } else {
            NativePreflightStatus::Fail
        },
        summary: if !windows_host || !whp.dll_loaded {
            "WHP exports were not checked because the WHP library is unavailable.".to_string()
        } else if missing_exports.is_empty() {
            "Required WHP symbols are available for a minimal boot-to-serial spike.".to_string()
        } else {
            format!(
                "Missing required WHP symbols: {}.",
                missing_exports.join(", ")
            )
        },
        remediation: if windows_host && whp.dll_loaded && !missing_exports.is_empty() {
            Some(
                "Update Windows or enable the full Windows Hypervisor Platform feature set."
                    .to_string(),
            )
        } else {
            None
        },
    });

    checks.push(NativePreflightCheck {
        id: "whp-hypervisor-present",
        status: if !windows_host || !whp.dll_loaded || !whp.get_capability_available {
            NativePreflightStatus::Skipped
        } else if whp.hypervisor_present == Some(true) {
            NativePreflightStatus::Pass
        } else {
            NativePreflightStatus::Fail
        },
        summary: match (
            windows_host,
            whp.dll_loaded,
            whp.get_capability_available,
            whp.hypervisor_present,
        ) {
            (false, _, _, _) => {
                "Hypervisor presence is only checked on Windows hosts.".to_string()
            }
            (true, false, _, _) => {
                "Hypervisor presence could not be checked because WHP is unavailable.".to_string()
            }
            (true, true, false, _) => {
                "Hypervisor presence could not be checked because WHvGetCapability is missing."
                    .to_string()
            }
            (true, true, true, Some(true)) => {
                "Windows reports that the hypervisor is present.".to_string()
            }
            (true, true, true, Some(false)) => {
                "Windows reports that the hypervisor is not present.".to_string()
            }
            (true, true, true, None) => {
                "WHvGetCapability did not return hypervisor presence.".to_string()
            }
        },
        remediation: if windows_host
            && whp.dll_loaded
            && whp.get_capability_available
            && whp.hypervisor_present != Some(true)
        {
            Some("Enable virtualization in firmware, enable Windows Hypervisor Platform, and reboot.".to_string())
        } else {
            None
        },
    });

    let ready_for_boot_spike = checks
        .iter()
        .filter(|check| check.id != "whp-hypervisor-present" || whp.get_capability_available)
        .all(|check| check.status == NativePreflightStatus::Pass);

    let mut next_steps = Vec::new();
    if !ready_for_boot_spike {
        next_steps.push(
            "Resolve failing native host checks before attempting a Pane-owned boot-to-serial spike."
                .to_string(),
        );
    }
    next_steps.extend([
        "Run `pane native-boot-spike --execute --run-fixture` to prove WHP guest memory, register setup, vCPU execution, and serial I/O on this host."
            .to_string(),
        "Replace the synthetic serial test image with a boot-to-serial kernel or loader."
            .to_string(),
        "Connect the serial boot spike to Pane runtime artifacts instead of WSL distro state."
            .to_string(),
        "Only after boot is measurable, add a Pane-owned framebuffer/input path for the contained app window."
            .to_string(),
    ]);

    NativeHostPreflightReport {
        product_shape: "Native host capability preflight for Pane's future WHP-backed OS runtime.",
        host_os,
        host_arch,
        windows_host,
        supported_arch,
        whp,
        ready_for_boot_spike,
        checks,
        next_steps,
    }
}

fn base_export_checks(available: bool) -> Vec<NativeExportCheck> {
    REQUIRED_WHP_EXPORTS
        .iter()
        .map(|symbol| NativeExportCheck { symbol, available })
        .collect()
}

fn planned_partition_smoke_report(run_fixture: bool) -> NativePartitionSmokeReport {
    NativePartitionSmokeReport {
        product_shape: "Non-persistent WHP boot-spike step for Pane's boot-to-serial milestone.",
        execute_requested: false,
        attempted: false,
        status: NativePartitionSmokeStatus::Planned,
        status_label: NativePartitionSmokeStatus::Planned.display_name(),
        partition_created: false,
        processor_count_configured: false,
        partition_setup: false,
        virtual_processor_created: false,
        virtual_processor_deleted: false,
        partition_deleted: false,
        fixture_requested: run_fixture,
        memory_mapped: false,
        registers_configured: false,
        virtual_processor_ran: false,
        memory_unmapped: false,
        exit_reason: None,
        exit_reason_label: None,
        boot_image_source: None,
        boot_image_path: None,
        boot_image_bytes: None,
        entry_mode: None,
        boot_params_gpa: None,
        guest_regions: Vec::new(),
        serial_port: None,
        serial_byte: None,
        serial_bytes: Vec::new(),
        serial_text: None,
        serial_expected_text: run_fixture.then(|| SERIAL_BOOT_BANNER_TEXT.to_string()),
        serial_expected_markers: Vec::new(),
        serial_markers_observed: false,
        serial_io_exit_count: 0,
        guest_exit_count: 0,
        guest_exit_budget: 0,
        framebuffer_snapshot: None,
        input_queue_snapshot: None,
        halt_observed: false,
        calls: Vec::new(),
        blocker: None,
        next_step: if run_fixture {
            "Rerun with `--execute --run-fixture` to create the WHP partition/vCPU and execute the deterministic serial test image."
                .to_string()
        } else {
            "Rerun with `--execute` to create and tear down a WHP partition and vCPU.".to_string()
        },
    }
}

fn skipped_partition_smoke_report(
    execute_requested: bool,
    run_fixture: bool,
    boot_image: Option<&NativeSerialBootImage>,
    blocker: impl Into<String>,
) -> NativePartitionSmokeReport {
    NativePartitionSmokeReport {
        product_shape: "Non-persistent WHP boot-spike step for Pane's boot-to-serial milestone.",
        execute_requested,
        attempted: false,
        status: NativePartitionSmokeStatus::Skipped,
        status_label: NativePartitionSmokeStatus::Skipped.display_name(),
        partition_created: false,
        processor_count_configured: false,
        partition_setup: false,
        virtual_processor_created: false,
        virtual_processor_deleted: false,
        partition_deleted: false,
        fixture_requested: run_fixture,
        memory_mapped: false,
        registers_configured: false,
        virtual_processor_ran: false,
        memory_unmapped: false,
        exit_reason: None,
        exit_reason_label: None,
        boot_image_source: boot_image.map(|image| image.source_label.clone()),
        boot_image_path: boot_image.and_then(|image| image.path.clone()),
        boot_image_bytes: boot_image.map(|image| image.bytes.len() as u64),
        entry_mode: boot_image.map(|image| image.entry_mode.label().to_string()),
        boot_params_gpa: boot_image
            .and_then(|image| image.boot_params_gpa)
            .map(|gpa| format!("{gpa:#010x}")),
        guest_regions: Vec::new(),
        serial_port: None,
        serial_byte: None,
        serial_bytes: Vec::new(),
        serial_text: None,
        serial_expected_text: boot_image.and_then(|image| {
            (image.entry_mode == NativeGuestEntryMode::RealModeSerial)
                .then(|| image.expected_serial_text.clone())
        }),
        serial_expected_markers: boot_image
            .map(|image| image.expected_serial_markers.clone())
            .unwrap_or_default(),
        serial_markers_observed: false,
        serial_io_exit_count: 0,
        guest_exit_count: 0,
        guest_exit_budget: 0,
        framebuffer_snapshot: None,
        input_queue_snapshot: None,
        halt_observed: false,
        calls: Vec::new(),
        blocker: Some(blocker.into()),
        next_step: if run_fixture {
            "Resolve the blocker, then rerun `pane native-boot-spike --execute --run-fixture`."
                .to_string()
        } else {
            "Resolve the blocker, then rerun `pane native-boot-spike --execute`.".to_string()
        },
    }
}

#[cfg(not(windows))]
fn probe_whp() -> WhpPreflightReport {
    WhpPreflightReport {
        dll_loaded: false,
        get_capability_available: false,
        hypervisor_present: None,
        get_capability_hresult: None,
        required_exports: base_export_checks(false),
    }
}

#[cfg(not(windows))]
fn run_whp_partition_smoke(
    run_fixture: bool,
    boot_image: Option<&NativeSerialBootImage>,
    _block_io_handler: Option<&NativeBlockIoHandler<'_>>,
) -> NativePartitionSmokeReport {
    skipped_partition_smoke_report(
        true,
        run_fixture,
        boot_image,
        "WHP partition smoke can only run on Windows hosts.",
    )
}

#[cfg(windows)]
fn probe_whp() -> WhpPreflightReport {
    windows_whp::probe_whp()
}

#[cfg(windows)]
fn run_whp_partition_smoke(
    run_fixture: bool,
    boot_image: Option<&NativeSerialBootImage>,
    block_io_handler: Option<&NativeBlockIoHandler<'_>>,
) -> NativePartitionSmokeReport {
    windows_whp::run_partition_smoke(run_fixture, boot_image, block_io_handler)
}

#[cfg(test)]
pub(crate) fn test_native_host_report(ready: bool) -> NativeHostPreflightReport {
    let whp = WhpPreflightReport {
        dll_loaded: ready,
        get_capability_available: ready,
        hypervisor_present: Some(ready),
        get_capability_hresult: Some("0x00000000".to_string()),
        required_exports: base_export_checks(ready),
    };

    build_native_host_preflight_report("windows".to_string(), "x86_64".to_string(), true, true, whp)
}

#[cfg(windows)]
mod windows_whp {
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        collections::HashMap,
        ffi::{c_char, c_void, CString},
        mem,
    };

    use super::{
        base_export_checks, NativeBlockIoHandler, NativeExportCheck,
        NativeFramebufferSnapshotReport, NativeGuestEntryMode, NativeGuestMemoryRegion,
        NativeGuestRegionReport, NativeInputQueueSnapshotReport, NativePartitionSmokeReport,
        NativePartitionSmokeStatus, NativeSerialBootImage, NativeWhpCallReport, WhpPreflightReport,
        LINUX_BOOT_CODE_SELECTOR, LINUX_BOOT_DATA_SELECTOR, LINUX_BOOT_GDT_GPA,
        LINUX_BOOT_STACK_GPA, REQUIRED_WHP_EXPORTS, SERIAL_BOOT_BANNER_TEXT,
        SERIAL_BOOT_TEST_IMAGE_SIZE,
    };
    use crate::native::{
        native_block_io_exit_can_resume, pane_block_io_port_offset,
        service_native_block_io_command, NativeBlockIoPortState,
    };

    const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 = 0;
    const WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT: u32 = 0x0000_1fff;
    const WHV_MAP_GPA_RANGE_FLAG_READ: u32 = 0x0000_0001;
    const WHV_MAP_GPA_RANGE_FLAG_WRITE: u32 = 0x0000_0002;
    const WHV_MAP_GPA_RANGE_FLAG_EXECUTE: u32 = 0x0000_0004;
    const WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS: u32 = 0x0000_0001;
    const WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS: u32 = 0x0000_0002;
    const WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION: u32 = 0x0000_0004;
    const WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE: u32 = 0x0000_0005;
    const WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW: u32 = 0x0000_0007;
    const WHV_RUN_VP_EXIT_REASON_X64_HALT: u32 = 0x0000_0008;
    const WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI: u32 = 0x0000_0009;
    const WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS: u32 = 0x0000_1000;
    const WHV_RUN_VP_EXIT_REASON_X64_CPUID: u32 = 0x0000_1001;
    const GUEST_PAGE_SIZE: usize = SERIAL_BOOT_TEST_IMAGE_SIZE;
    const LINUX_ENTRY_PROBE_EXIT_BUDGET: usize = 4096;
    const LINUX_ENTRY_PROBE_MINIMAL_EXIT_BUDGET: usize = 256;
    const SERIAL_COM1_PORT: u16 = 0x03f8;
    const SERIAL_COM1_LAST_PORT: u16 = SERIAL_COM1_PORT + 7;
    const SERIAL_LINE_STATUS_PORT: u16 = SERIAL_COM1_PORT + 5;
    const SERIAL_INTERRUPT_ID_PORT: u16 = SERIAL_COM1_PORT + 2;
    const SERIAL_MODEM_STATUS_PORT: u16 = SERIAL_COM1_PORT + 6;
    const PIC1_COMMAND_PORT: u16 = 0x0020;
    const PIC1_DATA_PORT: u16 = 0x0021;
    const PIT_CHANNEL0_PORT: u16 = 0x0040;
    const PIT_CHANNEL1_PORT: u16 = 0x0041;
    const PIT_CHANNEL2_PORT: u16 = 0x0042;
    const PIT_COMMAND_PORT: u16 = 0x0043;
    const PS2_DATA_PORT: u16 = 0x0060;
    const PS2_STATUS_COMMAND_PORT: u16 = 0x0064;
    const SYSTEM_CONTROL_PORT_B: u16 = 0x0061;
    const CMOS_ADDRESS_PORT: u16 = 0x0070;
    const CMOS_DATA_PORT: u16 = 0x0071;
    const POST_DELAY_PORT: u16 = 0x0080;
    const SYSTEM_CONTROL_PORT_A: u16 = 0x0092;
    const PIC2_COMMAND_PORT: u16 = 0x00a0;
    const PIC2_DATA_PORT: u16 = 0x00a1;
    const ALT_POST_DELAY_PORT: u16 = 0x00eb;
    const ALT_DELAY_PORT: u16 = 0x00ed;
    const VGA_ATTRIBUTE_PORT: u16 = 0x03c0;
    const VGA_ATTRIBUTE_DATA_READ_PORT: u16 = 0x03c1;
    const VGA_MISC_OUTPUT_WRITE_PORT: u16 = 0x03c2;
    const VGA_SEQUENCER_INDEX_PORT: u16 = 0x03c4;
    const VGA_SEQUENCER_DATA_PORT: u16 = 0x03c5;
    const VGA_DAC_MASK_PORT: u16 = 0x03c6;
    const VGA_DAC_STATE_PORT: u16 = 0x03c7;
    const VGA_DAC_WRITE_INDEX_PORT: u16 = 0x03c8;
    const VGA_DAC_DATA_PORT: u16 = 0x03c9;
    const VGA_MISC_OUTPUT_READ_PORT: u16 = 0x03cc;
    const VGA_GRAPHICS_INDEX_PORT: u16 = 0x03ce;
    const VGA_GRAPHICS_DATA_PORT: u16 = 0x03cf;
    const VGA_CRTC_MONO_INDEX_PORT: u16 = 0x03b4;
    const VGA_CRTC_MONO_DATA_PORT: u16 = 0x03b5;
    const VGA_INPUT_STATUS_MONO_PORT: u16 = 0x03ba;
    const VGA_CRTC_COLOR_INDEX_PORT: u16 = 0x03d4;
    const VGA_CRTC_COLOR_DATA_PORT: u16 = 0x03d5;
    const VGA_INPUT_STATUS_COLOR_PORT: u16 = 0x03da;
    const ELCR1_PORT: u16 = 0x04d0;
    const ELCR2_PORT: u16 = 0x04d1;
    const PCI_CONFIG_ADDRESS_PORT: u16 = 0x0cf8;
    const PCI_CONFIG_ADDRESS_END_PORT: u16 = 0x0cfb;
    const PCI_CONFIG_DATA_START_PORT: u16 = 0x0cfc;
    const PCI_CONFIG_DATA_END_PORT: u16 = 0x0cff;
    const VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET: usize = 10;
    const VP_CONTEXT_RIP_OFFSET: usize = 32;
    const MEMORY_CONTEXT_OFFSET: usize = 48;
    const MEMORY_ACCESS_INFO_OFFSET: usize = MEMORY_CONTEXT_OFFSET + 20;
    const MEMORY_GPA_OFFSET: usize = MEMORY_CONTEXT_OFFSET + 24;
    const MEMORY_GVA_OFFSET: usize = MEMORY_CONTEXT_OFFSET + 32;
    const IO_CONTEXT_OFFSET: usize = 48;
    const IO_ACCESS_INFO_OFFSET: usize = IO_CONTEXT_OFFSET + 20;
    const IO_PORT_OFFSET: usize = IO_CONTEXT_OFFSET + 24;
    const IO_RAX_OFFSET: usize = IO_CONTEXT_OFFSET + 32;
    const CPUID_CONTEXT_OFFSET: usize = 48;
    const CPUID_RAX_OFFSET: usize = CPUID_CONTEXT_OFFSET;
    const CPUID_RCX_OFFSET: usize = CPUID_CONTEXT_OFFSET + 8;
    const CPUID_DEFAULT_RAX_OFFSET: usize = CPUID_CONTEXT_OFFSET + 32;
    const CPUID_DEFAULT_RCX_OFFSET: usize = CPUID_CONTEXT_OFFSET + 40;
    const CPUID_DEFAULT_RDX_OFFSET: usize = CPUID_CONTEXT_OFFSET + 48;
    const CPUID_DEFAULT_RBX_OFFSET: usize = CPUID_CONTEXT_OFFSET + 56;
    const MSR_CONTEXT_OFFSET: usize = 48;
    const MSR_ACCESS_INFO_OFFSET: usize = MSR_CONTEXT_OFFSET;
    const MSR_NUMBER_OFFSET: usize = MSR_CONTEXT_OFFSET + 4;
    const MSR_RAX_OFFSET: usize = MSR_CONTEXT_OFFSET + 8;
    const MSR_RDX_OFFSET: usize = MSR_CONTEXT_OFFSET + 16;
    const WHV_REGISTER_RAX: u32 = 0x0000_0000;
    const WHV_REGISTER_RCX: u32 = 0x0000_0001;
    const WHV_REGISTER_RDX: u32 = 0x0000_0002;
    const WHV_REGISTER_RBX: u32 = 0x0000_0003;
    const WHV_REGISTER_RSP: u32 = 0x0000_0004;
    const WHV_REGISTER_RBP: u32 = 0x0000_0005;
    const WHV_REGISTER_RSI: u32 = 0x0000_0006;
    const WHV_REGISTER_RDI: u32 = 0x0000_0007;
    const WHV_REGISTER_RIP: u32 = 0x0000_0010;
    const WHV_REGISTER_RFLAGS: u32 = 0x0000_0011;
    const WHV_REGISTER_ES: u32 = 0x0000_0012;
    const WHV_REGISTER_CS: u32 = 0x0000_0013;
    const WHV_REGISTER_SS: u32 = 0x0000_0014;
    const WHV_REGISTER_DS: u32 = 0x0000_0015;
    const WHV_REGISTER_IDTR: u32 = 0x0000_001a;
    const WHV_REGISTER_GDTR: u32 = 0x0000_001b;
    const WHV_REGISTER_CR0: u32 = 0x0000_001c;
    const WHV_REGISTER_CR3: u32 = 0x0000_001e;
    const WHV_REGISTER_CR4: u32 = 0x0000_001f;
    type WhvGetCapability = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
    type WhvCreatePartition = unsafe extern "system" fn(*mut *mut c_void) -> i32;
    type WhvSetPartitionProperty =
        unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32) -> i32;
    type WhvSetupPartition = unsafe extern "system" fn(*mut c_void) -> i32;
    type WhvDeletePartition = unsafe extern "system" fn(*mut c_void) -> i32;
    type WhvCreateVirtualProcessor = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
    type WhvDeleteVirtualProcessor = unsafe extern "system" fn(*mut c_void, u32) -> i32;
    type WhvSetVirtualProcessorRegisters = unsafe extern "system" fn(
        *mut c_void,
        u32,
        *const u32,
        u32,
        *const WhvRegisterValue,
    ) -> i32;
    type WhvRunVirtualProcessor =
        unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u32) -> i32;
    type WhvMapGpaRange = unsafe extern "system" fn(*mut c_void, *mut c_void, u64, u64, u32) -> i32;
    type WhvUnmapGpaRange = unsafe extern "system" fn(*mut c_void, u64, u64) -> i32;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct WhvX64SegmentRegister {
        base: u64,
        limit: u32,
        selector: u16,
        attributes: u16,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct WhvX64TableRegister {
        pad: [u16; 3],
        limit: u16,
        base: u64,
    }

    #[repr(C, align(16))]
    #[derive(Copy, Clone)]
    union WhvRegisterValue {
        reg64: u64,
        segment: WhvX64SegmentRegister,
        table: WhvX64TableRegister,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const c_char) -> *mut c_void;
        fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const c_char) -> *mut c_void;
        fn FreeLibrary(h_lib_module: *mut c_void) -> i32;
    }

    pub(super) fn probe_whp() -> WhpPreflightReport {
        unsafe {
            let library_name = CString::new("WinHvPlatform.dll").expect("static string");
            let module = LoadLibraryA(library_name.as_ptr());
            if module.is_null() {
                return WhpPreflightReport {
                    dll_loaded: false,
                    get_capability_available: false,
                    hypervisor_present: None,
                    get_capability_hresult: None,
                    required_exports: base_export_checks(false),
                };
            }

            let required_exports = REQUIRED_WHP_EXPORTS
                .iter()
                .map(|symbol| NativeExportCheck {
                    symbol,
                    available: get_proc_address(module, symbol).is_some(),
                })
                .collect::<Vec<_>>();

            let get_capability = get_proc_address(module, "WHvGetCapability");
            let (hypervisor_present, get_capability_hresult) = if let Some(pointer) = get_capability
            {
                let function = mem::transmute::<*mut c_void, WhvGetCapability>(pointer);
                let mut capability_value: i32 = 0;
                let mut written_size: u32 = 0;
                let hresult = function(
                    WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT,
                    (&mut capability_value as *mut i32).cast::<c_void>(),
                    mem::size_of::<i32>() as u32,
                    &mut written_size,
                );

                if hresult >= 0 {
                    (Some(capability_value != 0), Some(format_hresult(hresult)))
                } else {
                    (None, Some(format_hresult(hresult)))
                }
            } else {
                (None, None)
            };

            FreeLibrary(module);

            WhpPreflightReport {
                dll_loaded: true,
                get_capability_available: get_capability.is_some(),
                hypervisor_present,
                get_capability_hresult,
                required_exports,
            }
        }
    }

    pub(super) fn run_partition_smoke(
        run_fixture: bool,
        boot_image: Option<&NativeSerialBootImage>,
        block_io_handler: Option<&NativeBlockIoHandler<'_>>,
    ) -> NativePartitionSmokeReport {
        let mut report = NativePartitionSmokeReport {
            product_shape:
                "Non-persistent WHP boot-spike step for Pane's boot-to-serial milestone.",
            execute_requested: true,
            attempted: true,
            status: NativePartitionSmokeStatus::Fail,
            status_label: NativePartitionSmokeStatus::Fail.display_name(),
            partition_created: false,
            processor_count_configured: false,
            partition_setup: false,
            virtual_processor_created: false,
            virtual_processor_deleted: false,
            partition_deleted: false,
            fixture_requested: run_fixture,
            memory_mapped: false,
            registers_configured: false,
            virtual_processor_ran: false,
            memory_unmapped: false,
            exit_reason: None,
            exit_reason_label: None,
            boot_image_source: boot_image.map(|image| image.source_label.clone()),
            boot_image_path: boot_image.and_then(|image| image.path.clone()),
            boot_image_bytes: boot_image.map(|image| image.bytes.len() as u64),
            entry_mode: boot_image.map(|image| image.entry_mode.label().to_string()),
            boot_params_gpa: boot_image
                .and_then(|image| image.boot_params_gpa)
                .map(|gpa| format!("{gpa:#010x}")),
            guest_regions: Vec::new(),
            serial_port: None,
            serial_byte: None,
            serial_bytes: Vec::new(),
            serial_text: None,
            serial_expected_text: boot_image
                .and_then(|image| {
                    (image.entry_mode == NativeGuestEntryMode::RealModeSerial)
                        .then(|| image.expected_serial_text.clone())
                })
                .or_else(|| run_fixture.then(|| SERIAL_BOOT_BANNER_TEXT.to_string())),
            serial_expected_markers: boot_image
                .map(|image| image.expected_serial_markers.clone())
                .unwrap_or_default(),
            serial_markers_observed: false,
            serial_io_exit_count: 0,
            guest_exit_count: 0,
            guest_exit_budget: 0,
            framebuffer_snapshot: None,
            input_queue_snapshot: None,
            halt_observed: false,
            calls: Vec::new(),
            blocker: None,
            next_step: if run_fixture {
                "After this passes, replace the controlled boot image with a boot-to-serial kernel or loader."
                    .to_string()
            } else {
                "After this passes, rerun with `--run-fixture` to execute a deterministic serial test image."
                    .to_string()
            },
        };

        unsafe {
            let library_name = CString::new("WinHvPlatform.dll").expect("static string");
            let module = LoadLibraryA(library_name.as_ptr());
            if module.is_null() {
                report.calls.push(NativeWhpCallReport {
                    name: "LoadLibraryA(WinHvPlatform.dll)",
                    hresult: None,
                    ok: false,
                    detail: "WinHvPlatform.dll could not be loaded.".to_string(),
                });
                report.blocker = Some(
                    "Enable Windows Hypervisor Platform and Virtual Machine Platform, then reboot."
                        .to_string(),
                );
                return report;
            }

            let create_partition = match resolve_whp_function::<WhvCreatePartition>(
                module,
                "WHvCreatePartition",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let set_partition_property = match resolve_whp_function::<WhvSetPartitionProperty>(
                module,
                "WHvSetPartitionProperty",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let setup_partition = match resolve_whp_function::<WhvSetupPartition>(
                module,
                "WHvSetupPartition",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let delete_partition = match resolve_whp_function::<WhvDeletePartition>(
                module,
                "WHvDeletePartition",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let create_virtual_processor = match resolve_whp_function::<WhvCreateVirtualProcessor>(
                module,
                "WHvCreateVirtualProcessor",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let delete_virtual_processor = match resolve_whp_function::<WhvDeleteVirtualProcessor>(
                module,
                "WHvDeleteVirtualProcessor",
                &mut report,
            ) {
                Some(function) => function,
                None => {
                    FreeLibrary(module);
                    return report;
                }
            };
            let set_virtual_processor_registers = if run_fixture {
                match resolve_whp_function::<WhvSetVirtualProcessorRegisters>(
                    module,
                    "WHvSetVirtualProcessorRegisters",
                    &mut report,
                ) {
                    Some(function) => Some(function),
                    None => {
                        FreeLibrary(module);
                        return report;
                    }
                }
            } else {
                None
            };
            let run_virtual_processor = if run_fixture {
                match resolve_whp_function::<WhvRunVirtualProcessor>(
                    module,
                    "WHvRunVirtualProcessor",
                    &mut report,
                ) {
                    Some(function) => Some(function),
                    None => {
                        FreeLibrary(module);
                        return report;
                    }
                }
            } else {
                None
            };
            let map_gpa_range = if run_fixture {
                match resolve_whp_function::<WhvMapGpaRange>(module, "WHvMapGpaRange", &mut report)
                {
                    Some(function) => Some(function),
                    None => {
                        FreeLibrary(module);
                        return report;
                    }
                }
            } else {
                None
            };
            let unmap_gpa_range = if run_fixture {
                match resolve_whp_function::<WhvUnmapGpaRange>(
                    module,
                    "WHvUnmapGpaRange",
                    &mut report,
                ) {
                    Some(function) => Some(function),
                    None => {
                        FreeLibrary(module);
                        return report;
                    }
                }
            } else {
                None
            };

            let mut partition: *mut c_void = std::ptr::null_mut();
            let hresult = create_partition(&mut partition);
            report.partition_created = hresult_succeeded(hresult) && !partition.is_null();
            report.calls.push(hresult_call(
                "WHvCreatePartition",
                hresult,
                if report.partition_created {
                    "Partition object created."
                } else {
                    "Partition object could not be created."
                },
            ));

            if report.partition_created {
                let processor_count = 1_u32;
                let hresult = set_partition_property(
                    partition,
                    WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT,
                    (&processor_count as *const u32).cast::<c_void>(),
                    mem::size_of::<u32>() as u32,
                );
                report.processor_count_configured = hresult_succeeded(hresult);
                report.calls.push(hresult_call(
                    "WHvSetPartitionProperty(ProcessorCount=1)",
                    hresult,
                    if report.processor_count_configured {
                        "Configured the partition for one virtual processor."
                    } else {
                        "Could not configure the partition processor count."
                    },
                ));
            }

            if report.partition_created && report.processor_count_configured {
                let hresult = setup_partition(partition);
                report.partition_setup = hresult_succeeded(hresult);
                report.calls.push(hresult_call(
                    "WHvSetupPartition",
                    hresult,
                    if report.partition_setup {
                        "Hypervisor partition setup completed."
                    } else {
                        "Hypervisor partition setup failed."
                    },
                ));
            }

            if report.partition_created && report.partition_setup {
                let hresult = create_virtual_processor(partition, 0, 0);
                report.virtual_processor_created = hresult_succeeded(hresult);
                report.calls.push(hresult_call(
                    "WHvCreateVirtualProcessor(0)",
                    hresult,
                    if report.virtual_processor_created {
                        "Virtual processor 0 created."
                    } else {
                        "Virtual processor 0 could not be created."
                    },
                ));
            }

            let guest_regions = if run_fixture && report.virtual_processor_created {
                if let Some(map_gpa_range) = map_gpa_range {
                    map_boot_image_regions(partition, map_gpa_range, boot_image, &mut report)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            if run_fixture && report.memory_mapped {
                if let Some(set_virtual_processor_registers) = set_virtual_processor_registers {
                    if let Some(boot_image) = boot_image {
                        let (register_names, register_values) = boot_image_registers(boot_image);
                        let hresult = set_virtual_processor_registers(
                            partition,
                            0,
                            register_names.as_ptr(),
                            register_names.len() as u32,
                            register_values.as_ptr(),
                        );
                        report.registers_configured = hresult_succeeded(hresult);
                        report.calls.push(hresult_call(
                            guest_entry_register_call_name(boot_image.entry_mode),
                            hresult,
                            if report.registers_configured {
                                "Configured vCPU registers for the selected guest entry mode."
                            } else {
                                "Could not configure vCPU registers for the selected guest entry mode."
                            },
                        ));
                    }
                }
            }

            if run_fixture && report.registers_configured {
                if let (Some(run_virtual_processor), Some(set_virtual_processor_registers)) =
                    (run_virtual_processor, set_virtual_processor_registers)
                {
                    if let Some(boot_image) = boot_image {
                        run_guest_image_until_boundary(
                            partition,
                            run_virtual_processor,
                            set_virtual_processor_registers,
                            boot_image,
                            block_io_handler,
                            &mut report,
                        );
                    }
                }
            }

            if run_fixture && report.memory_mapped {
                capture_runtime_surface_snapshots(&guest_regions, &mut report);
            }

            if run_fixture && report.memory_mapped {
                if let Some(unmap_gpa_range) = unmap_gpa_range {
                    let mut unmapped_all = true;
                    for region in guest_regions.iter().rev() {
                        let hresult = unmap_gpa_range(partition, region.guest_gpa, region.size);
                        let ok = hresult_succeeded(hresult);
                        unmapped_all &= ok;
                        report.calls.push(hresult_call(
                            "WHvUnmapGpaRange(guest-region)",
                            hresult,
                            if ok {
                                "Unmapped a Pane guest memory region."
                            } else {
                                "Could not unmap a Pane guest memory region cleanly."
                            },
                        ));
                    }
                    report.memory_unmapped = unmapped_all;
                }
            }

            drop(guest_regions);

            if report.partition_created && report.virtual_processor_created {
                let hresult = delete_virtual_processor(partition, 0);
                report.virtual_processor_deleted = hresult_succeeded(hresult);
                report.calls.push(hresult_call(
                    "WHvDeleteVirtualProcessor(0)",
                    hresult,
                    if report.virtual_processor_deleted {
                        "Virtual processor 0 deleted."
                    } else {
                        "Virtual processor 0 could not be deleted cleanly."
                    },
                ));
            }

            if report.partition_created {
                let hresult = delete_partition(partition);
                report.partition_deleted = hresult_succeeded(hresult);
                report.calls.push(hresult_call(
                    "WHvDeletePartition",
                    hresult,
                    if report.partition_deleted {
                        "Partition deleted and resources released."
                    } else {
                        "Partition cleanup failed."
                    },
                ));
            }

            FreeLibrary(module);
        }

        let selected_entry_mode = boot_image
            .map(|image| image.entry_mode)
            .unwrap_or(NativeGuestEntryMode::RealModeSerial);
        let passed = report.partition_created
            && report.processor_count_configured
            && report.partition_setup
            && report.virtual_processor_created
            && report.virtual_processor_deleted
            && report.partition_deleted
            && (!report.fixture_requested || guest_contract_passed(&report, selected_entry_mode));

        report.status = if passed {
            NativePartitionSmokeStatus::Pass
        } else {
            NativePartitionSmokeStatus::Fail
        };
        report.status_label = report.status.display_name();
        if !passed && report.blocker.is_none() {
            report.blocker = Some(
                "WHP partition/vCPU lifecycle did not complete; inspect the HRESULT call list."
                    .to_string(),
            );
        }
        report
    }

    unsafe fn get_proc_address(module: *mut c_void, symbol: &str) -> Option<*mut c_void> {
        let symbol = CString::new(symbol).expect("static symbol");
        let pointer = GetProcAddress(module, symbol.as_ptr());
        if pointer.is_null() {
            None
        } else {
            Some(pointer)
        }
    }

    fn format_hresult(value: i32) -> String {
        format!("0x{:08X}", value as u32)
    }

    unsafe fn resolve_whp_function<T>(
        module: *mut c_void,
        symbol: &'static str,
        report: &mut NativePartitionSmokeReport,
    ) -> Option<T> {
        let pointer = get_proc_address(module, symbol);
        report.calls.push(NativeWhpCallReport {
            name: symbol,
            hresult: None,
            ok: pointer.is_some(),
            detail: if pointer.is_some() {
                "Resolved WHP export.".to_string()
            } else {
                "Missing required WHP export.".to_string()
            },
        });

        if let Some(pointer) = pointer {
            Some(mem::transmute_copy::<*mut c_void, T>(&pointer))
        } else {
            report.blocker = Some(format!("Missing required WHP export `{symbol}`."));
            None
        }
    }

    fn hresult_succeeded(value: i32) -> bool {
        value >= 0
    }

    fn hresult_call(name: &'static str, hresult: i32, detail: &str) -> NativeWhpCallReport {
        NativeWhpCallReport {
            name,
            hresult: Some(format_hresult(hresult)),
            ok: hresult_succeeded(hresult),
            detail: detail.to_string(),
        }
    }

    struct MappedGuestRegion {
        label: String,
        guest_gpa: u64,
        size: u64,
        _memory: GuestMemory,
    }

    struct GuestMemory {
        ptr: *mut u8,
        layout: Layout,
        size: usize,
    }

    impl GuestMemory {
        fn new(size: usize) -> Option<Self> {
            let size = page_aligned_len(size)?;
            let layout = Layout::from_size_align(size, GUEST_PAGE_SIZE).ok()?;
            let ptr = unsafe { alloc_zeroed(layout) };
            if ptr.is_null() {
                None
            } else {
                Some(Self { ptr, layout, size })
            }
        }

        fn as_mut_ptr(&mut self) -> *mut u8 {
            self.ptr
        }

        fn as_mut_slice(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
        }

        fn as_slice(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
        }
    }

    impl Drop for GuestMemory {
        fn drop(&mut self) {
            unsafe {
                dealloc(self.ptr, self.layout);
            }
        }
    }

    fn page_aligned_len(size: usize) -> Option<usize> {
        if size == 0 {
            return None;
        }
        let remainder = size % GUEST_PAGE_SIZE;
        if remainder == 0 {
            Some(size)
        } else {
            size.checked_add(GUEST_PAGE_SIZE - remainder)
        }
    }

    fn map_boot_image_regions(
        partition: *mut c_void,
        map_gpa_range: WhvMapGpaRange,
        boot_image: Option<&NativeSerialBootImage>,
        report: &mut NativePartitionSmokeReport,
    ) -> Vec<MappedGuestRegion> {
        let Some(boot_image) = boot_image else {
            report.calls.push(NativeWhpCallReport {
                name: "SerialBootImage",
                hresult: None,
                ok: false,
                detail: "No runtime-backed boot image was provided to the WHP runner.".to_string(),
            });
            report.blocker = Some(
                "Create the Pane serial boot image with `pane runtime --create-serial-boot-image`, or register a loader with `pane runtime --register-boot-loader`."
                    .to_string(),
            );
            return Vec::new();
        };
        if boot_image.bytes.is_empty() {
            report.calls.push(NativeWhpCallReport {
                name: "SerialBootImage",
                hresult: None,
                ok: false,
                detail: "Runtime-backed boot image is empty.".to_string(),
            });
            report.blocker = Some("Runtime-backed boot image is empty.".to_string());
            return Vec::new();
        }

        let mut descriptors = Vec::with_capacity(1 + boot_image.extra_regions.len());
        descriptors.push(NativeGuestMemoryRegion {
            label: boot_image.source_label.clone(),
            guest_gpa: boot_image.guest_entry_gpa,
            bytes: boot_image.bytes.clone(),
            writable: true,
            executable: true,
        });
        descriptors.extend(boot_image.extra_regions.iter().cloned());

        let mut mapped_regions = Vec::with_capacity(descriptors.len());
        let mut mapped_all = true;
        for descriptor in descriptors {
            let Some(mut memory) = GuestMemory::new(descriptor.bytes.len()) else {
                report.calls.push(NativeWhpCallReport {
                    name: "HostPageAllocation",
                    hresult: None,
                    ok: false,
                    detail: format!(
                        "Could not allocate page-aligned host memory for guest region `{}`.",
                        descriptor.label
                    ),
                });
                report.blocker = Some(format!(
                    "Could not allocate page-aligned guest memory for `{}`.",
                    descriptor.label
                ));
                mapped_all = false;
                break;
            };

            memory.as_mut_slice()[..descriptor.bytes.len()].copy_from_slice(&descriptor.bytes);
            let size = memory.size as u64;
            report.calls.push(NativeWhpCallReport {
                name: "GuestMemoryRegion",
                hresult: None,
                ok: true,
                detail: format!(
                    "Loaded {} bytes into guest region `{}` at {:#010x} (mapped {} bytes).",
                    descriptor.bytes.len(),
                    descriptor.label,
                    descriptor.guest_gpa,
                    size
                ),
            });
            report.guest_regions.push(NativeGuestRegionReport {
                label: descriptor.label.clone(),
                guest_gpa: format!("{:#010x}", descriptor.guest_gpa),
                source_bytes: descriptor.bytes.len() as u64,
                mapped_bytes: size,
                writable: descriptor.writable,
                executable: descriptor.executable,
            });

            let mut flags = WHV_MAP_GPA_RANGE_FLAG_READ;
            if descriptor.writable {
                flags |= WHV_MAP_GPA_RANGE_FLAG_WRITE;
            }
            if descriptor.executable {
                flags |= WHV_MAP_GPA_RANGE_FLAG_EXECUTE;
            }
            let hresult = unsafe {
                map_gpa_range(
                    partition,
                    memory.as_mut_ptr().cast::<c_void>(),
                    descriptor.guest_gpa,
                    size,
                    flags,
                )
            };
            let ok = hresult_succeeded(hresult);
            mapped_all &= ok;
            report.calls.push(hresult_call(
                "WHvMapGpaRange(guest-region)",
                hresult,
                if ok {
                    "Mapped a Pane guest memory region."
                } else {
                    "Could not map a Pane guest memory region."
                },
            ));
            if !ok {
                report.blocker = Some(format!(
                    "Could not map guest memory region `{}` at {:#010x}.",
                    descriptor.label, descriptor.guest_gpa
                ));
                break;
            }

            mapped_regions.push(MappedGuestRegion {
                label: descriptor.label,
                guest_gpa: descriptor.guest_gpa,
                size,
                _memory: memory,
            });
        }

        report.memory_mapped = mapped_all && !mapped_regions.is_empty();
        mapped_regions
    }

    fn framebuffer_snapshot_report(
        label: &str,
        guest_gpa: u64,
        bytes: &[u8],
    ) -> Option<NativeFramebufferSnapshotReport> {
        if label != "pane-framebuffer" {
            return None;
        }
        let nonzero_bytes = bytes.iter().filter(|byte| **byte != 0).count() as u64;
        let first_nonzero_offset = bytes
            .iter()
            .position(|byte| *byte != 0)
            .map(|offset| offset as u64);
        Some(NativeFramebufferSnapshotReport {
            label: label.to_string(),
            guest_gpa: format!("{guest_gpa:#010x}"),
            bytes: bytes.len() as u64,
            nonzero_bytes,
            first_nonzero_offset,
            all_zero: nonzero_bytes == 0,
        })
    }

    fn input_queue_snapshot_report(
        label: &str,
        guest_gpa: u64,
        bytes: &[u8],
    ) -> Option<NativeInputQueueSnapshotReport> {
        if label != "pane-input-queue" {
            return None;
        }
        let nonzero_bytes = bytes.iter().filter(|byte| **byte != 0).count() as u64;
        let first_nonzero_offset = bytes
            .iter()
            .position(|byte| *byte != 0)
            .map(|offset| offset as u64);
        Some(NativeInputQueueSnapshotReport {
            label: label.to_string(),
            guest_gpa: format!("{guest_gpa:#010x}"),
            bytes: bytes.len() as u64,
            nonzero_bytes,
            first_nonzero_offset,
            all_zero: nonzero_bytes == 0,
        })
    }

    fn capture_runtime_surface_snapshots(
        mapped_regions: &[MappedGuestRegion],
        report: &mut NativePartitionSmokeReport,
    ) {
        for region in mapped_regions {
            let bytes = region._memory.as_slice();
            if report.framebuffer_snapshot.is_none() {
                if let Some(snapshot) =
                    framebuffer_snapshot_report(&region.label, region.guest_gpa, bytes)
                {
                    report.calls.push(NativeWhpCallReport {
                        name: "PaneFramebufferSnapshot",
                        hresult: None,
                        ok: true,
                        detail: format!(
                            "Captured {} bytes from `{}` at {}; nonzero_bytes={}.",
                            snapshot.bytes,
                            snapshot.label,
                            snapshot.guest_gpa,
                            snapshot.nonzero_bytes
                        ),
                    });
                    report.framebuffer_snapshot = Some(snapshot);
                }
            }
            if report.input_queue_snapshot.is_none() {
                if let Some(snapshot) =
                    input_queue_snapshot_report(&region.label, region.guest_gpa, bytes)
                {
                    report.calls.push(NativeWhpCallReport {
                        name: "PaneInputQueueSnapshot",
                        hresult: None,
                        ok: true,
                        detail: format!(
                            "Captured {} bytes from `{}` at {}; nonzero_bytes={}.",
                            snapshot.bytes,
                            snapshot.label,
                            snapshot.guest_gpa,
                            snapshot.nonzero_bytes
                        ),
                    });
                    report.input_queue_snapshot = Some(snapshot);
                }
            }
        }
    }

    fn boot_image_registers(image: &NativeSerialBootImage) -> (Vec<u32>, Vec<WhvRegisterValue>) {
        match image.entry_mode {
            NativeGuestEntryMode::RealModeSerial => {
                serial_test_image_registers(image.guest_entry_gpa)
            }
            NativeGuestEntryMode::LinuxProtectedMode32 => linux_protected_mode_registers(
                image.guest_entry_gpa,
                image
                    .boot_params_gpa
                    .expect("linux protected-mode entry requires boot params GPA"),
            ),
        }
    }

    fn guest_entry_register_call_name(entry_mode: NativeGuestEntryMode) -> &'static str {
        match entry_mode {
            NativeGuestEntryMode::RealModeSerial => {
                "WHvSetVirtualProcessorRegisters(real-mode-serial)"
            }
            NativeGuestEntryMode::LinuxProtectedMode32 => {
                "WHvSetVirtualProcessorRegisters(linux-protected-mode-32)"
            }
        }
    }

    fn serial_test_image_registers(entry_gpa: u64) -> (Vec<u32>, Vec<WhvRegisterValue>) {
        let code_segment = WhvX64SegmentRegister {
            base: entry_gpa,
            limit: 0xffff,
            selector: 0,
            attributes: 0x009b,
        };
        let data_segment = WhvX64SegmentRegister {
            base: entry_gpa,
            limit: 0xffff,
            selector: 0,
            attributes: 0x0093,
        };
        let register_names = vec![
            WHV_REGISTER_RAX,
            WHV_REGISTER_RDX,
            WHV_REGISTER_RSP,
            WHV_REGISTER_RIP,
            WHV_REGISTER_RFLAGS,
            WHV_REGISTER_CS,
            WHV_REGISTER_DS,
            WHV_REGISTER_ES,
            WHV_REGISTER_SS,
            WHV_REGISTER_CR0,
            WHV_REGISTER_CR3,
            WHV_REGISTER_CR4,
        ];
        let register_values = vec![
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0x800 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0x0002 },
            WhvRegisterValue {
                segment: code_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue { reg64: 0x0010 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0 },
        ];
        (register_names, register_values)
    }

    fn linux_protected_mode_registers(
        entry_gpa: u64,
        boot_params_gpa: u64,
    ) -> (Vec<u32>, Vec<WhvRegisterValue>) {
        let code_segment = WhvX64SegmentRegister {
            base: 0,
            limit: 0xffff_ffff,
            selector: LINUX_BOOT_CODE_SELECTOR,
            attributes: 0x0000_cf9b,
        };
        let data_segment = WhvX64SegmentRegister {
            base: 0,
            limit: 0xffff_ffff,
            selector: LINUX_BOOT_DATA_SELECTOR,
            attributes: 0x0000_cf93,
        };
        let gdt = WhvX64TableRegister {
            pad: [0; 3],
            limit: 0x1f,
            base: LINUX_BOOT_GDT_GPA,
        };
        let idt = WhvX64TableRegister {
            pad: [0; 3],
            limit: 0,
            base: 0,
        };
        let register_names = vec![
            WHV_REGISTER_RAX,
            WHV_REGISTER_RDX,
            WHV_REGISTER_RBX,
            WHV_REGISTER_RSP,
            WHV_REGISTER_RBP,
            WHV_REGISTER_RSI,
            WHV_REGISTER_RDI,
            WHV_REGISTER_RIP,
            WHV_REGISTER_RFLAGS,
            WHV_REGISTER_CS,
            WHV_REGISTER_DS,
            WHV_REGISTER_ES,
            WHV_REGISTER_SS,
            WHV_REGISTER_GDTR,
            WHV_REGISTER_IDTR,
            WHV_REGISTER_CR0,
            WHV_REGISTER_CR3,
            WHV_REGISTER_CR4,
        ];
        let register_values = vec![
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue {
                reg64: LINUX_BOOT_STACK_GPA,
            },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue {
                reg64: boot_params_gpa,
            },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: entry_gpa },
            WhvRegisterValue { reg64: 0x0002 },
            WhvRegisterValue {
                segment: code_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue {
                segment: data_segment,
            },
            WhvRegisterValue { table: gdt },
            WhvRegisterValue { table: idt },
            WhvRegisterValue { reg64: 0x0011 },
            WhvRegisterValue { reg64: 0 },
            WhvRegisterValue { reg64: 0 },
        ];
        (register_names, register_values)
    }

    fn run_guest_image_until_boundary(
        partition: *mut c_void,
        run_virtual_processor: WhvRunVirtualProcessor,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        boot_image: &NativeSerialBootImage,
        block_io_handler: Option<&NativeBlockIoHandler<'_>>,
        report: &mut NativePartitionSmokeReport,
    ) {
        match boot_image.entry_mode {
            NativeGuestEntryMode::RealModeSerial => run_serial_test_image(
                partition,
                run_virtual_processor,
                set_virtual_processor_registers,
                &boot_image.expected_serial_text,
                report,
            ),
            NativeGuestEntryMode::LinuxProtectedMode32 => run_linux_entry_probe(
                partition,
                run_virtual_processor,
                set_virtual_processor_registers,
                block_io_handler,
                report,
            ),
        }
    }

    fn run_serial_test_image(
        partition: *mut c_void,
        run_virtual_processor: WhvRunVirtualProcessor,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        expected_serial_text: &str,
        report: &mut NativePartitionSmokeReport,
    ) {
        report.serial_expected_text = Some(expected_serial_text.to_string());
        let max_serial_boot_exits = expected_serial_text.len() + 2;
        report.guest_exit_budget = max_serial_boot_exits as u32;

        for exit_index in 0..max_serial_boot_exits {
            report.guest_exit_count = (exit_index + 1) as u32;
            let mut exit_context = [0_u8; 1024];
            let hresult = unsafe {
                run_virtual_processor(
                    partition,
                    0,
                    exit_context.as_mut_ptr().cast::<c_void>(),
                    exit_context.len() as u32,
                )
            };
            let run_ok = hresult_succeeded(hresult);
            report.virtual_processor_ran |= run_ok;
            report.calls.push(hresult_call(
                "WHvRunVirtualProcessor(serial-test-image)",
                hresult,
                if run_ok {
                    "vCPU returned with a WHP exit context."
                } else {
                    "vCPU execution failed before producing a WHP exit context."
                },
            ));

            if !run_ok {
                break;
            }

            match decode_exit_context(&exit_context, report) {
                DecodedExit::IoPort {
                    instruction_length,
                    rip,
                    is_write,
                    access_size,
                    port,
                    serial_byte,
                    ..
                } => {
                    let serial_ok = is_write
                        && access_size == 1
                        && port == SERIAL_COM1_PORT
                        && report.serial_bytes.len() < expected_serial_text.len();
                    if !serial_ok {
                        break;
                    }

                    report.serial_bytes.push(serial_byte);
                    report.serial_port = Some(port);
                    report.serial_byte = Some(serial_byte);
                    report.serial_text =
                        Some(String::from_utf8_lossy(&report.serial_bytes).into_owned());

                    if instruction_length == 0 {
                        report.calls.push(NativeWhpCallReport {
                            name: "AdvanceGuestRip",
                            hresult: None,
                            ok: false,
                            detail:
                                "WHP reported a zero-length I/O instruction; refusing to resume."
                                    .to_string(),
                        });
                        break;
                    }

                    let next_rip = rip + u64::from(instruction_length);
                    if !set_guest_rip(partition, set_virtual_processor_registers, next_rip, report)
                    {
                        break;
                    }
                }
                DecodedExit::Halt => {
                    report.halt_observed = true;
                    let text = report.serial_text.as_deref().unwrap_or("");
                    let ok = text == expected_serial_text;
                    report.calls.push(NativeWhpCallReport {
                        name: "SerialBootBanner",
                        hresult: None,
                        ok,
                        detail: format!(
                            "Serial boot image halted after emitting {text:?}; expected {expected_serial_text:?}."
                        ),
                    });
                    break;
                }
                DecodedExit::MemoryAccess {
                    access_type,
                    gpa_unmapped,
                    gva_valid,
                    gpa,
                    gva,
                } => {
                    report.calls.push(NativeWhpCallReport {
                        name: "LinuxMemoryAccessBlocker",
                        hresult: None,
                        ok: false,
                        detail: format!(
                            "Linux probe stopped on {} access at gpa=0x{gpa:016x}, gva={}, unmapped={gpa_unmapped}.",
                            memory_access_type_label(access_type),
                            if gva_valid {
                                format!("0x{gva:016x}")
                            } else {
                                "invalid".to_string()
                            }
                        ),
                    });
                    break;
                }
                DecodedExit::MsrAccess { .. } => break,
                DecodedExit::Cpuid { .. } => break,
                DecodedExit::InterruptWindow => break,
                DecodedExit::ApicEoi => break,
                DecodedExit::Other => break,
            }

            if exit_index + 1 == max_serial_boot_exits {
                report.calls.push(NativeWhpCallReport {
                    name: "SerialBootExitBudget",
                    hresult: None,
                    ok: false,
                    detail: format!(
                        "Serial boot image exceeded {max_serial_boot_exits} WHP exits without halting."
                    ),
                });
            }
        }

        report.serial_io_exit_count = report.serial_bytes.len() as u32;
    }

    fn run_linux_entry_probe(
        partition: *mut c_void,
        run_virtual_processor: WhvRunVirtualProcessor,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        block_io_handler: Option<&NativeBlockIoHandler<'_>>,
        report: &mut NativePartitionSmokeReport,
    ) {
        report.serial_expected_text = None;
        let max_guest_exits = linux_entry_probe_exit_budget(report);
        report.guest_exit_budget = max_guest_exits as u32;
        let mut msr_state = default_linux_msr_state();
        let mut serial_state = Com1SerialState::default();
        let mut block_state = NativeBlockIoPortState::default();
        let mut legacy_io_state = LegacyDeviceIoState::default();

        for exit_index in 0..max_guest_exits {
            report.guest_exit_count = (exit_index + 1) as u32;
            let mut exit_context = [0_u8; 1024];
            let hresult = unsafe {
                run_virtual_processor(
                    partition,
                    0,
                    exit_context.as_mut_ptr().cast::<c_void>(),
                    exit_context.len() as u32,
                )
            };
            let run_ok = hresult_succeeded(hresult);
            report.virtual_processor_ran |= run_ok;
            report.calls.push(hresult_call(
                "WHvRunVirtualProcessor(linux-entry-probe)",
                hresult,
                if run_ok {
                    "vCPU returned with a WHP exit context for the Linux protected-mode entry probe."
                } else {
                    "vCPU execution failed before the Linux protected-mode entry probe produced a WHP exit context."
                },
            ));

            if !run_ok {
                break;
            }

            match decode_exit_context(&exit_context, report) {
                DecodedExit::IoPort {
                    instruction_length,
                    rip,
                    is_write,
                    access_size,
                    port,
                    serial_byte,
                    rax,
                } => {
                    if pane_block_io_port_offset(port).is_some() {
                        if access_size != 1 {
                            break;
                        }
                        if instruction_length == 0 {
                            report.calls.push(NativeWhpCallReport {
                                name: "AdvanceGuestRip",
                                hresult: None,
                                ok: false,
                                detail:
                                    "WHP reported a zero-length I/O instruction; refusing to resume."
                                        .to_string(),
                            });
                            break;
                        }

                        let next_rip = rip + u64::from(instruction_length);
                        if is_write {
                            if let Some(submission) = block_state.write(port, serial_byte) {
                                let outcome = service_native_block_io_command(
                                    &submission.command,
                                    block_io_handler,
                                    submission.write_payload.as_deref(),
                                );
                                block_state.set_service_result(
                                    outcome.status_code,
                                    outcome.response_bytes,
                                );
                                let can_resume =
                                    native_block_io_exit_can_resume(outcome.status_code);
                                report.calls.push(outcome.report);
                                if can_resume {
                                    if !set_guest_rip(
                                        partition,
                                        set_virtual_processor_registers,
                                        next_rip,
                                        report,
                                    ) {
                                        break;
                                    }
                                    continue;
                                }
                                break;
                            }

                            if !set_guest_rip(
                                partition,
                                set_virtual_processor_registers,
                                next_rip,
                                report,
                            ) {
                                break;
                            }
                        } else {
                            let value = block_state.read(port);
                            if !set_guest_rax_low_byte_and_rip(
                                partition,
                                set_virtual_processor_registers,
                                rax,
                                value,
                                next_rip,
                                report,
                            ) {
                                break;
                            }
                        }
                        continue;
                    }

                    if access_size != 1
                        || !(SERIAL_COM1_PORT..=SERIAL_COM1_LAST_PORT).contains(&port)
                    {
                        if let Some(value) =
                            legacy_io_state.access(port, is_write, access_size, rax)
                        {
                            if instruction_length == 0 {
                                report.calls.push(NativeWhpCallReport {
                                    name: "AdvanceGuestRip",
                                    hresult: None,
                                    ok: false,
                                    detail:
                                        "WHP reported a zero-length legacy I/O instruction; refusing to resume."
                                            .to_string(),
                                });
                                break;
                            }
                            let next_rip = rip + u64::from(instruction_length);
                            report.calls.push(NativeWhpCallReport {
                                name: "LegacyDeviceIo",
                                hresult: None,
                                ok: true,
                                detail: format!(
                                    "{} {}-byte legacy port 0x{port:04x} value=0x{value:08x}.",
                                    if is_write {
                                        "Accepted write to"
                                    } else {
                                        "Returned read from"
                                    },
                                    access_size
                                ),
                            });
                            if is_write {
                                if !set_guest_rip(
                                    partition,
                                    set_virtual_processor_registers,
                                    next_rip,
                                    report,
                                ) {
                                    break;
                                }
                            } else if !set_guest_rax_low_value_and_rip(
                                partition,
                                set_virtual_processor_registers,
                                rax,
                                value,
                                access_size,
                                next_rip,
                                report,
                            ) {
                                break;
                            }
                            continue;
                        }
                        report.calls.push(NativeWhpCallReport {
                            name: "UnsupportedIoPort",
                            hresult: None,
                            ok: false,
                            detail: format!(
                                "No Pane device model currently handles I/O port 0x{port:04x}."
                            ),
                        });
                        break;
                    }

                    if instruction_length == 0 {
                        report.calls.push(NativeWhpCallReport {
                            name: "AdvanceGuestRip",
                            hresult: None,
                            ok: false,
                            detail:
                                "WHP reported a zero-length I/O instruction; refusing to resume."
                                    .to_string(),
                        });
                        break;
                    }

                    let next_rip = rip + u64::from(instruction_length);
                    if is_write {
                        if serial_state.write(port, serial_byte) {
                            report.serial_bytes.push(serial_byte);
                            report.serial_port = Some(port);
                            report.serial_byte = Some(serial_byte);
                            report.serial_text =
                                Some(String::from_utf8_lossy(&report.serial_bytes).into_owned());
                        }

                        if !set_guest_rip(
                            partition,
                            set_virtual_processor_registers,
                            next_rip,
                            report,
                        ) {
                            break;
                        }
                    } else {
                        let value = serial_state.read(port);
                        if !set_guest_rax_low_byte_and_rip(
                            partition,
                            set_virtual_processor_registers,
                            rax,
                            value,
                            next_rip,
                            report,
                        ) {
                            break;
                        }
                    }
                }
                DecodedExit::Halt => {
                    report.halt_observed = true;
                    break;
                }
                DecodedExit::MemoryAccess {
                    access_type,
                    gpa_unmapped,
                    gva_valid,
                    gpa,
                    gva,
                } => {
                    report.calls.push(NativeWhpCallReport {
                        name: "SerialMemoryAccessBlocker",
                        hresult: None,
                        ok: false,
                        detail: format!(
                            "Serial guest stopped on {} access at gpa=0x{gpa:016x}, gva={}, unmapped={gpa_unmapped}.",
                            memory_access_type_label(access_type),
                            if gva_valid {
                                format!("0x{gva:016x}")
                            } else {
                                "invalid".to_string()
                            }
                        ),
                    });
                    break;
                }
                DecodedExit::Cpuid {
                    instruction_length,
                    rip,
                    leaf,
                    subleaf,
                    default_rax,
                    default_rbx,
                    default_rcx,
                    default_rdx,
                } => {
                    if instruction_length == 0 {
                        report.calls.push(NativeWhpCallReport {
                            name: "AdvanceGuestRip",
                            hresult: None,
                            ok: false,
                            detail:
                                "WHP reported a zero-length CPUID instruction; refusing to resume."
                                    .to_string(),
                        });
                        break;
                    }

                    let next_rip = rip + u64::from(instruction_length);
                    if !set_cpuid_result_and_advance_rip(
                        partition,
                        set_virtual_processor_registers,
                        CpuidResult {
                            leaf,
                            subleaf,
                            rax: default_rax,
                            rbx: default_rbx,
                            rcx: default_rcx,
                            rdx: default_rdx,
                            next_rip,
                        },
                        report,
                    ) {
                        break;
                    }
                }
                DecodedExit::MsrAccess {
                    instruction_length,
                    rip,
                    is_write,
                    msr_number,
                    value,
                } => {
                    if instruction_length == 0 {
                        report.calls.push(NativeWhpCallReport {
                            name: "AdvanceGuestRip",
                            hresult: None,
                            ok: false,
                            detail:
                                "WHP reported a zero-length MSR instruction; refusing to resume."
                                    .to_string(),
                        });
                        break;
                    }

                    let next_rip = rip + u64::from(instruction_length);
                    if !handle_msr_access_and_advance_rip(
                        partition,
                        set_virtual_processor_registers,
                        &mut msr_state,
                        MsrAccess {
                            is_write,
                            msr_number,
                            value,
                            next_rip,
                        },
                        report,
                    ) {
                        break;
                    }
                }
                DecodedExit::InterruptWindow => {
                    report.calls.push(NativeWhpCallReport {
                        name: "InterruptWindowResumed",
                        hresult: None,
                        ok: true,
                        detail:
                            "Guest reached an interrupt-window exit; Pane has no pending interrupt to inject and will resume the vCPU."
                                .to_string(),
                    });
                }
                DecodedExit::ApicEoi => {
                    report.calls.push(NativeWhpCallReport {
                        name: "ApicEoiObserved",
                        hresult: None,
                        ok: true,
                        detail:
                            "Guest reached an APIC EOI exit; Pane observed it and will resume the vCPU."
                                .to_string(),
                    });
                }
                DecodedExit::Other => break,
            }

            if exit_index + 1 == max_guest_exits {
                report.calls.push(NativeWhpCallReport {
                    name: "LinuxEntryProbeExitBudget",
                    hresult: None,
                    ok: false,
                    detail: format!(
                        "Linux protected-mode entry probe exceeded {max_guest_exits} WHP exits without reaching a stable serial or halt boundary."
                    ),
                });
            }
        }

        report.serial_io_exit_count = report.serial_bytes.len() as u32;
        report.serial_markers_observed = serial_markers_observed(report);
        if !report.serial_expected_markers.is_empty() {
            report.calls.push(NativeWhpCallReport {
                name: "LinuxSerialMilestones",
                hresult: None,
                ok: report.serial_markers_observed,
                detail: if report.serial_markers_observed {
                    format!(
                        "Observed expected Linux serial milestones: {}.",
                        report.serial_expected_markers.join(", ")
                    )
                } else {
                    format!(
                        "Expected Linux serial milestones were not all observed yet: {}.",
                        report.serial_expected_markers.join(", ")
                    )
                },
            });
        }
        report.calls.push(NativeWhpCallReport {
            name: "LinuxEntryProbeBoundary",
            hresult: None,
            ok: linux_entry_probe_passed(report),
            detail: linux_entry_probe_detail(report),
        });
    }

    fn linux_entry_probe_exit_budget(report: &NativePartitionSmokeReport) -> usize {
        if report.serial_expected_markers.is_empty() {
            LINUX_ENTRY_PROBE_MINIMAL_EXIT_BUDGET
        } else {
            LINUX_ENTRY_PROBE_EXIT_BUDGET
        }
    }

    fn set_guest_rip(
        partition: *mut c_void,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        rip: u64,
        report: &mut NativePartitionSmokeReport,
    ) -> bool {
        let register_names = [WHV_REGISTER_RIP];
        let register_values = [WhvRegisterValue { reg64: rip }];
        let hresult = unsafe {
            set_virtual_processor_registers(
                partition,
                0,
                register_names.as_ptr(),
                register_names.len() as u32,
                register_values.as_ptr(),
            )
        };
        let ok = hresult_succeeded(hresult);
        report.calls.push(hresult_call(
            "WHvSetVirtualProcessorRegisters(RIP)",
            hresult,
            if ok {
                "Advanced guest RIP past the emulated serial I/O instruction."
            } else {
                "Could not advance guest RIP after serial I/O exit."
            },
        ));
        ok
    }

    #[derive(Default)]
    struct Com1SerialState {
        interrupt_enable: u8,
        line_control: u8,
        modem_control: u8,
        divisor_latch_low: u8,
        divisor_latch_high: u8,
    }

    impl Com1SerialState {
        fn dlab_enabled(&self) -> bool {
            self.line_control & 0x80 != 0
        }

        fn write(&mut self, port: u16, value: u8) -> bool {
            match port - SERIAL_COM1_PORT {
                0 if self.dlab_enabled() => self.divisor_latch_low = value,
                0 => return true,
                1 if self.dlab_enabled() => self.divisor_latch_high = value,
                1 => self.interrupt_enable = value,
                3 => self.line_control = value,
                4 => self.modem_control = value,
                _ => {}
            }
            false
        }

        fn read(&self, port: u16) -> u8 {
            match port {
                SERIAL_COM1_PORT if self.dlab_enabled() => self.divisor_latch_low,
                SERIAL_COM1_PORT => 0,
                port if port == SERIAL_COM1_PORT + 1 && self.dlab_enabled() => {
                    self.divisor_latch_high
                }
                port if port == SERIAL_COM1_PORT + 1 => self.interrupt_enable,
                SERIAL_INTERRUPT_ID_PORT => 0x01,
                port if port == SERIAL_COM1_PORT + 3 => self.line_control,
                port if port == SERIAL_COM1_PORT + 4 => self.modem_control,
                SERIAL_LINE_STATUS_PORT => 0x60,
                SERIAL_MODEM_STATUS_PORT => 0x30,
                _ => 0,
            }
        }
    }

    #[derive(Default)]
    struct LegacyDeviceIoState {
        pic1_mask: u8,
        pic2_mask: u8,
        pit_latch: [u8; 3],
        pit_command: u8,
        system_control_a: u8,
        system_control_b: u8,
        cmos_index: u8,
        pci_config_address: u32,
        vga_attribute_index: u8,
        vga_attribute_flip_flop: bool,
        vga_attribute: [u8; 0x20],
        vga_misc_output: u8,
        vga_sequencer_index: u8,
        vga_sequencer: [u8; 0x08],
        vga_dac_mask: u8,
        vga_dac_index: u8,
        vga_graphics_index: u8,
        vga_graphics: [u8; 0x10],
        vga_crtc_index: u8,
        vga_crtc: [u8; 0x20],
        elcr1: u8,
        elcr2: u8,
    }

    impl LegacyDeviceIoState {
        fn access(&mut self, port: u16, is_write: bool, access_size: u32, rax: u64) -> Option<u32> {
            if !matches!(access_size, 1 | 2 | 4) {
                return None;
            }
            let value = (rax & access_mask(access_size)) as u32;
            if is_write {
                return self.write_value(port, access_size, value).then_some(value);
            }
            self.read_value(port, access_size)
        }

        fn write_value(&mut self, port: u16, access_size: u32, value: u32) -> bool {
            let bytes = value.to_le_bytes();
            for offset in 0..access_size {
                if !self.write_byte(port + offset as u16, bytes[offset as usize]) {
                    return false;
                }
            }
            true
        }

        fn write_byte(&mut self, port: u16, value: u8) -> bool {
            match port {
                PIC1_COMMAND_PORT | PIC2_COMMAND_PORT => true,
                PIC1_DATA_PORT => {
                    self.pic1_mask = value;
                    true
                }
                PIC2_DATA_PORT => {
                    self.pic2_mask = value;
                    true
                }
                PIT_CHANNEL0_PORT => {
                    self.pit_latch[0] = value;
                    true
                }
                PIT_CHANNEL1_PORT => {
                    self.pit_latch[1] = value;
                    true
                }
                PIT_CHANNEL2_PORT => {
                    self.pit_latch[2] = value;
                    true
                }
                PIT_COMMAND_PORT => {
                    self.pit_command = value;
                    true
                }
                PS2_DATA_PORT => true,
                PS2_STATUS_COMMAND_PORT => true,
                SYSTEM_CONTROL_PORT_B => {
                    self.system_control_b = value;
                    true
                }
                CMOS_ADDRESS_PORT => {
                    self.cmos_index = value & 0x7f;
                    true
                }
                CMOS_DATA_PORT => true,
                POST_DELAY_PORT | ALT_POST_DELAY_PORT | ALT_DELAY_PORT => true,
                SYSTEM_CONTROL_PORT_A => {
                    self.system_control_a = value;
                    true
                }
                PCI_CONFIG_ADDRESS_PORT..=PCI_CONFIG_ADDRESS_END_PORT
                | PCI_CONFIG_DATA_START_PORT..=PCI_CONFIG_DATA_END_PORT => {
                    self.write_pci_config_port(port, value);
                    true
                }
                VGA_ATTRIBUTE_PORT => {
                    if self.vga_attribute_flip_flop {
                        let index = usize::from(self.vga_attribute_index & 0x1f);
                        self.vga_attribute[index] = value;
                    } else {
                        self.vga_attribute_index = value & 0x1f;
                    }
                    self.vga_attribute_flip_flop = !self.vga_attribute_flip_flop;
                    true
                }
                VGA_MISC_OUTPUT_WRITE_PORT => {
                    self.vga_misc_output = value;
                    true
                }
                VGA_SEQUENCER_INDEX_PORT => {
                    self.vga_sequencer_index = value & 0x07;
                    true
                }
                VGA_SEQUENCER_DATA_PORT => {
                    let index = usize::from(self.vga_sequencer_index);
                    self.vga_sequencer[index] = value;
                    true
                }
                VGA_DAC_MASK_PORT => {
                    self.vga_dac_mask = value;
                    true
                }
                VGA_DAC_STATE_PORT | VGA_DAC_WRITE_INDEX_PORT => {
                    self.vga_dac_index = value;
                    true
                }
                VGA_DAC_DATA_PORT => true,
                VGA_GRAPHICS_INDEX_PORT => {
                    self.vga_graphics_index = value & 0x0f;
                    true
                }
                VGA_GRAPHICS_DATA_PORT => {
                    let index = usize::from(self.vga_graphics_index);
                    self.vga_graphics[index] = value;
                    true
                }
                VGA_CRTC_MONO_INDEX_PORT | VGA_CRTC_COLOR_INDEX_PORT => {
                    self.vga_crtc_index = value & 0x1f;
                    true
                }
                VGA_CRTC_MONO_DATA_PORT | VGA_CRTC_COLOR_DATA_PORT => {
                    let index = usize::from(self.vga_crtc_index);
                    self.vga_crtc[index] = value;
                    true
                }
                ELCR1_PORT => {
                    self.elcr1 = value;
                    true
                }
                ELCR2_PORT => {
                    self.elcr2 = value;
                    true
                }
                _ => false,
            }
        }

        fn read_value(&mut self, port: u16, access_size: u32) -> Option<u32> {
            let mut bytes = [0_u8; 4];
            for offset in 0..access_size {
                bytes[offset as usize] = self.read_byte(port + offset as u16)?;
            }
            Some(u32::from_le_bytes(bytes) & access_mask(access_size) as u32)
        }

        fn read_byte(&mut self, port: u16) -> Option<u8> {
            match port {
                PIC1_COMMAND_PORT | PIC2_COMMAND_PORT => Some(0),
                PIC1_DATA_PORT => Some(self.pic1_mask),
                PIC2_DATA_PORT => Some(self.pic2_mask),
                PIT_CHANNEL0_PORT => Some(self.pit_latch[0]),
                PIT_CHANNEL1_PORT => Some(self.pit_latch[1]),
                PIT_CHANNEL2_PORT => Some(self.pit_latch[2]),
                PIT_COMMAND_PORT => Some(self.pit_command),
                PS2_DATA_PORT => Some(0),
                PS2_STATUS_COMMAND_PORT => Some(0),
                SYSTEM_CONTROL_PORT_A => Some(self.system_control_a),
                SYSTEM_CONTROL_PORT_B => Some(self.system_control_b),
                CMOS_DATA_PORT => Some(self.cmos_value()),
                POST_DELAY_PORT | ALT_POST_DELAY_PORT | ALT_DELAY_PORT => Some(0),
                PCI_CONFIG_ADDRESS_PORT..=PCI_CONFIG_ADDRESS_END_PORT
                | PCI_CONFIG_DATA_START_PORT..=PCI_CONFIG_DATA_END_PORT => {
                    Some(self.read_pci_config_port(port))
                }
                VGA_ATTRIBUTE_DATA_READ_PORT => {
                    Some(self.vga_attribute[usize::from(self.vga_attribute_index & 0x1f)])
                }
                VGA_MISC_OUTPUT_READ_PORT => Some(self.vga_misc_output),
                VGA_SEQUENCER_INDEX_PORT => Some(self.vga_sequencer_index),
                VGA_SEQUENCER_DATA_PORT => {
                    Some(self.vga_sequencer[usize::from(self.vga_sequencer_index)])
                }
                VGA_DAC_MASK_PORT => Some(self.vga_dac_mask),
                VGA_DAC_STATE_PORT => Some(0),
                VGA_DAC_WRITE_INDEX_PORT => Some(self.vga_dac_index),
                VGA_DAC_DATA_PORT => Some(0),
                VGA_GRAPHICS_INDEX_PORT => Some(self.vga_graphics_index),
                VGA_GRAPHICS_DATA_PORT => {
                    Some(self.vga_graphics[usize::from(self.vga_graphics_index)])
                }
                VGA_CRTC_MONO_INDEX_PORT | VGA_CRTC_COLOR_INDEX_PORT => Some(self.vga_crtc_index),
                VGA_CRTC_MONO_DATA_PORT | VGA_CRTC_COLOR_DATA_PORT => {
                    Some(self.vga_crtc[usize::from(self.vga_crtc_index)])
                }
                VGA_INPUT_STATUS_MONO_PORT | VGA_INPUT_STATUS_COLOR_PORT => {
                    self.vga_attribute_flip_flop = false;
                    Some(0)
                }
                ELCR1_PORT => Some(self.elcr1),
                ELCR2_PORT => Some(self.elcr2),
                _ => None,
            }
        }

        fn cmos_value(&self) -> u8 {
            match self.cmos_index {
                0x00 => 0x00, // Seconds
                0x02 => 0x00, // Minutes
                0x04 => 0x00, // Hours
                0x07 => 0x01, // Day of month
                0x08 => 0x01, // Month
                0x09 => 0x26, // Year
                0x0a => 0x26, // Status A: divider/rate, update not in progress.
                0x0b => 0x02, // Status B: 24-hour BCD mode, no periodic interrupts.
                0x0c => 0x00, // Status C: no pending RTC interrupt.
                0x0d => 0x80, // Status D: CMOS battery valid.
                0x32 => 0x20, // Century
                _ => 0,
            }
        }

        fn write_pci_config_port(&mut self, port: u16, value: u8) {
            if (PCI_CONFIG_ADDRESS_PORT..=PCI_CONFIG_ADDRESS_PORT + 3).contains(&port) {
                let shift = u32::from(port - PCI_CONFIG_ADDRESS_PORT) * 8;
                self.pci_config_address &= !(0xff_u32 << shift);
                self.pci_config_address |= u32::from(value) << shift;
            }
        }

        fn read_pci_config_port(&self, port: u16) -> u8 {
            if (PCI_CONFIG_ADDRESS_PORT..=PCI_CONFIG_ADDRESS_PORT + 3).contains(&port) {
                let shift = u32::from(port - PCI_CONFIG_ADDRESS_PORT) * 8;
                return ((self.pci_config_address >> shift) & 0xff) as u8;
            }
            0xff
        }
    }

    fn access_mask(access_size: u32) -> u64 {
        match access_size {
            1 => 0xff,
            2 => 0xffff,
            4 => 0xffff_ffff,
            _ => 0,
        }
    }

    fn set_guest_rax_low_byte_and_rip(
        partition: *mut c_void,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        previous_rax: u64,
        value: u8,
        rip: u64,
        report: &mut NativePartitionSmokeReport,
    ) -> bool {
        let register_names = [WHV_REGISTER_RAX, WHV_REGISTER_RIP];
        let register_values = [
            WhvRegisterValue {
                reg64: (previous_rax & !0xff) | u64::from(value),
            },
            WhvRegisterValue { reg64: rip },
        ];
        let hresult = unsafe {
            set_virtual_processor_registers(
                partition,
                0,
                register_names.as_ptr(),
                register_names.len() as u32,
                register_values.as_ptr(),
            )
        };
        let ok = hresult_succeeded(hresult);
        report.calls.push(hresult_call(
            "WHvSetVirtualProcessorRegisters(COM1)",
            hresult,
            if ok {
                "Returned emulated COM1 input in AL and advanced RIP."
            } else {
                "Could not return emulated COM1 input to guest registers."
            },
        ));
        report.calls.push(NativeWhpCallReport {
            name: "Com1SerialInput",
            hresult: None,
            ok,
            detail: format!("Returned COM1 byte 0x{value:02x} to guest AL."),
        });
        ok
    }

    fn set_guest_rax_low_value_and_rip(
        partition: *mut c_void,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        previous_rax: u64,
        value: u32,
        access_size: u32,
        rip: u64,
        report: &mut NativePartitionSmokeReport,
    ) -> bool {
        let mask = match access_size {
            1 => 0xff_u64,
            2 => 0xffff_u64,
            4 => 0xffff_ffff_u64,
            _ => return false,
        };
        let register_names = [WHV_REGISTER_RAX, WHV_REGISTER_RIP];
        let register_values = [
            WhvRegisterValue {
                reg64: (previous_rax & !mask) | (u64::from(value) & mask),
            },
            WhvRegisterValue { reg64: rip },
        ];
        let hresult = unsafe {
            set_virtual_processor_registers(
                partition,
                0,
                register_names.as_ptr(),
                register_names.len() as u32,
                register_values.as_ptr(),
            )
        };
        let ok = hresult_succeeded(hresult);
        report.calls.push(hresult_call(
            "WHvSetVirtualProcessorRegisters(LegacyIO)",
            hresult,
            if ok {
                "Returned emulated legacy I/O input and advanced RIP."
            } else {
                "Could not return emulated legacy I/O input to guest registers."
            },
        ));
        ok
    }

    struct CpuidResult {
        leaf: u64,
        subleaf: u64,
        rax: u64,
        rbx: u64,
        rcx: u64,
        rdx: u64,
        next_rip: u64,
    }

    fn set_cpuid_result_and_advance_rip(
        partition: *mut c_void,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        result: CpuidResult,
        report: &mut NativePartitionSmokeReport,
    ) -> bool {
        let register_names = [
            WHV_REGISTER_RAX,
            WHV_REGISTER_RBX,
            WHV_REGISTER_RCX,
            WHV_REGISTER_RDX,
            WHV_REGISTER_RIP,
        ];
        let register_values = [
            WhvRegisterValue { reg64: result.rax },
            WhvRegisterValue { reg64: result.rbx },
            WhvRegisterValue { reg64: result.rcx },
            WhvRegisterValue { reg64: result.rdx },
            WhvRegisterValue {
                reg64: result.next_rip,
            },
        ];
        let hresult = unsafe {
            set_virtual_processor_registers(
                partition,
                0,
                register_names.as_ptr(),
                register_names.len() as u32,
                register_values.as_ptr(),
            )
        };
        let ok = hresult_succeeded(hresult);
        report.calls.push(hresult_call(
            "WHvSetVirtualProcessorRegisters(CPUID)",
            hresult,
            if ok {
                "Returned WHP default CPUID results to guest registers and advanced RIP."
            } else {
                "Could not return WHP default CPUID results to guest registers."
            },
        ));
        report.calls.push(NativeWhpCallReport {
            name: "CpuidPassthrough",
            hresult: None,
            ok,
            detail: format!(
                "leaf=0x{:016x} subleaf=0x{:016x} -> eax=0x{:016x} ebx=0x{:016x} ecx=0x{:016x} edx=0x{:016x}.",
                result.leaf, result.subleaf, result.rax, result.rbx, result.rcx, result.rdx
            ),
        });
        ok
    }

    struct MsrAccess {
        is_write: bool,
        msr_number: u32,
        value: u64,
        next_rip: u64,
    }

    fn default_linux_msr_state() -> HashMap<u32, u64> {
        HashMap::from([
            (0x0000_0010, 0),                     // IA32_TIME_STAMP_COUNTER
            (0x0000_0017, 0),                     // IA32_PLATFORM_ID
            (0x0000_001b, 0x0000_0000_fee0_0900), // IA32_APIC_BASE: BSP + enabled
            (0x0000_00ce, 0),                     // IA32_PLATFORM_INFO
            (0x0000_00fe, 0),                     // IA32_MTRR_CAP: fixed/range MTRRs absent
            (0x0000_0174, 0),                     // IA32_SYSENTER_CS
            (0x0000_0175, 0),                     // IA32_SYSENTER_ESP
            (0x0000_0176, 0),                     // IA32_SYSENTER_EIP
            (0x0000_0277, 0x0007_0406_0007_0406), // IA32_PAT reset memory types
            (0x0000_02ff, 0),                     // IA32_MTRR_DEF_TYPE: MTRRs disabled
            (0xc000_0080, 0),                     // IA32_EFER
            (0xc000_0081, 0),                     // IA32_STAR
            (0xc000_0082, 0),                     // IA32_LSTAR
            (0xc000_0084, 0),                     // IA32_FMASK
            (0xc000_0100, 0),                     // FS base
            (0xc000_0101, 0),                     // GS base
            (0xc000_0102, 0),                     // Kernel GS base
            (0xc000_0103, 0),                     // TSC AUX
        ])
    }

    fn handle_msr_access_and_advance_rip(
        partition: *mut c_void,
        set_virtual_processor_registers: WhvSetVirtualProcessorRegisters,
        msr_state: &mut HashMap<u32, u64>,
        access: MsrAccess,
        report: &mut NativePartitionSmokeReport,
    ) -> bool {
        if access.is_write {
            msr_state.insert(access.msr_number, access.value);
            let ok = set_guest_rip(
                partition,
                set_virtual_processor_registers,
                access.next_rip,
                report,
            );
            report.calls.push(NativeWhpCallReport {
                name: "MsrWrite",
                hresult: None,
                ok,
                detail: format!(
                    "Stored guest WRMSR msr=0x{:08x} value=0x{:016x}.",
                    access.msr_number, access.value
                ),
            });
            return ok;
        }

        let value = *msr_state.get(&access.msr_number).unwrap_or(&0);
        let register_names = [WHV_REGISTER_RAX, WHV_REGISTER_RDX, WHV_REGISTER_RIP];
        let register_values = [
            WhvRegisterValue {
                reg64: value & 0xffff_ffff,
            },
            WhvRegisterValue { reg64: value >> 32 },
            WhvRegisterValue {
                reg64: access.next_rip,
            },
        ];
        let hresult = unsafe {
            set_virtual_processor_registers(
                partition,
                0,
                register_names.as_ptr(),
                register_names.len() as u32,
                register_values.as_ptr(),
            )
        };
        let ok = hresult_succeeded(hresult);
        report.calls.push(hresult_call(
            "WHvSetVirtualProcessorRegisters(RDMSR)",
            hresult,
            if ok {
                "Returned stored MSR value to guest RDX:RAX and advanced RIP."
            } else {
                "Could not return stored MSR value to guest registers."
            },
        ));
        report.calls.push(NativeWhpCallReport {
            name: "MsrRead",
            hresult: None,
            ok,
            detail: format!(
                "Returned guest RDMSR msr=0x{:08x} value=0x{:016x}.",
                access.msr_number, value
            ),
        });
        ok
    }

    fn guest_contract_passed(
        report: &NativePartitionSmokeReport,
        entry_mode: NativeGuestEntryMode,
    ) -> bool {
        let common_guest_execution_passed = report.memory_mapped
            && report.registers_configured
            && report.virtual_processor_ran
            && report.memory_unmapped;

        common_guest_execution_passed
            && match entry_mode {
                NativeGuestEntryMode::RealModeSerial => serial_contract_passed(report),
                NativeGuestEntryMode::LinuxProtectedMode32 => linux_entry_probe_passed(report),
            }
    }

    fn serial_contract_passed(report: &NativePartitionSmokeReport) -> bool {
        report.halt_observed
            && report.exit_reason == Some(WHV_RUN_VP_EXIT_REASON_X64_HALT)
            && report.serial_port == Some(SERIAL_COM1_PORT)
            && report
                .serial_expected_text
                .as_ref()
                .map(|expected| report.serial_io_exit_count as usize == expected.len())
                .unwrap_or(false)
            && report.serial_text == report.serial_expected_text
    }

    fn serial_markers_observed(report: &NativePartitionSmokeReport) -> bool {
        let Some(text) = report.serial_text.as_deref() else {
            return report.serial_expected_markers.is_empty();
        };
        let mut cursor = 0;
        for marker in &report.serial_expected_markers {
            let Some(offset) = text[cursor..].find(marker) else {
                return false;
            };
            cursor += offset + marker.len();
        }
        true
    }

    fn linux_entry_probe_passed(report: &NativePartitionSmokeReport) -> bool {
        report.virtual_processor_ran
            && report.exit_reason.is_some()
            && (report.serial_expected_markers.is_empty() || report.serial_markers_observed)
            && !report.calls.iter().any(|call| {
                matches!(
                    call.name,
                    "LinuxEntryProbeExitBudget" | "AdvanceGuestRip" | "UnsupportedIoPort"
                ) && !call.ok
            })
            && !matches!(
                report.exit_reason,
                Some(
                    WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS
                        | WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE
                        | WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION
                )
            )
            && match report.exit_reason {
                Some(WHV_RUN_VP_EXIT_REASON_X64_CPUID) => report
                    .calls
                    .iter()
                    .any(|call| call.name == "CpuidPassthrough" && call.ok),
                Some(WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS) => report
                    .calls
                    .iter()
                    .any(|call| matches!(call.name, "MsrRead" | "MsrWrite") && call.ok),
                Some(WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW) => report
                    .calls
                    .iter()
                    .any(|call| call.name == "InterruptWindowResumed" && call.ok),
                Some(WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI) => report
                    .calls
                    .iter()
                    .any(|call| call.name == "ApicEoiObserved" && call.ok),
                _ => true,
            }
    }

    fn linux_entry_probe_detail(report: &NativePartitionSmokeReport) -> String {
        if linux_entry_probe_passed(report) {
            let exit = report.exit_reason_label.as_deref().unwrap_or("unknown");
            let serial = report.serial_text.as_deref().unwrap_or("");
            if !report.serial_expected_markers.is_empty() {
                format!(
                    "Linux protected-mode entry observed expected serial milestones before WHP exit `{exit}`: {}.",
                    report.serial_expected_markers.join(", ")
                )
            } else if serial.is_empty() {
                format!(
                    "Linux protected-mode entry was accepted and reached WHP exit `{exit}`; early Linux serial output is not proven yet."
                )
            } else {
                format!(
                    "Linux protected-mode entry emitted serial text {serial:?} before WHP exit `{exit}`."
                )
            }
        } else if !report.virtual_processor_ran {
            "Linux protected-mode entry did not run far enough to produce a WHP exit context."
                .to_string()
        } else if !report.serial_expected_markers.is_empty() && !report.serial_markers_observed {
            format!(
                "Linux protected-mode entry has not yet emitted the required serial milestones: {}.",
                report.serial_expected_markers.join(", ")
            )
        } else {
            let exit = report.exit_reason_label.as_deref().unwrap_or("unknown");
            let next = match report.exit_reason {
                Some(WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS) => {
                    "map the missing guest memory range or correct the E820/boot params layout"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_CPUID) => {
                    "inspect CPUID pass-through and guest RIP advancement"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS) => {
                    "inspect MSR state handling and guest RIP advancement"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS) => {
                    "add a Pane device model for the unsupported I/O port or fix port decoding"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW) => {
                    "resume the vCPU or inject a queued interrupt when Pane owns timer delivery"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI) => {
                    "resume the vCPU after observing APIC end-of-interrupt"
                }
                Some(WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE) => {
                    "correct the protected-mode register setup"
                }
                Some(WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION) => {
                    "inspect the guest exception path and boot params"
                }
                _ => "inspect CPU state, mapped memory, and boot params",
            };
            format!(
                "Linux protected-mode entry reached failing WHP exit `{exit}`; next step: {next}."
            )
        }
    }

    enum DecodedExit {
        MemoryAccess {
            access_type: u32,
            gpa_unmapped: bool,
            gva_valid: bool,
            gpa: u64,
            gva: u64,
        },
        IoPort {
            instruction_length: u8,
            rip: u64,
            is_write: bool,
            access_size: u32,
            port: u16,
            serial_byte: u8,
            rax: u64,
        },
        Halt,
        MsrAccess {
            instruction_length: u8,
            rip: u64,
            is_write: bool,
            msr_number: u32,
            value: u64,
        },
        Cpuid {
            instruction_length: u8,
            rip: u64,
            leaf: u64,
            subleaf: u64,
            default_rax: u64,
            default_rbx: u64,
            default_rcx: u64,
            default_rdx: u64,
        },
        InterruptWindow,
        ApicEoi,
        Other,
    }

    fn decode_exit_context(
        exit_context: &[u8],
        report: &mut NativePartitionSmokeReport,
    ) -> DecodedExit {
        let exit_reason = read_u32(exit_context, 0);
        report.exit_reason = Some(exit_reason);
        report.exit_reason_label = Some(exit_reason_label(exit_reason).to_string());

        if exit_reason == WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS {
            let access_info = read_u32(exit_context, MEMORY_ACCESS_INFO_OFFSET);
            let access_type = access_info & 0x3;
            let gpa_unmapped = ((access_info >> 2) & 0x1) == 0x1;
            let gva_valid = ((access_info >> 3) & 0x1) == 0x1;
            let gpa = read_u64(exit_context, MEMORY_GPA_OFFSET);
            let gva = read_u64(exit_context, MEMORY_GVA_OFFSET);
            report.calls.push(NativeWhpCallReport {
                name: "DecodeMemoryAccess",
                hresult: None,
                ok: false,
                detail: format!(
                    "Guest memory access type={} gpa=0x{gpa:016x} gva={} unmapped={gpa_unmapped}.",
                    memory_access_type_label(access_type),
                    if gva_valid {
                        format!("0x{gva:016x}")
                    } else {
                        "invalid".to_string()
                    }
                ),
            });
            DecodedExit::MemoryAccess {
                access_type,
                gpa_unmapped,
                gva_valid,
                gpa,
                gva,
            }
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS {
            let instruction_length = exit_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] & 0x0f;
            let rip = read_u64(exit_context, VP_CONTEXT_RIP_OFFSET);
            let access_info = read_u32(exit_context, IO_ACCESS_INFO_OFFSET);
            let port = read_u16(exit_context, IO_PORT_OFFSET);
            let rax = read_u64(exit_context, IO_RAX_OFFSET);
            let is_write = (access_info & 0x1) == 0x1;
            let access_size = (access_info >> 1) & 0x7;
            let serial_byte = (rax & 0xff) as u8;

            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64IoPortAccess",
                hresult: None,
                ok: access_size == 1 && (SERIAL_COM1_PORT..=SERIAL_COM1_LAST_PORT).contains(&port),
                detail: format!(
                    "I/O exit write={is_write} size={access_size} port=0x{port:04x} byte=0x{serial_byte:02x} rip=0x{rip:016x} instruction_length={instruction_length}."
                ),
            });
            DecodedExit::IoPort {
                instruction_length,
                rip,
                is_write,
                access_size,
                port,
                serial_byte,
                rax,
            }
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_HALT {
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64Halt",
                hresult: None,
                ok: true,
                detail: "The guest reached HLT.".to_string(),
            });
            DecodedExit::Halt
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS {
            let instruction_length = exit_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] & 0x0f;
            let rip = read_u64(exit_context, VP_CONTEXT_RIP_OFFSET);
            let access_info = read_u32(exit_context, MSR_ACCESS_INFO_OFFSET);
            let is_write = (access_info & 0x1) == 0x1;
            let msr_number = read_u32(exit_context, MSR_NUMBER_OFFSET);
            let rax = read_u64(exit_context, MSR_RAX_OFFSET);
            let rdx = read_u64(exit_context, MSR_RDX_OFFSET);
            let value = (rdx << 32) | (rax & 0xffff_ffff);
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64MsrAccess",
                hresult: None,
                ok: true,
                detail: format!(
                    "Guest reached {}MSR msr=0x{msr_number:08x} value=0x{value:016x}; using Pane's minimal Linux MSR state.",
                    if is_write { "WR" } else { "RD" }
                ),
            });
            DecodedExit::MsrAccess {
                instruction_length,
                rip,
                is_write,
                msr_number,
                value,
            }
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_CPUID {
            let instruction_length = exit_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] & 0x0f;
            let rip = read_u64(exit_context, VP_CONTEXT_RIP_OFFSET);
            let leaf = read_u64(exit_context, CPUID_RAX_OFFSET);
            let subleaf = read_u64(exit_context, CPUID_RCX_OFFSET);
            let default_rax = read_u64(exit_context, CPUID_DEFAULT_RAX_OFFSET);
            let default_rbx = read_u64(exit_context, CPUID_DEFAULT_RBX_OFFSET);
            let default_rcx = read_u64(exit_context, CPUID_DEFAULT_RCX_OFFSET);
            let default_rdx = read_u64(exit_context, CPUID_DEFAULT_RDX_OFFSET);
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64Cpuid",
                hresult: None,
                ok: true,
                detail: format!(
                    "Guest reached CPUID leaf=0x{leaf:016x} subleaf=0x{subleaf:016x}; using WHP default result registers."
                ),
            });
            DecodedExit::Cpuid {
                instruction_length,
                rip,
                leaf,
                subleaf,
                default_rax,
                default_rbx,
                default_rcx,
                default_rdx,
            }
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW {
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64InterruptWindow",
                hresult: None,
                ok: true,
                detail:
                    "Guest reached an interrupt-window exit; no instruction emulation is required."
                        .to_string(),
            });
            DecodedExit::InterruptWindow
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI {
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64ApicEoi",
                hresult: None,
                ok: true,
                detail: "Guest reached an APIC end-of-interrupt exit.".to_string(),
            });
            DecodedExit::ApicEoi
        } else {
            report.calls.push(NativeWhpCallReport {
                name: "DecodeVpExit",
                hresult: None,
                ok: false,
                detail: format!("Unexpected WHP exit reason 0x{exit_reason:08x}."),
            });
            DecodedExit::Other
        }
    }

    fn exit_reason_label(value: u32) -> &'static str {
        match value {
            0x0000_0000 => "none",
            WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS => "memory-access",
            WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS => "x64-io-port-access",
            WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION => "unrecoverable-exception",
            WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE => "invalid-vp-register-value",
            0x0000_0006 => "unsupported-feature",
            WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW => "x64-interrupt-window",
            WHV_RUN_VP_EXIT_REASON_X64_HALT => "x64-halt",
            WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI => "x64-apic-eoi",
            WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS => "x64-msr-access",
            WHV_RUN_VP_EXIT_REASON_X64_CPUID => "x64-cpuid",
            0x0000_1002 => "exception",
            0x0000_2001 => "canceled",
            _ => "unknown",
        }
    }

    fn memory_access_type_label(value: u32) -> &'static str {
        match value {
            0 => "read",
            1 => "write",
            2 => "execute",
            _ => "unknown",
        }
    }

    fn read_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ])
    }

    #[cfg(test)]
    mod tests {
        use super::{
            decode_exit_context, default_linux_msr_state, framebuffer_snapshot_report,
            guest_contract_passed, input_queue_snapshot_report, linux_entry_probe_detail,
            linux_entry_probe_exit_budget, linux_entry_probe_passed,
            linux_protected_mode_registers, serial_contract_passed, serial_markers_observed,
            Com1SerialState, DecodedExit, LegacyDeviceIoState, ALT_DELAY_PORT, ALT_POST_DELAY_PORT,
            CMOS_ADDRESS_PORT, CMOS_DATA_PORT, CPUID_DEFAULT_RAX_OFFSET, CPUID_DEFAULT_RBX_OFFSET,
            CPUID_DEFAULT_RCX_OFFSET, CPUID_DEFAULT_RDX_OFFSET, CPUID_RAX_OFFSET, CPUID_RCX_OFFSET,
            ELCR1_PORT, ELCR2_PORT, IO_ACCESS_INFO_OFFSET, IO_PORT_OFFSET, IO_RAX_OFFSET,
            LINUX_ENTRY_PROBE_EXIT_BUDGET, LINUX_ENTRY_PROBE_MINIMAL_EXIT_BUDGET,
            MEMORY_ACCESS_INFO_OFFSET, MEMORY_GPA_OFFSET, MEMORY_GVA_OFFSET,
            MSR_ACCESS_INFO_OFFSET, MSR_NUMBER_OFFSET, MSR_RAX_OFFSET, MSR_RDX_OFFSET,
            PCI_CONFIG_ADDRESS_PORT, PCI_CONFIG_DATA_START_PORT, PIC1_DATA_PORT, PIC2_DATA_PORT,
            PIT_CHANNEL0_PORT, PIT_COMMAND_PORT, POST_DELAY_PORT, PS2_DATA_PORT,
            PS2_STATUS_COMMAND_PORT, SERIAL_COM1_PORT, SYSTEM_CONTROL_PORT_A,
            SYSTEM_CONTROL_PORT_B, VGA_ATTRIBUTE_DATA_READ_PORT, VGA_ATTRIBUTE_PORT,
            VGA_CRTC_COLOR_DATA_PORT, VGA_CRTC_COLOR_INDEX_PORT, VGA_GRAPHICS_DATA_PORT,
            VGA_GRAPHICS_INDEX_PORT, VGA_INPUT_STATUS_COLOR_PORT, VGA_MISC_OUTPUT_READ_PORT,
            VGA_MISC_OUTPUT_WRITE_PORT, VGA_SEQUENCER_DATA_PORT, VGA_SEQUENCER_INDEX_PORT,
            VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET, VP_CONTEXT_RIP_OFFSET, WHV_REGISTER_CR0,
            WHV_REGISTER_CR3, WHV_REGISTER_CR4, WHV_REGISTER_CS, WHV_REGISTER_DS, WHV_REGISTER_ES,
            WHV_REGISTER_GDTR, WHV_REGISTER_IDTR, WHV_REGISTER_RBP, WHV_REGISTER_RBX,
            WHV_REGISTER_RDI, WHV_REGISTER_RFLAGS, WHV_REGISTER_RIP, WHV_REGISTER_RSI,
            WHV_REGISTER_RSP, WHV_REGISTER_SS, WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE,
            WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS, WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI,
            WHV_RUN_VP_EXIT_REASON_X64_CPUID, WHV_RUN_VP_EXIT_REASON_X64_HALT,
            WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW, WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS,
            WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS,
        };
        use crate::native::{
            evaluate_native_block_io, linux_boot_gdt_page_bytes, native_block_io_exit_can_resume,
            pane_block_io_port_offset, serial_boot_test_image_bytes,
            service_native_block_io_command, NativeBlockDeviceId, NativeBlockIoCommand,
            NativeBlockIoPortState, NativeBlockIoServiceResult, NativeBlockOperation,
            NativePartitionSmokeReport, NativePartitionSmokeStatus, LINUX_BOOT_CODE_SELECTOR,
            LINUX_BOOT_DATA_SELECTOR, LINUX_BOOT_GDT_GPA, LINUX_BOOT_STACK_GPA,
            PANE_BLOCK_IO_BASE_PORT, PANE_BLOCK_IO_BLOCK_SIZE_BYTES, PANE_BLOCK_IO_LAST_PORT,
            PANE_BLOCK_IO_STATUS_DENIED, PANE_BLOCK_IO_STATUS_FAILED,
            PANE_BLOCK_IO_STATUS_SERVICED, PANE_BLOCK_IO_STATUS_SUBMITTED, SERIAL_BOOT_BANNER_TEXT,
        };

        #[test]
        fn serial_test_image_outputs_the_expected_banner_then_halts() {
            let page = serial_boot_test_image_bytes();

            let mut offset = 0;
            for byte in SERIAL_BOOT_BANNER_TEXT.as_bytes() {
                assert_eq!(&page[offset..offset + 3], &[0xba, 0xf8, 0x03]);
                assert_eq!(&page[offset + 3..offset + 5], &[0xb0, *byte]);
                assert_eq!(page[offset + 5], 0xee);
                offset += 6;
            }
            assert_eq!(page[offset], 0xf4);
            assert!(page[offset + 1..].iter().all(|byte| *byte == 0));
        }

        #[test]
        fn linux_boot_gdt_contains_boot_protocol_descriptors() {
            let page = linux_boot_gdt_page_bytes();

            assert_eq!(
                &page[usize::from(LINUX_BOOT_CODE_SELECTOR)
                    ..usize::from(LINUX_BOOT_CODE_SELECTOR) + 8],
                &0x00cf_9a00_0000_ffff_u64.to_le_bytes()
            );
            assert_eq!(
                &page[usize::from(LINUX_BOOT_DATA_SELECTOR)
                    ..usize::from(LINUX_BOOT_DATA_SELECTOR) + 8],
                &0x00cf_9200_0000_ffff_u64.to_le_bytes()
            );
        }

        #[test]
        fn linux_protected_mode_registers_match_boot_protocol() {
            let (names, values) = linux_protected_mode_registers(0x0010_0000, 0x0000_7000);
            let find = |register| {
                names
                    .iter()
                    .position(|name| *name == register)
                    .expect("register is present")
            };

            unsafe {
                assert_eq!(values[find(WHV_REGISTER_RIP)].reg64, 0x0010_0000);
                assert_eq!(values[find(WHV_REGISTER_RSP)].reg64, LINUX_BOOT_STACK_GPA);
                assert_eq!(values[find(WHV_REGISTER_RSI)].reg64, 0x0000_7000);
                assert_eq!(values[find(WHV_REGISTER_RBX)].reg64, 0);
                assert_eq!(values[find(WHV_REGISTER_RBP)].reg64, 0);
                assert_eq!(values[find(WHV_REGISTER_RDI)].reg64, 0);
                assert_eq!(values[find(WHV_REGISTER_RFLAGS)].reg64, 0x0002);
                assert_eq!(values[find(WHV_REGISTER_CR0)].reg64, 0x0011);
                assert_eq!(values[find(WHV_REGISTER_CR3)].reg64, 0);
                assert_eq!(values[find(WHV_REGISTER_CR4)].reg64, 0);

                assert_eq!(
                    values[find(WHV_REGISTER_CS)].segment.selector,
                    LINUX_BOOT_CODE_SELECTOR
                );
                for register in [WHV_REGISTER_DS, WHV_REGISTER_ES, WHV_REGISTER_SS] {
                    let segment = values[find(register)].segment;
                    assert_eq!(segment.selector, LINUX_BOOT_DATA_SELECTOR);
                    assert_eq!(segment.base, 0);
                    assert_eq!(segment.limit, 0xffff_ffff);
                }

                let gdt = values[find(WHV_REGISTER_GDTR)].table;
                assert_eq!(gdt.base, LINUX_BOOT_GDT_GPA);
                assert_eq!(gdt.limit, 0x1f);
                let idt = values[find(WHV_REGISTER_IDTR)].table;
                assert_eq!(idt.base, 0);
                assert_eq!(idt.limit, 0);
            }
        }

        #[test]
        fn com1_serial_model_handles_linux_uart_probe_reads() {
            let mut serial = Com1SerialState::default();

            assert_eq!(serial.read(SERIAL_COM1_PORT + 5), 0x60);
            assert_eq!(serial.read(SERIAL_COM1_PORT + 2), 0x01);
            assert_eq!(serial.read(SERIAL_COM1_PORT + 6), 0x30);
            assert!(serial.write(SERIAL_COM1_PORT, b'P'));
            assert!(!serial.write(SERIAL_COM1_PORT + 3, 0x80));
            assert!(!serial.write(SERIAL_COM1_PORT, 0x01));
            assert!(!serial.write(SERIAL_COM1_PORT + 1, 0x00));
            assert_eq!(serial.read(SERIAL_COM1_PORT), 0x01);
            assert_eq!(serial.read(SERIAL_COM1_PORT + 1), 0x00);
        }

        #[test]
        fn legacy_device_io_model_handles_early_linux_ports() {
            let mut io = LegacyDeviceIoState::default();

            assert_eq!(io.access(PIC1_DATA_PORT, true, 1, 0xfe), Some(0xfe));
            assert_eq!(io.access(PIC1_DATA_PORT, false, 1, 0), Some(0xfe));
            assert_eq!(io.access(PIC2_DATA_PORT, true, 1, 0xff), Some(0xff));
            assert_eq!(io.access(PIC2_DATA_PORT, false, 1, 0), Some(0xff));
            assert_eq!(io.access(PIT_COMMAND_PORT, true, 1, 0x36), Some(0x36));
            assert_eq!(io.access(PIT_CHANNEL0_PORT, true, 1, 0x34), Some(0x34));
            assert_eq!(io.access(PIT_CHANNEL0_PORT, false, 1, 0), Some(0x34));
            assert_eq!(io.access(SYSTEM_CONTROL_PORT_A, true, 1, 0x02), Some(0x02));
            assert_eq!(io.access(SYSTEM_CONTROL_PORT_A, false, 1, 0), Some(0x02));
            assert_eq!(io.access(SYSTEM_CONTROL_PORT_B, true, 1, 0x03), Some(0x03));
            assert_eq!(io.access(SYSTEM_CONTROL_PORT_B, false, 1, 0), Some(0x03));
            assert_eq!(io.access(CMOS_ADDRESS_PORT, true, 1, 0x0d), Some(0x0d));
            assert_eq!(io.access(CMOS_DATA_PORT, false, 1, 0), Some(0x80));
            assert_eq!(io.access(ELCR1_PORT, true, 1, 0x00), Some(0x00));
            assert_eq!(io.access(ELCR1_PORT, false, 1, 0), Some(0x00));
            assert_eq!(io.access(ELCR2_PORT, true, 1, 0x0e), Some(0x0e));
            assert_eq!(io.access(ELCR2_PORT, false, 1, 0), Some(0x0e));
            assert_eq!(io.access(POST_DELAY_PORT, true, 1, 0), Some(0));
            assert_eq!(io.access(ALT_POST_DELAY_PORT, true, 1, 0), Some(0));
            assert_eq!(io.access(ALT_DELAY_PORT, true, 1, 0), Some(0));
            assert_eq!(io.access(0x1234, false, 1, 0), None);
        }

        #[test]
        fn legacy_device_io_model_reports_empty_pci_config_space() {
            let mut io = LegacyDeviceIoState::default();

            assert_eq!(io.access(POST_DELAY_PORT, true, 1, 0), Some(0));
            assert_eq!(
                io.access(PCI_CONFIG_ADDRESS_PORT, true, 4, 0x8000_0000),
                Some(0x8000_0000)
            );
            assert_eq!(
                io.access(PCI_CONFIG_ADDRESS_PORT, false, 4, 0),
                Some(0x8000_0000)
            );
            assert_eq!(
                io.access(PCI_CONFIG_DATA_START_PORT, false, 4, 0),
                Some(0xffff_ffff)
            );
            assert_eq!(
                io.access(PCI_CONFIG_DATA_START_PORT + 2, false, 2, 0),
                Some(0xffff)
            );
            assert_eq!(
                io.access(PCI_CONFIG_DATA_START_PORT + 3, false, 1, 0),
                Some(0xff)
            );
        }

        #[test]
        fn legacy_device_io_model_handles_ps2_controller_probe() {
            let mut io = LegacyDeviceIoState::default();

            assert_eq!(io.access(PS2_STATUS_COMMAND_PORT, false, 1, 0), Some(0));
            assert_eq!(io.access(PS2_DATA_PORT, false, 1, 0), Some(0));
            assert_eq!(
                io.access(PS2_STATUS_COMMAND_PORT, true, 1, 0xad),
                Some(0xad)
            );
            assert_eq!(io.access(PS2_DATA_PORT, true, 1, 0xf4), Some(0xf4));
            assert_eq!(io.access(PS2_STATUS_COMMAND_PORT, false, 1, 0), Some(0));
        }

        #[test]
        fn legacy_device_io_model_reports_deterministic_cmos_rtc() {
            let mut io = LegacyDeviceIoState::default();

            for (index, value) in [
                (0x00_u32, 0x00),
                (0x02, 0x00),
                (0x04, 0x00),
                (0x07, 0x01),
                (0x08, 0x01),
                (0x09, 0x26),
                (0x32, 0x20),
            ] {
                assert_eq!(
                    io.access(CMOS_ADDRESS_PORT, true, 1, u64::from(index)),
                    Some(index)
                );
                assert_eq!(io.access(CMOS_DATA_PORT, false, 1, 0), Some(value));
            }
        }

        #[test]
        fn legacy_device_io_model_handles_vga_probe_ports() {
            let mut io = LegacyDeviceIoState::default();

            assert_eq!(
                io.access(VGA_MISC_OUTPUT_WRITE_PORT, true, 1, 0x67),
                Some(0x67)
            );
            assert_eq!(
                io.access(VGA_MISC_OUTPUT_READ_PORT, false, 1, 0),
                Some(0x67)
            );

            assert_eq!(
                io.access(VGA_SEQUENCER_INDEX_PORT, true, 1, 0x04),
                Some(0x04)
            );
            assert_eq!(
                io.access(VGA_SEQUENCER_DATA_PORT, true, 1, 0x06),
                Some(0x06)
            );
            assert_eq!(io.access(VGA_SEQUENCER_DATA_PORT, false, 1, 0), Some(0x06));

            assert_eq!(
                io.access(VGA_GRAPHICS_INDEX_PORT, true, 1, 0x05),
                Some(0x05)
            );
            assert_eq!(io.access(VGA_GRAPHICS_DATA_PORT, true, 1, 0x40), Some(0x40));
            assert_eq!(io.access(VGA_GRAPHICS_DATA_PORT, false, 1, 0), Some(0x40));

            assert_eq!(
                io.access(VGA_CRTC_COLOR_INDEX_PORT, true, 1, 0x11),
                Some(0x11)
            );
            assert_eq!(
                io.access(VGA_CRTC_COLOR_DATA_PORT, true, 1, 0x20),
                Some(0x20)
            );
            assert_eq!(io.access(VGA_CRTC_COLOR_DATA_PORT, false, 1, 0), Some(0x20));

            assert_eq!(io.access(VGA_INPUT_STATUS_COLOR_PORT, false, 1, 0), Some(0));
            assert_eq!(io.access(VGA_ATTRIBUTE_PORT, true, 1, 0x10), Some(0x10));
            assert_eq!(io.access(VGA_ATTRIBUTE_PORT, true, 1, 0x41), Some(0x41));
            assert_eq!(
                io.access(VGA_ATTRIBUTE_DATA_READ_PORT, false, 1, 0),
                Some(0x41)
            );
        }

        #[test]
        fn pane_block_io_ports_are_classified_for_storage_exits() {
            assert_eq!(pane_block_io_port_offset(PANE_BLOCK_IO_BASE_PORT), Some(0));
            assert_eq!(
                pane_block_io_port_offset(PANE_BLOCK_IO_BASE_PORT + 7),
                Some(7)
            );
            assert_eq!(
                pane_block_io_port_offset(PANE_BLOCK_IO_LAST_PORT),
                Some(PANE_BLOCK_IO_LAST_PORT - PANE_BLOCK_IO_BASE_PORT)
            );
            assert_eq!(pane_block_io_port_offset(PANE_BLOCK_IO_BASE_PORT - 1), None);
            assert_eq!(pane_block_io_port_offset(PANE_BLOCK_IO_LAST_PORT + 1), None);
        }

        #[test]
        fn native_block_io_contract_enforces_base_read_only_policy() {
            let base_read = evaluate_native_block_io(&NativeBlockIoCommand {
                device: NativeBlockDeviceId::BaseOs,
                operation: NativeBlockOperation::Read,
                block_index: 9,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            });
            assert!(base_read.allowed);
            assert_eq!(base_read.status, "allowed");

            let base_write = evaluate_native_block_io(&NativeBlockIoCommand {
                device: NativeBlockDeviceId::BaseOs,
                operation: NativeBlockOperation::Write,
                block_index: 9,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            });
            assert!(!base_write.allowed);
            assert_eq!(base_write.status, "readonly-device");

            let user_write = evaluate_native_block_io(&NativeBlockIoCommand {
                device: NativeBlockDeviceId::UserDisk,
                operation: NativeBlockOperation::Write,
                block_index: 9,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            });
            assert!(user_write.allowed);

            let bad_block_size = evaluate_native_block_io(&NativeBlockIoCommand {
                device: NativeBlockDeviceId::UserDisk,
                operation: NativeBlockOperation::Read,
                block_index: 9,
                block_size_bytes: 512,
            });
            assert!(!bad_block_size.allowed);
            assert_eq!(bad_block_size.status, "unsupported-block-size");
        }

        #[test]
        fn native_block_io_port_state_builds_submit_commands() {
            let mut state = NativeBlockIoPortState::default();

            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT), 0);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 1), 0);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 3), 8);
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT, 1).is_none());
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 1, 1).is_none());
            for (index, byte) in 0x0102_0304_0506_0708_u64
                .to_le_bytes()
                .into_iter()
                .enumerate()
            {
                assert!(state
                    .write(PANE_BLOCK_IO_BASE_PORT + 4 + index as u16, byte)
                    .is_none());
            }

            let submission = state
                .write(PANE_BLOCK_IO_BASE_PORT + 2, 1)
                .expect("submit creates command");

            let command = submission.command;
            assert_eq!(command.device, NativeBlockDeviceId::UserDisk);
            assert_eq!(command.operation, NativeBlockOperation::Write);
            assert_eq!(command.block_index, 0x0102_0304_0506_0708);
            assert_eq!(command.block_size_bytes, PANE_BLOCK_IO_BLOCK_SIZE_BYTES);
            assert_eq!(submission.write_payload, Some(Vec::new()));
            assert_eq!(
                state.read(PANE_BLOCK_IO_BASE_PORT + 2),
                PANE_BLOCK_IO_STATUS_SUBMITTED
            );
        }

        #[test]
        fn native_block_io_port_state_collects_write_payload_bytes() {
            let mut state = NativeBlockIoPortState::default();

            assert!(state.write(PANE_BLOCK_IO_BASE_PORT, 1).is_none());
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 1, 1).is_none());
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 12, 0xde).is_none());
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 12, 0xad).is_none());
            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 12, 0xbe).is_none());

            let submission = state
                .write(PANE_BLOCK_IO_BASE_PORT + 2, 1)
                .expect("submit creates command");

            assert_eq!(submission.command.device, NativeBlockDeviceId::UserDisk);
            assert_eq!(submission.command.operation, NativeBlockOperation::Write);
            assert_eq!(submission.write_payload, Some(vec![0xde, 0xad, 0xbe]));
        }

        #[test]
        fn native_block_io_service_reports_pending_without_handler() {
            let command = NativeBlockIoCommand {
                device: NativeBlockDeviceId::BaseOs,
                operation: NativeBlockOperation::Read,
                block_index: 4,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            };

            let outcome = service_native_block_io_command(&command, None, None);

            assert_eq!(outcome.report.name, "PaneBlockIoExitPending");
            assert!(outcome.report.ok);
            assert_eq!(outcome.status_code, PANE_BLOCK_IO_STATUS_SUBMITTED);
        }

        #[test]
        fn native_block_io_service_invokes_runtime_handler_for_reads() {
            let command = NativeBlockIoCommand {
                device: NativeBlockDeviceId::BaseOs,
                operation: NativeBlockOperation::Read,
                block_index: 4,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            };
            let handler = |command: &NativeBlockIoCommand, payload: Option<&[u8]>| {
                assert_eq!(command.block_index, 4);
                assert!(payload.is_none());
                Ok(NativeBlockIoServiceResult {
                    decision: evaluate_native_block_io(command),
                    bytes: vec![0x7b_u8; PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize],
                })
            };

            let outcome = service_native_block_io_command(&command, Some(&handler), None);

            assert_eq!(outcome.report.name, "PaneBlockIoServiced");
            assert!(outcome.report.ok);
            assert_eq!(outcome.status_code, PANE_BLOCK_IO_STATUS_SERVICED);
            assert_eq!(
                outcome.response_bytes,
                vec![0x7b_u8; PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize]
            );
        }

        #[test]
        fn native_block_io_service_denies_readonly_writes_before_handler() {
            let command = NativeBlockIoCommand {
                device: NativeBlockDeviceId::BaseOs,
                operation: NativeBlockOperation::Write,
                block_index: 4,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            };
            let handler = |_command: &NativeBlockIoCommand, _payload: Option<&[u8]>| {
                panic!("denied commands must not reach storage handler")
            };

            let outcome = service_native_block_io_command(&command, Some(&handler), None);

            assert_eq!(outcome.report.name, "PaneBlockIoPolicyDenied");
            assert!(!outcome.report.ok);
            assert_eq!(outcome.status_code, PANE_BLOCK_IO_STATUS_DENIED);
        }

        #[test]
        fn native_block_io_service_passes_user_write_payload_to_handler() {
            let command = NativeBlockIoCommand {
                device: NativeBlockDeviceId::UserDisk,
                operation: NativeBlockOperation::Write,
                block_index: 4,
                block_size_bytes: PANE_BLOCK_IO_BLOCK_SIZE_BYTES,
            };
            let payload = vec![0x33_u8; PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize];
            let handler = |command: &NativeBlockIoCommand, payload: Option<&[u8]>| {
                assert_eq!(command.operation, NativeBlockOperation::Write);
                let payload = payload.expect("write payload is present");
                assert_eq!(payload.len(), PANE_BLOCK_IO_BLOCK_SIZE_BYTES as usize);
                assert!(payload.iter().all(|byte| *byte == 0x33));
                Ok(NativeBlockIoServiceResult {
                    decision: evaluate_native_block_io(command),
                    bytes: Vec::new(),
                })
            };

            let outcome = service_native_block_io_command(&command, Some(&handler), Some(&payload));

            assert_eq!(outcome.report.name, "PaneBlockIoServiced");
            assert!(outcome.report.ok);
            assert_eq!(outcome.status_code, PANE_BLOCK_IO_STATUS_SERVICED);
        }

        #[test]
        fn native_block_io_port_state_streams_serviced_read_bytes() {
            let mut state = NativeBlockIoPortState::default();

            state.set_service_result(PANE_BLOCK_IO_STATUS_SERVICED, vec![0xaa, 0xbb, 0xcc]);

            assert_eq!(
                state.read(PANE_BLOCK_IO_BASE_PORT + 2),
                PANE_BLOCK_IO_STATUS_SERVICED
            );
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 13), 3);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 14), 0);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 15), 0);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 12), 0xaa);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 12), 0xbb);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 12), 0xcc);
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 12), 0);

            assert!(state.write(PANE_BLOCK_IO_BASE_PORT + 4, 1).is_none());
            assert_eq!(state.read(PANE_BLOCK_IO_BASE_PORT + 13), 0);
        }

        #[test]
        fn native_block_io_runner_only_resumes_after_serviced_exits() {
            assert!(native_block_io_exit_can_resume(
                PANE_BLOCK_IO_STATUS_SERVICED
            ));
            assert!(!native_block_io_exit_can_resume(
                PANE_BLOCK_IO_STATUS_SUBMITTED
            ));
            assert!(!native_block_io_exit_can_resume(
                PANE_BLOCK_IO_STATUS_DENIED
            ));
            assert!(!native_block_io_exit_can_resume(
                PANE_BLOCK_IO_STATUS_FAILED
            ));
        }

        #[test]
        fn decodes_io_port_reads_with_original_rax() {
            let mut exit_context = [0_u8; 128];
            exit_context[..4]
                .copy_from_slice(&WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS.to_le_bytes());
            exit_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] = 1;
            exit_context[VP_CONTEXT_RIP_OFFSET..VP_CONTEXT_RIP_OFFSET + 8]
                .copy_from_slice(&0x0010_0100_u64.to_le_bytes());
            exit_context[IO_ACCESS_INFO_OFFSET..IO_ACCESS_INFO_OFFSET + 4]
                .copy_from_slice(&(1_u32 << 1).to_le_bytes());
            exit_context[IO_PORT_OFFSET..IO_PORT_OFFSET + 2]
                .copy_from_slice(&(SERIAL_COM1_PORT + 5).to_le_bytes());
            exit_context[IO_RAX_OFFSET..IO_RAX_OFFSET + 8]
                .copy_from_slice(&0xffff_ffff_ffff_ff00_u64.to_le_bytes());
            let mut report = base_report();

            match decode_exit_context(&exit_context, &mut report) {
                DecodedExit::IoPort {
                    instruction_length,
                    rip,
                    is_write,
                    access_size,
                    port,
                    rax,
                    ..
                } => {
                    assert_eq!(instruction_length, 1);
                    assert_eq!(rip, 0x0010_0100);
                    assert!(!is_write);
                    assert_eq!(access_size, 1);
                    assert_eq!(port, SERIAL_COM1_PORT + 5);
                    assert_eq!(rax, 0xffff_ffff_ffff_ff00);
                }
                _ => panic!("expected I/O port read exit"),
            }
        }

        fn base_report() -> NativePartitionSmokeReport {
            NativePartitionSmokeReport {
                product_shape: "test",
                execute_requested: true,
                attempted: true,
                status: NativePartitionSmokeStatus::Fail,
                status_label: NativePartitionSmokeStatus::Fail.display_name(),
                partition_created: true,
                processor_count_configured: true,
                partition_setup: true,
                virtual_processor_created: true,
                virtual_processor_deleted: true,
                partition_deleted: true,
                fixture_requested: true,
                memory_mapped: true,
                registers_configured: true,
                virtual_processor_ran: true,
                memory_unmapped: true,
                exit_reason: None,
                exit_reason_label: None,
                boot_image_source: None,
                boot_image_path: None,
                boot_image_bytes: None,
                entry_mode: None,
                boot_params_gpa: None,
                guest_regions: Vec::new(),
                serial_port: None,
                serial_byte: None,
                serial_bytes: Vec::new(),
                serial_text: None,
                serial_expected_text: None,
                serial_expected_markers: Vec::new(),
                serial_markers_observed: false,
                serial_io_exit_count: 0,
                guest_exit_count: 0,
                guest_exit_budget: 0,
                framebuffer_snapshot: None,
                input_queue_snapshot: None,
                halt_observed: false,
                calls: Vec::new(),
                blocker: None,
                next_step: "test".to_string(),
            }
        }

        #[test]
        fn serial_contract_requires_expected_banner_and_halt() {
            let mut report = base_report();
            report.halt_observed = true;
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_HALT);
            report.serial_port = Some(0x03f8);
            report.serial_bytes = SERIAL_BOOT_BANNER_TEXT.as_bytes().to_vec();
            report.serial_io_exit_count = report.serial_bytes.len() as u32;
            report.serial_text = Some(SERIAL_BOOT_BANNER_TEXT.to_string());
            report.serial_expected_text = Some(SERIAL_BOOT_BANNER_TEXT.to_string());

            assert!(serial_contract_passed(&report));
            assert!(guest_contract_passed(
                &report,
                crate::native::NativeGuestEntryMode::RealModeSerial
            ));

            report.serial_text = Some("wrong".to_string());
            assert!(!serial_contract_passed(&report));
        }

        #[test]
        fn linux_entry_probe_accepts_decoded_nonfatal_exit_without_serial_banner() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS);
            report.exit_reason_label = Some("x64-io-port-access".to_string());

            assert!(linux_entry_probe_passed(&report));
            assert!(guest_contract_passed(
                &report,
                crate::native::NativeGuestEntryMode::LinuxProtectedMode32
            ));
            assert!(report.serial_expected_text.is_none());
        }

        #[test]
        fn linux_entry_probe_rejects_unsupported_io_port_blocker() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS);
            report.exit_reason_label = Some("x64-io-port-access".to_string());
            report.calls.push(crate::native::NativeWhpCallReport {
                name: "UnsupportedIoPort",
                hresult: None,
                ok: false,
                detail: "unsupported".to_string(),
            });

            assert!(!linux_entry_probe_passed(&report));
            assert!(linux_entry_probe_detail(&report).contains("unsupported I/O port"));
        }

        #[test]
        fn linux_entry_probe_requires_declared_serial_milestones() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS);
            report.exit_reason_label = Some("x64-io-port-access".to_string());
            report.serial_expected_markers = vec![
                "PANE_INITRAMFS_DISCOVERY_START".to_string(),
                "PANE_BLOCK_IO_PROBE_OK".to_string(),
                "PANE_BLOCK_MODULE_LOAD_OK".to_string(),
                "PANE_INITRAMFS_DISCOVERY_DONE".to_string(),
                "PANE_DISPLAY_CONTRACT_DISCOVERED".to_string(),
                "PANE_ROOT_MOUNT_ATTEMPT".to_string(),
                "PANE_ROOT_MOUNT_OK".to_string(),
                "PANE_INIT_EXEC".to_string(),
            ];
            report.serial_text =
                Some("PANE_INITRAMFS_DISCOVERY_START\nPANE_BLOCK_IO_PROBE_OK\n".to_string());
            report.serial_markers_observed = serial_markers_observed(&report);

            assert!(!report.serial_markers_observed);
            assert!(!linux_entry_probe_passed(&report));

            report.serial_text = Some(
                "PANE_INITRAMFS_DISCOVERY_START\nPANE_BLOCK_IO_PROBE_OK\nPANE_BLOCK_MODULE_LOAD_OK\nPANE_INITRAMFS_DISCOVERY_DONE\nPANE_DISPLAY_CONTRACT_DISCOVERED\nPANE_ROOT_MOUNT_ATTEMPT\nPANE_ROOT_MOUNT_OK fs=ext4\nPANE_INIT_EXEC\n"
                    .to_string(),
            );
            report.serial_markers_observed = serial_markers_observed(&report);

            assert!(report.serial_markers_observed);
            assert!(linux_entry_probe_passed(&report));
        }

        #[test]
        fn linux_entry_probe_uses_extended_budget_for_serial_milestones() {
            let mut report = base_report();
            assert_eq!(
                linux_entry_probe_exit_budget(&report),
                LINUX_ENTRY_PROBE_MINIMAL_EXIT_BUDGET
            );

            report.serial_expected_markers = vec!["PANE_INIT_EXEC".to_string()];

            assert_eq!(
                linux_entry_probe_exit_budget(&report),
                LINUX_ENTRY_PROBE_EXIT_BUDGET
            );
            assert!(LINUX_ENTRY_PROBE_EXIT_BUDGET > LINUX_ENTRY_PROBE_MINIMAL_EXIT_BUDGET);
        }

        #[test]
        fn framebuffer_snapshot_reports_nonzero_guest_pixels() {
            let snapshot =
                framebuffer_snapshot_report("pane-framebuffer", 0x0e00_0000, &[0, 0, 7, 0, 9])
                    .expect("framebuffer snapshot");

            assert_eq!(snapshot.label, "pane-framebuffer");
            assert_eq!(snapshot.guest_gpa, "0x0e000000");
            assert_eq!(snapshot.bytes, 5);
            assert_eq!(snapshot.nonzero_bytes, 2);
            assert_eq!(snapshot.first_nonzero_offset, Some(2));
            assert!(!snapshot.all_zero);
            assert!(framebuffer_snapshot_report("pane-input-queue", 0x0dff_0000, &[1]).is_none());
        }

        #[test]
        fn input_queue_snapshot_reports_host_event_boundary() {
            let snapshot = input_queue_snapshot_report("pane-input-queue", 0x0dff_0000, &[0, 3, 0])
                .expect("input queue snapshot");

            assert_eq!(snapshot.label, "pane-input-queue");
            assert_eq!(snapshot.guest_gpa, "0x0dff0000");
            assert_eq!(snapshot.bytes, 3);
            assert_eq!(snapshot.nonzero_bytes, 1);
            assert_eq!(snapshot.first_nonzero_offset, Some(1));
            assert!(!snapshot.all_zero);
            assert!(input_queue_snapshot_report("pane-framebuffer", 0x0e00_0000, &[1]).is_none());
        }

        #[test]
        fn linux_entry_probe_rejects_invalid_vp_register_exit() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE);
            report.exit_reason_label = Some("invalid-vp-register-value".to_string());

            assert!(!linux_entry_probe_passed(&report));
            assert!(!guest_contract_passed(
                &report,
                crate::native::NativeGuestEntryMode::LinuxProtectedMode32
            ));
        }

        #[test]
        fn linux_entry_probe_rejects_unhandled_hardware_exits() {
            for (reason, label, expected_next_step) in [
                (
                    WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS,
                    "memory-access",
                    "map the missing guest memory range",
                ),
                (
                    WHV_RUN_VP_EXIT_REASON_X64_CPUID,
                    "x64-cpuid",
                    "inspect CPUID pass-through",
                ),
                (
                    WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS,
                    "x64-msr-access",
                    "inspect MSR state handling",
                ),
            ] {
                let mut report = base_report();
                report.exit_reason = Some(reason);
                report.exit_reason_label = Some(label.to_string());

                assert!(!linux_entry_probe_passed(&report));
                assert!(linux_entry_probe_detail(&report).contains(expected_next_step));
            }
        }

        #[test]
        fn linux_entry_probe_accepts_resumed_interrupt_window_exit() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW);
            report.exit_reason_label = Some("x64-interrupt-window".to_string());
            report.calls.push(crate::native::NativeWhpCallReport {
                name: "InterruptWindowResumed",
                hresult: None,
                ok: true,
                detail: "resumed".to_string(),
            });

            assert!(linux_entry_probe_passed(&report));
        }

        #[test]
        fn linux_entry_probe_accepts_observed_apic_eoi_exit() {
            let mut report = base_report();
            report.exit_reason = Some(WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI);
            report.exit_reason_label = Some("x64-apic-eoi".to_string());
            report.calls.push(crate::native::NativeWhpCallReport {
                name: "ApicEoiObserved",
                hresult: None,
                ok: true,
                detail: "observed".to_string(),
            });

            assert!(linux_entry_probe_passed(&report));
        }

        #[test]
        fn decodes_interrupt_window_and_apic_eoi_exits() {
            let mut interrupt_context = [0_u8; 128];
            interrupt_context[..4]
                .copy_from_slice(&WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW.to_le_bytes());
            let mut interrupt_report = base_report();
            assert!(matches!(
                decode_exit_context(&interrupt_context, &mut interrupt_report),
                DecodedExit::InterruptWindow
            ));
            assert!(interrupt_report
                .calls
                .iter()
                .any(|call| call.name == "DecodeX64InterruptWindow" && call.ok));

            let mut eoi_context = [0_u8; 128];
            eoi_context[..4].copy_from_slice(&WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI.to_le_bytes());
            let mut eoi_report = base_report();
            assert!(matches!(
                decode_exit_context(&eoi_context, &mut eoi_report),
                DecodedExit::ApicEoi
            ));
            assert!(eoi_report
                .calls
                .iter()
                .any(|call| call.name == "DecodeX64ApicEoi" && call.ok));
        }

        #[test]
        fn decodes_cpuid_exit_with_whp_default_registers() {
            let mut exit_context = [0_u8; 128];
            exit_context[..4].copy_from_slice(&WHV_RUN_VP_EXIT_REASON_X64_CPUID.to_le_bytes());
            exit_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] = 2;
            exit_context[VP_CONTEXT_RIP_OFFSET..VP_CONTEXT_RIP_OFFSET + 8]
                .copy_from_slice(&0x0010_0000_u64.to_le_bytes());
            exit_context[CPUID_RAX_OFFSET..CPUID_RAX_OFFSET + 8]
                .copy_from_slice(&0x0000_0001_u64.to_le_bytes());
            exit_context[CPUID_RCX_OFFSET..CPUID_RCX_OFFSET + 8]
                .copy_from_slice(&0x0000_0002_u64.to_le_bytes());
            exit_context[CPUID_DEFAULT_RAX_OFFSET..CPUID_DEFAULT_RAX_OFFSET + 8]
                .copy_from_slice(&0x0003_06a9_u64.to_le_bytes());
            exit_context[CPUID_DEFAULT_RBX_OFFSET..CPUID_DEFAULT_RBX_OFFSET + 8]
                .copy_from_slice(&0x0010_0800_u64.to_le_bytes());
            exit_context[CPUID_DEFAULT_RCX_OFFSET..CPUID_DEFAULT_RCX_OFFSET + 8]
                .copy_from_slice(&0x7ffafbff_u64.to_le_bytes());
            exit_context[CPUID_DEFAULT_RDX_OFFSET..CPUID_DEFAULT_RDX_OFFSET + 8]
                .copy_from_slice(&0xbfebfbff_u64.to_le_bytes());
            let mut report = base_report();

            let decoded = decode_exit_context(&exit_context, &mut report);

            match decoded {
                DecodedExit::Cpuid {
                    instruction_length,
                    rip,
                    leaf,
                    subleaf,
                    default_rax,
                    default_rbx,
                    default_rcx,
                    default_rdx,
                } => {
                    assert_eq!(instruction_length, 2);
                    assert_eq!(rip, 0x0010_0000);
                    assert_eq!(leaf, 1);
                    assert_eq!(subleaf, 2);
                    assert_eq!(default_rax, 0x0003_06a9);
                    assert_eq!(default_rbx, 0x0010_0800);
                    assert_eq!(default_rcx, 0x7ffa_fbff);
                    assert_eq!(default_rdx, 0xbfeb_fbff);
                }
                _ => panic!("expected CPUID exit"),
            }
            assert!(report
                .calls
                .iter()
                .any(|call| call.name == "DecodeX64Cpuid" && call.ok));
        }

        #[test]
        fn decodes_msr_read_and_write_exits() {
            let mut read_context = [0_u8; 128];
            read_context[..4].copy_from_slice(&WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS.to_le_bytes());
            read_context[VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET] = 2;
            read_context[VP_CONTEXT_RIP_OFFSET..VP_CONTEXT_RIP_OFFSET + 8]
                .copy_from_slice(&0x0010_0100_u64.to_le_bytes());
            read_context[MSR_NUMBER_OFFSET..MSR_NUMBER_OFFSET + 4]
                .copy_from_slice(&0xc000_0080_u32.to_le_bytes());
            let mut report = base_report();

            match decode_exit_context(&read_context, &mut report) {
                DecodedExit::MsrAccess {
                    instruction_length,
                    rip,
                    is_write,
                    msr_number,
                    value,
                } => {
                    assert_eq!(instruction_length, 2);
                    assert_eq!(rip, 0x0010_0100);
                    assert!(!is_write);
                    assert_eq!(msr_number, 0xc000_0080);
                    assert_eq!(value, 0);
                }
                _ => panic!("expected MSR read exit"),
            }

            let mut write_context = read_context;
            write_context[MSR_ACCESS_INFO_OFFSET..MSR_ACCESS_INFO_OFFSET + 4]
                .copy_from_slice(&1_u32.to_le_bytes());
            write_context[MSR_RAX_OFFSET..MSR_RAX_OFFSET + 8]
                .copy_from_slice(&0x0000_0501_u64.to_le_bytes());
            write_context[MSR_RDX_OFFSET..MSR_RDX_OFFSET + 8]
                .copy_from_slice(&0x0000_0001_u64.to_le_bytes());

            match decode_exit_context(&write_context, &mut report) {
                DecodedExit::MsrAccess {
                    is_write,
                    msr_number,
                    value,
                    ..
                } => {
                    assert!(is_write);
                    assert_eq!(msr_number, 0xc000_0080);
                    assert_eq!(value, 0x0000_0001_0000_0501);
                }
                _ => panic!("expected MSR write exit"),
            }
            assert!(report
                .calls
                .iter()
                .any(|call| call.name == "DecodeX64MsrAccess" && call.ok));
        }

        #[test]
        fn linux_msr_defaults_cover_common_cpu_bringup_reads() {
            let msrs = default_linux_msr_state();

            assert_eq!(msrs.get(&0x0000_001b), Some(&0x0000_0000_fee0_0900));
            assert_eq!(msrs.get(&0x0000_0277), Some(&0x0007_0406_0007_0406));
            assert_eq!(msrs.get(&0x0000_00fe), Some(&0));
            assert_eq!(msrs.get(&0x0000_0174), Some(&0));
            assert_eq!(msrs.get(&0x0000_0175), Some(&0));
            assert_eq!(msrs.get(&0x0000_0176), Some(&0));
            assert_eq!(msrs.get(&0xc000_0080), Some(&0));
            assert_eq!(msrs.get(&0xc000_0103), Some(&0));
        }

        #[test]
        fn decodes_memory_access_exit_with_guest_addresses() {
            let mut exit_context = [0_u8; 128];
            exit_context[..4].copy_from_slice(&WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS.to_le_bytes());
            let access_info = 1_u32 | (1 << 2) | (1 << 3);
            exit_context[MEMORY_ACCESS_INFO_OFFSET..MEMORY_ACCESS_INFO_OFFSET + 4]
                .copy_from_slice(&access_info.to_le_bytes());
            exit_context[MEMORY_GPA_OFFSET..MEMORY_GPA_OFFSET + 8]
                .copy_from_slice(&0x0000_0000_fee0_0000_u64.to_le_bytes());
            exit_context[MEMORY_GVA_OFFSET..MEMORY_GVA_OFFSET + 8]
                .copy_from_slice(&0xffff_ffff_fee0_0000_u64.to_le_bytes());
            let mut report = base_report();

            match decode_exit_context(&exit_context, &mut report) {
                DecodedExit::MemoryAccess {
                    access_type,
                    gpa_unmapped,
                    gva_valid,
                    gpa,
                    gva,
                } => {
                    assert_eq!(access_type, 1);
                    assert!(gpa_unmapped);
                    assert!(gva_valid);
                    assert_eq!(gpa, 0x0000_0000_fee0_0000);
                    assert_eq!(gva, 0xffff_ffff_fee0_0000);
                }
                _ => panic!("expected memory access exit"),
            }
            assert!(report.calls.iter().any(|call| {
                call.name == "DecodeMemoryAccess"
                    && call.detail.contains("write")
                    && call.detail.contains("0x00000000fee00000")
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        base_export_checks, build_native_host_preflight_report, run_partition_smoke,
        NativeGuestEntryMode, NativePartitionSmokeStatus, NativePreflightStatus,
        NativeSerialBootImage, WhpPreflightReport, SERIAL_BOOT_BANNER_TEXT,
    };

    fn whp_report(
        dll_loaded: bool,
        exports_available: bool,
        hypervisor_present: Option<bool>,
    ) -> WhpPreflightReport {
        WhpPreflightReport {
            dll_loaded,
            get_capability_available: exports_available,
            hypervisor_present,
            get_capability_hresult: hypervisor_present.map(|_| "0x00000000".to_string()),
            required_exports: base_export_checks(exports_available),
        }
    }

    #[test]
    fn windows_with_whp_and_hypervisor_is_ready_for_boot_spike() {
        let report = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(true, true, Some(true)),
        );

        assert!(report.ready_for_boot_spike);
        assert!(report
            .checks
            .iter()
            .all(|check| check.status == NativePreflightStatus::Pass));
    }

    #[test]
    fn non_windows_host_is_not_ready() {
        let report = build_native_host_preflight_report(
            "linux".to_string(),
            "x86_64".to_string(),
            false,
            true,
            whp_report(false, false, None),
        );

        assert!(!report.ready_for_boot_spike);
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "host-os" && check.status == NativePreflightStatus::Fail));
    }

    #[test]
    fn missing_whp_library_is_a_blocker() {
        let report = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(false, false, None),
        );

        assert!(!report.ready_for_boot_spike);
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "whp-library" && check.status == NativePreflightStatus::Fail));
    }

    #[test]
    fn missing_whp_exports_are_a_blocker() {
        let report = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(true, false, None),
        );

        assert!(!report.ready_for_boot_spike);
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "whp-exports" && check.status == NativePreflightStatus::Fail));
    }

    #[test]
    fn partition_smoke_is_planned_until_execute_is_requested() {
        let host = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(true, true, Some(true)),
        );

        let report = run_partition_smoke(false, false, None, &host, None);

        assert_eq!(report.status, NativePartitionSmokeStatus::Planned);
        assert!(!report.attempted);
        assert!(report.blocker.is_none());
    }

    #[test]
    fn partition_smoke_plan_preserves_fixture_intent() {
        let host = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(true, true, Some(true)),
        );

        let report = run_partition_smoke(false, true, None, &host, None);

        assert_eq!(report.status, NativePartitionSmokeStatus::Planned);
        assert!(report.fixture_requested);
        assert!(report.next_step.contains("--execute --run-fixture"));
    }

    #[test]
    fn partition_smoke_is_skipped_when_host_preflight_fails() {
        let host = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(false, false, None),
        );

        let report = run_partition_smoke(true, false, None, &host, None);

        assert_eq!(report.status, NativePartitionSmokeStatus::Skipped);
        assert!(!report.attempted);
        assert!(report.blocker.is_some());
    }

    #[test]
    fn partition_smoke_skip_preserves_fixture_intent() {
        let host = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(false, false, None),
        );

        let report = run_partition_smoke(true, true, None, &host, None);

        assert_eq!(report.status, NativePartitionSmokeStatus::Skipped);
        assert!(report.fixture_requested);
        assert!(report.next_step.contains("--execute --run-fixture"));
    }

    #[test]
    fn guest_entry_modes_have_stable_report_labels() {
        assert_eq!(
            NativeGuestEntryMode::RealModeSerial.label(),
            "real-mode-serial"
        );
        assert_eq!(
            NativeGuestEntryMode::LinuxProtectedMode32.label(),
            "linux-protected-mode-32"
        );
    }

    #[test]
    fn skipped_partition_smoke_preserves_linux_entry_metadata() {
        let host = build_native_host_preflight_report(
            "windows".to_string(),
            "x86_64".to_string(),
            true,
            true,
            whp_report(false, false, None),
        );
        let image = NativeSerialBootImage {
            source_label: "pane-runtime-linux-bzimage-protected-mode".to_string(),
            path: Some("vmlinuz-linux".to_string()),
            bytes: vec![0_u8; 128],
            expected_serial_text: SERIAL_BOOT_BANNER_TEXT.to_string(),
            expected_serial_markers: vec!["PANE_INITRAMFS_DISCOVERY_START".to_string()],
            guest_entry_gpa: 0x0010_0000,
            entry_mode: NativeGuestEntryMode::LinuxProtectedMode32,
            boot_params_gpa: Some(0x7000),
            extra_regions: Vec::new(),
        };

        let report = run_partition_smoke(true, true, Some(&image), &host, None);

        assert_eq!(report.status, NativePartitionSmokeStatus::Skipped);
        assert_eq!(
            report.boot_image_source.as_deref(),
            Some("pane-runtime-linux-bzimage-protected-mode")
        );
        assert_eq!(
            report.entry_mode.as_deref(),
            Some("linux-protected-mode-32")
        );
        assert_eq!(report.boot_params_gpa.as_deref(), Some("0x00007000"));
        assert_eq!(
            report.serial_expected_markers,
            vec!["PANE_INITRAMFS_DISCOVERY_START".to_string()]
        );
    }
}
