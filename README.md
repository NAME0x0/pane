<p align="center">
  <img src="assets/pane-icon.png" alt="Pane icon" width="96" height="96">
</p>

<h1 align="center">Pane</h1>

<p align="center">
  A Windows-only Linux desktop app, starting with a managed Arch Linux experience.
</p>

<p align="center">
  <a href="https://github.com/NAME0x0/pane/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/NAME0x0/pane/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/NAME0x0/pane/actions/workflows/release.yml"><img alt="Release" src="https://github.com/NAME0x0/pane/actions/workflows/release.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-black.svg"></a>
  <a href="https://github.com/NAME0x0/pane/releases"><img alt="Downloads" src="https://img.shields.io/badge/downloads-GitHub%20Releases-black.svg"></a>
</p>

<p align="center">
  <a href="#what-pane-is">What It Is</a> ·
  <a href="#quick-setup">Quick Setup</a> ·
  <a href="#launch-linux-desktop">Launch Desktop</a> ·
  <a href="#cli-usage">CLI</a> ·
  <a href="#faq">FAQ</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

## What Pane Is

Pane is a Windows app for launching and managing Linux desktop environments without making users act like WSL, VM, or remote-desktop operators.

The goal is simple: install one Windows app, launch a real Linux desktop, keep user data in predictable places, recover from common setup problems, and eventually run the OS through a Pane-owned runtime and display surface.

Pane exists because using Linux on Windows is still too fragmented. Users often have to understand distro imports, passwords, display managers, RDP profiles, QEMU flags, WSL state, package installs, and diagnostic logs before they can do anything useful. Pane is meant to own that lifecycle.

## Current Status

Pane is pre-release and Windows-only at the moment.

The current practical path is Arch-first:

| Area | Current State |
|------|---------------|
| Host OS | Windows 10/11 |
| Primary distro | Arch Linux |
| App entrypoint | `pane.exe` opens the Pane Control Center when launched without arguments |
| Boot engine | QEMU + WHPX when QEMU and a registered Arch base image are available |
| Legacy fallback | WSL2 + XRDP remains available for the older Arch + XFCE bridge path |
| Desktop profiles | XFCE is the recommended/default profile; GNOME and KDE installs exist but are heavier and less proven |
| Shared storage | PaneShared, durable by default and scratchable when requested |
| Native Pane-owned runtime | In progress; not yet the default boot/display engine |

Pane should not be described as a finished zero-latency contained VM yet. The product direction is a Pane-owned OS runtime and display window, but the reliable near-term path uses proven QEMU/WHPX pieces while the native runtime is still being built.

## Quick Setup

