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
- base OS image registration, SHA-256 verification metadata, raw disk/rootfs format inspection, Linux root partition hints, strict native root-disk registration gating, and kernel-layout root handoff decisions,
- a sparse Pane user disk artifact for future Linux system, package, account, and customization data,
- a sparse user disk block I/O primitive with zero-filled unallocated blocks for Pane-owned runtime storage,
- a read-only base OS image block I/O primitive with EOF zero-fill behavior for verified Arch raw disk images, with `--require-native-root-disk` and storage-backed kernel layouts gated on a detectable Linux root partition before Pane exposes `/dev/pane0pN` as root,
- a native block I/O command policy, stateful Pane block-port submit/status protocol, WHP Pane block-port classification boundary, storage contract discovery fields, shared-memory block transfer window, host callback submission path, serviced-exit resume, guest-visible read response placement, and guest-to-host write-payload collection for the future storage device,
- a host-side native block I/O adapter that services allowed kernel-layout read/write commands through verified base-image/user-disk artifacts while preserving base-image read-only policy,
- verified Pane user disk snapshots, restore validation, portable export/import packages, conservative grow-only resize, and metadata repair under the runtime snapshot/storage boundary,
- a runtime-backed `serial-boot.paneimg` test image plus SHA-256 metadata for the WHP boot-spike runner,
- a verified `boot-to-serial-loader.paneimg` candidate path for executing runtime-provided boot code under an explicit serial-output contract,
- a prevalidated native Arch boot-set registration path for the root disk, Linux bzImage, initramfs, and serial-safe kernel cmdline,
- manifest-based native Arch boot-set intake, including a Pane-generated template and packaged SHA-256 manifest handoff helper, for reproducible artifact builders that emit the root disk, kernel, initramfs, SHA-256 values, and cmdline as one JSON handoff,
- a verified kernel/initramfs boot-plan contract with a serial-console cmdline for the next WHP kernel-entry milestone,
- a materialized kernel boot-layout contract for boot params, cmdline, kernel, and optional initramfs guest-memory placement,
- a materialized storage attachment in the kernel layout when the verified Arch base image and Pane user disk are present, including a guest-mapped storage contract page, shared block DMA window, Pane block-port ABI discovery, root handoff decision, sparse disk geometry, and header verification data,
- a first Pane-owned `x8r8g8b8` framebuffer contract and initialized input queue mapped into guest memory for the future app-rendered display boundary, plus Linux `boot_params.screen_info` population and host-side framebuffer/input snapshot reporting after guarded WHP runs,
- a first keyboard/pointer input queue contract mapped into guest memory for the future app-owned input path, including a `PANEINQ1` ABI header with queue size, event-record size, producer/consumer indexes, and capacity metadata,
- Pane runtime contract discovery arguments added to the Linux kernel command line for early boot consumers,
- separate readiness reporting in `pane runtime` and `pane native-preflight` for the guarded WHP boot spike and the stricter native Arch boot attempt path, so missing kernel plans, initramfs driver bundles, pane-block modules, discovery initramfs artifacts, or materialized kernel layouts are surfaced before execution,
- native host preflight through `pane native-preflight`,
- a guarded WHP partition/vCPU lifecycle smoke through `pane native-boot-spike --execute`,
- a guarded WHP guest-memory/register/vCPU execution test image through `pane native-boot-spike --prepare-runtime --execute --run-fixture`,
- a guarded WHP boot-loader candidate execution path through `pane native-boot-spike --prepare-runtime --execute --run-boot-loader`.

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
- sparse user-disk artifact readiness.

It must remain side-effect-free. It reports blockers; it does not enable Windows features, modify firmware settings, download an OS, or attempt to boot.

## Implementation Sequence

