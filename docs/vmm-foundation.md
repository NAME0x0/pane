# Pane VMM Foundation

Pane's native runtime should not grow into a bespoke virtual machine monitor one port handler at a time. The implementation direction is now:

- use `rust-vmm/linux-loader` for Linux image loading, command line placement, and boot-parameter construction;
- use `rust-vmm/vm-virtio` and virtio device semantics for storage first, then display/input;
- use crosvm as the reference VMM architecture for WHPX, device-loop, virtio, display, and input decisions;
- keep Pane as the app/runtime owner rather than becoming a generic VM wrapper.

## Why This Matters

The current WHP probe path has proven useful, but it is not a sustainable final architecture. Linux expects standard device models, interrupt behavior, boot parameters, and display/input paths. Rebuilding all of that through Pane-specific ports and a custom `pane-block.ko` would create years of avoidable compatibility work.

The product target is still a Pane-owned app that boots and renders Arch without WSL, XRDP, or `mstsc.exe`. The foundation changes how we get there: proven boot/device components under Pane's app experience, not a QEMU wrapper and not a hand-rolled VMM stack.

## Selected Components

| Component | Role | License posture |
| --- | --- | --- |
| crosvm | Reference architecture for Rust VMM structure, WHPX support, virtio devices, display, and input. | BSD-3-Clause |
| Microsoft `windows-sys` | Generated WHP ABI definitions and delayed Windows DLL loading primitives. | MIT OR Apache-2.0 |
| rust-vmm/vm-memory | Page-backed guest-memory ownership, bounded guest-address access, and stable host mappings. | Apache-2.0 OR BSD-3-Clause |
| rust-vmm/virtio-queue | Linked split-virtqueue validation, descriptor iteration, available-ring consumption, and used-ring publication. | Apache-2.0 AND BSD-3-Clause |
| rust-vmm/virtio-bindings | Linked source of virtio-MMIO register offsets, virtio-blk request/status/feature constants, ring descriptor flags, and device IDs, replacing hand-transcribed magic numbers. | BSD-3-Clause OR Apache-2.0 |
| rust-vmm/linux-loader | Linked adapter for bzImage loading, cmdline placement, and Linux boot params. | Apache-2.0 AND BSD-3-Clause |
| rust-vmm/vm-virtio | Direct dependency candidate for virtio queues and device semantics. | Apache-2.0 OR BSD-3-Clause |
| virtio-blk | Replacement target for Pane's custom block-port protocol and generated `pane-block.ko` boot dependency. | Linux-standard guest model via rust-vmm implementation |

## Migration Contract

1. Keep current WHP probes as diagnostics only.
2. Add a narrow linux-loader adapter behind existing `native-kernel-plan` output.
3. Replace custom Pane block-port root storage with a virtio-blk backend for the read-only Arch base disk and writable Pane user disk.
4. Move WHP exit dispatch into a typed crosvm-style device loop.
5. Promote display/input from fixed contracts to virtio-gpu/input-inspired app rendering.

The CLI command `pane native-foundation` prints the current machine-readable version of this contract.

Current implementation status: the WHP binding migration is linked and advancing incrementally. Pane directly depends on Microsoft `windows-sys 0.61.2`, uses its generated Kernel32 loader bindings while preserving delayed `WinHvPlatform.dll`/`WinHvEmulation.dll` capability checks, derives core WHP capability/property/GPA-map/exit constants from generated definitions, and uses the generated interrupt-control, GVA-translation, emulator callback/status/access, memory-access, and vCPU-exit structures directly. Pane retains one narrow 16-byte-aligned register-value compatibility wrapper because live WHP register-array calls require 16-byte slot alignment while the generated Rust union is only 8-byte aligned; emulator callbacks copy values across that documented ABI boundary. The guarded serial fixture now terminates at the exact expected banner boundary instead of issuing another run that can leave a halted WHP vCPU blocked, while still accepting a reported HLT exit. The partition/vCPU run loop remains behavior-compatible and dynamically resolved; the remaining Step 1 work is reducing manually declared delayed function signatures without sacrificing optional host capability detection.

The guest-memory migration is linked through `vm-memory 0.17.1`, the newest release accepted by `linux-loader 0.13.2`. Pane's narrow `PaneMmapGuestMemory` adapter owns rust-vmm `GuestMemoryMmap` regions, enforces guest-address bounds, supplies stable host addresses to `WHvMapGpaRange`, and backs both live mapped WHP regions and production virtio smoke execution. Pane's device interfaces still depend on the Pane-owned `PaneGuestMemory` trait, so later queue/device migrations do not leak upstream types across module boundaries.

