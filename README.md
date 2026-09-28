# desktop-foundation

Give a CLI tool a menu bar icon on macOS, a notification-area tray icon on
Windows, and an AppIndicator tray icon on Linux.

One prebuilt `hraness-companion` executable draws the menu for any product. The
TypeScript SDK downloads and verifies it, launches it, finds an instance that
is already running, reports status, stops it, and can start it at login. Your
CLI supplies the menu contents and handles the clicks; larger screens open in
the user's browser. There is no `.app`, DMG, MSI, AppImage, publisher
certificate, Apple Developer account, or notarization step. The companion is
still a native executable, so the operating system's trust controls still
apply to it.

Each release builds arm64 and x64 executables for all three systems, and the
tray runs on each of them. Features that depend on Apple Messages, Contacts,
or a macOS capture helper work only on macOS. See the [platform
contract](docs/platforms.md), [installation and OS approval
guide](docs/installation.md), [architecture](docs/architecture.md),
[product adoption guide](docs/adoption.md), [language-neutral protocol](docs/protocol.md),
and [agent instructions](skills/companion/SKILL.md).

## JavaScript / TypeScript products

The versioned SDK archive includes the exact native asset manifest. First
start fetches and verifies the matching executable; later launches use the
checked local cache. Users need no Cargo, Swift or Xcode. A source checkout
has no release manifest until all six binary artifacts have been assembled.

```sh
npm install https://github.com/hraness/desktop-foundation/releases/download/v0.8.1/hraness-desktop-foundation-0.8.1.tgz
```

