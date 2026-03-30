# Arch MVP Guide

Pane currently supports one operational path: Windows + WSL2 + Arch Linux + XFCE.

## Required Preconditions

- `wsl.exe` is installed and working.
- Either Arch Linux already exists as a WSL distro, or Windows can reach the official WSL online install source so Pane can provision `pane-arch` for you. `pane init --rootfs-tar` remains an advanced fallback.
- `/etc/wsl.conf` contains:

```ini
[boot]
systemd=true
```

- The Arch distro has a non-root default user.
- That default user has a usable password for XRDP login.
- `mstsc.exe` is present on Windows.

## Packaged App Flow

If you are using the packaged build instead of the repo:

1. Double-click `Install Pane Shortcuts.cmd` once.
2. Launch `Pane` from the Desktop or Start Menu, or double-click `Pane Control Center.cmd` from the extracted package folder.
3. Use `Onboard Arch` first if Pane is not yet managing an Arch distro. Pane will create or adopt the managed Arch distro, configure the Arch login user, enable the required WSL settings, and verify launch readiness from one flow.
4. Use the control center as the main app surface for onboarding, launch, terminal access, update, repair, reconnect, logs, shared-folder access, reset, and support bundles.
5. Use the control center's `Setup User` action later when you only need to repair the Arch login or WSL config without rerunning full onboarding.
6. Use `Open Pane Arch Terminal.cmd` when you want a shell for first-run setup, package installs, password changes, or customization inside Arch.
7. Use `Open Pane Shared Folder.cmd` when you want the Windows-side folder that appears in Arch as `~/PaneShared`.
8. Use `Collect Pane Support Bundle.cmd` before asking for help.

## Why The Desktop Choice Is Locked

The package intentionally does not expose KDE, GNOME, Niri, or other desktop profiles yet. The current product only has a supportable bootstrap, diagnostics, reconnect path, and recovery story for Arch + XFCE. Pane will widen the profile list only after those other paths are real enough that first-run success stays boring.

## Recommended CLI Flow

1. Run `"strong-password" | pane onboard --username archuser --password-stdin` as the preferred first-run flow. Pane will initialize or reuse the managed Arch distro, create or repair the Arch login, enable the required WSL settings, and verify launch readiness.
2. Run `pane terminal` when you want shell-level work such as `pacman`, dotfiles, or later password changes. Use `pane terminal --user root` when you need a root shell.
3. Run `pane launch --de xfce`.
4. If you want to verify reconnect readiness later, run `pane doctor --de xfce --skip-bootstrap`. That path now checks both the Pane-managed XRDP session assets and whether the Arch user's XFCE config/cache directories exist, are owned correctly, and are writable.
5. Use `pane connect` to reopen the saved `.rdp` profile. Pane will use direct localhost when it is available and fall back to pane-relay when Windows cannot reach the WSL XRDP port directly.
6. Use `pane share` to open or print the Windows-side directory exposed in Arch as `~/PaneShared`.
7. Use `pane logs` when bootstrap or XRDP readiness fails.
8. Run `pane bundle` before asking for support so the current diagnostics are packaged once.
9. Use `pane init` plus `pane setup-user` only when you want to split onboarding into separate lifecycle steps or adopt a specific existing Arch distro.

## Recovery Commands

- `pane stop` stops XRDP inside the selected distro.
- `pane share --print-only` prints the shared Windows and WSL paths for the active session.
- `pane setup-user` creates or repairs the Arch login user, writes `systemd=true` plus the default WSL user to `/etc/wsl.conf`, and can restart WSL automatically.
- `pane terminal` opens an interactive shell inside the resolved Arch distro.
- `pane terminal --user root` opens a root shell for password changes, package repair, or other first-run admin work.
- `pane update` refreshes Arch packages and reapplies the Pane-managed integration without opening `mstsc.exe`.
- `pane repair` reapplies the Pane-managed bootstrap and XRDP/session wiring, including the `pane-relay` helper, session launcher, notifyd X11 override, and required XFCE config/cache directory ownership, without opening `mstsc.exe`.
- `pane reset` removes Pane-managed Windows assets for the active session.
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