1. Native preflight: dynamically load WHP and report host/runtime blockers without linking Pane to WHP at startup. `pane native-preflight --prepare-runtime` also creates the Pane-owned runtime directories, manifests, framebuffer contract, input contract, sparse user disk, and serial fixture so kernel-layout work starts from a valid app-owned boundary.
2. Partition smoke: create a WHP partition, configure one vCPU, create that vCPU, and tear everything down cleanly.
3. Runtime-backed serial test image: materialize `serial-boot.paneimg` under the Pane runtime, map it as guest memory, configure vCPU registers, run it, decode the `PANE_BOOT_OK` COM1 banner across repeated I/O exits, observe HLT, unmap memory, and tear everything down cleanly.
4. Runtime-provided boot-loader candidate: register a verified `boot-to-serial-loader.paneimg`, require a SHA-256 match and expected serial text, then execute that artifact through WHP with `--run-boot-loader`.
5. Kernel boot plan: register a verified Linux kernel, inspect its bzImage header for boot-protocol/setup metadata, register an optional verified initramfs, and require an explicit `console=ttyS0` cmdline under `kernel-boot.json` without claiming it executes yet.
6. Kernel boot layout: materialize `kernel-boot-layout.json` with boot params built from the original bzImage setup header plus Pane-owned loader fields, augmented runtime-discovery cmdline, native-spike serial visibility defaults (`earlycon`, `earlyprintk`, `quiet`, `loglevel=4`, `panic=-1`, and `nomodeset` when absent), bzImage setup bytes, protected-mode payload placement, explicit Linux entry metadata, an initial E820 guest memory map, mapped storage/framebuffer/input contract ranges, a shared block DMA transfer window, Pane block-port ABI discovery, and optional initramfs guest-physical addresses with the loaded initramfs range reserved in E820.
7. Boot-to-serial spike: implement WHP kernel entry, boot parameters, initramfs placement, and serial output capture far enough to prove Linux boot progress.
8. CPU, I/O, and MMIO exit handling: configure the Linux 32-bit boot-protocol register contract with a stack in mapped low RAM, map a Pane-owned boot GDT, classify unhandled memory-access exits as blockers with exact access type/GPA/GVA diagnostics, pass WHP default CPUID results back to the guest, maintain an explicit Linux MSR state for RDMSR/WRMSR including APIC base, PAT, SYSENTER, EFER, TSC AUX, and MTRR probe defaults, emulate the basic COM1 UART register behavior needed for early serial setup, emulate width-aware legacy PIC/PIT/ELCR/CMOS RTC/system-control A/B/delay/PCI-config/i8042/VGA/ACPI PM probe port accesses including deterministic i8042 command-byte, self-test, interface-test, output-port responses, and ACPI PM timer reads, classify unsupported I/O ports as explicit blockers, classify Pane block I/O port exits as storage work instead of undefined serial failures, submit allowed block commands to Pane's host runtime storage adapter, use the mapped Pane block DMA window for read/write payload bytes when present, fall back to the port data window for older layouts, resume the guest after serviced block exits, observe and resume WHP interrupt-window/APIC-EOI exits, track total guest exits, expand the Linux probe exit budget when storage/initramfs serial milestones are expected, map reserved IOAPIC/local APIC MMIO stub pages for early probes, and keep expanding the Linux boot CPU/device contract only when the next real exit demands it.
9. Runtime artifact boot: connect the kernel path to Pane's verified Arch base image and Pane sparse user disk. The layout now carries `/dev/pane0` as the read-only base OS device, a machine-readable root handoff such as `/dev/pane0p1` when a Linux root partition is detected, and `/dev/pane1` as writable user/package/customization storage when both artifacts are verified, including a guest-mapped storage contract page, shared block DMA window, Pane block-port ABI discovery, sparse logical size, block size, backing mode, and header hash.
10. Storage materialization: turn the descriptor into a durable block-device format with resize, snapshot, repair, export, and import semantics. Pane now has verified user-disk snapshot, restore, export, import, grow-only resize, and metadata-repair artifacts; deeper filesystem repair remains an explicit future increment.
11. Display milestone: add a Pane-owned framebuffer and input path inside the app window. The runtime now records the first fixed linear framebuffer and keyboard/pointer contracts, and the WHP kernel-layout runner maps those memory ranges. The remaining work is making them active guest-visible devices and rendering the framebuffer in Pane's app surface.
12. Integration milestone: add clipboard, file exchange boundaries, audio, resize, recovery, logging, and diagnostics.
13. Compatibility milestone: measure performance, hardware requirements, Windows feature requirements, and failure modes before exposing the native runtime as a default.

