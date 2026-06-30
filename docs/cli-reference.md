# Pane CLI Reference

This file is the README companion for command names, terms, and flags. The source of truth is still the binary itself:

```powershell
.\pane.exe --help
.\pane.exe <command> --help
```

When running from source, replace `.\pane.exe` with `cargo run --`.

## Core Terms

| Term | Meaning |
|------|---------|
| Control Center | The Windows GUI opened by running `pane.exe` with no arguments. |
| Runtime | The backend used to launch Linux. Current values are `auto`, `qemu-whpx`, `wsl-bridge`, and `pane-owned`. |
| QEMU + WHPX | The practical current boot engine for the Arch desktop path when QEMU and a base image are available. |
| WSL bridge | The older Windows Subsystem for Linux + XRDP path. It remains available as a fallback/legacy path. |
| Pane-owned runtime | The future native runtime under development. It is exposed through planning/preflight commands, not as the finished desktop path. |
| Base image | The immutable Arch OS image used by the QEMU/WHPX path. |
| Persistent root | A writable qcow2 overlay backed by the base image, enabled with `--persist-root`. |
| User disk | Pane-managed writable storage intended for accounts, packages, and customizations. |
| PaneShared | Windows-side shared folder surfaced to Linux. Durable by default; scratch mode is disposable. |
| Session name | A slug used to separate runtime/workspace state, defaulting to `pane`. |

## Value Sets

| Flag | Values |
|------|--------|
| `--runtime` | `auto`, `qemu-whpx`, `wsl-bridge`, `pane-owned` |
| `--display` | `serial`, `gtk`, `sdl`, `vnc` |
| `--de` for `launch`, `doctor`, `repair`, `update`, `onboard` | `xfce` |
| `--de` for `install-desktop` | `xfce`, `gnome`, `kde` |
| `--shared-storage` | `durable`, `scratch` |

## First-Run Desktop Flow

Create credentials:

```powershell
.\pane.exe provision --username pane
```

Install the recommended desktop:

```powershell
.\pane.exe install-desktop --de xfce --disk-gib 8
```

Launch the desktop:

```powershell
.\pane.exe launch --runtime qemu-whpx --display gtk --persist-root
```

## Command Index

| Command | Purpose |
|---------|---------|
| `pane` | Opens the Control Center when no CLI arguments are supplied. |
| `pane --help` | Shows top-level CLI help. |
| `pane app-status` | Shows the app lifecycle, next action, storage, and display state. |
| `pane environments` | Shows the managed distro roadmap and support tiers. |
| `pane provision` | Sets credentials inside the QEMU guest. |
| `pane install-desktop` | Installs XFCE/GNOME/KDE into the QEMU guest image. |
| `pane launch` | Launches the selected runtime. |
| `pane stop` | Stops the selected running backend where supported. |
| `pane workspace` | Resets or compacts QEMU workspace disks. |
| `pane doctor` | Runs support-oriented readiness checks. |
| `pane logs` | Prints saved launch/bootstrap logs. |
| `pane bundle` | Creates a support bundle zip. |
| `pane share` | Opens or prints PaneShared paths. |
| `pane status` | Prints WSL/managed environment state. |
| `pane init` | Creates, imports, or adopts a Pane-managed Arch WSL distro. |
| `pane onboard` | Runs the WSL bridge first-run flow. |
| `pane setup-user` | Creates or repairs the WSL bridge Linux user. |
| `pane terminal` | Opens a shell in the WSL bridge distro. |
| `pane connect` | Reopens the saved RDP profile for the WSL bridge path. |
| `pane repair` | Reapplies WSL bridge integration. |
| `pane update` | Updates Arch packages and reapplies WSL bridge integration. |
| `pane reset` | Removes Pane-managed session state and optionally WSL/shared data. |
| `pane runtime` | Manages future Pane-owned runtime storage and artifacts. |
| `pane native-preflight` | Checks WHP/native-runtime readiness. |
| `pane native-kernel-plan` | Materializes the native kernel boot layout. |
| `pane native-foundation` | Prints the crosvm/rust-vmm foundation plan. |
| `pane native-boot-spike` | Runs or previews guarded WHP boot-spike milestones. |

## Common Flags

These flags appear across multiple commands:

| Flag | Meaning |
|------|---------|
| `--json` | Print structured JSON. |
| `--session-name <name>` | Select the Pane session/workspace, default `pane`. |
| `--distro <name>` | Select a WSL distro for bridge commands. |
| `--dry-run` | Print or generate a plan without making the full change. |
| `--de <name>` | Select a desktop profile. |
| `--port <number>` | Select the XRDP port for WSL bridge commands, default `3390`. |
| `--shared-storage durable|scratch` | Select durable or disposable PaneShared storage. |

## QEMU/WHPX Commands

### `launch`

Important flags:

