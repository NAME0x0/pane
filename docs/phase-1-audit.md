# Phase 1 Audit

## Contract

Phase 1 is complete when the repo delivers all of the following:

- a buildable Rust CLI,
- `pane launch` and `pane status`,
- WSL distro detection and validation,
- generated XRDP bootstrap and `.rdp` assets,
- persisted launch state that reflects real execution progress,
- readiness checks that inspect both WSL and local workspace assets,
- documentation that matches the actual behavior,
- validation through format, clippy, tests, and live CLI smoke checks.

## Foundation Closed

The Phase 1 foundation is complete. The repo now has the launcher, state model, workspace assets, readiness checks, and RDP handoff needed to mark that phase done.

## MVP Hardening Added After Phase 1

To reduce support load, the current product intentionally narrows the supported path and adds operational commands around it:

1. The supported MVP path is Arch Linux + XFCE only.
2. `pane doctor` blocks unsupported or broken setups before launch or reconnect.
3. `pane connect`, `pane stop`, `pane reset`, and `pane logs` give the product a real recovery surface.
4. Bootstrap transcripts are persisted under the Pane workspace for support and debugging.
5. Docs now describe the Arch-first MVP boundary instead of implying broad distro support.

## Remaining Out Of Scope

These are intentionally not part of the current Phase 1 + MVP boundary:

- replacing `mstsc.exe` with an embedded RDP client,
- adding non-Arch distro support,
- adding KDE or GNOME support,
- richer keyboard, clipboard, or audio integration,
- GPU-aware rendering optimization,
- any transport that bypasses RDP entirely.
