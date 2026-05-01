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

#[derive(Clone, Debug)]
pub(crate) struct NativeSerialBootImage {
    pub(crate) source_label: String,
    pub(crate) path: Option<String>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) expected_serial_text: String,
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

pub(crate) fn serial_boot_test_image_bytes() -> Vec<u8> {
    let mut image = vec![0_u8; SERIAL_BOOT_TEST_IMAGE_SIZE];
    write_serial_boot_test_image(&mut image);
    image
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
    pub(crate) serial_io_exit_count: u32,
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

#[derive(Debug, Clone, Serialize)]
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

    run_whp_partition_smoke(run_fixture, boot_image)
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
        serial_io_exit_count: 0,
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
        serial_expected_text: run_fixture.then(|| SERIAL_BOOT_BANNER_TEXT.to_string()),
        serial_io_exit_count: 0,
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
) -> NativePartitionSmokeReport {
    windows_whp::run_partition_smoke(run_fixture, boot_image)
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
        ffi::{c_char, c_void, CString},
        mem,
    };

    use super::{
        base_export_checks, NativeExportCheck, NativeGuestEntryMode, NativeGuestMemoryRegion,
        NativeGuestRegionReport, NativePartitionSmokeReport, NativePartitionSmokeStatus,
        NativeSerialBootImage, NativeWhpCallReport, WhpPreflightReport, REQUIRED_WHP_EXPORTS,
        SERIAL_BOOT_BANNER_TEXT, SERIAL_BOOT_TEST_IMAGE_SIZE,
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
    const WHV_RUN_VP_EXIT_REASON_X64_HALT: u32 = 0x0000_0008;
    const WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS: u32 = 0x0000_1000;
    const WHV_RUN_VP_EXIT_REASON_X64_CPUID: u32 = 0x0000_1001;
    const GUEST_PAGE_SIZE: usize = SERIAL_BOOT_TEST_IMAGE_SIZE;
    const SERIAL_COM1_PORT: u16 = 0x03f8;
    const VP_CONTEXT_INSTRUCTION_LENGTH_OFFSET: usize = 10;
    const VP_CONTEXT_RIP_OFFSET: usize = 32;
    const IO_CONTEXT_OFFSET: usize = 48;
    const IO_ACCESS_INFO_OFFSET: usize = IO_CONTEXT_OFFSET + 20;
    const IO_PORT_OFFSET: usize = IO_CONTEXT_OFFSET + 24;
    const IO_RAX_OFFSET: usize = IO_CONTEXT_OFFSET + 32;
    const WHV_REGISTER_RAX: u32 = 0x0000_0000;
    const WHV_REGISTER_RDX: u32 = 0x0000_0002;
    const WHV_REGISTER_RSP: u32 = 0x0000_0004;
    const WHV_REGISTER_RSI: u32 = 0x0000_0006;
    const WHV_REGISTER_RIP: u32 = 0x0000_0010;
    const WHV_REGISTER_RFLAGS: u32 = 0x0000_0011;
    const WHV_REGISTER_ES: u32 = 0x0000_0012;
    const WHV_REGISTER_CS: u32 = 0x0000_0013;
    const WHV_REGISTER_SS: u32 = 0x0000_0014;
    const WHV_REGISTER_DS: u32 = 0x0000_0015;
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

    #[repr(C, align(16))]
    #[derive(Copy, Clone)]
    union WhvRegisterValue {
        reg64: u64,
        segment: WhvX64SegmentRegister,
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
            serial_io_exit_count: 0,
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
                            &mut report,
                        );
                    }
                }
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
                guest_gpa: descriptor.guest_gpa,
                size,
                _memory: memory,
            });
        }

        report.memory_mapped = mapped_all && !mapped_regions.is_empty();
        mapped_regions
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
            selector: 0x08,
            attributes: 0x0000_cf9b,
        };
        let data_segment = WhvX64SegmentRegister {
            base: 0,
            limit: 0xffff_ffff,
            selector: 0x10,
            attributes: 0x0000_cf93,
        };
        let register_names = vec![
            WHV_REGISTER_RAX,
            WHV_REGISTER_RDX,
            WHV_REGISTER_RSP,
            WHV_REGISTER_RSI,
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
            WhvRegisterValue { reg64: 0x90000 },
            WhvRegisterValue {
                reg64: boot_params_gpa,
            },
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

        for exit_index in 0..max_serial_boot_exits {
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
        report: &mut NativePartitionSmokeReport,
    ) {
        report.serial_expected_text = None;
        let max_guest_exits = 32;

        for exit_index in 0..max_guest_exits {
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
                } => {
                    if !(is_write && access_size == 1 && port == SERIAL_COM1_PORT) {
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
                    break;
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
        report.calls.push(NativeWhpCallReport {
            name: "LinuxEntryProbeBoundary",
            hresult: None,
            ok: linux_entry_probe_passed(report),
            detail: linux_entry_probe_detail(report),
        });
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

    fn linux_entry_probe_passed(report: &NativePartitionSmokeReport) -> bool {
        report.virtual_processor_ran
            && report.exit_reason.is_some()
            && !matches!(
                report.exit_reason,
                Some(
                    WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS
                        | WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE
                        | WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION
                        | WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS
                        | WHV_RUN_VP_EXIT_REASON_X64_CPUID
                )
            )
    }

    fn linux_entry_probe_detail(report: &NativePartitionSmokeReport) -> String {
        if linux_entry_probe_passed(report) {
            let exit = report.exit_reason_label.as_deref().unwrap_or("unknown");
            let serial = report.serial_text.as_deref().unwrap_or("");
            if serial.is_empty() {
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
        } else {
            let exit = report.exit_reason_label.as_deref().unwrap_or("unknown");
            let next = match report.exit_reason {
                Some(WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS) => {
                    "map the missing guest memory range or correct the E820/boot params layout"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_CPUID) => {
                    "implement CPUID exit handling for the Linux boot CPU contract"
                }
                Some(WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS) => {
                    "implement MSR exit handling for the Linux boot CPU contract"
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
        IoPort {
            instruction_length: u8,
            rip: u64,
            is_write: bool,
            access_size: u32,
            port: u16,
            serial_byte: u8,
        },
        Halt,
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
            report.calls.push(NativeWhpCallReport {
                name: "DecodeMemoryAccess",
                hresult: None,
                ok: false,
                detail:
                    "Guest accessed memory outside the currently mapped Pane RAM/artifact regions."
                        .to_string(),
            });
            DecodedExit::Other
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
                ok: is_write && access_size == 1 && port == SERIAL_COM1_PORT,
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
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64MsrAccess",
                hresult: None,
                ok: false,
                detail:
                    "Guest reached an MSR access exit; Pane does not emulate Linux boot MSRs yet."
                        .to_string(),
            });
            DecodedExit::Other
        } else if exit_reason == WHV_RUN_VP_EXIT_REASON_X64_CPUID {
            report.calls.push(NativeWhpCallReport {
                name: "DecodeX64Cpuid",
                hresult: None,
                ok: false,
                detail:
                    "Guest reached a CPUID exit; Pane does not emulate Linux boot CPUID leaves yet."
                        .to_string(),
            });
            DecodedExit::Other
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
            0x0000_0007 => "x64-interrupt-window",
            WHV_RUN_VP_EXIT_REASON_X64_HALT => "x64-halt",
            0x0000_0009 => "x64-apic-eoi",
            WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS => "x64-msr-access",
            WHV_RUN_VP_EXIT_REASON_X64_CPUID => "x64-cpuid",
            0x0000_1002 => "exception",
            0x0000_2001 => "canceled",
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
            guest_contract_passed, linux_entry_probe_detail, linux_entry_probe_passed,
            serial_contract_passed, WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE,
            WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS, WHV_RUN_VP_EXIT_REASON_X64_CPUID,
            WHV_RUN_VP_EXIT_REASON_X64_HALT, WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS,
            WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS,
        };
        use crate::native::{
            serial_boot_test_image_bytes, NativePartitionSmokeReport, NativePartitionSmokeStatus,
            SERIAL_BOOT_BANNER_TEXT,
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
                serial_io_exit_count: 0,
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
                    "implement CPUID exit handling",
                ),
                (
                    WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS,
                    "x64-msr-access",
                    "implement MSR exit handling",
                ),
            ] {
                let mut report = base_report();
                report.exit_reason = Some(reason);
                report.exit_reason_label = Some(label.to_string());

                assert!(!linux_entry_probe_passed(&report));
                assert!(linux_entry_probe_detail(&report).contains(expected_next_step));
            }
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

        let report = run_partition_smoke(false, false, None, &host);

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

        let report = run_partition_smoke(false, true, None, &host);

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

        let report = run_partition_smoke(true, false, None, &host);

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

        let report = run_partition_smoke(true, true, None, &host);

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
            guest_entry_gpa: 0x0010_0000,
            entry_mode: NativeGuestEntryMode::LinuxProtectedMode32,
            boot_params_gpa: Some(0x7000),
            extra_regions: Vec::new(),
        };

        let report = run_partition_smoke(true, true, Some(&image), &host);

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
    }
}
