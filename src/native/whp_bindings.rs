use std::{ffi::c_void, os::raw::c_char};

use windows_sys::Win32::System::Hypervisor::{
    WHvCapabilityCodeHypervisorPresent, WHvMapGpaRangeFlagExecute, WHvMapGpaRangeFlagRead,
    WHvMapGpaRangeFlagWrite, WHvPartitionPropertyCodeLocalApicEmulationMode,
    WHvPartitionPropertyCodeProcessorCount, WHvRunVpExitReasonCanceled,
    WHvRunVpExitReasonInvalidVpRegisterValue, WHvRunVpExitReasonMemoryAccess,
    WHvRunVpExitReasonUnrecoverableException, WHvRunVpExitReasonX64ApicEoi,
    WHvRunVpExitReasonX64Cpuid, WHvRunVpExitReasonX64Halt, WHvRunVpExitReasonX64InterruptWindow,
    WHvRunVpExitReasonX64IoPortAccess, WHvRunVpExitReasonX64MsrAccess,
    WHvX64LocalApicEmulationModeXApic,
};
pub(crate) use windows_sys::Win32::System::Hypervisor::{
    WHV_EMULATOR_CALLBACKS as WhvEmulatorCallbacks,
    WHV_EMULATOR_IO_ACCESS_INFO as WhvEmulatorIoAccessInfo,
    WHV_EMULATOR_MEMORY_ACCESS_INFO as WhvEmulatorMemoryAccessInfo,
    WHV_EMULATOR_STATUS as WhvEmulatorStatus, WHV_INTERRUPT_CONTROL as WhvInterruptControl,
    WHV_MEMORY_ACCESS_CONTEXT as WhvMemoryAccessContext,
    WHV_REGISTER_VALUE as OfficialWhvRegisterValue,
    WHV_TRANSLATE_GVA_RESULT as WhvTranslateGvaResult, WHV_VP_EXIT_CONTEXT as WhvVpExitContext,
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{GetProcAddress, LoadLibraryA},
};

pub(crate) type ModuleHandle = HMODULE;

pub(crate) const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 =
    WHvCapabilityCodeHypervisorPresent as u32;
pub(crate) const WHV_PARTITION_PROPERTY_CODE_PROCESSOR_COUNT: u32 =
    WHvPartitionPropertyCodeProcessorCount as u32;
pub(crate) const WHV_PARTITION_PROPERTY_CODE_LOCAL_APIC_EMULATION_MODE: u32 =
    WHvPartitionPropertyCodeLocalApicEmulationMode as u32;
pub(crate) const WHV_X64_LOCAL_APIC_EMULATION_MODE_XAPIC: u32 =
    WHvX64LocalApicEmulationModeXApic as u32;
pub(crate) const WHV_MAP_GPA_RANGE_FLAG_READ: u32 = WHvMapGpaRangeFlagRead as u32;
pub(crate) const WHV_MAP_GPA_RANGE_FLAG_WRITE: u32 = WHvMapGpaRangeFlagWrite as u32;
pub(crate) const WHV_MAP_GPA_RANGE_FLAG_EXECUTE: u32 = WHvMapGpaRangeFlagExecute as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_MEMORY_ACCESS: u32 = WHvRunVpExitReasonMemoryAccess as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_IO_PORT_ACCESS: u32 =
    WHvRunVpExitReasonX64IoPortAccess as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_UNRECOVERABLE_EXCEPTION: u32 =
    WHvRunVpExitReasonUnrecoverableException as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_INVALID_VP_REGISTER_VALUE: u32 =
    WHvRunVpExitReasonInvalidVpRegisterValue as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_INTERRUPT_WINDOW: u32 =
    WHvRunVpExitReasonX64InterruptWindow as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_HALT: u32 = WHvRunVpExitReasonX64Halt as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_APIC_EOI: u32 = WHvRunVpExitReasonX64ApicEoi as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_MSR_ACCESS: u32 = WHvRunVpExitReasonX64MsrAccess as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_X64_CPUID: u32 = WHvRunVpExitReasonX64Cpuid as u32;
pub(crate) const WHV_RUN_VP_EXIT_REASON_CANCELED: u32 = WHvRunVpExitReasonCanceled as u32;

pub(crate) unsafe fn load_library(name: *const c_char) -> ModuleHandle {
    LoadLibraryA(name.cast())
}

pub(crate) unsafe fn get_proc_address(module: ModuleHandle, symbol: *const c_char) -> *mut c_void {
    GetProcAddress(module, symbol.cast())
        .map(|function| function as *const () as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

pub(crate) unsafe fn free_library(module: ModuleHandle) -> i32 {
    FreeLibrary(module)
}
