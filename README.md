<p align="center">
  <img src="assets/pane-icon.png" alt="Pane icon" width="96" height="96">
</p>

<h1 align="center">Pane</h1>

<p align="center">
  A Windows-native Linux environment platform, starting with a deeply supported Arch desktop.
</p>

<p align="center">
  <a href="https://github.com/NAME0x0/pane/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/NAME0x0/pane/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/NAME0x0/pane/actions/workflows/release.yml"><img alt="Release" src="https://github.com/NAME0x0/pane/actions/workflows/release.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-black.svg"></a>
  <a href="https://github.com/NAME0x0/pane/releases"><img alt="Downloads" src="https://img.shields.io/badge/downloads-GitHub%20Releases-black.svg"></a>
</p>

<p align="center">
  <a href="#status">Status</a> ·
  <a href="#quick-start">Quick Start</a> ·
  <a href="#supported-mvp">Supported MVP</a> ·
  <a href="#native-runtime">Native Runtime</a> ·
  <a href="#development">Development</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

Pane is building toward a simple product promise: open one Windows app, get a real Linux environment with a real GUI, customize it freely, and let the app own setup, launch, reconnect, repair, reset, file sharing, diagnostics, and support.

The current product is intentionally narrower than the long-term vision. Pane is Arch-first, XFCE-first, and support-first. It already ships a packaged `pane.exe`, a Windows Control Center, managed Arch onboarding, account setup, PaneShared storage, diagnostics, repair/update/reset flows, support bundles, and a real GUI handoff. It still uses WSL2, XRDP, and `mstsc.exe` for the visible desktop while the Pane-owned native runtime is being built behind a clear contract.

## Status

Pane is a pre-release MVP. It is usable for the supported Arch + XFCE bridge path, but it is not yet the final contained runtime/display architecture.

| Area | Current State |
|------|---------------|
| App entrypoint | Packaged `pane.exe` opens the Control Center when launched without CLI arguments. |
| Supported Linux path | Managed Arch Linux on WSL2 with XFCE. |
| Display path | External Windows RDP client over XRDP, with Pane-managed connection assets and fallback handling. |
| Shared files | PaneShared, durable by default and scratchable for disposable sessions. |
| Supportability | Doctor checks, logs, repair/update/reset commands, and support bundles. |
| Native runtime | WHP host preflight, serial boot fixture, boot-loader contract, kernel/initramfs plan, kernel layout, storage attachment, root handoff contract, mapped framebuffer/input queue contracts, protected-mode entry probe, E820 map, COM1 probe model. |
| Not complete yet | Pane-owned Arch disk boot, native framebuffer/window, input, networking, audio, snapshots, and full GUI inside Pane's own renderer. |

The project deliberately avoids claiming "zero latency", "full compatibility", or "contained VM" until the native boot, storage, and display milestones prove those properties.

## Why Pane Exists

Running a Linux GUI on Windows is possible today, but the user experience is fragmented. A normal user should not have to understand WSL distro state, XRDP services, Linux account passwords, `.rdp` profiles, systemd settings, reconnect failure modes, support logs, or reset semantics.

Pane's thesis is that the product has to own the lifecycle, not just launch a terminal:

- create or adopt the Linux environment,
- create a normal Linux user safely,
- prepare the desktop session,
- launch and reconnect predictably,
- expose user files clearly,
- repair common breakages,
- collect useful support bundles,
- reset without deleting durable user data by accident,
- then progressively replace the bridge with a Pane-owned runtime.

## Supported MVP

The current supported path is intentionally strict:

| Requirement | Supported Value |
|-------------|-----------------|
| Host OS | Windows 10 or Windows 11 |
| Linux backend | WSL2 |
| Distro | Arch Linux |
| Desktop | XFCE |
| Login | Non-root Linux user with a usable password |
| Init | `systemd=true` in `/etc/wsl.conf` |
| Shared storage | PaneShared mounted inside Arch as `~/PaneShared` |
| Display | XRDP session opened through `mstsc.exe` |

Deferred until later:

- Ubuntu LTS and Debian managed environments,
- KDE, GNOME, Niri, and other desktop profiles,
- embedded display rendering,
- Pane-owned block device boot,
- GPU/display acceleration work,
- networking/audio/clipboard integration for the native runtime.

## Quick Start

### Option 1: Use The Packaged App

