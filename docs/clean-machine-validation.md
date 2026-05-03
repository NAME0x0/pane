# Clean-Machine Validation

This is the packaged-binary validation path for Pane's Arch-first MVP. It is meant to be run on a Windows machine without Cargo or the source tree.

## Validation Target

A clean-machine pass should prove all of the following from the shipped package:

- `pane.exe` starts outside the repo,
- `pane.exe` launched without arguments opens the packaged Control Center, hydrating embedded app scripts first when sidecars are missing,
- the package contains the docs and launcher files a user needs,
- the packaged control center entrypoint exists and self-identifies correctly,
- `app-status` reports the app lifecycle and current display-transport boundary without claiming a contained window,
- `runtime` can prepare the dedicated app-owned runtime-space layout, config, native-runtime manifest, serial boot image, verified boot-loader metadata, verified kernel boot-plan metadata, materialized kernel boot-layout metadata, mapped framebuffer/input queue contracts, and user-disk descriptor without requiring a live WSL install,
- `native-preflight` reports Windows Hypervisor Platform host checks and runtime artifact blockers without requiring a live WSL install,
- `native-boot-spike` reports its safe plan by default without creating a WHP partition unless `--execute` is passed, and its explicit fixture plus registered boot-loader modes can run controlled guest code without WSL, XRDP, or `mstsc.exe`,
- `status` and `doctor` run from the packaged binary,
- the packaged Arch launcher can generate the Windows-side MVP assets in dry-run mode,
- the packaged shared-folder launcher resolves PaneShared storage,
- the packaged shortcut installer can create Desktop and Start Menu entries,
- the packaged support-bundle launcher can package diagnostics from the shipped binary,
- the validation flow cleans up after itself.

This is not a substitute for a real manual session test. It is the repeatable gate before deeper first-run QA.

## Preconditions

Use a Windows 10 or 11 machine with:

- WSL2 installed,
- an Arch Linux distro present or a Pane-managed `pane-arch` provisioning path available,
- `systemd=true` in `/etc/wsl.conf`,
- a non-root default user with a usable password,
- `xrdp` and `xorgxrdp` available from trusted configured Arch package sources,
- `mstsc.exe` available.

## Run It

From an extracted package directory:

```powershell
powershell -ExecutionPolicy Bypass -File .\certify-fresh-machine.ps1 -Mode PackageOnly
```

From the repo after building a package:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64.zip -Mode PackageOnly
```

From the package builder in one step:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1 -RunSmoke
```

Use `-Mode FreshMachinePreflight` on a real clean Windows VM when you want to verify host prerequisites without modifying WSL. Use `-Mode LiveArchSession` only when the VM is allowed to create/use the managed Arch session and run the existing live package validator.

## What It Does

The certification script in `PackageOnly` mode:

1. verifies the package contents, including the control center, double-click launchers, and shortcut installer,
2. runs `pane.exe --help`, `pane.exe environments`, `pane app-status`, `pane runtime --prepare --create-user-disk`, `pane runtime --register-boot-loader`, `pane runtime --register-kernel`, `pane native-kernel-plan --materialize`, `pane native-preflight`, `pane native-boot-spike`, `pane native-boot-spike --run-kernel-layout`, `pane launch --runtime pane-owned --dry-run`, `pane doctor --no-write`, `pane init --dry-run`, and `pane onboard --dry-run`,
3. runs `Pane Control Center.ps1 -PrintOnly` as a headless self-test,
4. runs `Launch Pane Arch.ps1 -DryRun -NoConnect` with a unique session name and scratch PaneShared storage,
5. verifies the generated `.rdp` profile contains the latency-oriented settings Pane currently depends on,
6. runs `Open Pane Shared Folder.ps1 -PrintOnly` to verify the PaneShared path,
7. runs `Open Pane Arch Terminal.ps1 -PrintOnly`,
8. runs `Install Pane Shortcuts.ps1` into temporary Desktop and Start Menu folders and checks the generated `.lnk` files,
9. copies the standalone exe to a sidecar-free directory and verifies it hydrates its Control Center assets,
10. writes a summary JSON under the printed artifact directory.

The existing `validate-package.ps1` remains the live Arch-session validator. `certify-fresh-machine.ps1 -Mode LiveArchSession` runs the package-only checks first, then delegates to that live validator.

## Artifacts

The PackageOnly certification script writes a temporary artifact directory and prints its path at the end. That directory contains:

- `doctor-no-write.json`
- `app-status.json`
- `runtime.json`
- `native-preflight.json`
- `native-boot-spike.json`
- `native-launch-dry-run.txt`
- `init-dry-run.json`
- `onboard-dry-run.json`
- `control-center.txt`
- `launch-dry-run.txt`
- `share.txt`
- `terminal.txt`
- `shortcuts.txt`
- `standalone-hydration.txt`
- `summary.json`

LiveArchSession also runs the live validator, which adds `status.json`, `doctor.json`, `doctor-reconnect.json`, `bundle.txt`, the generated support bundle zip, and a backup of any pre-existing Pane state file when one existed.

## Expected Outcome

Treat the clean-machine package validation as passing only when:

- the script exits successfully,
- `pane.exe` and the packaged Control Center self-identify from outside the repo,
- `PackageOnly` certification passes on CI and on a repo-free extracted package,
- `FreshMachinePreflight` passes on a clean Windows VM before first-run testing,
- `LiveArchSession` reports `doctor_supported_for_mvp` as `true`,
- shortcut files were created for both the Desktop and Start Menu targets,
- the generated support bundle contains `status.json`, `doctor.json`, `state.json`, `manifest.json`, `shared-directory.txt`, `workspace/pane-bootstrap.sh`, and `workspace/pane.rdp`.

If this script fails, use the printed artifact directory first. It is the minimum diagnostic bundle for fixing package or first-run regressions.




