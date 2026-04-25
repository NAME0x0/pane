use serde::Serialize;

const REQUIRED_WHP_EXPORTS: &[&str] = &[
    "WHvGetCapability",
    "WHvCreatePartition",
    "WHvSetPartitionProperty",
    "WHvSetupPartition",
    "WHvDeletePartition",
    "WHvCreateVirtualProcessor",
    "WHvDeleteVirtualProcessor",
    "WHvRunVirtualProcessor",
    "WHvMapGpaRange",
];

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
    pub(crate) calls: Vec<NativeWhpCallReport>,
    pub(crate) blocker: Option<String>,
    pub(crate) next_step: String,
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
    host: &NativeHostPreflightReport,
) -> NativePartitionSmokeReport {
    if !execute {
        return planned_partition_smoke_report();
    }

    if !host.ready_for_boot_spike {
        return skipped_partition_smoke_report(
            true,
            "Native host preflight is not ready; run `pane native-preflight` and resolve failures first.",
        );
    }

    run_whp_partition_smoke()
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
        "Run `pane native-boot-spike --execute` to prove WHP partition/vCPU lifecycle on this host."
            .to_string(),
        "Map guest memory and run a controlled serial-console test kernel.".to_string(),
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

fn planned_partition_smoke_report() -> NativePartitionSmokeReport {
    NativePartitionSmokeReport {
        product_shape:
            "Non-persistent WHP partition/vCPU smoke step for Pane's boot-to-serial milestone.",
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
        calls: Vec::new(),
        blocker: None,
        next_step: "Rerun with `--execute` to create and tear down a WHP partition and vCPU."
            .to_string(),
    }
}

fn skipped_partition_smoke_report(
    execute_requested: bool,
    blocker: impl Into<String>,
) -> NativePartitionSmokeReport {
    NativePartitionSmokeReport {
        product_shape:
            "Non-persistent WHP partition/vCPU smoke step for Pane's boot-to-serial milestone.",
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
        calls: Vec::new(),
        blocker: Some(blocker.into()),
        next_step: "Resolve the blocker, then rerun `pane native-boot-spike --execute`."
            .to_string(),
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
fn run_whp_partition_smoke() -> NativePartitionSmokeReport {
    skipped_partition_smoke_report(true, "WHP partition smoke can only run on Windows hosts.")
}

#[cfg(windows)]
fn probe_whp() -> WhpPreflightReport {
    windows_whp::probe_whp()
}

#[cfg(windows)]
fn run_whp_partition_smoke() -> NativePartitionSmokeReport {
    windows_whp::run_partition_smoke()
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
        ffi::{c_char, c_void, CString},
        mem,
    };

    use super::{
        base_export_checks, NativeExportCheck, NativePartitionSmokeReport,
        NativePartitionSmokeStatus, NativeWhpCallReport, WhpPreflightReport, REQUIRED_WHP_EXPORTS,
    };

    const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 = 0;
    const WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT: u32 = 0x0000_1fff;

    type WhvGetCapability = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
    type WhvCreatePartition = unsafe extern "system" fn(*mut *mut c_void) -> i32;
    type WhvSetPartitionProperty =
        unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32) -> i32;
    type WhvSetupPartition = unsafe extern "system" fn(*mut c_void) -> i32;
    type WhvDeletePartition = unsafe extern "system" fn(*mut c_void) -> i32;
    type WhvCreateVirtualProcessor = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
    type WhvDeleteVirtualProcessor = unsafe extern "system" fn(*mut c_void, u32) -> i32;

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

    pub(super) fn run_partition_smoke() -> NativePartitionSmokeReport {
        let mut report = NativePartitionSmokeReport {
            product_shape:
                "Non-persistent WHP partition/vCPU smoke step for Pane's boot-to-serial milestone.",
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
            calls: Vec::new(),
            blocker: None,
            next_step: "After this passes, map guest memory and run a controlled serial-console test kernel.".to_string(),
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

        let passed = report.partition_created
            && report.processor_count_configured
            && report.partition_setup
            && report.virtual_processor_created
            && report.virtual_processor_deleted
            && report.partition_deleted;

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
}

#[cfg(test)]
mod tests {
    use super::{
        base_export_checks, build_native_host_preflight_report, run_partition_smoke,
        NativePartitionSmokeStatus, NativePreflightStatus, WhpPreflightReport,
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

        let report = run_partition_smoke(false, &host);

        assert_eq!(report.status, NativePartitionSmokeStatus::Planned);
        assert!(!report.attempted);
        assert!(report.blocker.is_none());
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

        let report = run_partition_smoke(true, &host);

        assert_eq!(report.status, NativePartitionSmokeStatus::Skipped);
        assert!(!report.attempted);
        assert!(report.blocker.is_some());
    }
}
