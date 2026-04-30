<p align="center">
  <img src="assets/pane-icon.png" alt="Pane icon" width="96" height="96">
</p>
<p align="center">
  <strong>Pane</strong>
</p>

<p align="center">
  Windows-native Linux environment platform, starting with Arch.
</p>

<p align="center">
  <a href="#what-pane-is">What Pane Is</a> · <a href="#supported-mvp">Supported MVP</a> · <a href="#quick-start">Quick Start</a> · <a href="#roadmap">Roadmap</a> · <a href="docs/mvp-arch.md">Arch MVP Guide</a> · <a href="docs/vision.md">Vision</a> · <a href="docs/product-contract.md">Product Contract</a> · <a href="docs/native-runtime-architecture.md">Native Runtime</a>
</p>

---

Pane is a Windows-native app for creating, launching, and supporting real Linux environments on Windows. It is not just a WSL helper, a script packager, or a thin RDP wrapper. The product goal is simple: open Pane, get a real Linux environment with a real GUI desktop, customize it freely, and let Pane own the setup, launch, reconnect, repair, reset, file sharing, and support flows.

Today Pane is a pre-release Arch-first MVP on top of the completed Phase 1 foundation. It already provides a packaged `pane.exe`, a Windows-side Control Center, a managed `pane-arch` path, first-run onboarding for Arch user creation and WSL defaults, PaneShared storage, diagnostics, repair/update/reset flows, support bundles, and a real Linux GUI handoff through `mstsc.exe`. The product is already app-shaped and supportable, but it still uses XRDP under the hood and is not yet the final contained display architecture.

## What Pane Is

- a Windows-native Linux environment platform
- an app-shaped control surface for creating, launching, and supporting Linux environments
- Arch-first today, with Ubuntu LTS next and Debian later
- a product for real Linux use and real user customization, not a fake sandbox
- a managed integration layer that owns onboarding, launch, reconnect, repair, reset, updates, shared files, and support workflows

## What Pane Is Not Yet

- not a broad distro picker with equal support for everything
- not a desktop-profile matrix full of half-supported options
- not an embedded display window yet
- not honest to market as zero-latency while `mstsc.exe` + XRDP still carry the visible session
- not yet a bootable Pane-owned OS runtime, though Pane now owns the dedicated runtime storage/config contract and WHP native-runtime preflight path

## Why

The hard part of a real WSL desktop workflow is not drawing a window. It is making first-run setup, account creation, launch, reconnect, recovery, and support predictable enough that normal users do not need tribal knowledge. Pane narrows the current MVP to one supported distro and one desktop environment so the recovery and diagnostics can be explicit instead of aspirational. That discipline is what lets Pane grow into a real Linux-on-Windows product instead of a fragile pile of toggles.

## Supported MVP

Pane currently supports one blessed path only:

- Windows 10 or 11
- WSL2 installed and working
- Arch Linux as the target distro
- XFCE as the desktop environment
- `systemd=true` in `/etc/wsl.conf`
- a non-root default WSL user with a usable password
- PaneShared storage exposed inside Arch as `~/PaneShared`, durable by default and scratchable when requested
- RDP handoff through `mstsc.exe` with a localhost-first path plus an automatic Pane relay fallback
- a Windows-side Pane Control Center for launch, update, repair, reconnect, support, and recovery actions

Everything else is intentionally deferred until after the MVP. Ubuntu, Debian, Fedora, KDE, GNOME, Niri, embedded RDP, GPU tuning, and custom transports are not part of the current support boundary. The current XRDP path now prefers direct localhost and falls back to Pane relay when Windows loopback cannot reach WSL directly, but true zero-latency still depends on the later non-RDP transport phase described in [docs/vision.md](docs/vision.md).

## Quick Start

If you are using the packaged build, double-click `pane.exe` to open the Control Center. When sidecar scripts are missing, `pane.exe` hydrates the app entrypoint into `%LOCALAPPDATA%\Pane\app` first. You can also double-click `Install Pane Shortcuts.cmd` once, then launch `Pane` from the Desktop or Start Menu.

The control center is the intended app surface. It exposes initialization, user setup, launch, terminal access, update, repair, reconnect, logs, PaneShared access, support-bundle generation, reset, and shortcut installation without forcing users into the CLI.

Inspect the managed-environment roadmap first when you want the platform view:

```bash
cargo run -- environments
```

Initialize and onboard Pane-managed Arch in one first-run flow. By default Pane provisions the official Arch WSL image into a managed `pane-arch` distro, creates the Arch login user, enables the required WSL settings, and verifies launch readiness:

