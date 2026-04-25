# Product Contract

## Purpose

This document is the product contract for Pane. It exists to keep development aligned with the real product we are trying to build, not just the current implementation details.

Unless this document is intentionally revised, new work should be evaluated against it before being treated as on-strategy.

## Product Definition

Pane is a Windows-native app that gives users real Linux environments on Windows without making them think like WSL, XRDP, or session-management operators.

Pane is not just a launcher. It is a managed Linux environment platform, executed through deeply supported managed environments, starting with Arch.

The intended user experience is:

- install Pane,
- open Pane,
- create or launch a real Linux environment,
- get a real Linux desktop GUI,
- customize that Linux environment freely,
- rely on Pane to handle launch, integration, repair, file sharing, and support workflows.

## Core User Promise

Pane should let a Windows user have a real, personal, customizable Linux environment with as little friction as possible.

That means Pane must eventually provide:

- a Windows app surface as the main point of interaction,
- real Linux GUI desktop sessions,
- a Pane-managed environment lifecycle,
- a supportable first-run experience,
- a supportable reconnect, repair, reset, and update experience,
- a shared file workflow that feels built in,
- a clear path toward near-native responsiveness.

## What Pane Is

Pane is:

- a Windows-first product,
- a Linux-environment platform,
- Arch-first in execution today,
- a product for real Linux use, not a fake sandbox,
- a managed integration layer between Windows and Linux desktop environments,
- a product that should feel like opening an app, not operating remoting infrastructure.

## What Pane Is Not

Pane is not:

- just a CLI wrapper around WSL,
- just a script packager,
- just an RDP launcher,
- just a generic distro switcher with no lifecycle ownership,
- a promise to support every distro, desktop, compositor, or Linux edge case immediately.

## Product Shape We Are Actually Building

The clearest description of Pane is:

Pane is a Windows-native Linux environment platform, starting with Arch.

In practical terms, that means:

- Pane owns the Windows app experience.
- Pane owns the integration and support layer.
- The user owns their Linux life inside Pane-managed environments.
- Pane offers curated distro choices and curated desktop profiles over time.
- Pane should not expose product choices that it cannot honestly support yet.

## Current Truth

Today, Pane can launch a real Arch GUI desktop session, but it does not yet render that session inside a fully contained Pane display window.

The current implementation:

- provides a Windows-side app surface,
- prepares and validates the Arch + XFCE path,
- manages session assets and PaneShared storage,
- launches a real Linux desktop,
- still hands the visible desktop window off to `mstsc.exe` over XRDP.

This means the current product is already app-shaped, but it is not yet the final contained display architecture.

Pane is now reserving a future runtime boundary: a dedicated 8 GiB default app-owned space for downloaded OS images, the base system image, an expandable user disk, snapshots, package/customization data, runtime state, runtime config, base-image verification metadata, user-disk metadata, native-runtime manifest, Windows Hypervisor Platform host preflight, and a guarded WHP partition/vCPU smoke step. That reserved space is not a bootable runtime yet; it is the contract that lets Pane move toward an app-owned OS engine without confusing it with PaneShared or the current WSL bridge.

## End-State Vision

The target experience is:

- the user opens Pane,
- Pane creates or launches the user's own Linux environment,
- the Linux desktop appears as part of the Pane experience,
- the user can customize Linux deeply,
- Pane keeps setup, repair, recovery, support, and shared files simple.

The end-state should feel closer to a personal Linux workstation hub than a remote session utility.

## Ownership Model

Pane should eventually own:

- creation or import of dedicated Pane-managed environments,
- first-run provisioning,
- launch and reconnect flows,
- support diagnostics and support bundles,
- update and repair tooling,
- reset and recovery flows,
- Windows-to-Linux PaneShared storage integration,
- the app-side presentation and, later, the display transport,
- the runtime image, user disk, snapshot, export/import, and repair boundary once Pane moves beyond the WSL bridge.
- the native WHP boot host once Pane moves from preflight to real boot execution.

The user should own:

- durable files they intentionally place in PaneShared,
- packages installed inside their Linux environment,
- dotfiles,
- themes,
- shells,
- editors,
- desktop customization,
- day-to-day Linux usage within the supported boundaries.

## Support Policy

Pane should expose only what it can actually support end to end.

That means a managed environment or desktop profile should not be surfaced in the app unless Pane has all of the following for it:

- creation or import logic,
- bootstrap/install logic,
- launch logic,
- reconnect logic,
- reset/repair logic,
- logging/support logic,
- known-good defaults,
- a clear support statement.

Until that exists, hidden or locked is better than exposed but unreliable.

## Managed Environment Order

The first three managed environments are:

1. Arch Linux
2. Ubuntu LTS
3. Debian

Their intended support tiers are:

- Arch Linux: current flagship, first-class managed environment
- Ubuntu LTS: next first-class managed environment
- Debian: later curated preview managed environment

Kali is intentionally not part of the first three. It may be considered later, but it should not dilute the first support wave.

## Profile Model

Pane should eventually support multiple curated desktop profiles, but not as arbitrary launch toggles.

The model should be:

- one Pane-managed Linux environment per created environment,
- one supported default profile first for each environment,
- additional desktop profiles added deliberately as supported experiences,
- profile choices presented as curated options, not raw unsupported switches.