## Non-Negotiable Acceptance Gates

Pane cannot claim the native runtime is real until:

- a clean Windows machine can run `pane native-preflight` and receive actionable host checks,
- `pane native-boot-spike --execute` can create and tear down a WHP partition and vCPU without leaking resources,
- `pane native-boot-spike --prepare-runtime --execute --run-fixture` can load the runtime-backed serial boot image, map guest memory, set registers, run guest code, decode the deterministic `PANE_BOOT_OK` serial banner, observe HLT, unmap memory, and release all WHP resources,
- `pane native-boot-spike --prepare-runtime --execute --run-boot-loader` can load a verified runtime-provided boot-loader candidate, validate its expected serial output, observe HLT, and release all WHP resources,
- `pane runtime --register-kernel` can prepare a verified kernel/initramfs boot plan with serial console output required before any WHP kernel-entry work starts,
- `pane runtime --write-initramfs-driver` can generate a reproducible Pane initramfs driver source/build-script bundle with a self-contained C `/init` that discovers `pane.storage_contract`, `pane.block_io`, `pane.block_dma`, `pane.block_devices`, `pane.root`, and `pane.user`, writes `/run/pane/native-storage.env`, loads `pane-block.ko` with exact base/user `device_blocks` geometry and shared-buffer parameters when present, waits for the declared root device, mounts it at `/newroot`, executes the real init once the Pane block device exists, and includes the Pane block-driver source/build contract that exposes the verified base OS as `/dev/pane0` plus the writable user disk as `/dev/pane1` with 4096-byte guest I/O blocks, shared-memory read/write payload transfer, partial read/write handling, and flush/discard tolerance for partition scanners and filesystems,
- `pane runtime --build-pane-block-module --kernel-build-dir <path>` can run the generated module build script against a target Arch kernel build tree and register the resulting `pane-block.ko` with a computed SHA,
- `pane runtime --register-pane-block-module` can copy a compiled `pane-block.ko` into the generated initramfs driver bundle only after SHA-256 verification, binds it to the current verified kernel artifact plus generated `pane-block.c` hash, and the discovery initramfs build refuses to package an unverified or stale manually dropped module,
- `pane runtime --build-discovery-initramfs` can compile the generated guest `/init` and `pane-port-probe` with `PANE_LINUX_CC`/`PANE_LINUX_CC_ARGS`, `cc`, or `zig cc -target x86_64-linux-musl`, verify both outputs are ELF binaries, package the `newc` initramfs archive inside Pane without host `cpio` or `bsdtar`, then register the produced discovery cpio as a verified initramfs artifact in the existing kernel boot plan,
- `pane runtime --build-discovery-initramfs --discovery-init-binary <elf> --discovery-probe-binary <elf>` can consume reproducible-builder guest binaries without invoking `cc`, while still rejecting non-ELF Windows-host binaries before packaging,
- Pane block-module verification is tied to the target kernel plus the stable Pane block-driver ABI hash rather than every generated source byte, so compatible logging/diagnostic changes do not unnecessarily invalidate an otherwise usable `pane-block.ko`,
- `pane native-kernel-plan --prepare-runtime --materialize` can prepare the runtime boundary, then write and re-validate the deterministic kernel boot layout before the WHP runner maps those guest addresses; layouts now include a `rust-vmm/linux-loader` adapter/provenance record, and storage-backed layouts require verified Pane initramfs driver bundle metadata plus a verified Pane block module before reporting ready,
- materialized kernel layouts attach the verified base OS image and Pane user disk when both exist, including both the current Pane block-port diagnostic bridge and the replacement virtio-blk backend contract that maps the read-only Arch base disk to `vda` and the writable Pane user disk to `vdb`,
- `pane native-preflight` and `pane native-boot-spike` publish a typed crosvm-style device-loop route contract for WHP exits, making the current serial, block diagnostic, legacy platform I/O, timer, display, input, and CPU-control ownership explicit before those handlers are extracted into real devices,
- runtime preparation writes explicit framebuffer and input contracts, the kernel boot params advertise the framebuffer through Linux `screen_info`, and the mapped input queue begins with a deterministic `PANEINQ1` header, so display/input work has a stable guest/device boundary instead of an undefined "draw pixels somehow" milestone,
- `pane native-boot-spike --prepare-runtime --execute --run-kernel-layout` now uses the stricter Arch boot-attempt readiness contract before loading the kernel layout into WHP: the verified root disk, user disk, kernel plan, initramfs driver bundle, pane-block module, discovery initramfs, materialized layout, framebuffer/input contracts, runtime config, native manifest, and WHP host checks must all be ready. When they are ready, Pane maps boot params, cmdline, kernel, optional initramfs, non-overlapping RAM regions, reserved initramfs E820 placement, a boot-protocol GDT, reserved APIC MMIO stub pages, framebuffer memory, input queue memory, and the Pane block DMA window, then selects the protected-mode Linux entry probe with boot params in `rsi`; storage-backed Linux layouts must report module-load, display-contract, root-mount, and init-exec serial milestones before being treated as Arch userspace boot progress, and `--trace-checkpoint <path>` writes incremental JSON diagnostics while long native runs are still in progress. The WHP runner now guards long vCPU runs, treats canceled runs as explicit time-slice boundaries, applies a longer storage-backed live-run budget while preserving the no-progress escape hatch, tracks PIC/PIT state enough to derive the timer vector, requests the first native timer interrupt through `WHvRequestInterrupt`, performs one guarded post-interrupt resume, captures interrupt-delivery state through `WHvGetVirtualProcessorRegisters`, records block-I/O resume checkpoints, and services 4096-byte data payloads through shared guest memory instead of requiring 128 data-port exits per 512-byte block. The generated `pane-block.ko` also caps successful per-transfer status logging after the first diagnostic samples, and the generated discovery init uses Pane's root filesystem hint and mounts the immutable base root read-only first so root-mount probing matches the actual storage ownership model. Current live traces prove the generated `pane-block.ko` loads with `PANE_BLOCK_SHARED_BUFFER_OK` and can complete real 4096-byte root-device reads through Pane-owned storage; the remaining blocker is the WHP/Linux timing path that reaches `PANE_ROOT_MOUNT_ATTEMPT` but still times out before `PANE_ROOT_MOUNT_OK`,
- a test image can boot under a Pane-owned WHP host without WSL, XRDP, `mstsc.exe`, QEMU, VirtualBox, or Hyper-V Manager,
- Pane can boot a verified Arch base image plus a Pane-owned user disk,
- Pane renders the guest through its own app surface,
- shutdown, reset, repair, support bundles, logs, and failure recovery work without asking users to debug hypervisor internals,
- the current WSL/XRDP bridge remains clearly labeled as the fallback or legacy bridge.

## Product Implication

This is slower than shipping a wrapper around an existing VM product, but it is the path that matches Pane's stated vision. The near-term work should bias toward measurable native-runtime milestones instead of adding more distro and desktop options on top of the bridge.

The VMM implementation direction is now explicitly crosvm/rust-vmm based. Pane should use crosvm as the reference architecture and adopt rust-vmm building blocks such as linux-loader and vm-virtio behind narrow Pane-owned adapters instead of expanding the bespoke Pane block-port protocol into a full storage/display stack. See [vmm-foundation.md](vmm-foundation.md).
