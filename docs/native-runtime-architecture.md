# Native Runtime Architecture

Pane's long-term runtime goal is a contained Windows app that owns both the Linux OS runtime and the desktop presentation. The current MVP is not there yet. It uses WSL2, XRDP, and `mstsc.exe` for the launchable Arch desktop while Pane builds the app-owned runtime contract in parallel.

This document is the contract for moving from that bridge to a Pane-owned runtime without drifting into another thin wrapper.

## Current Boundary

Pane currently owns:

- the packaged `pane.exe` app entrypoint,
- the Control Center,
- managed Arch onboarding and repair flows,
- PaneShared user file exchange,
- dedicated runtime storage under `%LOCALAPPDATA%\Pane\runtime\<session>`,
- base OS image registration and SHA-256 verification metadata,
- a user-disk descriptor for future Linux system, package, account, and customization data,
- a runtime-backed `serial-boot.paneimg` test image plus SHA-256 metadata for the WHP boot-spike runner,
- a verified `boot-to-serial-loader.paneimg` candidate path for executing runtime-provided boot code under an explicit serial-output contract,
- a verified kernel/initramfs boot-plan contract with a serial-console cmdline for the next WHP kernel-entry milestone,
- a materialized kernel boot-layout contract for boot params, cmdline, kernel, and optional initramfs guest-memory placement,
- native host preflight through `pane native-preflight`,
- a guarded WHP partition/vCPU lifecycle smoke through `pane native-boot-spike --execute`,
- a guarded WHP guest-memory/register/vCPU execution test image through `pane native-boot-spike --execute --run-fixture`,
- a guarded WHP boot-loader candidate execution path through `pane native-boot-spike --execute --run-boot-loader`.

Pane does not yet own:

- booting the Linux kernel or init system from Pane runtime artifacts,
- presenting a Linux framebuffer inside a Pane-owned window,
- app-owned input, clipboard, audio, resize, GPU, or display transport,
- snapshot, export, import, and repair semantics for a booted Pane-owned disk.

## Runtime Decision

The native runtime path should target the Windows Hypervisor Platform (WHP) directly before considering any bundled third-party hypervisor dependency. WHP is the closest fit for Pane's product claim because it lets Pane become the host application that creates the partition, maps memory, creates virtual processors, runs the guest, and owns the surrounding UX.

Pane should not treat QEMU, VirtualBox, Hyper-V Manager, WSL, XRDP, or `mstsc.exe` as the final runtime/display architecture. Those can be temporary references, migration aids, or compatibility comparisons, but the product should not ship as "Pane-owned" if another app is still the real runtime or renderer.

Relevant Microsoft API surfaces:

- https://learn.microsoft.com/en-us/virtualization/api/
- https://learn.microsoft.com/en-us/virtualization/api/hypervisor-platform/hypervisor-platform
- https://learn.microsoft.com/en-us/virtualization/api/hypervisor-platform/funcs/whvrunvirtualprocessor
- https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/user-guide/make-integration-service

## Preflight Gate

`pane native-preflight` is the first native-runtime implementation slice. It is intentionally not a boot engine. It answers whether the current host is suitable for the first WHP boot spike and whether Pane runtime artifacts are ready enough to use.

The command checks:

- Windows host requirement,
- supported CPU architecture,
- `WinHvPlatform.dll` loadability,
- required WHP exports such as `WHvGetCapability`, `WHvCreatePartition`, `WHvSetupPartition`, `WHvCreateVirtualProcessor`, `WHvSetVirtualProcessorRegisters`, `WHvRunVirtualProcessor`, `WHvMapGpaRange`, and `WHvUnmapGpaRange`,
- hypervisor presence reported by WHP,
- runtime storage preparation,
- verified base OS image metadata,
- user-disk descriptor readiness.

It must remain side-effect-free. It reports blockers; it does not enable Windows features, modify firmware settings, download an OS, or attempt to boot.

## Implementation Sequence

