# Clean-Machine Validation

This is the packaged-binary validation path for Pane's Arch-first MVP. It is meant to be run on a Windows machine without Cargo or the source tree.

## Validation Target

A clean-machine pass should prove all of the following from the shipped package:

- `pane.exe` starts outside the repo,
- the package contains the docs and launcher files a user needs,
- the packaged control center entrypoint exists and self-identifies correctly,
- `status` and `doctor` run from the packaged binary,
- the packaged Arch launcher can generate the Windows-side MVP assets in dry-run mode,
- the packaged shared-folder launcher resolves the managed session workspace,
- the packaged shortcut installer can create Desktop and Start Menu entries,
- the packaged support-bundle launcher can package diagnostics from the shipped binary,
- the validation flow cleans up after itself.

This is not a substitute for a real manual session test. It is the repeatable gate before deeper first-run QA.

## Preconditions

Use a Windows 10 or 11 machine with:

- WSL2 installed,
- an Arch Linux distro present,
- `systemd=true` in `/etc/wsl.conf`,
- a non-root default user with a usable password,
- `mstsc.exe` available.

## Run It

From an extracted package directory:

```powershell
powershell -ExecutionPolicy Bypass -File .\validate-package.ps1
```

From the repo after building a package:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\validate-package.ps1 -PackagePath .\dist\pane-windows-x86_64.zip
```

From the package builder in one step:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1 -RunSmoke
```

## What It Does

The validation script:

1. verifies the package contents, including the control center, double-click launchers, and shortcut installer,
2. runs `pane.exe status --json`,
3. runs `pane.exe doctor --json --distro pane-arch --de xfce`,
4. runs `pane.exe doctor --json --distro pane-arch --de xfce --skip-bootstrap` to verify reconnect diagnostics, including Pane-managed session assets and user-home readiness,
5. runs `Pane Control Center.ps1 -PrintOnly` as a headless self-test,
6. runs `Launch Pane Arch.ps1 -DryRun -NoConnect` with a unique session name,
7. runs `Open Pane Shared Folder.ps1 -PrintOnly` to verify the shared workspace path,
8. runs `Install Pane Shortcuts.ps1` into temporary Desktop and Start Menu folders and checks the generated `.lnk` files,
9. runs `Collect Pane Support Bundle.ps1` and verifies the expected archive entries,
10. resets the temporary Pane session and restores any pre-existing local `state.json`.

## Artifacts

The script writes a temporary artifact directory and prints its path at the end. That directory contains:

- `status.json`
- `doctor.json`
- `doctor-reconnect.json`
- `control-center.txt`
- `launch-dry-run.txt`
- `share.txt`
- `shortcuts.txt`
- `bundle.txt`
- `summary.json`
- the generated support bundle zip
- a backup of any pre-existing Pane state file when one existed

## Expected Outcome

Treat the clean-machine package validation as passing only when:

- the script exits successfully,
- `doctor_supported_for_mvp` is `true`,
- shortcut files were created for both the Desktop and Start Menu targets,
- the generated support bundle contains `status.json`, `doctor.json`, `state.json`, `manifest.json`, `shared-directory.txt`, `workspace/pane-bootstrap.sh`, and `workspace/pane.rdp`.

If this script fails, use the printed artifact directory first. It is the minimum diagnostic bundle for fixing package or first-run regressions.




