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

Current implementation status: `pane native-kernel-plan --materialize` emits a `linux_loader` adapter/provenance record inside `kernel-boot-layout.json`. That record deliberately marks the adapter as `adapter-boundary-not-yet-linked`: Pane has created the seam and readiness gate, but has not yet linked the `linux-loader` crate directly. Storage-backed layouts also emit a `virtio_block` backend contract that maps the read-only Arch base image to `vda`, the writable Pane user disk to `vdb`, and the root partition to `vda1` when detected. Pane now reserves a modern virtio-MMIO block register aperture at `0x0dfc0000`, advertises it to Linux with `virtio_mmio.device=4K@0xdfc0000:5`, leaves that aperture unmapped as ordinary RAM so WHP can route MMIO exits into Pane, executes live MMIO instructions through `WinHvEmulation.dll`, advertises Linux-compatible modern virtio-blk feature bits, handles width-aware config reads, masks unsupported driver features, gates FEATURES_OK and DRIVER_OK, reports absent queues correctly, resets queue runtime state on QueueReady clear, ignores queue notifications until DRIVER_OK and QueueReady are both set, drains batched split-virtqueue notifications into the verified native block handler, requests the virtio IRQ through WHP after queue completion, and records guest interrupt acknowledgements when Linux writes the virtio interrupt-ack register. `pane native-preflight` and `pane native-boot-spike` emit a `whp-exit-route-contract-v1` device-loop report that names each current WHP exit owner and its migration target: COM1 serial, the temporary Pane block-port diagnostic bridge, virtio-MMIO block, WHP instruction-emulator callbacks, legacy platform I/O, timer interrupts, display, input, and CPU-control exits. The next implementation step is proving guest acknowledgement/root mount through the virtio-MMIO block path, then retiring the custom block-port bridge from the root-storage boot path.