`pane native-kernel-plan --materialize` emits a linked `linux_loader` adapter/provenance record inside `kernel-boot-layout.json`. Linux layouts are validated and loaded with `BzImage`, generated `boot_params` are serialized through `LinuxBootConfigurator`, and command lines are materialized through `load_cmdline`. Pane vendors the exact 0.13.2 source because the published crate enables Unix-only `vm-memory/rawfd` defaults on Windows; the local change only disables that default and pins the compatible memory release. Storage-backed layouts emit a `virtio_block` backend contract that maps the read-only Arch base image to `vda`, the writable Pane user disk to `vdb`, and the root partition to `vda1` when detected. The custom block-port path remains diagnostic-only until the standard virtio-blk path proves root mount; no further product functionality should be added to the custom protocol.

The production virtio split-ring path is linked through `virtio-queue 0.17.0`. Pane now allocates one rust-vmm multi-region guest address space for WHP, translates its MMIO queue state into a rust-vmm `Queue`, consumes available entries and descriptor chains with upstream validation, and publishes used entries through the upstream implementation. Pane retains its narrow request/backend model while real Arch guest acknowledgement and the final `vm-virtio` device abstraction remain unproven. The exact queue source is vendored because its published dependency also enables Unix-only `vm-memory/rawfd` defaults on Windows; Pane disables only that default.

Both the live WHP mapped-memory view and the production virtio smoke memory expose the same underlying `vm_memory::GuestMemoryMmap` through Pane's additive guest-memory adapter. Multi-region tests verify that rust-vmm sees the complete WHP guest address space while Pane's existing memory interface remains available during migration.

The June 20 live kernel-layout validation exercised the rust-vmm-only split-ring service and reached `PANE_BLOCK_MODULE_LOAD_OK` plus `PANE_DISPLAY_CONTRACT_DISCOVERED`. The guest attempted `/dev/vda1` but reported `PANE_VIRTIO_ROOT_DEVICE_WAIT_TIMEOUT`; it then entered the diagnostic pane-block root path, where Pane serviced one base-image read before the root-mount budget expired. Neither `PANE_ROOT_MOUNT_OK` nor `PANE_INIT_EXEC` was reached, and no virtio interrupt request or acknowledgement was observed. The custom pane-block subsystem therefore remains a labeled fallback and must not be deleted yet.

The June 21 diagnosis identified why the virtio-MMIO device saw zero accesses: the guest never instantiated it. The stock Arch kernel (`7.0.5-arch1-1`) was confirmed by extracting its embedded config to build virtio-MMIO as a loadable module (`CONFIG_VIRTIO_MMIO=m`) with the cmdline-device path enabled (`CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`) and virtio-blk built in (`CONFIG_VIRTIO_BLK=y`). The Pane discovery initramfs neither bundled nor loaded `virtio_mmio.ko`, so `virtio_mmio.device=4K@0xdfc0000:5` had no bus driver to act on and no `/dev/vda` was ever created. Pane now loads the virtio-MMIO bus before the virtio-root wait: `pane runtime --register-virtio-mmio-module <virtio_mmio.ko> --virtio-mmio-module-expected-sha256 <sha>` copies the kernel-matched module into the discovery initramfs as `/lib/modules/virtio_mmio.ko`, and the generated `/init` `finit_module`s it so the device registers and the built-in virtio-blk driver binds. Because the kernel does not replay the boot-cmdline `virtio_mmio.device=` value to a userspace-loaded module, `/init` parses that value and passes it explicitly as the module's `device=` parameter.

The June 21 live boot with the device parameter cleared the zero-MMIO blocker: the guest loaded the bus module, enumerated `/dev/vda1`, mounted the virtio root with no wait timeout and no pane-block fallback, and drove the split virtqueue with delivered and acknowledged interrupts (43 queue completions observed). The remaining gap is throughput: the ext4 root mount did not reach `PANE_ROOT_MOUNT_OK` within the 120s root-mount phase budget because virtio-mmio uses a level-triggered interrupt while Pane injects a single edge per `interrupt_status` 0->1 transition, so completions that arrive while the status is still asserted do not re-notify the guest, which then depends on the capped 1s root-mount timer pulses. Reliable level-style virtio completion interrupt delivery is the next milestone; with it the mount should finish well inside budget.
