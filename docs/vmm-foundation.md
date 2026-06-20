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