Download the latest package from [GitHub Releases](https://github.com/NAME0x0/pane/releases), extract it, then run:

```powershell
.\pane.exe
```

The no-argument app entrypoint opens the Pane Control Center. If the sidecar app scripts are missing, the standalone executable hydrates them into `%LOCALAPPDATA%\Pane\app`.

To install Windows shortcuts from the package:

```powershell
.\Install Pane Shortcuts.ps1
```

### Option 2: Run From Source

Prerequisites:

- Windows 10 or 11
- Rust stable, compatible with `rust-version = "1.75"`
- WSL2 installed for the current bridge path
- Windows Hypervisor Platform enabled for native-runtime experiments

From the repository root:

```powershell
cargo run -- app-status
cargo run -- environments
```

Run the preferred first-run Arch onboarding flow:

```powershell
"strong-password" | cargo run -- onboard --username archuser --password-stdin
```

Launch the supported Arch + XFCE desktop path:

```powershell
cargo run -- launch --de xfce
```

Open an Arch shell for customization:

```powershell
cargo run -- terminal
```

Use root only when you explicitly need administrative package or repair work:

```powershell
cargo run -- terminal --user root
```

## Core Workflows

### Diagnose Before Launch

```powershell
cargo run -- doctor --de xfce
```

Use a side-effect-free support pass when you do not want Pane to create workspace state:

```powershell
cargo run -- doctor --de xfce --no-write --no-connect
```

### Preview Without Touching WSL Services

```powershell
cargo run -- launch --dry-run --de xfce
```

### Reconnect To An Existing Session

```powershell
cargo run -- doctor --de xfce --skip-bootstrap
cargo run -- connect
```

### Repair Or Update The Managed Arch Path

```powershell
cargo run -- repair --de xfce
cargo run -- update --de xfce
```

### Open PaneShared

```powershell
cargo run -- share
```

PaneShared is durable by default and survives `pane reset`. For disposable sessions:

```powershell
cargo run -- launch --de xfce --shared-storage scratch
```

### Collect Support Data

```powershell
cargo run -- logs
cargo run -- bundle
```

## Native Runtime

Pane's long-term runtime target is not WSL, XRDP, `mstsc.exe`, QEMU, VirtualBox, or Hyper-V Manager. The target is a Pane-owned Windows app that creates the guest runtime, owns the storage boundary, boots Linux through Windows Hypervisor Platform, and renders the guest through its own app surface.

The native engine direction is now crosvm/rust-vmm based: crosvm is the reference architecture, `rust-vmm/linux-loader` is the intended Linux boot adapter, and `rust-vmm/vm-virtio`/virtio semantics are the intended replacement for Pane's current custom block-port experiment. Pane remains the app/runtime owner; this is not a pivot to a generic QEMU wrapper.

What exists today:

- runtime storage reservation under `%LOCALAPPDATA%\Pane\runtime\<session>`,
- verified base-image metadata slot with raw disk/rootfs format inspection, Linux root partition hints, native root-disk registration gating, and kernel-layout root handoff decisions,
- sparse Pane user disk artifact for future Linux packages, accounts, and customization data,
- sparse user disk block I/O primitive with zero-fill semantics for Pane-owned runtime storage,
- read-only base OS image block I/O primitive with EOF zero-fill semantics for verified Arch images,
- native storage-backed kernel layouts now require the verified Arch base image to be registered with `--require-native-root-disk` as a raw disk with a detectable Linux root partition before Pane exposes it as `/dev/pane0pN`,
- native block I/O policy, stateful Pane block-port submit/status protocol, WHP block-port classification, storage contract discovery fields, shared-memory block transfer window, host callback submission, serviced-exit resume, guest-visible read response placement, and guest-to-host write-payload collection for the future storage device,
- host-side native block I/O adapter that services allowed kernel-layout read/write commands through verified base/user artifacts while preserving base-image read-only policy,
- verified Pane user disk snapshots under the runtime snapshot store,
- runtime-backed serial boot image,
- verified boot-to-serial loader candidate slot,
- verified native Arch boot-set registration for the raw disk, Linux kernel, initramfs, and serial-safe cmdline,
- manifest-based native Arch boot-set registration plus a built-in manifest-template writer so reproducible artifact builders can hand Pane one reviewed JSON file instead of six fragile CLI flags,
- verified kernel/initramfs boot-plan metadata,
- materialized Linux kernel boot layout with a `rust-vmm/linux-loader` adapter/provenance record so stale hand-built layouts are rejected before WHP execution,
- kernel-layout attachment for the verified Arch base image plus guest-mapped Pane sparse user disk, current block-port submit/status diagnostics, shared-memory transfer window, and a standard virtio-blk backend contract for the future `vda`/`vdb` device model,
- Pane-owned virtio-MMIO block register model at `0x0dfc0000`, reserved in storage-backed kernel layouts, advertised to Linux through `virtio_mmio.device=4K@0xdfc0000:5`, intentionally left unmapped as ordinary RAM for live WHP device exits, backed by tested split-virtqueue descriptor-chain execution, and exposed through a typed MMIO service boundary while live WHP IRQ completion remains pending,
- typed crosvm-style WHP device-loop reporting in `native-preflight` and `native-boot-spike`, covering serial, diagnostic block I/O, virtio-MMIO block routing, legacy platform I/O, timer interrupts, display, input, and CPU-control exits,
- kernel-layout cmdline hardening for early serial console, early printk, verbose logging, panic retention, and `nomodeset` during the native boot spike,
- generated Pane initramfs driver source/build-script bundle with a self-contained discovery/root-handoff `/init`, exact `/dev/pane0` and `/dev/pane1` geometry handoff via `pane.block_devices`, a Pane block-driver source/build contract with partial read/write handling plus flush/discard tolerance for Linux partition/filesystem probes, a build/register path for the discovery cpio, and kernel-layout serial milestone gates for guest-side native storage discovery,
- fixed linear framebuffer contract mapped into guest memory for the future Pane-rendered display path,
- Linux `boot_params.screen_info` population for the Pane `x8r8g8b8` framebuffer so early Linux boot receives a standard framebuffer description rather than only Pane-specific cmdline metadata,
- keyboard/pointer input queue contract mapped into guest memory for the future app-owned input path, including a `PANEINQ1` ABI header with queue size, record size, producer/consumer indexes, and capacity metadata,
- host-side framebuffer and input-queue snapshot reporting for the mapped Pane runtime surface regions after guarded WHP kernel-layout runs,
- Linux bzImage setup header copying into boot params,
- Pane runtime contract discovery arguments on the Linux kernel command line,
- E820 memory map including boot params, GDT, initramfs, RAM, APIC stubs, framebuffer memory, and input queue memory,
- protected-mode register handoff with boot params in `rsi`,
- explicit default MSR state for APIC base, PAT, SYSENTER, EFER, TSC AUX, and MTRR probes during early Linux CPU bring-up,
- resumable WHP interrupt-window and APIC-EOI exit handling so early Linux interrupt plumbing is observed instead of treated as an unknown hard stop,
- width-aware legacy PIC/PIT/ELCR/CMOS RTC/system-control A/B/delay/PCI-config/i8042/VGA/ACPI PM probe port handling with explicit unsupported-I/O blockers for the next device-model gap,
- deterministic i8042 command-byte, controller self-test, interface-test, and output-port responses for Linux PS/2 controller probing,
- milestone-aware Linux probe exit budgeting and reporting so storage-backed Arch boot gets enough WHP exits to reach initramfs/root-handoff markers,
- separate `pane runtime` and `pane native-preflight` readiness reporting for the first WHP boot spike versus a stricter native Arch boot attempt, including explicit blockers for missing kernel plans, initramfs driver bundles, pane-block modules, discovery initramfs artifacts, and materialized kernel layouts,
- minimal COM1 UART behavior for early serial setup,
- guarded WHP partition/vCPU execution with deterministic fixture validation.

Prepare native-runtime state:

```powershell
cargo run -- runtime --prepare --create-user-disk --create-serial-boot-image --capacity-gib 8
```

Create a recovery snapshot of the Pane user disk artifact:

```powershell
cargo run -- runtime --snapshot-user-disk
```

Restore the Pane user disk from a verified snapshot metadata file:

```powershell
cargo run -- runtime --restore-user-disk-snapshot C:\path\to\user-disk-<timestamp>.json
```

Export or import a portable Pane user disk package:

```powershell
cargo run -- runtime --export-user-disk C:\path\to\pane-user-disk-export
cargo run -- runtime --import-user-disk C:\path\to\pane-user-disk-export
```

Grow the Pane user disk logical capacity:

```powershell
cargo run -- runtime --resize-user-disk-gib 4
```

Repair user disk metadata when the disk header is still valid:

```powershell
cargo run -- runtime --repair-user-disk
```

Check WHP host and runtime readiness:

```powershell
cargo run -- native-preflight --prepare-runtime --json
```

`--prepare-runtime` creates the runtime directories, manifests, framebuffer/input contracts, sparse user disk, and serial fixture before reporting the remaining native boot blockers.

Run the safe plan-only boot-spike report:

```powershell
cargo run -- native-boot-spike --json
```

Inspect the native VMM foundation contract:

```powershell
cargo run -- native-foundation --json
```

Run the deterministic WHP fixture:

```powershell
cargo run -- native-boot-spike --json --execute --run-fixture
```

Register a verified boot-to-serial candidate:

```powershell
cargo run -- runtime --register-boot-loader C:\path\to\loader.img --boot-loader-expected-sha256 <64-char-sha256> --boot-loader-expected-serial "PANE_BOOT_OK\n"
cargo run -- native-boot-spike --json --execute --run-boot-loader
```

Register a verified Linux kernel and optional initramfs:

```powershell
cargo run -- runtime --register-kernel C:\path\to\vmlinuz-linux --kernel-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"
cargo run -- runtime --register-initramfs C:\path\to\initramfs-linux.img --initramfs-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"
cargo run -- runtime --write-initramfs-driver
cargo run -- runtime --build-pane-block-module --kernel-build-dir C:\path\to\arch-kernel-build-dir
cargo run -- runtime --register-pane-block-module C:\path\to\pane-block.ko --pane-block-module-expected-sha256 <64-char-sha256>
cargo run -- runtime --build-discovery-initramfs
cargo run -- runtime --build-discovery-initramfs --discovery-init-binary C:\path\to\init.elf --discovery-probe-binary C:\path\to\pane-port-probe.elf
cargo run -- native-kernel-plan --prepare-runtime --materialize
cargo run -- native-boot-spike --prepare-runtime --execute --run-kernel-layout --trace-checkpoint .\native-boot-checkpoint.json
```

`--build-pane-block-module` runs the generated `build-pane-block-module.sh` with `KERNEL_BUILD_DIR` set to the target Arch kernel build tree, then registers the resulting module with a computed SHA. `--register-pane-block-module` is the manual equivalent for externally built modules; it copies a compiled and SHA-verified `pane-block.ko` into the generated initramfs driver bundle, binding it to the currently verified target kernel and Pane block-driver ABI hash. `--build-discovery-initramfs` compiles the generated guest `/init` and `pane-port-probe` with `PANE_LINUX_CC`/`PANE_LINUX_CC_ARGS`, `cc`, or `zig cc -target x86_64-linux-musl`, verifies both outputs are ELF binaries, packages the `newc` initramfs archive inside Pane without host `cpio` or `bsdtar`, includes the verified module when present, then registers the produced cpio as a verified initramfs artifact in the existing kernel boot plan. Reproducible builders can skip local compilation by passing `--discovery-init-binary` and `--discovery-probe-binary`; Pane verifies those inputs are Linux ELF binaries before packaging them. If those paths are unavailable, build a complete initramfs externally from the generated bundle and register it with `--register-initramfs`.

That path is still a kernel-entry probe. It is not a completed Arch boot until Pane proves deterministic early Linux serial output, root filesystem mounting, userspace start, and eventually display. Pane now applies the stricter Arch boot-attempt readiness contract before loading the kernel layout into WHP, so a missing kernel plan, linux-loader adapter record, initramfs driver, `pane-block.ko`, discovery initramfs, materialized layout, framebuffer/input contract, storage artifact, or WHP host capability blocks execution up front instead of failing as a vague partial run.
Storage-backed kernel layouts still require the generated Pane initramfs driver bundle and a SHA-verified `pane-block.ko` for the current WHP diagnostic bridge, but they now also carry the replacement virtio-blk contract: read-only Arch base disk as `vda`, writable Pane user disk as `vdb`, root partition hint as `vda1` when available, and Linux's required `virtio_mmio.device=4K@0xdfc0000:5` discovery argument. `native-preflight` and `native-boot-spike` now also publish a typed device-loop route contract so the remaining WHP exits have explicit owners instead of growing as undocumented inline cases. The generated discovery initramfs now boots a C `/init` directly, records the Pane storage/display contracts, loads `pane-block.ko` with `device_blocks=<base>,<user>` from `pane.block_devices` plus the `pane.block_dma` shared-buffer window, waits for the declared root device, uses Pane's detected root filesystem hint when present, mounts Pane's immutable base root read-only at `/newroot`, and executes the real init once a Pane block device exists. The bundle also emits `pane-block.c` and `build-pane-block-module.sh`, which define the early Linux diagnostic block-device contract that maps Pane's verified base OS to `/dev/pane0` and the writable user disk to `/dev/pane1`; the generated block path supports partition minors, partial reads, read-modify-write partial writes, 4096-byte guest I/O blocks, shared-memory payload transfer, capped per-transfer serial status logging, and a narrow port submit/status path so filesystems are no longer forced through 128 data-port exits per block. The kernel-layout runner now requires deterministic serial milestones through `PANE_BLOCK_MODULE_LOAD_OK`, `PANE_DISPLAY_CONTRACT_DISCOVERED`, `PANE_ROOT_MOUNT_OK`, and `PANE_INIT_EXEC` before treating storage-backed Arch boot as proven progress; it keeps Pane's explicit `/init` markers visible, quiets generic kernel serial noise by default, and applies a longer storage-aware live-run budget while preserving the no-progress watchdog. Use `--trace-checkpoint <path>` on long native boot probes to receive incremental JSON diagnostics, including WHP time-slice boundaries, block-DMA service reports, total live-run budget, the guarded post-interrupt resume result, and interrupt-delivery snapshots such as guest IF/APIC blocker state.

## Command Reference

| Command | Purpose |
|---------|---------|
| `pane` | Opens the Control Center when no CLI arguments are supplied. |
| `pane app-status` | Prints app lifecycle phase, next action, PaneShared policy, and display boundary. |
| `pane environments` | Shows the managed environment roadmap and support tiers. |
| `pane init` | Creates, imports, or adopts the Arch WSL distro. |
| `pane onboard` | Preferred first-run flow: init/adopt Arch, configure user, verify readiness. |
| `pane setup-user` | Creates or repairs the Linux login without granting passwordless sudo. |
| `pane launch` | Builds launch assets, runs bootstrap when requested, and opens the current desktop handoff. |
| `pane connect` | Reopens the saved RDP profile after readiness checks. |
| `pane doctor` | Validates host, distro, desktop, account, and transport readiness. |
| `pane repair` | Reapplies managed Arch bootstrap/session wiring. |
| `pane update` | Refreshes Arch packages and reapplies integration. |
| `pane terminal` | Opens a shell inside the resolved Arch distro. |
| `pane share` | Opens or prints PaneShared. |
| `pane logs` | Prints saved bootstrap and XRDP logs when available. |
| `pane bundle` | Creates a diagnostic support zip. |
| `pane reset` | Removes Pane-managed session state while preserving durable PaneShared by default. |
| `pane runtime` | Manages the future Pane-owned runtime storage and artifact contract. |
| `pane native-preflight` | Checks WHP host/runtime readiness without side effects. |
| `pane native-kernel-plan` | Materializes the verified kernel boot layout. |
| `pane native-foundation` | Shows the crosvm/rust-vmm foundation selected for the Pane-owned runtime. |
| `pane native-boot-spike` | Runs or previews guarded WHP boot-spike milestones. |

Run any command with `--help` for the current argument list:

```powershell
cargo run -- --help
cargo run -- launch --help
```

## Packaging

Build the Windows package:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

Build offline when dependencies are already cached:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -Offline
```

Package outputs:

- `dist\pane-windows-x86_64.exe`
- `dist\pane-windows-x86_64.zip`
- `dist\pane-windows-x86_64\pane.exe`
- app launcher scripts,
- shortcut installer,
- support-bundle entrypoint,
- native boot-set manifest handoff helper,
- package validation and certification scripts,
- core docs and icon assets.

Certify the package without requiring a live Arch session:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64 -Mode PackageOnly
```

Run the live Arch gate only on a machine where the managed Arch session is expected to work:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64 -Mode LiveArchSession
```

## Development

Core local checks:

```powershell
cargo fmt --check
cargo check --offline
cargo test --offline
cargo clippy --offline -- -D warnings
```

Useful smoke checks:

```powershell
cargo run -- app-status --json
cargo run -- runtime --json
cargo run -- native-preflight --prepare-runtime --json
cargo run -- native-boot-spike --json
cargo run -- launch --dry-run --de xfce
```

Package validation:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -Profile release -Offline
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -Mode PackageOnly
```

Project docs:

- [Arch MVP Guide](docs/mvp-arch.md)
- [Product Contract](docs/product-contract.md)
- [Native Runtime Architecture](docs/native-runtime-architecture.md)
- [VMM Foundation](docs/vmm-foundation.md)
- [Clean Machine Validation](docs/clean-machine-validation.md)
- [Vision](docs/vision.md)
- [Phase 1 Audit](docs/phase-1-audit.md)

## CI/CD

Pane's GitHub Actions pipeline is designed around release trust, not only compilation:

- Rust formatting, check, clippy, and tests on every push and pull request.
- Cross-platform Rust checks where useful, with Windows as the packaging authority.
- Native runtime plan/preflight smoke checks.
- PowerShell parser checks for shipped scripts.
- README/docs local-link validation.
- Windows package build and package-only certification.
- Draft prerelease workflow with SHA-256 checksums and artifact upload.
- Security workflow for dependency review and CodeQL analysis.
- GitHub Pages deployment for the docs site.

Release artifacts are intentionally draft/prerelease until the public-release gate is met.

## Security

Pane touches local runtime storage, WSL state, Windows launchers, support bundles, and eventually a native virtualization boundary. Treat issues in these areas as security-sensitive:

- path traversal or unsafe file deletion,
- accidental deletion of durable PaneShared data,
- privilege escalation inside the Linux environment,
- unsafe handling of support bundles or logs,
- unsigned or unverified runtime images,
- native-runtime host/guest isolation mistakes.

Report security issues using [SECURITY.md](SECURITY.md). Do not publish exploitable details in public issues.

## Roadmap

| Stage | Focus | Exit Criteria |
|-------|-------|---------------|
| Current MVP | Arch + XFCE through managed WSL2/XRDP bridge. | First-run, reconnect, repair, reset, PaneShared, support bundles, package certification. |
| Native Boot | WHP kernel-entry to deterministic Linux serial output. | Verified kernel/initramfs layout reaches early Linux serial output without WSL/RDP. |
| Native Storage | App-owned bootable Arch root disk. | Rootfs mounts, userspace starts, package/user customizations persist. |
| Native Display | Pane-owned framebuffer/window/input path. | Linux desktop appears inside Pane without `mstsc.exe` or XRDP. |
| Platform Expansion | Ubuntu LTS, Debian, and curated desktops. | Each environment has onboarding, repair, reset, diagnostics, and support parity. |
| Release Quality | Public release gate. | Clean-machine certification, signed/reproducible packages where available, documented recovery, known limitations, and support process. |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR. Keep claims precise: if Pane does not boot, render, accelerate, or support something end to end yet, document it as a future milestone rather than product reality.

## License

Pane is licensed under the [MIT License](LICENSE).

## Credits And Upstream Foundations

Pane's native runtime direction is informed by established open-source virtualization work:

| Project | How Pane Uses It | License Posture |
|---------|------------------|-----------------|
| [crosvm](https://github.com/google/crosvm) | Reference architecture for Rust VMM structure, Windows WHPX handling, instruction-emulator callback flow, virtio devices, display, and input. Pane is not vendoring crosvm wholesale. | BSD-3-Clause |
| [rust-vmm/linux-loader](https://github.com/rust-vmm/linux-loader) | Planned adapter boundary for Linux bzImage loading, command-line placement, and boot parameters. | Apache-2.0 / BSD-3-Clause; exact `AND` vs `OR` terms must be verified for the pinned version before distribution. |
| [rust-vmm/vm-virtio](https://github.com/rust-vmm/vm-virtio) | Planned/reference foundation for virtio queues and device semantics. Pane's current virtio-MMIO block work is a narrow in-repo implementation shaped by these semantics. | Apache-2.0 OR BSD-3-Clause |
| [rust-vmm/vm-memory](https://github.com/rust-vmm/vm-memory) | Reference for guest-memory abstractions needed by future WHP-backed device dispatch. | Apache-2.0 OR BSD-3-Clause |

If Pane later vendors or directly copies upstream source files, the copied files must keep their original license headers and the release package must include the required notices. Before a public release, add a `THIRD_PARTY_NOTICES` file listing each copied project or dependency, pinned version/commit, source URL, license, and required license text. GPL components such as QEMU may be useful for comparison, but they are not part of Pane's intended native runtime unless distribution obligations are explicitly accepted.