```powershell
"strong-password" | cargo run -- onboard --username archuser --password-stdin
```

The split lifecycle commands still exist when you want more control:

```bash
cargo run -- init
```

```powershell
"strong-password" | cargo run -- setup-user --username archuser --password-stdin
```

Open an interactive shell inside the managed Arch distro when you want to customize the environment, edit dotfiles, or change passwords later:

```bash
cargo run -- terminal
```

Use `--user root` for package installation, repair work, or other admin tasks unless you configure sudo yourself inside Arch:

```bash
cargo run -- terminal --user root
```

If you already have an Arch distro you want Pane to adopt instead:

```bash
cargo run -- init --existing-distro archlinux
```

Advanced fallback: import a rootfs tar manually only when you do not want the online provisioning path:

```bash
cargo run -- init --rootfs-tar C:\path\to\archlinux.tar
```

Validate the machine and the selected Arch distro from the operator view:

```bash
cargo run -- doctor --de xfce
```

Inspect the app-facing phase, next action, storage policy, and display-transport boundary:

```bash
cargo run -- app-status --json
```

Prepare the dedicated Pane runtime space, create the app-owned user disk descriptor that will eventually hold package/account/customization data, and write the deterministic serial boot image used by the WHP boot-spike runner:

```bash
cargo run -- runtime --prepare --create-user-disk --create-serial-boot-image --capacity-gib 8
```

Register a controlled boot-to-serial loader candidate once you have a tiny loader image that emits a known COM1 serial banner and halts. This does not boot Arch yet; it proves Pane can execute a verified runtime-provided boot artifact instead of only its built-in fixture:

```powershell
cargo run -- runtime --register-boot-loader C:\path\to\loader.img --boot-loader-expected-sha256 <64-char-sha256> --boot-loader-expected-serial "PANE_BOOT_OK\n"
```

Prepare the first kernel/initramfs boot-plan contract once you have a trusted kernel artifact. Pane requires an explicit serial console cmdline so the next WHP milestone can prove boot progress before any GUI work:

```powershell
cargo run -- runtime --register-kernel C:\path\to\vmlinuz-linux --kernel-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"
cargo run -- runtime --register-initramfs C:\path\to\initramfs-linux.img --initramfs-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"
```

Materialize the native kernel boot layout after the kernel plan is verified. This writes the guest-physical-address contract for boot params, cmdline, kernel, and optional initramfs. `--run-kernel-layout` consumes that contract by mapping those regions under WHP, but it still does not boot Arch; real Linux boot-protocol entry is the next milestone:

```powershell
cargo run -- native-kernel-plan --materialize
cargo run -- native-boot-spike --execute --run-kernel-layout
```

Probe whether the Windows host and Pane runtime artifacts are ready for the first Pane-owned WHP boot spike:

```bash
cargo run -- native-preflight --json
```

Preview the first WHP boot-spike step. This is plan-only by default; add `--execute` to create and tear down a temporary WHP partition/vCPU, and add `--run-fixture` when you intentionally want Pane to map guest memory, set vCPU registers, run a deterministic serial test image, observe the `PANE_BOOT_OK` COM1 banner, and halt cleanly:

```bash
cargo run -- native-boot-spike --json
cargo run -- native-boot-spike --json --execute --run-fixture
cargo run -- native-boot-spike --json --execute --run-boot-loader
```

Register a local Arch base OS image into Pane's runtime image store. Pass the expected SHA-256 whenever you have it; without that digest Pane records the image but keeps it untrusted:

```bash
cargo run -- runtime --register-base-image C:\path\to\arch-base.img --expected-sha256 <64-char-sha256>
```

Preview the Pane-owned runtime launch path without invoking WSL, `mstsc.exe`, or XRDP:

```bash
cargo run -- launch --runtime pane-owned --dry-run
```

Run a side-effect-free diagnostic pass before creating any Pane workspace state:

```bash
cargo run -- doctor --de xfce --no-write --no-connect
```

Preview the generated assets without touching WSL services:

```bash
cargo run -- launch --dry-run --de xfce
```

Run the real bootstrap and open the Windows RDP client:

```bash
cargo run -- launch --de xfce
```

Open or print PaneShared, the Windows-side storage that appears inside Arch as `~/PaneShared`:

```bash
cargo run -- share --print-only
```

PaneShared is durable user storage by default and survives `pane reset`. Use scratch storage for disposable sessions:

```bash
cargo run -- launch --de xfce --shared-storage scratch
```

