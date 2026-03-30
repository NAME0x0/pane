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
  <a href="#what-pane-is">What Pane Is</a> · <a href="#supported-mvp">Supported MVP</a> · <a href="#quick-start">Quick Start</a> · <a href="#roadmap">Roadmap</a> · <a href="docs/mvp-arch.md">Arch MVP Guide</a> · <a href="docs/vision.md">Vision</a> · <a href="docs/product-contract.md">Product Contract</a>
</p>

---

Pane is a Windows-native app for creating, launching, and supporting real Linux environments on Windows. It is not just a WSL helper, a script packager, or a thin RDP wrapper. The product goal is simple: open Pane, get a real Linux environment with a real GUI desktop, customize it freely, and let Pane own the setup, launch, reconnect, repair, reset, file sharing, and support flows.

Today Pane ships as an Arch-first MVP on top of the completed Phase 1 foundation. It already provides a packaged `pane.exe`, a Windows-side Control Center, a managed `pane-arch` path, first-run onboarding for Arch user creation and WSL defaults, a shared workspace, diagnostics, repair/update/reset flows, support bundles, and a real Linux GUI handoff through `mstsc.exe`. The product is already app-shaped and supportable, but it still uses XRDP under the hood and is not yet the final contained display architecture.

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
- a Pane-managed shared directory exposed inside Arch as `~/PaneShared`
- RDP handoff through `mstsc.exe` with a localhost-first path plus an automatic Pane relay fallback
- a Windows-side Pane Control Center for launch, update, repair, reconnect, support, and recovery actions

Everything else is intentionally deferred until after the MVP. Ubuntu, Debian, Fedora, KDE, GNOME, Niri, embedded RDP, GPU tuning, and custom transports are not part of the current support boundary. The current XRDP path now prefers direct localhost and falls back to Pane relay when Windows loopback cannot reach WSL directly, but true zero-latency still depends on the later non-RDP transport phase described in [docs/vision.md](docs/vision.md).

## Quick Start

If you are using the packaged build, double-click `Install Pane Shortcuts.cmd` once, then launch `Pane` from the Desktop or Start Menu. You can also double-click `Pane Control Center.cmd` directly from the extracted package directory.

The control center is the intended app surface. It exposes initialization, user setup, launch, terminal access, update, repair, reconnect, logs, shared-folder access, support-bundle generation, reset, and shortcut installation without forcing users into the CLI.

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

Open an interactive shell inside the managed Arch distro when you want to install packages, change passwords later, or customize the environment:

```bash
cargo run -- terminal
```

Use `--user root` when you need a root shell for repair work:

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

Preview the generated assets without touching WSL services:

```bash
cargo run -- launch --dry-run --de xfce
```

Run the real bootstrap and open the Windows RDP client:

```bash
cargo run -- launch --de xfce
```

Open or print the Windows-side shared directory that appears inside Arch as `~/PaneShared`:

```bash
cargo run -- share --print-only
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
- `pane doctor` validates the supported MVP path and prints actionable fixes before launch or reconnect.
- `pane setup-user` creates or repairs the Arch login user, writes the default-user/systemd WSL config, and can restart WSL so the change takes effect immediately when you do not need the full onboarding flow.
- `pane launch` writes the bootstrap script, `.rdp` profile, persisted launch state, and optionally executes the Arch bootstrap.
- `pane update` refreshes Arch packages and reapplies the Pane-managed integration without opening `mstsc.exe`.
- `pane repair` reapplies the Pane-managed Arch bootstrap and session wiring without opening `mstsc.exe`.
- `pane connect` reopens the saved `.rdp` profile after readiness checks pass.
- `pane share` opens or prints the Pane-managed shared directory for the active session.
- `pane terminal` opens an interactive shell inside the resolved Arch distro, with `--user root` available for first-run repair or password work.
- `pane stop` stops XRDP inside the selected distro.
- `pane reset` removes Pane-managed local assets, can release an adopted managed distro, and can factory-reset a Pane-provisioned distro.
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

- `Pane Control Center.cmd` and `Pane Control Center.ps1`
- `Launch Pane Arch.cmd` and `Launch Pane Arch.ps1`
- `Open Pane Arch Terminal.cmd` and `Open Pane Arch Terminal.ps1`
- `Open Pane Shared Folder.cmd` and `Open Pane Shared Folder.ps1`
- `Collect Pane Support Bundle.cmd` and `Collect Pane Support Bundle.ps1`
- `Install Pane Shortcuts.cmd` and `Install Pane Shortcuts.ps1`

You can smoke-test the packaged bundle itself with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -RunSmoke
```

That validates `status`, `doctor`, `onboard --dry-run`, `setup-user --dry-run`, the packaged control-center self-test, the packaged launch/terminal/share/bundle entrypoints, and the shortcut installer, then verifies the generated support zip. The standalone guides are in [docs/clean-machine-validation.md](docs/clean-machine-validation.md) and [docs/vision.md](docs/vision.md).

## Validation

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo run -- status --json
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
| **Now** | Finish the Pane-owned Arch path: managed `pane-arch`, onboarding, account setup, repair/reset/update, support bundles, shared workspace, and the Windows control surface. | Make first-run and recovery boring enough to trust. |
| **Next** | Harden fresh-machine validation and make the Pane-managed Arch path the default supported experience everywhere. | Reduce support debt and remove remaining setup ambiguity. |
| **Later** | Add Ubuntu LTS, then Debian, then curated desktop profiles only when their lifecycle, reconnect, repair, and support path are real. | Expand honestly instead of cosmetically. |
| **Final Architecture** | Replace the XRDP handoff with a more contained transport and remove the extra remoting feel. | Make the "contained app" and near-native responsiveness vision real. |

## Development

```bash
cargo check
cargo test
cargo run -- launch --dry-run --print-script --distro pane-arch --de xfce
```

See [docs/mvp-arch.md](docs/mvp-arch.md) for the operator guide, [docs/product-contract.md](docs/product-contract.md) for the long-term product contract, and [docs/phase-1-audit.md](docs/phase-1-audit.md) for the closure checklist that marked the Phase 1 foundation complete.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)











