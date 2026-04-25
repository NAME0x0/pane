# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `pane doctor`, `pane connect`, `pane stop`, `pane reset`, and `pane logs` for support-oriented MVP operations
- Persisted bootstrap transcripts under the Pane workspace for launch debugging
- `docs/mvp-arch.md` with the supported Arch Linux + XFCE operating guide
- `scripts/package.ps1` for building a Windows package directory and zip archive
- Durable PaneShared storage with an explicit scratch mode for disposable sessions
- Packaged `pane.exe` no-argument app entrypoint for opening the Control Center
- Package-entrypoint smoke checks in CI and release packaging workflows
- `certify-fresh-machine.ps1` for repo-free package certification, fresh-machine preflight, and live Arch-session certification
- `pane doctor --no-write` for support diagnostics that do not create the Pane workspace or PaneShared
- `pane app-status` for the Control Center lifecycle phase, next action, storage policy, and display-transport boundary
- Control Center Start First Run flow backed by the app-facing lifecycle report
- `pane runtime` for preparing dedicated app-owned runtime storage for the future contained OS engine
- Control Center Prepare Runtime action for creating the runtime-space layout and manifest
- `pane launch --runtime pane-owned --dry-run` for exercising the native runtime path without WSL, `mstsc.exe`, or XRDP
- `pane runtime --register-base-image` and `--create-user-disk` for moving native runtime readiness from missing artifacts toward boot-engine work
- Control Center base-image registration for copying local Arch images into Pane runtime storage with SHA-256 metadata
- `pane native-preflight` for probing Windows Hypervisor Platform host readiness and runtime artifact blockers before the Pane-owned boot spike
- `pane native-boot-spike` for the guarded WHP partition/vCPU smoke milestone, with `--execute` required before creating temporary hypervisor resources
- `docs/native-runtime-architecture.md` documenting the WHP-first native runtime contract and acceptance gates

### Changed
- The supported product boundary is now explicitly Arch Linux + XFCE for the MVP
- `pane launch` now runs preflight diagnostics before touching WSL services
- `pane status` now reports MVP support state, systemd readiness, password state, and bootstrap log presence
- The generated bootstrap script now targets the Arch-first MVP path and fails fast outside that support boundary
- README, contributing docs, and site copy now describe the Arch-first MVP instead of broad distro support
- `pane reset` now preserves durable PaneShared storage unless `--purge-shared` is passed
- The Arch bootstrap no longer builds AUR packages automatically or grants temporary passwordless sudo
- `pane setup-user` creates the Linux login and default-user config without editing `/etc/sudoers`
- Release packaging is manual and draft/prerelease-only until the public-release gate is passed
- Generated RDP profiles now prefer latency-oriented single-display settings and disable avoidable redirected devices and desktop effects
- The default Pane runtime reservation is now 8 GiB while the native OS runtime is still being built
- Package certification now records `native-preflight.json` and validates that host/runtime native readiness is reported without requiring a bootable native engine
