# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `pane doctor`, `pane connect`, `pane stop`, `pane reset`, and `pane logs` for support-oriented MVP operations
- Persisted bootstrap transcripts under the Pane workspace for launch debugging
- `docs/mvp-arch.md` with the supported Arch Linux + XFCE operating guide
- `scripts/package.ps1` for building a Windows package directory and zip archive

### Changed
- The supported product boundary is now explicitly Arch Linux + XFCE for the MVP
- `pane launch` now runs preflight diagnostics before touching WSL services
- `pane status` now reports MVP support state, systemd readiness, password state, and bootstrap log presence
- The generated bootstrap script now targets the Arch-first MVP path and fails fast outside that support boundary
- README, contributing docs, and site copy now describe the Arch-first MVP instead of broad distro support