| Flag | Meaning |
|------|---------|
| `--runtime auto|qemu-whpx|wsl-bridge|pane-owned` | Select backend. `auto` prefers QEMU/WHPX when available, otherwise WSL bridge. |
| `--display serial|gtk|sdl|vnc` | Select console/window mode. |
| `--persist-root` | Use the persistent root overlay so installed packages and desktop changes survive reboot. |
| `--detach` | Start the VM in the background where supported. |
| `--vcpus <n>` | Override recommended vCPU count. |
| `--memory-mb <mib>` | Override recommended memory. |
| `--disk-gib <gib>` | Grow the persistent root overlay before launch. |
| `--resolution <width>x<height>` | Request a guest display size. |
| `--no-gpu-acceleration` | Disable VirGL/GPU acceleration and use compatibility graphics. |
| `--session-name <name>` | Select runtime workspace. |

Examples:

```powershell
.\pane.exe launch --runtime qemu-whpx --display gtk --persist-root
.\pane.exe launch --runtime qemu-whpx --display serial --persist-root
.\pane.exe launch --runtime qemu-whpx --display gtk --persist-root --memory-mb 4096 --vcpus 4
```

### `provision`

Flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select runtime workspace. |
| `--root-password <password>` | Set an explicit root password. If omitted, Pane generates one. |
| `--username <name>` | Create a normal login user. |
| `--password <password>` | Set that user's password. If omitted, Pane generates one. |

Example:

```powershell
.\pane.exe provision --username pane
```

### `install-desktop`

Flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select runtime workspace. |
| `--de xfce|gnome|kde` | Install a desktop profile. XFCE is recommended today. |
| `--disk-gib <gib>` | Grow the root disk before installing. Defaults are 8 GiB for XFCE and 24 GiB for GNOME/KDE. |
| `--timeout-minutes <n>` | Override install timeout. `0` means automatic by desktop profile. |

Examples:

```powershell
.\pane.exe install-desktop --de xfce --disk-gib 8
.\pane.exe install-desktop --de gnome --disk-gib 24 --timeout-minutes 90
```

### `workspace`

Flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select runtime workspace. |
| `--reset` | Remove the persistent root overlay so next launch starts from the base image. |
| `--purge` | With `--reset`, also remove the user disk. |
| `--compact` | Reclaim free space from qcow2 disks. |

## WSL Bridge Commands

The WSL bridge path remains available for the older Arch + XFCE flow. See [Arch MVP Guide](mvp-arch.md) for the full WSL recovery guide.

### `init`

Flags:

| Flag | Meaning |
|------|---------|
| `--distro-name <name>` | Name for the Pane-managed WSL distro, default `pane-arch`. |
| `--existing-distro <name>` | Adopt an existing Arch WSL distro. |
| `--rootfs-tar <path>` | Import an Arch rootfs tarball. |
| `--install-dir <path>` | Choose install directory for rootfs import. |
| `--dry-run` | Print the plan only. |
| `--json` | Print JSON. |

### `onboard`

Flags:

| Flag | Meaning |
|------|---------|
| `--distro-name <name>` | Managed distro name, default `pane-arch`. |
| `--existing-distro <name>` | Adopt an existing Arch WSL distro. |
| `--rootfs-tar <path>` | Import an Arch rootfs tarball. |
| `--install-dir <path>` | Choose install directory for rootfs import. |
| `--username <name>` | Linux username to create or repair. |
| `--password <password>` | Linux password. Prefer `--password-stdin`. |
| `--password-stdin` | Read password from stdin. |
| `--de xfce` | Validate desktop profile. |
| `--session-name <name>` | Select workspace. |
| `--port <number>` | XRDP port, default `3390`. |
| `--dry-run` | Print the plan only. |
| `--no-shutdown` | Do not restart WSL after config changes. |
| `--json` | Print JSON. |

Example:

```powershell
"strong-password" | .\pane.exe onboard --username archuser --password-stdin
```

### `setup-user`

Flags:

| Flag | Meaning |
|------|---------|
| `--distro <name>` | Select WSL distro. |
| `--username <name>` | Linux username to create or repair. |
| `--password <password>` | Linux password. Prefer `--password-stdin`. |
| `--password-stdin` | Read password from stdin. |
| `--dry-run` | Print the plan only. |
| `--no-shutdown` | Do not restart WSL after config changes. |
| `--json` | Print JSON. |

### `doctor`

Flags:

| Flag | Meaning |
|------|---------|
| `--distro <name>` | Select WSL distro. |
| `--de xfce` | Validate desktop profile. |
| `--session-name <name>` | Select workspace. |
| `--port <number>` | XRDP port, default `3390`. |
| `--skip-bootstrap` | Validate an existing session rather than first bootstrap. |
| `--no-connect` | Skip RDP client validation. |
| `--no-write` | Do not create or repair workspace directories. |
| `--json` | Print JSON. |

## Support Commands

### `status`

Flags: `--distro <name>`, `--json`.

### `app-status`

Flags: `--session-name <name>`, `--json`.

### `share`

Flags: `--session-name <name>`, `--shared-storage durable|scratch`, `--print-only`.

### `terminal`

Flags: `--distro <name>`, `--user <name>`, `--print-only`.

### `connect`

Flags: `--session-name <name>`, `--force`.

