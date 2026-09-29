# desktop-foundation

Shared pieces that let a Hraness CLI product run in the background with no
window and no menu bar, on macOS, Windows and Linux: one owner process per
product, short-lived commands that print JSON, a human gate for decisions a
person owns, terminal views, and one small native helper for the few things
a terminal cannot do.

1.0 removed the menu bar. The tray runner, `./menu-kit` and the companion
start, stop and status calls are gone; `hraness-companion` stays as an alias
of `hraness-helper`. Upgrading from 0.9? Read the
[migration guide](docs/migration-1.0.md).

There is no `.app` download, DMG, MSI, AppImage, publisher certificate,
Apple Developer account or notarization step. The helper is still a native
executable, so the operating system's trust controls apply to it. See the
[architecture](docs/architecture.md), [control kit](docs/control.md),
[human gate](docs/human-gate.md), [helper protocol](docs/protocol.md),
[platform contract](docs/platforms.md), [installation guide](docs/installation.md),
[product adoption guide](docs/adoption.md) and
[agent instructions](skills/companion/SKILL.md).

## Install

```sh
npm install https://github.com/hraness/desktop-foundation/releases/download/v1.0.0/hraness-desktop-foundation-1.0.0.tgz
```

The package is published only as GitHub Release assets, not on npm. Check
the [Releases page](https://github.com/hraness/desktop-foundation/releases)
for newer versions. Rust products pin the same tag:

```toml
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v1.0.0" }
hraness-control-kit = { git = "https://github.com/hraness/desktop-foundation", tag = "v1.0.0", features = ["tui"] }
```

## Headless control

One owner process holds a product's state, and every other command is a
short-lived client that prints JSON with `--json`.

| Subpath | What it gives a product |
|---|---|
| `./registry` | Verbs with an operation class (`read`, `operate`, `decide`, `decide-legacy`), the JSON envelope, stable error codes and exit codes, and `commands --json`. |
| `./control` | The owner process: agent and admin Unix sockets in a 0700 directory, `ensureOwner` to start it on demand, `controlStatus` that never sends a signal. |
| `./human-gate` | The gate for `decide` verbs: a foreground terminal plus a one-time code typed at `/dev/tty`. |
| `./tui` | Views that render as an interactive screen, a plain-text snapshot, or the same JSON `status --json` prints. |
| `./login` | An opt-in login item that starts the owner. |
| `./retire` | Moves a product's old menu bar login item aside, after checking it is the one the product installed. Nothing is deleted. |
| `./helper` | Finds and verifies the `hraness-helper` executable for one-shot dialogs and the macOS local app. |
| `./permissions`, `./audience`, `./cli-style` | Permission notices, who is reading, and the shared CLI output style. |

Rust products use the `hraness-control-kit` crate (with the `tui` feature for
ratatui views) and `hraness-local-app`, pinned by the same tag.

To adopt it:

1. Put every product action behind a registry verb and give each a class.
   Anything a person owns is `decide` and declares a gate.
2. Keep the product's state in one owner (`control serve`) and have the other
   commands call it through `agentRequest` or `adminRequest`.
3. Offer `status`, `tui` and the verbs. Show pending decisions in `status`
   rather than a notification.
4. On upgrade from a menu bar release, call `retireLegacyLoginItem` so the
   old menu bar no longer starts at login.

Read [docs/control.md](docs/control.md) for the wire format and
[docs/human-gate.md](docs/human-gate.md) for what the gate does and does not
protect against.

## The native helper

`hraness-helper` runs one task and exits. Each release ships it for macOS,
Windows and Linux on arm64 and x64; macOS builds are ad-hoc signed.

| Mode | What it does |
|---|---|
| `--notice` | One native alert with the product's buttons, optionally opening a Settings pane. |
| `--prompt`, `--prompt-probe` | One text-entry dialog (a credential, say), and a check that a dialog can be shown. |
| `--assemble-app`, `--signing-identity`, `--launch` | The macOS local app a product assembles and signs on the person's own Mac, so its permissions stick across updates. |
| `--version` | `hraness-helper 1.0.0 protocol/1,2`. |

The SDK downloads it on first use, checks its size and SHA-256 against the
manifest in the package, and caches it:

```js
import { packagedManifest } from '@hraness/desktop-foundation';
import { resolveHelper } from '@hraness/desktop-foundation/helper';

const helper = await resolveHelper({ manifest: await packagedManifest() });
```

`hraness-companion` is an alias kept for products and local apps built before
1.0. It gives the same output and exit status as `hraness-helper` for every
mode above; only `--version` prints its own name. Asked for the old menu bar
(no arguments, `--state-dir`, `--check-protocol` or `--foreground`), it
draws nothing and exits 2 with a `tray-removed` error that points to
`<product> tui` and `<product> status --json`. See the
[helper protocol](docs/protocol.md).

## Permission notices and CLI output

`@hraness/desktop-foundation/permissions` tells people what macOS is about to
ask before it asks, and explains a denial afterwards. It never triggers a
prompt itself and has no native dependencies, so any CLI can use it:

```js
import { MESSAGES_FDA, prePrompt, permissionStatus, reportPermissionFailure } from '@hraness/desktop-foundation/permissions';

const need = MESSAGES_FDA({ product: 'Textbutler', command: 'textbutler' });
if (await permissionStatus('full-disk-access', 'Messages') !== 'granted') {
  if (await prePrompt(need) === 'skip') process.exit(0);
}
// … on EPERM:
await reportPermissionFailure(need, 'denied');
```

A person at a terminal sees:

```text
🔐 Textbutler needs Full Disk Access to read your Messages.
   macOS doesn't ask for this. Turn on Terminal in System Settings › Privacy & Security › Full Disk Access. Only the chats you pick are read.
   Press Enter to open Settings · s to skip
```

Presets cover login items, Chrome Safe Storage, Messages, Automation,
Contacts, Local Network, the firewall, Apple's command line tools, screen
recording and local signing. `permissionErrorJson` returns the `--json`
error. See
[permission notices](docs/permissions.md).

`detectAudience()` decides whether a person, an agent or nobody is reading
(`HRANESS_AUDIENCE` wins, then exact agent markers such as `CLAUDECODE`, then
a terminal on stderr). `createCliOutput()` prints results, `✗` errors with one
`→` next step, `Next:` hints and a progress line with the shared symbols, and
respects `NO_COLOR`, `TERM=dumb` and pipes.

Rust CLIs use the std-only `hraness-cli-kit` crate for the same audience
rule, permission copy and output style. Its optional `clap` feature turns
clap's usage errors into one line with a suggestion and the help to read,
exit 2, or a JSON error on stdout for `--json` and agents:

```rust
use hraness_cli_kit::permissions::{self, presets, ProductRef, ProcessIo};

let need = presets::messages_fda(ProductRef::new("Textbutler", "textbutler"), None);
permissions::pre_prompt(&need, None, &mut ProcessIo)?;
```

```text
✗ Unknown command "stauts". Did you mean "status"?
→ textbutler --help
```

## Validation and releases

Run `npm run check` on macOS: the SDK build and tests, `cargo test --locked
--release` and a release build of both executables. Tests use private
temporary homes, fake `security` and `codesign` tools, and never touch the
real login keychain, LaunchAgents or login items.

Release from the reviewed, validated tree: bump `package.json`, the Cargo
versions (workspace and crates) and the CHANGELOG section, merge, then push
an annotated immutable `v*` tag. The tag workflow builds the six targets,
the package and `SHA256SUMS`, attests every asset and creates the GitHub
Release. Never move existing tags. Consumers update their tgz URL or Git
tag and their lockfile.