Examples of future profiles may include:

- XFCE,
- KDE Plasma,
- GNOME,
- Niri.

But those should appear only after Pane can actually carry them operationally.

## Sequencing Rules

To stay on the right path, Pane should follow these sequencing rules:

1. Do not prioritize a broad profile picker before Pane owns environment lifecycle well.
2. Do not surface KDE, GNOME, Niri, or other desktop choices before their support path is real.
3. Do not expand distro breadth faster than Pane can honestly support it.
4. Do not confuse more visible options with more product maturity.
5. Do not claim a contained display experience while the app still hands the session to `mstsc.exe`.
6. Do not optimize for configurability before first-run success, recovery, and support are boring.

## Near-Term Development Order

The preferred product order is:

1. make Pane own a dedicated Arch environment,
2. make the Windows control surface the primary experience,
3. make first-run install and provisioning work without terminal knowledge,
4. keep one supported desktop profile until the path is stable,
5. add Ubuntu LTS as the second first-class managed environment,
6. add Debian later as a curated preview environment,
7. replace the XRDP handoff with a more contained transport.

## Things We Should Not Do Prematurely

We should not:

- add a full distro catalog before lifecycle ownership is real,
- add a desktop picker full of unsupported environments,
- market the current stack as zero-latency,
- treat enum values or experimental scripts as product support,
- let the UI promise more than the support story can sustain.

## Fresh Environment Requirement

If Pane is meant to give users a fresh Linux environment with low hassle, then Pane must eventually own environment lifecycle rather than assume an existing user-managed distro.

That implies future work on:

- creating or importing dedicated Pane-managed environments,
- onboarding for a brand-new user,
- preserving user customizations while updating Pane-managed integration,
- reset behavior that repairs the Pane layer without casually destroying user work.

This is a product-level requirement, not a nice-to-have.

## PaneShared Storage Policy

PaneShared must be treated as user data unless the user explicitly opts into scratch storage.

The default behavior should be:

- durable PaneShared storage lives outside disposable session workspaces,
- `pane reset` preserves durable PaneShared by default,
- destructive cleanup requires an explicit purge option,
- scratch PaneShared storage is allowed for disposable sessions and may be removed with the session workspace.

This distinction matters because file sharing can become user storage. Pane should never casually destroy it during repair or reset flows.

## Display Requirement

Pane should eventually present Linux desktops in a way that feels contained and local to Pane.

Until Pane owns the display path:

- the Linux GUI is real,
- the app experience can still improve,
- but the product should not claim the final contained-window experience yet.

Replacing the XRDP handoff is the milestone that makes the "contained app" vision real in the strongest sense.

## Release Gate

Pane should not be treated as a first public release until:

- `pane.exe` is the real packaged app entrypoint,
- standalone `pane.exe` can hydrate the Control Center assets it needs instead of falling back to CLI-only behavior,
- the Control Center is the primary non-CLI surface,
- the Control Center can consume `pane app-status` for a single lifecycle phase, next action, PaneShared policy, and current display-transport boundary,
- `pane runtime` can prepare a dedicated runtime-space manifest, config, native-runtime contract, verified base-image metadata, and user-disk descriptor for the future Pane-owned OS engine without claiming that engine is already bootable,
- `pane native-preflight` can report WHP host capability and runtime artifact blockers without side effects,
- `pane native-boot-spike --execute` can create and tear down a temporary WHP partition/vCPU without claiming Arch is bootable,
- `pane launch --runtime pane-owned --dry-run` can exercise the native-runtime path without invoking WSL, `mstsc.exe`, or XRDP, and reports concrete blockers instead of pretending the native engine is ready,
- clean-machine validation proves the first-run Arch path outside the repo,
- package-only certification proves a repo-free package can self-test and hydrate standalone `pane.exe` without touching a live WSL install,
- fresh-machine preflight proves the clean Windows VM has WSL, required WSL install flags, and `mstsc.exe` before live first-run testing,
- PaneShared durability and reset semantics are documented and tested,
- the release workflow publishes only intentional draft/prerelease artifacts until the public-release gate is passed.

## Decision Standard

When evaluating a feature or change, the key questions should be:

- Does this make Pane feel more like a self-contained Linux environment app?
- Does this reduce user exposure to WSL/XRDP/session-management hassle?
- Does this improve first-run success, recovery, or support?
- Does this widen scope honestly, or only cosmetically?
- Would this create support debt if surfaced now?

If a change increases visible scope but weakens product honesty or supportability, it is probably off-sequence.

## Next Product Topics

The next major product topics that follow from this contract are:

1. the product model for a dedicated Pane-managed Arch environment,
2. how much Linux customization freedom Pane should explicitly support,
3. how desktop profiles should be packaged and installed,
4. how Ubuntu LTS should be introduced as a second first-class environment,
5. how reset, repair, and update should behave without destroying user ownership,
6. how the future contained display path should replace the current XRDP handoff,
7. how the Pane-owned boot engine should acquire, verify, boot, snapshot, export, and repair OS images without WSL as the execution backend.

## Amendment Rule

This contract should change only on purpose.

If future work suggests a different product direction, this document should be revised explicitly rather than allowed to drift silently.

