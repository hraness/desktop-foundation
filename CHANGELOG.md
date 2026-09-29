# Changelog

## Unreleased

### Changes

- `runCompanion` takes an optional `startupTimeoutMs` for the runner's ready event. It defaults to `timeoutMs`, so existing callers see no change.

## 0.9.0 - 2026-09-28

Products can now run without a menu bar: one owner process per product, short-lived commands that print JSON, a human gate for decisions a person owns, and terminal views in place of the menu. Everything is additive; the tray, `./menu-kit` and the companion lifecycle calls work as before.

### Changes

- New SDK subpaths `./registry`, `./control`, `./human-gate`, `./tui`, `./login`, `./retire` and `./helper`, with a new Rust crate, `hraness-control-kit`, that matches them. Both read the shared contract in `contract/`, which now ships in the package.
- Every `--json` command prints one envelope with a stable error code and exit code: 2 usage, 3 needs a person, 4 owner not running, 5 conflict. `commands --json` lists every verb with its operation class.
- A `decide` verb needs a person at a foreground terminal who types back a one-time code shown on `/dev/tty`. Run from an agent with `--json`, it exits 3 with the command a person should run, and never prompts. This stops accidental approvals, not a determined process running as the same user; see `docs/human-gate.md`.
- The owner listens on an agent socket and an admin socket in a 0700 directory. `ensureOwner` starts it on demand, and `controlStatus` reports it without sending signals.
- `tui` views render interactively on a terminal, as a plain-text snapshot when piped, or as the same JSON `status --json` prints.
- `retireLegacyLoginItem` moves a product's old menu bar login item aside after checking it is the product's own, and `installLoginItem` adds an opt-in login item for the owner.
- A new `hraness-helper-<target>` executable runs the one-shot modes (`--notice`, `--prompt`, `--launch`, `--assemble-app` and the others) with the same arguments and output as `hraness-companion`, without the tray. macOS builds are ad-hoc signed. The release manifest lists it under `helperAssets`, and `resolveHelper` falls back to the companion for older manifests. SDK 0.8.x refuses the new field, so use a 0.9.0 or later manifest only with SDK 0.9.0 or later; the manifest that ships in the package always matches.
- The macOS local app, login items and one-shot dialogs moved into the `hraness-local-app` crate, which has no Tauri dependency. The `desktop-foundation` crate's public API is unchanged.