1. Native preflight: dynamically load WHP and report host/runtime blockers without linking Pane to WHP at startup.
2. Partition smoke: create a WHP partition, configure one vCPU, create that vCPU, and tear everything down cleanly.
3. Runtime-backed serial test image: materialize `serial-boot.paneimg` under the Pane runtime, map it as guest memory, configure vCPU registers, run it, decode the `PANE_BOOT_OK` COM1 banner across repeated I/O exits, observe HLT, unmap memory, and tear everything down cleanly.
4. Runtime-provided boot-loader candidate: register a verified `boot-to-serial-loader.paneimg`, require a SHA-256 match and expected serial text, then execute that artifact through WHP with `--run-boot-loader`.
5. Kernel boot plan: register a verified Linux kernel, inspect its bzImage header for boot-protocol/setup metadata, register an optional verified initramfs, and require an explicit `console=ttyS0` cmdline under `kernel-boot.json` without claiming it executes yet.
6. Kernel boot layout: materialize `kernel-boot-layout.json` with boot params built from the original bzImage setup header plus Pane-owned loader fields, cmdline, bzImage setup bytes, protected-mode payload placement, explicit Linux entry metadata, an initial E820 guest memory map, and optional initramfs guest-physical addresses with the loaded initramfs range reserved in E820.
7. Boot-to-serial spike: implement WHP kernel entry, boot parameters, initramfs placement, and serial output capture far enough to prove Linux boot progress.
8. CPU and MMIO exit handling: configure the Linux 32-bit boot-protocol register contract with a stack in mapped low RAM, map a Pane-owned boot GDT, classify unhandled memory-access exits as blockers with exact access type/GPA/GVA diagnostics, pass WHP default CPUID results back to the guest, maintain a minimal guest MSR state for RDMSR/WRMSR, emulate the basic COM1 UART register behavior needed for early serial setup, map reserved IOAPIC/local APIC MMIO stub pages for early probes, and keep expanding the Linux boot CPU/device contract only when the next real exit demands it.
9. Runtime artifact boot: connect the kernel path to Pane's verified Arch base image and Pane user disk descriptor.
10. Storage materialization: turn the descriptor into a durable block-device format with resize, snapshot, repair, export, and import semantics.
11. Display milestone: add a Pane-owned framebuffer and input path inside the app window.
12. Integration milestone: add clipboard, file exchange boundaries, audio, resize, recovery, logging, and diagnostics.
13. Compatibility milestone: measure performance, hardware requirements, Windows feature requirements, and failure modes before exposing the native runtime as a default.

## Non-Negotiable Acceptance Gates

Pane cannot claim the native runtime is real until:

- a clean Windows machine can run `pane native-preflight` and receive actionable host checks,
- `pane native-boot-spike --execute` can create and tear down a WHP partition and vCPU without leaking resources,
- `pane native-boot-spike --execute --run-fixture` can load the runtime-backed serial boot image, map guest memory, set registers, run guest code, decode the deterministic `PANE_BOOT_OK` serial banner, observe HLT, unmap memory, and release all WHP resources,
- `pane native-boot-spike --execute --run-boot-loader` can load a verified runtime-provided boot-loader candidate, validate its expected serial output, observe HLT, and release all WHP resources,
- `pane runtime --register-kernel` can prepare a verified kernel/initramfs boot plan with serial console output required before any WHP kernel-entry work starts,
- `pane native-kernel-plan --materialize` can write and re-validate the deterministic kernel boot layout before the WHP runner maps those guest addresses,
- `pane native-boot-spike --execute --run-kernel-layout` can consume that layout, map boot params, cmdline, kernel, optional initramfs, non-overlapping RAM regions, reserved initramfs E820 placement, a boot-protocol GDT, and reserved APIC MMIO stub pages, then select the correct guest-entry contract: real-mode serial/HALT validation for the controlled candidate or a protected-mode Linux entry probe with boot params in `rsi` for a bzImage payload,
- a test image can boot under a Pane-owned WHP host without WSL, XRDP, `mstsc.exe`, QEMU, VirtualBox, or Hyper-V Manager,
- Pane can boot a verified Arch base image plus a Pane-owned user disk,
- Pane renders the guest through its own app surface,
- shutdown, reset, repair, support bundles, logs, and failure recovery work without asking users to debug hypervisor internals,
- the current WSL/XRDP bridge remains clearly labeled as the fallback or legacy bridge.

## Product Implication

This is slower than shipping a wrapper around an existing VM product, but it is the path that matches Pane's stated vision. The near-term work should bias toward measurable native-runtime milestones instead of adding more distro and desktop options on top of the bridge.
