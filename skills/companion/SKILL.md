---
name: companion
description: Install, diagnose, or integrate Hraness CLI products built on desktop-foundation on macOS, Windows, or Linux. Use for the owner process, status and tui, the native helper, release selection, OS approval handoffs, and migrating off the removed menu bar; not for general desktop automation or provider permissions.
---

# Hraness CLI products

Read [installation](../../docs/installation.md) for installing or diagnosing
the helper. Read [platforms](../../docs/platforms.md) and
[adoption](../../docs/adoption.md) when integrating a product or claiming
platform support, and [the 1.0 migration guide](../../docs/migration-1.0.md)
when a product still uses the menu bar. The [repository README](../../README.md)
and installed CLI help are authoritative for the current API and commands.

1.0 has no menu bar. `hraness-companion` is an alias of `hraness-helper`; with
no arguments or a 0.x tray flag it exits 2 with `tray-removed`. That is the
expected answer, not a fault to repair: use `<product> tui` or
`<product> status --json`.

## Install or repair

- Identify the exact product/version, OS/architecture and graphical session.
  Start with the product's `status --json` and `doctor` commands.
- Use the product's immutable release manifest and expected digest. Never
  replace missing release assets with a mutable latest download, source build,
  arbitrary executable, or another architecture. Ordinary users need no
  compiler.
- Preserve the product's owner and data. Stop the owner only through the
  product's own command; do not kill by process-name pattern, remove live
  sockets or locks, stop other products, or reset daemon/account state.
- A native executable can still be blocked by Gatekeeper or Windows policy.
  For an exact verified file, explain the OS-provided human approval flow in
  the installation guide. The human performs the trust decision. Never remove
  quarantine/download metadata, disable protection, or change managed policy.
- Distinguish SmartScreen's optional Run Anyway from Smart App Control, which
  has no per-app exception. A policy block makes the helper's dialogs
  unavailable; the product's CLI keeps working.
- On Linux the helper needs GTK 3 and a graphical session for dialogs. Without
  one, `--prompt-probe` reports unavailable and the product uses the terminal.

## Integrate a product

Reuse `@hraness/desktop-foundation`. Put every action behind a registry verb
with an operation class, keep state in one owner (`./control`), gate `decide`
verbs with `./human-gate`, and offer `status --json` and `tui`. Keep daemon
authority, permissions and action handlers in the product. The helper must not
become a shell-command transport or credential store.

For credential entry, the SDK's `promptSecret` shows one bounded native dialog
or falls back to a masked terminal prompt; it returns the value to the product
and stores nothing. Probe hosts with `promptCapability` and keep secrets out of
argv, logs and the request's own title and message.

Preserve separate evidence for portable tests, native builds, the helper
smokes, clean installation and login behavior. Report untested combinations
clearly. On macOS each product can have a local app,
`~/Applications/Hraness/<Product>.app`, assembled on the person's Mac around
the verified helper (`--assemble-app`, or `identity::assemble_app` in Rust)
and signed with the one `Hraness Local Signing` identity, so macOS shows the
product's name and keeps its approvals across updates. Create that identity
only after showing the `LOCAL_SIGNING` notice. Never distribute the app: no
zip, DMG, cask or installer belongs in this model. Publisher credentials
never enter this local assembly flow. Separately, the release pipeline is
prepared to sign and notarize raw Mac helper and companion downloads from
version 1.1.3, before recording their manifest hashes. This does not change
the local app identity or grant permissions. See `docs/identity.md`.
