# desktop-foundation

Shared pieces that let a Hraness CLI product run in the background with no
window and no menu bar, on macOS, Windows and Linux: one owner process per
product, short-lived commands that print JSON, a human gate for decisions a
person owns, terminal views, and one small native helper for the few things
a terminal cannot do.

For products upgrading from a menu bar integration, follow the
[migration guide](docs/migration-1.0.md).

Releases ship raw executables, without an `.app` download, DMG, MSI or
AppImage. Developer ID signing and Apple notarization are prepared for Mac
helper and companion releases starting with 1.1.3; the current 1.1.2 assets
retain their original signatures. Installing a release needs no Apple
Developer account. The operating system's trust controls still apply. See the
[architecture](docs/architecture.md), [control kit](docs/control.md),
[human gate](docs/human-gate.md), [helper protocol](docs/protocol.md),
[platform contract](docs/platforms.md), [installation guide](docs/installation.md),
[product adoption guide](docs/adoption.md) and
[agent instructions](skills/companion/SKILL.md).

## Install

```sh
npm install https://github.com/hraness/desktop-foundation/releases/download/v1.1.2/hraness-desktop-foundation-1.1.2.tgz
```

The package is published only as GitHub Release assets, not on npm. Check
the [Releases page](https://github.com/hraness/desktop-foundation/releases)
for newer versions. Rust products pin the same tag:

```toml
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v1.1.2" }
hraness-control-kit = { git = "https://github.com/hraness/desktop-foundation", tag = "v1.1.2", features = ["tui"] }
```

## Verify the TypeScript SDK

Use Node.js 22 or later. After installing, run this in your terminal:

```sh
node --input-type=module -e 'import { detectAudience } from "@hraness/desktop-foundation/audience"; console.log(detectAudience({ env: {}, stderrIsTTY: false }));'
```

The result is `quiet`. This checks that Node can load the installed SDK without
downloading a helper, showing a dialog, registering login startup, or creating
a signing identity. It does not check native helper installation or OS trust.

Choose your next task:

- [Adopt the control kit](docs/adoption.md) to connect your product's commands and owner process.
- [Install and diagnose the helper](docs/installation.md) when you need native dialogs or macOS local app identity.
- [Understand human decisions](docs/human-gate.md) before exposing a decision command. Keep the product's permission checks; a terminal challenge is not protection against malicious code running as the same user.

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
| `--assemble-app`, `--signing-identity`, `--launch` | A local macOS app that gives dialogs and login startup a stable product identity. |
| `--version` | `hraness-helper 1.1.2 protocol/1,2`. |

The SDK downloads it on first use, checks its size and SHA-256 against the
manifest in the package, and caches it:

```js
import { packagedManifest } from '@hraness/desktop-foundation';
import { resolveHelper } from '@hraness/desktop-foundation/helper';

const helper = await resolveHelper({ manifest: await packagedManifest() });
```

`hraness-companion` is an alias kept for products and local apps built before
1.0. It gives the same output and exit status as `hraness-helper` for every
mode above; only `--version` prints its own name. Given the old menu bar argv
(`--state-dir`, `--check-protocol` or `--foreground`) or no arguments, it
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