1. Download the latest `pane-windows-x86_64.zip` package from [GitHub Releases](https://github.com/NAME0x0/pane/releases).
2. Extract the zip somewhere writable.
3. Optional: run `Install Pane Shortcuts.cmd`.
4. Launch `pane.exe` to open the Control Center.
5. Use `Doctor` if you want a host readiness check before launching anything.

Pane looks for QEMU in this order:

1. bundled `engine\pane-engine.exe` from the Pane package,
2. `pane-engine.exe` next to `pane.exe`,
3. `qemu-system-x86_64` on `PATH`,
4. `C:\Program Files\qemu\qemu-system-x86_64.exe`,
5. automatic `winget install SoftwareFreedomConservancy.QEMU` when the QEMU path is explicitly used and QEMU is missing.

Pane can register the Arch base image automatically from `images\arch-base.paneimg` inside the package, or download it from the configured release asset URL. For manual intake, use:

```powershell
.\pane.exe runtime --prepare --register-base-image "C:\path\to\arch-base.img" --expected-sha256 "<64-char-sha256>" --require-native-root-disk
```

Use the same `--session-name` during registration and launch if you are not using the default `pane` session.

## Launch Linux Desktop

### From The App

1. Open `pane.exe`.
2. Use the setup/provision action to create a Linux user.
3. Install a desktop profile, preferably XFCE for now.
4. Click Launch.
5. Log in at the Linux display manager using the user credentials you created.

### From The CLI

Open PowerShell in the extracted package directory.

Prepare credentials:

```powershell
.\pane.exe provision --username pane
```

Install the recommended desktop:

```powershell
.\pane.exe install-desktop --de xfce --disk-gib 8
```

Launch the graphical desktop:

```powershell
.\pane.exe launch --runtime qemu-whpx --display gtk --persist-root
```

Stop a detached/running VM:

```powershell
.\pane.exe stop
```

Reset the persistent root overlay if you want to start fresh from the base image:

```powershell
.\pane.exe workspace --reset
```

For the older WSL/XRDP bridge path, see [Arch MVP Guide](docs/mvp-arch.md).

## CLI Usage

The executable has two modes:

```powershell
.\pane.exe
```

opens the Control Center.

```powershell
.\pane.exe --help
.\pane.exe launch --help
.\pane.exe install-desktop --help
```

prints CLI help.

The detailed command, term, and flag reference lives in [CLI Reference](docs/cli-reference.md). Use that file instead of scanning this README when you need exact flags such as `--runtime`, `--display`, `--persist-root`, `--session-name`, `--shared-storage`, `--disk-gib`, or `--no-gpu-acceleration`.

Common support commands:

```powershell
.\pane.exe status
.\pane.exe doctor
.\pane.exe logs
.\pane.exe bundle
.\pane.exe share
```

## FAQ

### Is Pane Windows-only?

Yes. Pane is Windows-only right now because the product is specifically about making Linux desktop use on Windows feel like an app.

### Does Pane boot Arch inside its own native runtime yet?

Not fully. The current working boot path uses QEMU with WHPX acceleration when configured. Pane's own WHP/native runtime work exists behind `runtime`, `native-preflight`, `native-kernel-plan`, and `native-boot-spike`, but it is not yet the default desktop path.

### Why use QEMU if Pane wants to be self-contained?

Because booting Linux, storage, graphics, and input correctly is a large VMM problem. QEMU/WHPX gives Pane a practical bootable desktop path while the Pane-owned runtime is developed behind explicit contracts. The long-term goal remains a Pane-owned runtime and app display surface.

### Does Pane support Ubuntu, Debian, or Kali?

Not yet. Arch is the first supported distro. Ubuntu and Debian are future managed environments. Kali and other distros are out of scope until the core lifecycle is reliable.

### Can I use GNOME or KDE?

`install-desktop` supports `xfce`, `gnome`, and `kde`, but XFCE is the recommended path today. GNOME and KDE need more validation and may require more disk, memory, and graphics tuning.

### Where does Pane store data?

Pane stores runtime state under `%LOCALAPPDATA%\Pane`. PaneShared is durable by default and is meant for user files. Scratch storage is available for disposable sessions.

### How do I get help when something fails?

Run:

```powershell
.\pane.exe doctor
.\pane.exe bundle
```

The bundle command creates support diagnostics with state, logs, and relevant workspace files.

## Documentation

- [CLI Reference](docs/cli-reference.md) - commands, terms, flags, and examples.
- [Arch MVP Guide](docs/mvp-arch.md) - older WSL/XRDP bridge flow and recovery notes.
- [Vision](docs/vision.md) - product direction and current limits.
- [Product Contract](docs/product-contract.md) - what Pane is trying to become.
- [Native Runtime Architecture](docs/native-runtime-architecture.md) - WHP/native runtime contract.
- [VMM Foundation](docs/vmm-foundation.md) - crosvm/rust-vmm direction.
- [Clean Machine Validation](docs/clean-machine-validation.md) - package certification and QA gates.

## Development

Useful local checks:

```powershell
cargo fmt --check
cargo check --offline
cargo test --offline
cargo clippy --offline -- -D warnings
```

Build the Windows package:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -Profile release
```

The normal release package requires QEMU and bundles it under `engine\`. For a developer-only package that relies on the host's QEMU install instead:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -Profile release -BundleQemu Disabled
```

Certify the package entrypoints:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64 -Mode PackageOnly
```

## Roadmap

| Stage | Focus |
|-------|-------|
| Current | Package `pane.exe`, Control Center, Arch setup, QEMU/WHPX desktop launch, diagnostics, support bundle |
| Near term | Better base-image distribution, first-run UX, storage selection, GNOME/KDE hardening, clearer recovery |
| Native runtime | Pane-owned WHP boot, storage, input, display, networking, and repair paths |
| Platform expansion | Ubuntu, Debian, and curated desktop profiles after Arch is supportable |
| Release quality | Signed/reproducible packages where possible, clean-machine certification, sharper docs and support process |

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
