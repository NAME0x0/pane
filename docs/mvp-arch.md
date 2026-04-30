# Arch MVP Guide

Pane currently supports one operational path: Windows + WSL2 + Arch Linux + XFCE.

## Required Preconditions

- `wsl.exe` is installed and working.
- The installed WSL supports managed online install flags (`--name`, `--location`, `--no-launch`, and `--web-download`) if you want Pane to provision `pane-arch` online. Otherwise use `pane init --rootfs-tar`.
- Either Arch Linux already exists as a WSL distro, or Windows can reach the official WSL online install source so Pane can provision `pane-arch` for you. `pane init --rootfs-tar` remains an advanced fallback.
- `/etc/wsl.conf` contains:

```ini
[boot]
systemd=true
```

- The Arch distro has a non-root default user.
- That default user has a usable password for XRDP login.
- The configured Arch package repositories provide `xrdp` and `xorgxrdp`, or you are using a Pane-approved Arch image/package source that provides them. Pane does not automatically build AUR packages in this pre-release path.
- `mstsc.exe` is present on Windows.

## Packaged App Flow

If you are using the packaged build instead of the repo:

1. Double-click `Install Pane Shortcuts.cmd` once.
2. Launch `Pane` from the Desktop or Start Menu, or double-click `Pane Control Center.cmd` from the extracted package folder.
3. Use `Onboard Arch` first if Pane is not yet managing an Arch distro. Pane will create or adopt the managed Arch distro, configure the Arch login user, enable the required WSL settings, and verify launch readiness from one flow.
4. Use the control center as the main app surface for onboarding, launch, terminal access, update, repair, reconnect, logs, PaneShared access, reset, and support bundles.
5. Use the control center's `Setup User` action later when you only need to repair the Arch login or WSL config without rerunning full onboarding.
6. Use `Open Pane Arch Terminal.cmd` when you want a shell for first-run setup, package installs, password changes, or customization inside Arch.
7. Use `Open Pane Shared Folder.cmd` when you want the Windows-side folder that appears in Arch as `~/PaneShared`. PaneShared is durable by default and can be switched to scratch storage for disposable sessions.
8. Use `Collect Pane Support Bundle.cmd` before asking for help.

## Why The Desktop Choice Is Locked

The package intentionally does not expose KDE, GNOME, Niri, or other desktop profiles yet. The current product only has a supportable bootstrap, diagnostics, reconnect path, and recovery story for Arch + XFCE. Pane will widen the profile list only after those other paths are real enough that first-run success stays boring.

## Recommended CLI Flow

1. Prefer the packaged Control Center and its Start First Run flow when using Pane as an app.
2. Run `pane app-status --json` when you need the same app-facing lifecycle phase and next-action recommendation from the CLI.
3. Run `pane runtime --prepare --create-user-disk --create-serial-boot-image --capacity-gib 8` when you want to prepare Pane's dedicated future runtime space, user-disk descriptor, and WHP serial boot image artifact. This does not boot Arch yet.
4. Run `pane native-preflight --json` when you want to inspect Windows Hypervisor Platform readiness plus the runtime artifact blockers for the future boot-to-serial spike.
5. Run `pane runtime --register-boot-loader C:\path\to\loader.img --boot-loader-expected-sha256 <64-char-sha256> --boot-loader-expected-serial "PANE_BOOT_OK\n"` when you have a controlled loader candidate that should be executed from Pane runtime storage.
6. Run `pane runtime --register-kernel C:\path\to\vmlinuz-linux --kernel-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"` when you have a trusted kernel artifact for the first native kernel boot plan. Add `--register-initramfs` and `--initramfs-expected-sha256` when your boot path needs an initramfs.
7. Run `pane native-kernel-plan --materialize` to write the deterministic boot layout for boot params, cmdline, kernel, and optional initramfs before the WHP kernel-entry milestone starts.
8. Run `pane native-boot-spike --json` to inspect the first executable WHP milestone in safe plan mode. Add `--execute` when you intentionally want Pane to create and tear down the temporary WHP partition/vCPU; add `--execute --run-fixture` when you also want Pane to map guest memory, set registers, run controlled guest code, and observe the COM1 serial I/O exit. Add `--execute --run-boot-loader` when you want to run the verified runtime-provided loader candidate.
9. Run `pane runtime --register-base-image C:\path\to\arch-base.img --expected-sha256 <64-char-sha256>` when you have a Pane-approved local Arch base OS image to copy into the app-owned runtime store. Images registered without an expected digest are recorded but treated as untrusted.
10. Run `pane launch --runtime pane-owned --dry-run` when you want to exercise the future native-runtime path without invoking WSL, `mstsc.exe`, or XRDP. This currently reports the remaining host, base-image, boot-engine, and display-engine blockers.
11. Run `pane doctor --de xfce --no-write --no-connect` when you want an initial diagnostic pass that does not create Pane workspace state or PaneShared.
12. Run `"strong-password" | pane onboard --username archuser --password-stdin` as the preferred first-run CLI flow. Pane will initialize or reuse the managed Arch distro, create or repair the Arch login, enable the required WSL settings, and verify launch readiness.
13. Run `pane terminal` when you want shell-level work such as dotfiles or later password changes. Use `pane terminal --user root` for package installation, repair work, or any admin task unless you have configured sudo yourself inside Arch.
14. Run `pane launch --de xfce`.
15. If you want to verify reconnect readiness later, run `pane doctor --de xfce --skip-bootstrap`. That path now checks both the Pane-managed XRDP session assets and whether the Arch user's XFCE config/cache directories exist, are owned correctly, and are writable.
13. Use `pane connect` to reopen the saved `.rdp` profile. Pane will use direct localhost when it is available and fall back to pane-relay when Windows cannot reach the WSL XRDP port directly.
14. Use `pane share` to open or print the Windows-side directory exposed in Arch as `~/PaneShared`.
15. Keep the default durable PaneShared mode when the folder is user data. Use `pane launch --shared-storage scratch` when the shared folder should be disposable with the session workspace.
16. Use `pane logs` when bootstrap or XRDP readiness fails.
17. Run `pane bundle` before asking for support so the current diagnostics are packaged once.
18. Use `pane init` plus `pane setup-user` only when you want to split onboarding into separate lifecycle steps or adopt a specific existing Arch distro.

