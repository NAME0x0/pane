# Contributing to Pane

Thanks for your interest in contributing. Phase 1 is complete, and the current product is an Arch-first MVP. New work should either harden that supported path or move deliberately into post-MVP expansion.

## Current Focus

### MVP Hardening
- Arch Linux + XFCE reliability improvements
- Better diagnostics around systemd, XRDP, login, and reconnect failures
- Packaging, operator docs, and recovery tooling
- Supportability improvements that reduce manual troubleshooting

### Post-MVP
- Additional distro support
- Additional desktop environments
- Embedded RDP client work
- Richer keyboard, clipboard, and audio behavior

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) stable
- Windows 10/11 with WSL2
- An Arch Linux WSL distro for end-to-end MVP testing

### Build

```bash
cargo build
```

### Validate

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

### Run

```bash
cargo run -- doctor --distro archlinux --de xfce
cargo run -- launch --dry-run --distro archlinux --de xfce
cargo run -- status --json
```

## Code Guidelines

- Keep the Windows-side launcher behavior transparent and inspectable.
- Prefer small, testable functions over large orchestration blocks.
- Make error messages actionable.
- Keep generated scripts and transcripts readable because users may need to inspect them.
- If a behavior is intentionally post-MVP, say so directly instead of implying broad support.

## Pull Requests

- Keep PRs focused.
- Describe what changed and why.
- Note what you validated locally.
- Be explicit about whether a change hardens the Arch MVP or expands scope beyond it.

## Reporting Bugs

Include:

1. What you expected.
2. What happened instead.
3. Your Windows version, WSL distro, desktop environment, and whether `pane doctor` passed.
4. The exact `pane` command you ran.
5. Relevant output from `pane logs` when available.

## Code of Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
