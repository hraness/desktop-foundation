# Changelog

## 1.1.1 - 2026-09-29

Fixes from the review of 1.1.0. Exit codes, schema ids, error codes and the envelope shape are unchanged. Some behaviour, message text and helper signatures did change, and every change is listed below.

### Changes

- **`help` is a command only as the first word** (after `--json` at most). In 1.1.0 `runCli` removed the first `help` anywhere in the command line, so `approvals decide a1 --digest help deny` lost its `--digest` value, and `-- help` printed help. Now a flag's value and every word after `--` are left alone.
- **`--debug`** works like `HRANESS_DEBUG=1`, as `CLI_MENU_STYLE.md` D5 says: a text error adds the code and detail. `runCli` takes `--debug` (before any `--`) out of the command line, unless one of the product's verbs declares its own `debug` flag, in which case it stays that verb's flag and the other verbs reject it as before. 1.1.0 answered `--debug` with an unknown-option error.
- **An undeclared code no longer shows its code to a person.** The `internal` error reads `The command failed with an error it did not declare.`, and the code moved to `detail` as `Undeclared code <code>.`, which text mode shows only with `--debug` or `HRANESS_DEBUG=1`. In 1.1.0 the message was `The command answered an undeclared code <code>.` This changes `--json` `error.message` and adds `error.detail`.
- **A wrong or expired code points at the same command.** `gate-failed` and `gate-expired` from `runCli` now have one `next` step for a person, the same command again, so the `→` line re-runs it. In 1.1.0 they had no `next`, and the `→` line pointed at `--help`. This adds `error.next` to those `--json` envelopes. T3 `unsupported-platform` still points at `--help`, since running it again cannot succeed in this release.
- **`help commands --json`** lists one descriptor for `commands` in `data.verbs` instead of an empty list.
- **`error.permission` is built the same way by both kits.** In 1.1.0 TypeScript kept only the known System Settings panes, while Rust kept any `x-apple.systempreferences:` link.
  - Both now keep only the twelve panes in `contract/names.json` `settingsUrls`. Rust exports them as `SETTINGS_URLS`.
  - Both leave `permission` out for a kind outside `^[a-z][a-z0-9-]*$` (`validPermissionKind`, `valid_permission_kind`).
  - `contract/golden/error-permission-cases.json` checks that both kits print the same bytes.
  - `errorPermission` now returns `ErrorPermission | undefined`. TypeScript code that assigns its result to an `ErrorPermission` must handle `undefined`.
  - Rust `ErrorPermission::with_settings_url` drops a link that is not one of the twelve panes, and `ErrorBody::with_permission` drops a permission with an invalid kind.
  - The Rust reader now rejects a `permission` whose kind or link the schema rejects. 1.1.0 accepted it.
  - `permissionErrorJson` keeps its 1.0 shape. Its `error.permission` is now tested to hold the same kind and link as `errorPermission` and to pass the schema.
- **Documented from 1.1.0:** an agent that runs a `decide` verb without `--json` gets `human-required` (exit 3) and no prompt. In 1.0 only `--json` did that, and an agent without it was prompted at `/dev/tty`. The 1.1.0 notes left this out. A person who wants to decide runs the command at their own terminal, as the `next` step says.

## 1.1.0 - 2026-09-29

Command-line errors and help from `./registry` now follow `CLI_MENU_STYLE.md`, agents get JSON without asking for it, and an error can name the macOS permission behind it. Exit codes, schema ids, error codes, the envelope shape and the order of `next` are unchanged. Some message text did change, and every change is listed below.

### Changes

- A person at a terminal sees a text error as two lines on stderr: `✗` and one sentence, then `→` and the one command to run next (`FAIL` and `->` without UTF-8). A mistyped command adds `Did you mean "status"?`, and `HRANESS_DEBUG=1` adds the error code.
- Help starts with `Usage:`. It says in plain words when a person has to decide, instead of the `[decide T1T2]` tags. `<product> help <command>` works like `<command> --help`, and `help help` prints the root help. User-facing text no longer says "gate".
- When `detectAudience` reports an agent, `runCli` prints the JSON envelope even without `--json`. Raw verbs still see only the `--json` the caller passed.
- A single-dash option such as `-x`, before or after the command, is a usage error (exit 2), and the command does not run. In 1.0 it reached the verb as a word. After `--`, and for a bare `-` or `-5`, it is still a word.
- `error.permission` is new and optional: `{ kind, settingsUrl }`, the macOS permission behind the failure and the System Settings pane that fixes it.
  - It is in `contract/envelope.schema.json`, the TypeScript `ErrorBody` and `HranessError`, and the Rust `ErrorBody`.
  - New helpers: `errorPermission(kind, settingsUrl)` in TypeScript, and `ErrorPermission::new(kind).with_settings_url(url)` with `ErrorBody::with_permission` in Rust. Both drop a link outside System Settings; TypeScript also keeps only the known panes, and Rust checks the `x-apple.systempreferences:` prefix.
  - `permissionErrorJson` keeps its 1.0 shape.
- **Scripts (the quiet audience: no terminal, no agent)** get what 1.0 gave them. A failure still prints the error envelope on stdout. The stderr line carries `code: message` after a plain `FAIL `, in ASCII whatever the locale: `FAIL usage: Unknown command "x".` then `-> example --help`. A grep anchored at the line start (`^human-required:`) must allow for the `FAIL ` prefix. The 1.0 `  next:` lines are gone.
- **`--json` output** has the same shape, with these message changes:
  - An unknown command reads `Unknown command "x".` instead of `Unknown command: x.`, adds `Did you mean …?` when there is a close match, and has two `next` steps (agent first, as before, then `<product> --help` for a person).
  - `Name a command after "approvals".` is new for a group name typed with no command after it.
  - Unknown options read `Unknown option "--x" for "example status", so nothing ran.`, and a malformed one reads `Unknown option "--Bad", so nothing ran.`
  - `Put options after "example status".` uses quotes instead of backticks.
  - Messages that ended with "… . Nothing changed." now end with ", so nothing changed." This covers `human-required`, `gate-failed`, `gate-expired` and T3 `unsupported-platform`, in TypeScript and Rust. The T3 message now reads `Deciding with your macOS login is not supported yet, so nothing changed.`
  - An undeclared code reads `The command answered an undeclared code …` instead of `The verb answered …`.
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