## Recovery Commands

- `pane stop` stops XRDP inside the selected distro.
- `pane share --print-only` prints the shared Windows and WSL paths for the active session.
- `pane setup-user` creates or repairs the Arch login user, writes `systemd=true` plus the default WSL user to `/etc/wsl.conf`, and can restart WSL automatically. It does not grant passwordless sudo or edit `/etc/sudoers`; use `pane terminal --user root` for admin work.
- `pane terminal` opens an interactive shell inside the resolved Arch distro.
- `pane terminal --user root` opens a root shell for password changes, package installation, package repair, or other first-run admin work.
- `pane update` refreshes Arch packages and reapplies the Pane-managed integration without opening `mstsc.exe`.
- `pane repair` reapplies the Pane-managed bootstrap and XRDP/session wiring, including the `pane-relay` helper, session launcher, notifyd X11 override, and required XFCE config/cache directory ownership, without opening `mstsc.exe`.
- `pane reset` removes Pane-managed Windows assets for the active session while preserving durable PaneShared storage by default.
- `pane reset --purge-shared` also deletes durable PaneShared storage for the selected session.
- `pane reset --purge-wsl` also stops XRDP and removes Pane-managed XRDP session assets when they match the known desktop commands.
- `pane reset --release-managed-environment --purge-wsl` detaches Pane from an adopted managed Arch distro without deleting it.
- `pane reset --factory-reset` destroys a Pane-provisioned managed distro and clears Pane ownership.
- `pane reset --dry-run` prints the reset plan without changing WSL, workspaces, or Pane state.
- `pane logs` prints the bootstrap transcript and live XRDP logs.
- `pane bundle` creates a zip file with status, doctor output, state, workspace assets, and live XRDP logs when they are available.

## Common Fixes

### systemd is not configured

Prefer the Pane-managed path first:
`"strong-password" | pane setup-user --username archuser --password-stdin`

Manual fallback if you want to edit `/etc/wsl.conf` yourself:

```ini
[boot]
systemd=true
```

Then run:

```powershell
wsl --shutdown
```

Start the Arch distro again and rerun `pane doctor`.

### Default user is root or missing

Prefer the Pane-managed path first:
`"strong-password" | pane setup-user --username archuser --password-stdin`

Manual fallback: create a regular user yourself and make it the default WSL user before using Pane.

### Password check fails

Prefer the Pane-managed path first:
`"strong-password" | pane setup-user --username archuser --password-stdin`

Manual fallback if you still want to set or reset the Linux password directly:

```powershell
wsl -d archlinux -u root -- passwd <user>
```

### `pane doctor --skip-bootstrap` fails

The distro is not reconnect-ready yet. Run a real `pane launch`, then inspect `pane logs` and capture `pane bundle` if XRDP still does not come up.

### `pane doctor --no-write` reports SKIP checks

That is expected. `--no-write` intentionally does not create the Windows workspace or PaneShared, so writable probes are shown as skipped instead of pass/fail. Rerun `pane doctor` without `--no-write` when you want Pane to create and verify those directories.

## Clean-Machine QA

For a repo-free package check on a Windows box that already meets the Arch MVP prerequisites, run:

```powershell
powershell -ExecutionPolicy Bypass -File .\validate-package.ps1
```

If you are still in the repo, the same smoke can be driven from the packager:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1 -RunSmoke
```

The full checklist lives in [clean-machine-validation.md](clean-machine-validation.md). The product direction and current transport limit are spelled out in [vision.md](vision.md).

## Current Non-Goals

These are intentionally outside the MVP support boundary:

- non-Arch distros,
- KDE, GNOME, or Niri,
- embedded/native RDP,
- custom non-RDP transport,
- GPU-specific optimizations.





