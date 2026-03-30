## Phase 1 Foundation

### Goal
Complete the Windows-first Rust launcher that prepares and hands off a Linux desktop session inside WSL2.

### Delivered Scope
- a buildable `pane` binary,
- persisted launch state with explicit execution stages,
- WSL distro detection and validation,
- generated XRDP bootstrap and `.rdp` assets under `%LOCALAPPDATA%\Pane`,
- readiness reporting through `pane status`,
- a Windows handoff to `mstsc.exe`.

### Current MVP Policy
Phase 1 is the technical foundation. The currently supported MVP path is intentionally narrower:

- Arch Linux only,
- XFCE only,
- systemd-enabled WSL,
- a non-root default user with a usable password,
- support-oriented commands such as `doctor`, `connect`, `reset`, `stop`, and `logs`.

### Intentionally Out Of Scope For Phase 1
- embedded/native RDP,
- shared-memory graphics transport,
- GPU-specific tuning,
- richer audio/clipboard integration,
- bypassing RDP entirely.