### `stop`

Flags: `--distro <name>`.

### `reset`

Flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select workspace. |
| `--distro <name>` | Select WSL distro for bridge cleanup. |
| `--purge-wsl` | Remove Pane-managed WSL session wiring. |
| `--purge-shared` | Delete durable PaneShared for the selected session. |
| `--release-managed-environment` | Detach Pane from an adopted distro without deleting it. |
| `--factory-reset` | Destroy a Pane-provisioned managed distro and clear ownership. |
| `--dry-run` | Print the reset plan only. |

### `logs`

Flags: `--session-name <name>`, `--distro <name>`, `--lines <n>`.

### `bundle`

Flags: `--session-name <name>`, `--distro <name>`, `--output <zip-path>`.

## Native Runtime Commands

These commands are for the Pane-owned runtime work. They are useful for development and diagnostics, but they do not yet replace the practical QEMU/WHPX desktop path.

### `runtime`

Common flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select runtime workspace. |
| `--capacity-gib <gib>` | Runtime storage reservation size. |
| `--prepare` | Create runtime directory layout and manifest. |
| `--register-base-image <path>` | Copy a local Arch base image into runtime storage. |
| `--expected-sha256 <sha>` | Expected digest for base image registration. |
| `--require-native-root-disk` | Require a raw disk with a detectable Linux root partition. |
| `--register-native-boot-set` | Register base disk, kernel, initramfs, and cmdline together. |
| `--register-native-boot-set-manifest <path>` | Register from a builder-emitted JSON manifest. |
| `--write-native-boot-set-manifest-template <path>` | Write a manifest template. |
| `--register-boot-loader <path>` | Register a controlled boot-to-serial loader. |
| `--boot-loader-expected-sha256 <sha>` | Expected loader digest. |
| `--boot-loader-expected-serial <text>` | Serial text expected from the loader. |
| `--register-kernel <path>` | Register a Linux kernel image. |
| `--kernel-expected-sha256 <sha>` | Expected kernel digest. |
| `--register-initramfs <path>` | Register an initramfs image. |
| `--initramfs-expected-sha256 <sha>` | Expected initramfs digest. |
| `--kernel-cmdline <text>` | Kernel command line. |
| `--write-initramfs-driver` | Write Pane's generated guest-side discovery bundle. |
| `--build-discovery-initramfs` | Build and register the generated discovery initramfs. |
| `--discovery-init-binary <path>` | Use a prebuilt Linux ELF init binary. |
| `--discovery-probe-binary <path>` | Use a prebuilt Linux ELF probe binary. |
| `--build-pane-block-module` | Build `pane-block.ko` from generated source. |
| `--kernel-build-dir <path>` | Kernel build directory for module build. |
| `--register-pane-block-module <path>` | Register a compiled `pane-block.ko`. |
| `--pane-block-module-expected-sha256 <sha>` | Expected module digest. |
| `--register-virtio-mmio-module <path>` | Register `virtio_mmio.ko` for the discovery initramfs. |
| `--virtio-mmio-module-expected-sha256 <sha>` | Expected virtio-mmio module digest. |
| `--create-user-disk` | Create the Pane user disk descriptor. |
| `--snapshot-user-disk` | Snapshot the Pane user disk. |
| `--restore-user-disk-snapshot <path>` | Restore from snapshot metadata. |
| `--export-user-disk <path>` | Export a portable user disk package. |
| `--import-user-disk <path>` | Import a portable user disk package. |
| `--resize-user-disk-gib <gib>` | Grow user disk capacity. |
| `--repair-user-disk` | Repair metadata from a valid disk header. |
| `--create-serial-boot-image` | Create the WHP serial test image. |
| `--force` | Replace existing runtime artifact. |
| `--json` | Print JSON. |

### `native-preflight`

Flags: `--session-name <name>`, `--prepare-runtime`, `--json`.

### `native-kernel-plan`

Flags: `--session-name <name>`, `--materialize`, `--prepare-runtime`, `--json`.

### `native-foundation`

Flags: `--json`.

### `native-boot-spike`

Flags:

| Flag | Meaning |
|------|---------|
| `--session-name <name>` | Select runtime workspace. |
| `--prepare-runtime` | Prepare runtime state before the probe. |
| `--execute` | Actually create and tear down a WHP partition/vCPU. |
| `--run-fixture` | Run the controlled serial fixture. |
| `--run-boot-loader` | Run the registered boot-to-serial loader. |
| `--run-kernel-layout` | Run the materialized kernel-layout artifact. |
| `--qemu-whpx` | Use QEMU+WHPX for the boot probe instead of Pane's from-scratch WHP loop. |
| `--qemu-initramfs <path>` | Override QEMU initramfs path. |
| `--interactive` | Make QEMU serial interactive. |
| `--display serial|gtk|sdl|vnc` | Select QEMU display mode. |
| `--persist-root` | Use persistent root overlay. |
| `--detach` | Start VM in background where supported. |
| `--trace-checkpoint <path>` | Write incremental diagnostics. |
| `--json` | Print JSON. |