Check whether an already-bootstrapped distro is actually reconnect-ready:

```bash
cargo run -- doctor --de xfce --skip-bootstrap
```

That reconnect view now verifies both the Pane-managed session assets and the default user's XFCE home/config layout, so it catches the exact blank-desktop and stale-session failures Pane now repairs automatically.

If bootstrap or reconnect still fails, package the diagnostics before asking for help:

```bash
cargo run -- bundle
```
## Commands

- `pane init` provisions the official Arch WSL image by default, adopts an existing Arch distro with `--existing-distro`, or imports a rootfs tar with `--rootfs-tar` as an advanced fallback.
- `pane onboard` is the preferred first-run path: it initializes or reuses the managed Arch distro, configures the Arch login user, and verifies launch readiness in one command.
- `pane environments` prints Pane's managed Linux environment roadmap and support tiers.
- `pane app-status` reports the Control Center lifecycle phase, recommended next action, PaneShared policy, and current-vs-planned display transport.
- `pane runtime` inspects or prepares dedicated Pane-owned runtime storage, config, native-runtime manifest, verified base-image metadata, the user-disk descriptor, the runtime-backed serial boot image, an optional verified boot-to-serial loader candidate, and the first verified kernel/initramfs boot plan for the future contained OS engine. This is separate from PaneShared and does not boot Arch yet.
- `pane native-preflight` checks the Windows Hypervisor Platform host boundary plus runtime artifacts for the future Pane-owned boot-to-serial spike. It is side-effect-free and does not claim the OS is bootable.
- `pane native-boot-spike` is the first executable WHP host milestone. By default it prints the safe plan; with `--execute` it creates one temporary WHP partition and vCPU; with `--execute --run-fixture` it also maps guest memory, configures registers, runs a deterministic serial test image, decodes the `PANE_BOOT_OK` COM1 banner across repeated I/O exits, observes HLT, and tears everything down. With `--execute --run-boot-loader`, it runs the verified runtime-provided boot-loader candidate under the same serial/HALT contract.
- `pane doctor` validates the supported MVP path and prints actionable fixes before launch or reconnect. Use `--no-write --no-connect` for support diagnostics that must not create the Pane workspace or PaneShared.
- `pane setup-user` creates or repairs the Arch login user, writes the default-user/systemd WSL config, and can restart WSL so the change takes effect immediately when you do not need the full onboarding flow. It does not grant passwordless sudo or edit `/etc/sudoers`.
- `pane launch` writes the bootstrap script, `.rdp` profile, persisted launch state, and optionally executes the Arch bootstrap. `--shared-storage durable` is the default; `--shared-storage scratch` scopes PaneShared to the disposable session workspace. `--runtime pane-owned --dry-run` exercises the native runtime contract without touching WSL/RDP.
- `pane update` refreshes Arch packages and reapplies the Pane-managed integration without opening `mstsc.exe`.
- `pane repair` reapplies the Pane-managed Arch bootstrap and session wiring without opening `mstsc.exe`.
- `pane connect` reopens the saved `.rdp` profile after readiness checks pass.
- `pane share` opens or prints PaneShared storage for the active session.
- `pane terminal` opens an interactive shell inside the resolved Arch distro, with `--user root` available for first-run repair or password work.
- `pane stop` stops XRDP inside the selected distro.
- `pane reset` removes Pane-managed local session assets while preserving durable PaneShared storage by default. Use `--purge-shared` only when that user data should be deleted too.
- `pane reset --release-managed-environment` detaches Pane from the managed distro without deleting it.
- `pane reset --factory-reset` unregisters a Pane-provisioned distro from WSL, removes its install root, and clears Pane ownership.
- `pane reset --dry-run` prints the reset plan without changing WSL, workspaces, or Pane state.
- `pane logs` prints the saved bootstrap transcript and the live XRDP logs from WSL when available.
- `pane bundle` writes a zip archive with status, doctor output, state, workspace assets, and live XRDP logs when they can be collected.
- `pane status` reports WSL inventory, MVP support status, readiness state, and the last generated Pane workspace.
## Packaging

Build a Windows package directory and zip archive with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

The script builds `pane.exe`, copies the core docs, and ships these app-like entrypoints in the package root:

- `pane.exe`, which opens the Control Center when launched without CLI arguments and can hydrate the bundled app scripts when run alone
- `Pane Control Center.cmd` and `Pane Control Center.ps1`
- `Launch Pane Arch.cmd` and `Launch Pane Arch.ps1`
- `Open Pane Arch Terminal.cmd` and `Open Pane Arch Terminal.ps1`
- `Open Pane Shared Folder.cmd` and `Open Pane Shared Folder.ps1`
- `Collect Pane Support Bundle.cmd` and `Collect Pane Support Bundle.ps1`
- `Install Pane Shortcuts.cmd` and `Install Pane Shortcuts.ps1`
- `certify-fresh-machine.ps1`

You can smoke-test the packaged bundle itself with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64 -Mode PackageOnly
```

Use the stronger live Arch gate only on a machine where the managed Arch session is expected to work:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/certify-fresh-machine.ps1 -PackagePath .\dist\pane-windows-x86_64 -Mode LiveArchSession
```

`PackageOnly` validates package contents, standalone `pane.exe` hydration, Control Center self-test, native-runtime preflight reporting, dry-run onboarding, dry-run launch asset generation, shortcut creation, and the latency-oriented RDP profile without requiring a live Arch session. `LiveArchSession` also runs the full Arch package validator and verifies the generated support zip. The standalone guides are in [docs/clean-machine-validation.md](docs/clean-machine-validation.md), [docs/vision.md](docs/vision.md), and [docs/native-runtime-architecture.md](docs/native-runtime-architecture.md).

If you regenerate the app icon, install the Python development dependency first with `pip install -r requirements-dev.txt`.

## Validation

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo run -- status --json
cargo run -- app-status --json
cargo run -- runtime --json --prepare --create-user-disk --create-serial-boot-image --capacity-gib 8
cargo run -- runtime --json --register-boot-loader C:\path\to\loader.img --boot-loader-expected-sha256 <64-char-sha256> --boot-loader-expected-serial "PANE_BOOT_OK\n"
cargo run -- runtime --json --register-kernel C:\path\to\vmlinuz-linux --kernel-expected-sha256 <64-char-sha256> --kernel-cmdline "console=ttyS0 panic=-1"
cargo run -- native-kernel-plan --json --materialize
cargo run -- native-preflight --json
cargo run -- native-boot-spike --json
cargo run -- native-boot-spike --json --run-kernel-layout
cargo run -- native-boot-spike --json --execute --run-fixture
cargo run -- native-boot-spike --json --execute --run-boot-loader
cargo run -- doctor --json --distro pane-arch --de xfce --no-write --no-connect
cargo run -- doctor --json --distro pane-arch --de xfce
cargo run -- doctor --json --distro pane-arch --de xfce --skip-bootstrap
```

## Roadmap

### Managed Environments

| Environment | Role | Status |
|-------------|------|--------|
| **Arch Linux** | Flagship first-class managed environment | Current |
| **Ubuntu LTS** | Second first-class managed environment | Next |
| **Debian** | Curated preview managed environment | Later |

Kali, Fedora, and other distros are intentionally not in the first support wave. Pane is trying to become the place where Windows users can use Linux without hassle, but it has to earn breadth slowly enough that support quality stays real.

### Platform Development Order

| Stage | Focus | Why it matters |
|-------|-------|----------------|
| **Now** | Finish the Pane-owned Arch path: managed `pane-arch`, onboarding, account setup, repair/reset/update, support bundles, PaneShared storage, 8 GiB runtime-space reservation, base-image registration, user-disk descriptor creation, native-runtime preflight, and the Windows control surface. | Make first-run, recovery, and data boundaries boring enough to trust. |
| **Next** | Move from the verified runtime-provided boot-to-serial loader to a verified kernel/initramfs boot plan, then implement WHP kernel entry, boot parameters, initramfs placement, and serial boot-progress capture. | Make Pane behave more like an app-owned Linux appliance instead of a WSL helper. |
| **Later** | Embed the display into a Pane-owned window, then add Ubuntu LTS, Debian, and curated desktop profiles only when their lifecycle, reconnect, repair, and support path are real. | Expand honestly instead of cosmetically. |
| **Final Architecture** | Replace the XRDP handoff with a Pane-owned runtime/display transport. | Make the contained app and near-native responsiveness vision real. |

## Development

```bash
cargo check
cargo test
cargo run -- launch --dry-run --print-script --distro pane-arch --de xfce
```

See [docs/mvp-arch.md](docs/mvp-arch.md) for the operator guide, [docs/product-contract.md](docs/product-contract.md) for the long-term product contract, [docs/native-runtime-architecture.md](docs/native-runtime-architecture.md) for the WHP runtime path, and [docs/phase-1-audit.md](docs/phase-1-audit.md) for the closure checklist that marked the Phase 1 foundation complete.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)











