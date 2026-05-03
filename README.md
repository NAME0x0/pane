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
| Native runtime | WHP host preflight, serial boot fixture, boot-loader contract, kernel/initramfs plan, kernel layout, storage attachment, mapped framebuffer/input queue contracts, protected-mode entry probe, E820 map, COM1 probe model. |
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

What exists today:

- runtime storage reservation under `%LOCALAPPDATA%\Pane\runtime\<session>`,
- verified base-image metadata slot,
- user-disk descriptor,
- runtime-backed serial boot image,
- verified boot-to-serial loader candidate slot,
- verified kernel/initramfs boot-plan metadata,
- materialized Linux kernel boot layout,
- kernel-layout attachment for the verified Arch base image plus Pane user disk,
- fixed linear framebuffer contract mapped into guest memory for the future Pane-rendered display path,
- keyboard/pointer input queue contract mapped into guest memory for the future app-owned input path,
- Linux bzImage setup header copying into boot params,
- E820 memory map including boot params, GDT, initramfs, RAM, APIC stubs, framebuffer memory, and input queue memory,
- protected-mode register handoff with boot params in `rsi`,
- minimal COM1 UART behavior for early serial setup,
- guarded WHP partition/vCPU execution with deterministic fixture validation.

Prepare native-runtime state:

```powershell
cargo run -- runtime --prepare --create-user-disk --create-serial-boot-image --capacity-gib 8
```

Check WHP host and runtime readiness:

```powershell
cargo run -- native-preflight --json
```

Run the safe plan-only boot-spike report:

```powershell
cargo run -- native-boot-spike --json
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
cargo run -- native-kernel-plan --materialize
cargo run -- native-boot-spike --execute --run-kernel-layout
```

That path is still a kernel-entry probe. It is not a completed Arch boot until Pane proves deterministic early Linux serial output, root filesystem mounting, userspace start, and eventually display.

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
cargo run -- native-preflight --json
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
