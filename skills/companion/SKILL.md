---
name: companion
description: Install, diagnose, or integrate Hraness CLI menu companions built on desktop-foundation on macOS, Windows, or Linux. Use for tray lifecycle, release selection, OS approval handoffs, and shared menu adapters; not for general desktop automation or provider permissions.
---

# CLI companions

Read [installation](../../docs/installation.md) for installing or diagnosing a
launch. Read [platforms](../../docs/platforms.md) when implementing an adapter,
designing a menu, or claiming platform support. The [repository README](../../README.md)
and installed CLI help are authoritative for the current API and commands.

## Install or repair

- Identify the exact product/version, OS/architecture and graphical session.
  Inspect `menubar --help`; older products may not yet implement the common
  command contract. Start with the product's supported status/doctor commands.
- Use the product's immutable runner manifest and expected digest. Never replace
  missing release assets with a mutable latest download, source build, arbitrary
  executable, or another architecture. Ordinary users need no compiler.
- Preserve the product/user singleton and data. Stop only the task-owned
  companion through its supported command; do not kill by process-name pattern,
  remove live locks, stop other products, or reset daemon/account state.
- A native executable can still be blocked by Gatekeeper or Windows policy.
  For an exact verified file, explain the OS-provided human approval flow in
  the installation guide. The human performs the trust decision. Never remove
  quarantine/download metadata, disable protection, or change managed policy.
- Distinguish SmartScreen's optional Run Anyway from Smart App Control, which
  has no per-app exception. A policy block may make the unsigned companion
  unavailable; use already-permitted CLI/browser controls where available.
- On Linux, distinguish missing loader libraries, no graphical session and no
  panel host. Offer documented distro packages or a compatible GNOME extension
  when relevant; do not install another desktop environment to force a tray.
- After repair, verify the running status and an observable harmless menu action.
  A spawn, successful checksum or build alone does not prove a working tray.

## Integrate a product

Reuse `@hraness/desktop-foundation` and the shared `hraness-companion` runner.
Keep daemon authority, permissions and action handlers in the product; send
bounded snapshots and receive stable action IDs over JSON lines. The renderer
must not become a shell-command transport or credential store.

Use confirmed visible state, actions, checkmarks and short submenus. Open larger
interfaces in the browser. Include status, a useful setup/recovery action and
Quit; distinguish quitting the companion from stopping the product daemon.
Do not depend on tray-title text, tooltips, hover or global shortcuts across all
platforms. Keep protected provider capabilities on their supported platforms.

Preserve separate evidence for portable tests, native builds, interactive smoke,
clean installation and login behavior. Report untested combinations clearly.
No app bundles, desktop installers, publisher signing credentials or Apple
notarization workflow belong in this distribution model.