The package is published only as GitHub Release assets, not on npm. Check the
[Releases page](https://github.com/hraness/desktop-foundation/releases) for
newer versions.

Mount the handler inside the product's `menubar` command. Supply the absolute
command that re-enters that same CLI with `--foreground`:

```js
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { handleCompanionCommand, userPaths, openBrowser } from '@hraness/desktop-foundation';

await handleCompanionCommand({
  appId: 'org.example.mytool', name: 'My Tool', title: 'Mt',
  stateDir: join(userPaths().dataDir, 'org.example.mytool'),
  snapshot: async signal => [
    { kind: 'label', label: 'Ready' },
    { kind: 'action', id: 'dashboard', label: 'Open dashboard' },
    { kind: 'quit', label: 'Quit My Tool' },
  ],
  onAction: async (id, signal) => {
    if (id === 'dashboard') await openBrowser('https://example.com/dashboard');
  },
}, {
  args: process.argv.slice(2),
  foreground: {
    executable: process.execPath,
    args: [fileURLToPath(import.meta.url), '--foreground'],
  },
});
```

The default command returns after startup is confirmed. The companion remains
until Quit or `menubar stop`; another start reports the existing instance.
`install` opts into next-login startup, `uninstall` removes that entry,
`status` checks the authenticated owner, and `doctor` checks the platform and
the helper. Uninstall preserves the running companion and shared cache; use
`stop` separately.

People at a terminal get plain lines (`✓ My Tool is in your menu bar.`, one
`Next:` hint), and the first start shows one `↻ Downloading the menu bar
helper (1.8 MB)…` line. `--json`, or an agent audience from `detectAudience`,
prints the JSON result instead, through `write` when the product passes one.
Pass `command: 'mytool menubar'` so help and next steps name the real command.
`install` prints the login item notice first.

A login item starts without the terminal's environment. List the variables
the menu bar needs in `loginEnv` (for example `['MYTOOL_API_TOKEN']`):
`install` saves their current values to a private file in `stateDir`, and the
login-started process fills in any that are unset. `uninstall` deletes the
file. Values never go into the login item, argv, menus or logs.

On macOS, a product that has built its local app (see
[docs/identity.md](docs/identity.md)) passes
`app: { name: 'My Tool', argvFile: join(stateDir, 'launch.json') }` to
`handleCompanionCommand`. `install` then writes the same LaunchAgent as the
Rust helper: it starts `My Tool.app --launch <argvFile>`, so Login Items and
the notice name My Tool instead of `node` or `bun`. The product command goes
in the owner-only `argvFile` (at most 64 entries and 64 KiB, or `install`
refuses it), and the older `app.hraness.companion.<appId>` entry is removed in
the same command. If the app isn't built, `install` stops with `app_missing`
before showing the notice. Without `app`, nothing changes. The menu's "Open at
login" row uses the same app, even when the product passes its own
`loginItem` without one, so the row and `install` never add two items.

Reads and actions receive cancellation signals. Product handlers preserve
their permission checks and reconcile uncertain outcomes; the SDK never
retries a mutation. The native renderer receives menu data and returns action
IDs. It cannot request shell commands or browse product files. This is a
protocol boundary, not an OS sandbox.

The SDK also exports `runCompanion` for foreground owners and `planAutostart`
for inspectable startup plans. `companion` runs a demonstration tray;
`companion doctor --json` is network-silent.

To collect a credential, such as a new password or an edited existing value,
use `promptSecret(request)`. It shows a native dialog through the pinned runner
when the host supports one, and a masked TTY prompt otherwise. `prefill`
carries an existing value for editing and `timeoutSeconds` bounds the wait.
Values travel over the child's stdin/stdout only; storage stays with the
product. See [the prompt contract](docs/protocol.md#credential-prompt-one-shot-mode).

### Permission notices and CLI output

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
recording and local signing. `permissionMenuItems` returns the matching menu
rows, and `permissionErrorJson` the `--json` error. See
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

### Menu kit v2: layout and lint

`@hraness/desktop-foundation/menu-kit` builds menus in the shared layout and
checks them against the menu rules in [protocol v2](docs/protocol-v2.md#menu-lint):

```js
import { layout, assertMenuFixture } from '@hraness/desktop-foundation/menu-kit';

const items = layout({
  name: 'My Tool',
  status: { kind: 'status', symbol: 'status.running', label: 'Running', detail: '3 jobs today' },
  primary: { kind: 'action', id: 'dashboard', label: 'Open dashboard', symbol: 'action.open', opens: 'browser' },
  openAtLogin: true,
  help: { kind: 'action', id: 'help', label: 'Help & support', symbol: 'action.support', opens: 'browser' },
});
```

`layout` adds the header, separators, the foundation's "Open at login" row and
`Quit My Tool`. In a product test, `assertMenuFixture(snapshot)` validates a
state fixture, lints it strictly and throws with the rendered tree when a rule
fails. `renderMenuTree` prints that tree for pull requests, and
`companion lint-menu --strict fixtures/*.json` runs the same check in CI.
`validateSnapshotV2` and `downlevelSnapshot` are exported for tests.

To send v2 menus, pass `mark` (for example `{ symbol: 'mark.chat', letters: 'Tb' }`)
with the other companion options and return v2 items from `snapshot`. The SDK
speaks v2 only to a runner that reports `protocol/…2` and down-levels to v1
text rows otherwise, so one product build works with both. `snapshot` may also
return `{ items, mark, tooltip }` to change the menu-bar dot, count or tooltip
with product state. The session also:

- refreshes as soon as the product calls the `refresh` it receives in
  `subscribe(refresh, signal)`, instead of waiting for the timer;
- replaces a menu it can no longer read with "Can't reach {name} · Retrying…"
  (`degraded` picks the action that still works) and restores it on the next
  good read;
- shows a failed action as a ⚠︎ row under the status rows for at least 30
  seconds. Throw `MenuActionError("Couldn't pause replies")` to choose the
  words; other errors show a generic row and never their text;
- handles `foundation.login` ("Open at login") and
  `foundation.settings.<kind>` itself. The product never sees those actions.

## Compared with Tauri, Electron, and tray libraries

| Option | Choose it when |
| --- | --- |
| [Tauri](https://v2.tauri.app/learn/system-tray/), or Electron with [menubar](https://github.com/max-mapper/menubar) | The product needs app windows and you will ship a signed app bundle. |
| [tray-icon](https://github.com/tauri-apps/tray-icon) (Rust) or [systray](https://github.com/getlantern/systray) (Go) | You want to build, sign, and distribute the tray binary yourself. |
| [SwiftBar](https://github.com/swiftbar/SwiftBar) | A script's output should appear in the macOS menu bar, with no app to write. |
| desktop-foundation | A CLI needs a menu bar or tray icon on macOS, Windows, and Linux without an app bundle, installer, or notarization. The SDK downloads and verifies one prebuilt companion; your CLI supplies the menu. |

The companion is itself a Tauri 2 app that uses Tauri's `tray-icon` feature.
This project builds and releases that app once, so each CLI does not have to.
Checked on 2026-09-28.

## Existing Rust products

A product supplies a [`Host`](src/lib.rs) implementation that renders a
`MenuModel` snapshot and answers action ids. The foundation owns the
accessory-mode application lifecycle, the status item, menu construction,
and the refresh loop. The product's daemon stays in charge of state and
permissions. The menu-bar process is a replaceable client of that daemon with
no privileges of its own.

`service` gives Rust products the same `menubar install`, `uninstall` and
`status` behavior as the SDK: one owned LaunchAgent at
`~/Library/LaunchAgents/app.hraness.<appId>.plist` (it replaces an older
`app.hraness.companion.<appId>` agent and refuses to touch a file it did not
write), an `InstanceLock` so a second launch exits with code 3, a liveness
check for `status`, and the shared copy: the login-item notice to print before
installing, results, status lines and errors, each with an ASCII fallback.
Nothing calls `launchctl`; the agent starts at the next login and
`menubar start` opens the menu now.

```rust
use desktop_foundation::service::{self, Glyphs, LoginItem, ServiceStatus};

let plan = service::plan(&item, &home)?;
eprint!("{}", service::login_item_notice(&item.name, &item.name, Glyphs::Unicode));
let change = service::install(&plan)?;
print!("{}", service::install_result(&item, change, Glyphs::Unicode));
print!("{}", ServiceStatus::read(&plan, &state_dir, &item.app_id).human(&item, Glyphs::Unicode));
```

## Rust usage

Pin by immutable tag:

```toml
[dependencies]
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v0.8.1" }
```

Implement `Host`, then run the event loop with the product's own
`tauri::generate_context!()`:

```rust
fn main() {
    let host = Arc::new(MyHost::new());
    desktop_foundation::run(
        tauri::generate_context!(),
        host,
        Options::default(),
        |builder| builder, // or add invoke handlers / managed state here
    )
    .unwrap();
}
```

The binary runs unbundled. The shared runner is the preferred integration for
new adapters so products do not each maintain a native build. OS permissions
for camera, microphone, screen recording and other privileged capabilities
remain owned by their existing product helpers; a tray does not grant them.

## Rust host contract

- `snapshot()` returns the complete status-item state: title, optional
  full-color RGBA icon, tooltip, and menu nodes. Called off the UI thread
  on a refresh interval and after accepted dispatch. One worker coalesces
  invalidations and rejects superseded results. `started_with_refresh` supplies
  a `RefreshHandle` for product events; polling remains the fallback.
- `dispatch(id)` receives a menu action id. It must not block; the host
  owns whatever thread does the work.
- `started(app)` runs once after the status item exists. Spawn sidecars and
  `manage` product state there. `cancel_snapshot()` signals cancellation
  before the worker joins; `stopping()` runs after it stops. Host reads must
  have finite deadlines. Use `snapshot_with_context` for cooperative cancellation.
- `MenuNode::quit(title)` adds a working Quit item; inert items
  (`MenuNode::disabled`) never reach `dispatch`.
- With `Options::companion_window`, a hidden `main` window becomes an
  on-demand panel: `MenuNode::show_window(title)` shows and focuses it, and
  closing the window hides rather than destroys it.
- `MenuNode::item_with_icon` renders a full-color preview icon beside the
  item title.

## Interactive items

### Open a browser from a menu item

Existing Rust adapters can keep their `Host` implementation and store a
`browser::BrowserOpener`. Call `open` only from an explicit menu dispatch with
a product-owned HTTPS URL. Construction and `status()` are network-silent;
keep the menu action available when the product daemon is offline. An accepted
request runs away from the UI thread, admits one launcher at a time, and kills
and reaps the launcher after its ten-second deadline. There is no automatic
retry. Show a generic failure label using `BrowserStatus::Failed` in the next
snapshot; `Opened` proves only a successful launcher exit.

```rust
use desktop_foundation::browser::{BrowserOpener, BrowserStatus};

let browser = BrowserOpener::new();
// Inside a human menu-action callback:
browser.open("https://example.com/support?source=desktop")?;
// A later snapshot may display an unavailable message:
let failed = matches!(browser.status(), BrowserStatus::Failed(_));
```

The SDK's existing `openBrowser` provides the same explicit handoff with a
Promise and a ten-second launcher deadline; it also preserves loopback HTTP
for existing local dashboards. Both APIs use direct arguments without a shell,
reject credentials and malformed addresses, and keep addresses out of error
messages. Fixed public product/source routing parameters are appropriate;
email, credentials, session data, and daemon-supplied URLs are not. A browser
handoff does not establish signup, payment, consent, or successful navigation.

Use a stable product command ID. The renderer gives each displayed row a separate
native ID, so several rows may open the same panel and old menu events cannot
activate a replacement row.

```rust
use desktop_foundation::{MenuItem, MenuNode};

let paused = true; // Read from the product's confirmed snapshot.
let pause = MenuNode::interactive(
    MenuItem::toggle("automation.pause", "Pause automatic replies", paused)
        .with_shortcut("CmdOrCtrl+P"),
);
let approvals = MenuNode::interactive(
    MenuItem::action("approvals.open", "Pending approvals")
        .with_badge("3")
        .with_shortcut("CmdOrCtrl+Shift+A"),
);
```

| Model feature | Current native presentation |
| --- | --- |
| Action and shortcut | Native menu command and accelerator |
| Check / toggle | Native checkmark; the next confirmed snapshot owns its value |
| Radio group | Native checkmarks with at most one selected item per group; the host implements selection |
| Action icon | RGBA image beside the label |
| Badge / progress | Bounded text in the label, such as `Pending approvals [3]` or `Export 42%` |
| Accessibility metadata | Retained for richer renderers; Tauri does not apply custom labels, values or hints. Native menu accessibility uses the visible label/state |
| Companion window | Show/focus the product's hidden `main` window and hide it on close |

The foundation does not render an anchored popover, searchable panel, slider,
custom progress bar, or notification center. Those remain product/native-host
work. Do not advertise metadata as a working native control.

Model validation bounds menu size, nesting, action IDs and icon buffers. Invalid
models leave the last usable menu in place. `render_failed` reports a safe error
category; `snapshot_result` and `snapshot_failed` provide typed read failures and
degraded views without copying raw daemon errors into labels.

`DispatchOutcome::Accepted` means the request was accepted, not completed.
`Indeterminate` requires reconciliation; the framework never retries a mutation.
The renderer restores checked state until the host supplies a confirmed value.

## Outputs sections

`outputs::OutputsSection` is a reusable building block for the common
"agent drops outputs into a directory" pattern:

```rust
let outputs = OutputsSection::new(state_dir.join("outputs"));

impl Host for MyHost {
    fn snapshot(&self) -> MenuModel {
        MenuModel { nodes: [product_nodes, self.outputs.nodes()].concat(), ..Default::default() }
    }
    fn dispatch(&self, id: &str) {
        if self.outputs.dispatch(id) { return; }
        // product actions
    }
}
```

It lists the 5 newest files (`with_limit` changes that) with the file type as a
badge, size and age in days as a subtitle and an image thumbnail, opens a file
on click, and shows it in Finder when the row is chosen with ⌥ held (a "Show in
folder" submenu on Windows and Linux). The last row opens
the folder, reading "Show all N outputs" when more files exist, and stays when
the folder is empty. Hidden entries, directories, symlinks and non-UTF-8 names
are skipped. Listing scans at most 10,000 entries; a larger or partially
unreadable directory adds "Some outputs couldn't be listed". Ties are ordered
by filename.

Actions refer only to files offered by the latest snapshot and verify size,
timestamp and (on Unix) device/inode/change-time before opening. Replaced or
removed files require a refreshed menu. This is stale-action protection, not a
sandbox for other software running as the same user. Thumbnails have separate
encoded-byte, dimension and allocation limits.

Ghostget, PeopleBlade, Slopcamera, AI Charts, Valhalla, Sponge, and Textbutler
use this package.

## Validation and releases

Run `cargo test --locked` and `cargo build --locked` on macOS. Tests use
synthetic output files and do not open Finder or connect to product daemons.

Release from the reviewed, validated tree by updating both Cargo version records
and pushing a new immutable `v*` tag. Never move existing tags. Consumers update
both their Git dependency tag and the exact Cargo lockfile commit, and they
keep their own native and source release checks.
