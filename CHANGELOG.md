# Changelog

## 1.1.0 - 2026-09-29

Command-line errors and help from `./registry` now follow `CLI_MENU_STYLE.md`, agents get JSON without asking for it, and an error can name the macOS permission behind it. Exit codes, schema ids, error codes, the envelope shape and the order of `next` are unchanged. Some message text did change, and every change is listed below.

### Changes

- A person at a terminal sees a text error as two lines on stderr: `✗` and one sentence, then `→` and the one command to run next (`FAIL` and `->` without UTF-8). A mistyped command adds `Did you mean "status"?`, and `--debug` or `HRANESS_DEBUG=1` adds the error code and detail. `runCli` takes `--debug` (before any `--`) out of the command line, unless a verb declares its own `debug` flag.
- Help starts with `Usage:`. It says in plain words when a person has to decide, instead of the `[decide T1T2]` tags. `<product> help <command>` works like `<command> --help` when `help` is the first word (after `--json` at most); a `help` that is a flag's value or comes after `--` is left alone. `help help` prints the root help, and `help commands --json` describes `commands`. User-facing text no longer says "gate".
- When `detectAudience` reports an agent, `runCli` prints the JSON envelope even without `--json`. Raw verbs still see only the `--json` the caller passed.
- **An agent that runs a `decide` verb without `--json` now gets `human-required` (exit 3) and no prompt.** In 1.0 only `--json` did that, and an agent without it was prompted at `/dev/tty`. A person who wants to decide runs the command at their own terminal, as the `next` step says.
- A wrong or expired code (`gate-failed`, `gate-expired`) now has one `next` step, the same command again, so the `→` line re-runs it. In 1.0 these had no `next`.
- A single-dash option such as `-x`, before or after the command, is a usage error (exit 2), and the command does not run. In 1.0 it reached the verb as a word. After `--`, and for a bare `-` or `-5`, it is still a word.
- `error.permission` is new and optional: `{ kind, settingsUrl }`, the macOS permission behind the failure and the System Settings pane that fixes it.
  - It is in `contract/envelope.schema.json`, the TypeScript `ErrorBody` and `HranessError`, and the Rust `ErrorBody`.
  - New helpers: `errorPermission(kind, settingsUrl)` and `validPermissionKind` in TypeScript, and `ErrorPermission::new(kind).with_settings_url(url)`, `ErrorBody::with_permission`, `valid_permission_kind` and `SETTINGS_URLS` in Rust. Both kits keep only the twelve known System Settings panes (`contract/names.json` `settingsUrls`) and leave `permission` out for a kind outside `^[a-z][a-z0-9-]*$`; `contract/golden/error-permission-cases.json` checks that they print the same bytes. `errorPermission` returns `undefined` for such a kind. The Rust reader rejects a `permission` whose kind or link the schema rejects.
  - `permissionErrorJson` keeps its 1.0 shape, including `settingsUrl: null` for a kind with no pane. Its `error.permission` holds the same kind and link as `errorPermission` and passes the same schema.
- **Scripts (the quiet audience: no terminal, no agent)** get what 1.0 gave them. A failure still prints the error envelope on stdout. The stderr line carries `code: message` after a plain `FAIL `, in ASCII whatever the locale: `FAIL usage: Unknown command "x".` then `-> example --help`. A grep anchored at the line start (`^human-required:`) must allow for the `FAIL ` prefix. The 1.0 `  next:` lines are gone.
- **`--json` output** has the same shape, with these message changes:
  - An unknown command reads `Unknown command "x".` instead of `Unknown command: x.`, adds `Did you mean …?` when there is a close match, and has two `next` steps (agent first, as before, then `<product> --help` for a person).
  - `Name a command after "approvals".` is new for a group name typed with no command after it.
  - Unknown options read `Unknown option "--x" for "example status", so nothing ran.`, and a malformed one reads `Unknown option "--Bad", so nothing ran.`
  - `Put options after "example status".` uses quotes instead of backticks.
  - Messages that ended with "… . Nothing changed." now end with ", so nothing changed." This covers `human-required`, `gate-failed`, `gate-expired` and T3 `unsupported-platform`, in TypeScript and Rust. The T3 message now reads `Deciding with your macOS login is not supported yet, so nothing changed.`
  - An undeclared code reads `The command failed with an error it did not declare.`, with `Undeclared code <code>.` in `detail`, instead of 1.0's `The verb answered an undeclared code <code>.` A person sees the code only with `--debug` or `HRANESS_DEBUG=1`.
  - `help commands --json` lists one descriptor for `commands` in `data.verbs`.
  - Match on `error.code`, never on message text.
- **`commands` text output** is a `Usage:` line and a `Commands` list without class tags. It is no longer one bare line per verb. Scripts should read `commands --json`.
- **1.0 readers reject `error.permission`**. The 1.0 schema's `error` has `additionalProperties: false`, and the 1.0 Rust `ErrorBody` denies unknown fields. An envelope without it is byte-identical to 1.0. Set it only when every reader is on 1.1.
- **Rust `ErrorBody` has a new public field**, `permission`. Code that builds `ErrorBody` with a struct literal, or destructures it without `..`, must add the field or use `ErrorBody::new`. No Hraness repository does either.

## 1.0.0 - 2026-09-29

The menu bar is gone. Products run through the owner process, `status --json`, `tui` and the verbs that 0.9 added; `hraness-helper` handles the few things a terminal cannot. Every product already moved off the tray, and each pins an exact release, so nothing changes until a product bumps. See `docs/migration-1.0.md`.

### Changes

- Removed the tray runner. `hraness-companion` is now an alias of `hraness-helper`: for `--version`, `--prompt-probe`, `--assemble-app`, `--signing-identity`, `--launch`, `--notice` and `--prompt` it prints the same bytes and exits with the same status (only `--version` names the binary), checked against `contract/helper-argv.v0.8.1.json` and the new `contract/companion-alias.v1.json`. Local apps assembled from the companion, including Ghostget's Safe Storage cookie reader, keep working.
- Given the old menu bar argv (`--state-dir`, `--check-protocol` or `--foreground`) or no arguments, the alias draws nothing and exits 2 with a `tray-removed` error that points to `<product> tui` and `<product> status --json`.
- Removed `./menu-kit`, the `companion` CLI, the companion lifecycle calls (`runCompanion`, `startCompanion`, `stopCompanion`, `companionStatus`, `handleCompanionCommand`), the menu snapshot protocol and its validators, `openBrowser` and `permissionMenuItems`. The Rust crate drops `MenuModel`, `Host`, `run`, `outputs` and the Tauri, WebKitGTK and AppIndicator dependencies, and re-exports `hraness-local-app` and `hraness-cli-kit`.
- `diagnosePlatform` on Linux reports only what the helper needs: GTK 3, and a display for dialogs (`graphical_session_missing` is now a warning, since prompts fall back to the terminal). The `session_bus_missing` and `tray_host_unverified` codes are gone.
- The login-item notice now says the product starts in the background and shows no window or icon, instead of describing a menu bar icon.
- `resolveHelper` and the prompt calls look for `hraness-helper` first. The release manifest keeps both `assets` (the alias) and `helperAssets`, so an SDK 0.9 can still read it.

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
