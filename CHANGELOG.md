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
- `pane runtime --create-serial-boot-image` for materializing the runtime-backed WHP serial boot image used by the boot-spike runner
- `pane runtime --register-boot-loader` and `pane native-boot-spike --execute --run-boot-loader` for verifying and executing a runtime-provided boot-to-serial loader candidate under the WHP serial/HALT contract
- `pane runtime --register-kernel` and optional `--register-initramfs` for preparing a verified kernel boot plan with a required serial console cmdline
- `pane native-kernel-plan --materialize` for writing the deterministic WHP kernel boot layout covering boot params, cmdline, kernel, and optional initramfs placement
- `pane native-boot-spike --run-kernel-layout` for consuming the materialized kernel layout, including boot params, cmdline, and optional initramfs mappings, in the guarded WHP serial/HALT runner with controlled small candidates
- Linux bzImage header inspection during `pane runtime --register-kernel`, with boot-protocol/setup metadata surfaced in runtime reports
- Linux bzImage layout splitting into setup bytes, protected-mode payload bytes, and an explicit protected-mode entry contract for the next WHP CPU-state milestone
- WHP guest-entry reporting now distinguishes the controlled real-mode serial candidate from the Linux protected-mode entry contract and carries the boot-params GPA through CLI/JSON output
- Linux protected-mode WHP runs now use an entry-probe contract instead of pretending a real bzImage must emit the controlled `PANE_BOOT_OK` serial/HALT fixture banner
- Linux kernel layouts now materialize a deterministic guest memory map, write an E820 table into boot params, and map non-overlapping low/high RAM regions for protected-mode entry probes
- Linux entry probes now classify unhandled memory-access, CPUID, and MSR exits as explicit blockers instead of treating them as successful boot progress
- Linux entry probes now pass WHP default CPUID results back to guest registers and advance RIP, removing the first expected CPU-identification blocker for early kernel execution
- Linux entry probes now decode RDMSR/WRMSR exits, maintain a minimal guest MSR state for early Linux CPU setup, and advance RIP after handled MSR accesses
- Memory-access exits now include access type, unmapped status, GPA, and GVA diagnostics so the next Arch boot blocker identifies the exact missing RAM/MMIO range
- Control Center base-image registration for copying local Arch images into Pane runtime storage with SHA-256 metadata
- `pane native-preflight` for probing Windows Hypervisor Platform host readiness and runtime artifact blockers before the Pane-owned boot spike
- `pane native-boot-spike --execute --run-fixture` for the guarded WHP guest execution milestone: temporary partition/vCPU creation, guest memory mapping, register setup, repeated COM1 serial I/O exit decoding for the `PANE_BOOT_OK` banner, final HLT observation, cleanup, and no Arch boot claim
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
