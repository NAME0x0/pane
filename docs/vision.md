# Vision

Pane is trying to become a self-contained Windows app for creating, launching, and owning Linux environments on Windows without asking the user to think like a WSL operator.

## What Exists Now

The current Arch-first MVP already has the shape of the product:

- a packaged `pane.exe`,
- a Windows-side Pane Control Center,
- `pane init` as the first ownership slice for provisioning a managed `pane-arch` distro through WSL or adopting an existing Arch distro,
- Arch + XFCE as the single launchable managed path today,
- a Pane-managed session workspace under `%LOCALAPPDATA%\Pane\sessions\<session>`,
- a shared Windows directory surfaced inside Arch as `~/PaneShared`,
- preflight diagnostics, reconnect, reset, logs, and support bundles,
- a managed-environment roadmap exposed through `pane environments`,
- a localhost-first XRDP handoff with a Pane relay fallback that keeps the current stack practical across more Windows hosts.

## What It Is Not Yet

Pane does not yet own the display path end to end. The current launcher still hands the session to `mstsc.exe` over XRDP, even though Pane can now bridge the localhost path with its own relay when Windows loopback is unreliable.

That means:

- the current stack can be tuned for lower perceived latency,
- the current stack cannot honestly claim true zero latency,
- true near-native responsiveness requires the later non-RDP transport phase.

Pane also does not yet expose multiple desktop environments in the app surface. That is intentional. KDE, GNOME, Niri, and other profiles stay hidden until their bootstrap, reconnect, and recovery path are supportable enough to avoid creating a support burden.

Pane now has the first distro-lifecycle ownership shape through `pane init`, `pane update`, `pane repair`, and `pane reset`, but it still does not own the full first-launch OOBE and lifecycle breadth end to end. Arch is the current launchable environment, while Ubuntu LTS and Debian are codified as the next managed environments rather than exposed as launchable promises.

## Product Direction

The intended direction is:

1. make the packaged Arch experience and control center feel complete and self-contained,
2. make Pane own environment lifecycle rather than assume user-managed distros,
3. keep shared files and support diagnostics inside Pane's own workspace,
4. narrow support until first-run success is boring,
5. add Ubuntu LTS as the second first-class managed environment,
6. add Debian later as a curated preview managed environment,
7. replace the RDP handoff with an embedded transport that removes the extra remoting layer.

The current codebase is now moving from step 1 into step 2: the control surface is app-shaped, and the first Arch ownership flow exists through `pane init`. The transport milestone is still what makes the zero-latency part of the vision real.
