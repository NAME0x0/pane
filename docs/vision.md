# Vision

Pane is trying to become a self-contained Windows app for creating, launching, and owning Linux environments on Windows without asking the user to think like a WSL operator.

## What Exists Now

The current Arch-first MVP already has the shape of the product:

- a packaged `pane.exe`,
- a Windows-side Pane Control Center,
- an app-facing lifecycle and next-action report through `pane app-status`,
- a dedicated 8 GiB default runtime-space reservation through `pane runtime`,
- native host/runtime readiness reporting through `pane native-preflight`,
- a guarded WHP guest execution milestone through `pane native-boot-spike --execute --run-fixture`,
- a verified runtime-provided boot-loader candidate path through `pane runtime --register-boot-loader` and `pane native-boot-spike --execute --run-boot-loader`,
- a verified kernel/initramfs boot-plan contract through `pane runtime --register-kernel`,
- Linux bzImage header inspection before Pane attempts native kernel entry,
- explicit bzImage setup/protected-mode payload layout before the WHP CPU-state entry milestone,
- a materialized kernel boot-layout contract through `pane native-kernel-plan --materialize`,
- guarded WHP execution of a controlled kernel-layout candidate through `pane native-boot-spike --run-kernel-layout`,
- `pane init` as the first ownership slice for provisioning a managed `pane-arch` distro through WSL or adopting an existing Arch distro,
- Arch + XFCE as the single launchable managed path today,
- a Pane-managed session workspace under `%LOCALAPPDATA%\Pane\sessions\<session>`,
- PaneShared storage surfaced inside Arch as `~/PaneShared`, durable by default and scratchable for disposable sessions,
- preflight diagnostics, reconnect, reset, logs, and support bundles,
- a managed-environment roadmap exposed through `pane environments`,
- a localhost-first XRDP handoff with a Pane relay fallback that keeps the current stack practical across more Windows hosts.

## What It Is Not Yet

Pane does not yet own the display path end to end. The current launcher still hands the session to `mstsc.exe` over XRDP, even though Pane can now bridge the localhost path with its own relay when Windows loopback is unreliable.

That means:

- the current stack can be tuned for lower perceived latency through conservative RDP settings and transport fallback checks,
- the current stack cannot honestly claim true zero latency,
- true near-native responsiveness requires the later non-RDP transport phase.

Pane also does not yet expose multiple desktop environments in the app surface. That is intentional. KDE, GNOME, Niri, and other profiles stay hidden until their bootstrap, reconnect, and recovery path are supportable enough to avoid creating a support burden.

Pane now has the first distro-lifecycle ownership shape through `pane init`, `pane update`, `pane repair`, and `pane reset`, but it still does not own the full first-launch OOBE and lifecycle breadth end to end. Arch is the current launchable environment, while Ubuntu LTS and Debian are codified as the next managed environments rather than exposed as launchable promises.

Pane also does not yet boot a Pane-owned OS image from its own disk. The `pane runtime` command prepares the dedicated app space for that future engine: downloads, base OS image, user disk, snapshots, runtime state, runtime config, a native-runtime manifest, a runtime-backed serial boot image, a verified boot-to-serial loader candidate slot, and a verified kernel/initramfs boot-plan slot. It can register a local Arch base image with SHA-256 metadata, register a controlled loader candidate with SHA-256 and expected serial-output metadata, register a kernel plus optional initramfs with a required `console=ttyS0` cmdline, and create the Pane-owned user-disk descriptor. `pane native-kernel-plan --materialize` turns that verified kernel plan into the deterministic guest-memory layout for boot params, cmdline, kernel, and optional initramfs. `pane native-preflight` probes the Windows Hypervisor Platform host boundary and runtime artifact readiness for the first boot-to-serial spike. `pane native-boot-spike --execute --run-fixture` creates a temporary WHP partition/vCPU, loads `serial-boot.paneimg` from the Pane runtime, maps guest memory, configures registers, observes the `PANE_BOOT_OK` COM1 banner across repeated I/O exits, observes HLT, then tears everything down. `pane native-boot-spike --execute --run-boot-loader` applies the same guarded WHP execution path to a verified runtime-provided loader candidate. `pane native-boot-spike --execute --run-kernel-layout` consumes the materialized kernel layout, maps boot params, cmdline, kernel, and optional initramfs regions, and now carries the guest-entry contract through the WHP runner: real-mode serial/HALT validation for the controlled candidate or a protected-mode Linux entry probe with boot params in `rsi` for a bzImage payload. It still does not boot Arch or render a desktop until real early Linux serial output is deterministic. `pane launch --runtime pane-owned --dry-run` exercises that future path without invoking WSL, `mstsc.exe`, or XRDP, but it intentionally stops at the current boot/display blockers instead of pretending the native engine exists.

## Product Direction

The intended direction is:

1. make the packaged Arch experience and control center feel complete and self-contained,
2. make Pane own environment lifecycle rather than assume user-managed distros,
3. reserve and manage dedicated runtime space for OS images, user disks, packages, snapshots, and export/import,
4. keep shared files and support diagnostics inside Pane's own app flows while preserving user-owned PaneShared data by default,
5. narrow support until first-run success is boring,
6. replace the RDP handoff with an embedded Pane-owned display window,
7. move from the WSL bridge to a Pane-owned WHP OS runtime only when boot, storage, display, networking, and repair are measurable,
8. add Ubuntu LTS as the second first-class managed environment,
9. add Debian later as a curated preview managed environment.

The current codebase is now moving from step 1 into step 3 and preparing step 7: the control surface is app-shaped, the first Arch ownership flow exists through `pane init` and `pane onboard`, `pane app-status` gives the app a single lifecycle/next-action model, `pane runtime` defines the dedicated storage/config/artifact boundary for a future Pane-owned OS engine, and `pane native-boot-spike` proves Pane can run both built-in and runtime-provided controlled guest code through WHP without WSL, XRDP, or `mstsc.exe`. The display/runtime milestones are still what make the contained-app and near-native responsiveness parts of the vision real.

See [native-runtime-architecture.md](native-runtime-architecture.md) for the WHP-first implementation contract.

## Current Transport Tuning

Pane's current `.rdp` profile is intentionally tuned for responsiveness rather than a rich remoting session:

- single-display app-like launch instead of multimonitor span,
- compression and bitmap cache enabled,
- wallpaper, menu animations, full-window drag, themes, desktop composition, font smoothing, redirected printers, redirected drives, serial ports, smart cards, position devices, audio playback, and audio capture disabled,
- dynamic sizing enabled so the Windows client can behave more like a resizable app window.

This reduces avoidable overhead in the current XRDP path. It is not a substitute for the later contained display transport.
