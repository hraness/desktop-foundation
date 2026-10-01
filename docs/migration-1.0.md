# Migrating from 0.9 to 1.0

1.0 removes the menu bar. Everything a product needs to run without one
shipped in 0.9 (`./registry`, `./control`, `./human-gate`, `./tui`,
`./login`, `./retire`, `./helper` and `hraness-helper`), so the upgrade is
mostly deletion. Every product pins an exact release, so nothing changes
until you bump the pin.

## Before you bump

Your product should already have, on 0.9:

1. `status --json`, `tui` and a verb for every former menu action.
2. Its state in one owner (`control serve`), reached through `agentRequest`
   or `adminRequest`.
3. A call to `retireLegacyLoginItem` so the old menu bar login item no
   longer starts at login.

Then look for the common uses of what 1.0 removed:

```sh
git grep -nE "desktop-foundation/menu-kit|[Cc]ompanion[A-Z]|Companion|MenuModel|MenuItem|TrayIcon|runnerProtocols|Snapshot|RunnerProtocols|parseRunnerEvent|assertAppId|openBrowser|permissionMenuItems|menuLoginItem|lintMenu|desktop_foundation::|permission_menu_items|PermissionMenuRow|Surface::Menu" -- ':!*.lock'
```

Review each hit against the table below. The grep is a first pass, not
proof: after bumping the pin, `tsc` and `cargo build` report every removed
name that is still in use.

## What was removed

| Removed | Use instead |
|---|---|
| The tray: `hraness-companion --state-dir <dir>`. The alias also refuses the argv around it: no arguments and malformed `--state-dir` or `--check-protocol` argv (0.x answered those with `invalid-arguments`), and `--foreground` (the flag product CLIs passed to their menu bar command) | `<product> tui` for a person, `<product> status --json` for a script |
| `hraness-companion --check-protocol` and the menu snapshot protocol (v1 and v2 frames, `docs/protocol-v2.md`) | Nothing; there is no menu to check |
| `./menu-kit` and the same names at the package root (`layout`, `lintMenu`, `assertMenuFixture`, `renderMenuTree`, `openAtLoginItem`, `degradedMenu`, `actionErrorItem`, `MenuActionError`) | `./tui` views and `status --json`. build-governance 0.5 drops the menu fixture check |
| The `companion` CLI (`companion doctor`, `companion lint-menu`) | `<product> doctor` and `resolveHelper` for the helper |
| Root exports for the companion lifecycle: `runCompanion`, `startCompanion`, `stopCompanion`, `companionStatus`, `handleCompanionCommand`, `describeCompanionError`, `serveCompanion`, `companionHelp`, `menuLoginItem`, `CompanionOptions` and related types and helpers | `./control` (`ensureOwner`, `controlStatus`, `adminRequest`) and `./login` |
| Root exports for menu snapshots: `validateSnapshot`, `validateSnapshotV2`, `downlevelSnapshot`, `parseRunnerProtocols`, `runnerProtocols`, `parseRunnerEvent`, `assertAppId`, `PROTOCOL_VERSION`, `ACTION_ERROR_MS` and the `MenuModel`, `MenuItem`, `Snapshot` and `TrayIcon` types, and related helpers, constants and `*V2` and `Runner*` types (`MAX_FRAME_BYTES` stays, for the notice and prompt frames) | Nothing |
| `openBrowser` and the Rust `browser` module | The product's own launcher, or print the URL |
| `permissionMenuItems` (TypeScript), `permission_menu_items` and `PermissionMenuRow` (Rust), and the menu surface in permission copy (`Surface::Menu` in Rust) | `renderPrePrompt`, `renderRecovery` and `permissionErrorJson` |
| The Rust crate's `MenuModel`, `MenuNode`, `Host`, `run`, `outputs`, `protocol`, `protocol_v2` and symbol table, the `Options`, `DispatchOutcome` and `RefreshHandle` types, and the Tauri, WebKitGTK and AppIndicator dependencies | `hraness-control-kit` with the `tui` feature |

Still exported, unchanged: `packagedManifest`, `parseReleaseManifest`,
`ensureBinary`, `inspectBinary`, `userPaths`, `diagnosePlatform`, the
prompt calls, the notice and permission-kind types, `./permissions`,
`./audience`, `./cli-style` and every other 0.9 subpath. The Rust crate
re-exports `identity`, `notice`, `prompt`, `service`, `audience` and
`permissions` from `hraness-local-app` and `hraness-cli-kit`.

## The `hraness-companion` alias

`hraness-companion` stays so a local app assembled before 1.0, or a product
that still names the companion asset, keeps working. For `--version`,
`--prompt-probe`, `--assemble-app`, `--signing-identity`, `--launch`,
`--notice` and `--prompt` it prints the same bytes and exits with the same
status as `hraness-helper` (only `--version` names the binary). Ghostget's
Safe Storage cookie reader, which assembles `Ghostget.app` from the resolved
helper with `HRANESS_LOCAL_APP=1`, needs no change.

Asked for the old menu bar, the alias draws nothing, reads no input and
exits **2**:

```text
$ hraness-companion --state-dir ~/Library/Application\ Support/example
{"type":"error","version":1,"code":"tray-removed"}          (stdout)
hraness-companion: the menu bar was removed in desktop-foundation 1.0. Run `<product> tui` to watch a product, or `<product> status --json` from a script. See docs/migration-1.0.md.   (stderr)
```

Exit 2 is the usage code of the control contract, and no 0.x runner exited
2, so a supervisor can tell this apart from a crash. A login item that
still starts the old tray fails the same way on every login until the
product retires it; `retireLegacyLoginItem` moves it aside.

Prefer `hraness-helper` in new code. `resolveHelper` selects that executable.

## Release assets

The 1.0 release keeps both executables for every target
(`hraness-helper-<target>` and `hraness-companion-<target>`), the SDK
package, `release-manifest.json` (`assets` for the alias and `helperAssets`
for the helper, as in 0.9) and `SHA256SUMS`. The manifest format is
unchanged, so an SDK 0.9 can still read a 1.0 manifest. Every asset carries
a GitHub build attestation.

## Rolling back

Re-pin to `v0.9.0`. It restores the tray runner, `./menu-kit` and the
companion lifecycle calls. Login items moved aside by `retireLegacyLoginItem`
are renamed to `*.retired-<timestamp>` and can be renamed back; nothing was
deleted.
