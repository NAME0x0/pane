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
| rust-vmm/linux-loader | Direct dependency candidate for bzImage loading, cmdline placement, and Linux boot params. | Apache-2.0 OR BSD-3-Clause |
| rust-vmm/vm-virtio | Direct dependency candidate for virtio queues and device semantics. | Apache-2.0 OR BSD-3-Clause |
| virtio-blk | Replacement target for Pane's custom block-port protocol and generated `pane-block.ko` boot dependency. | Linux-standard guest model via rust-vmm implementation |

## Migration Contract

1. Keep current WHP probes as diagnostics only.
2. Add a narrow linux-loader adapter behind existing `native-kernel-plan` output.
3. Replace custom Pane block-port root storage with a virtio-blk backend for the read-only Arch base disk and writable Pane user disk.
4. Move WHP exit dispatch into a typed crosvm-style device loop.
5. Promote display/input from fixed contracts to virtio-gpu/input-inspired app rendering.

The CLI command `pane native-foundation` prints the current machine-readable version of this contract.

Current implementation status: `pane native-kernel-plan --materialize` emits a `linux_loader` adapter/provenance record inside `kernel-boot-layout.json`. That record deliberately marks the adapter as `adapter-boundary-not-yet-linked`: Pane has created the seam and readiness gate, but has not yet linked the `linux-loader` crate directly. The next implementation step is replacing the manual boot-params writer behind that seam with the crate-backed adapter.
