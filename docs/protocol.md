# Native runner protocol v1

Any CLI language that can start a child process and read and write pipes can
use `hraness-companion`. A product sends menu snapshots; the runner returns
action IDs. The product owns authorization, state, and every action handler.
The runner runs as the same OS user. This protocol is not an OS sandbox.

The authoritative definitions are [the Rust wire schema](../src/protocol.rs),
[runner lifecycle](../src/bin/hraness-companion.rs), and the
[TypeScript validator](../sdk/src/protocol.ts). The SDK also implements
installation, detached ownership, authenticated status/stop, and optional login
startup. A direct integration must supply the lifecycle features it needs; the
wire protocol alone does not provide them. See [architecture](architecture.md),
[adoption](adoption.md), and [installation](installation.md).

## Start and keep the owner alive

Launch the admitted native executable with these separate argv elements:

```text
hraness-companion --state-dir ABSOLUTE_PRIVATE_STATE_DIRECTORY
```

On Windows the executable is `hraness-companion.exe`. Use direct process
creation with pipes for stdin, stdout, and stderr. Do not interpolate product
labels or action IDs into a shell command. Verify the pinned executable before
launch; the protocol does not download or verify its own executable.

Send the initial snapshot immediately, flush stdin, and keep that pipe open.
The runner reads the first snapshot before acquiring its lock or creating UI.
It waits for input if no complete line arrives. Set your own startup deadline
and observe process launch failures and exit as well as protocol events.

The CLI owner must remain alive to provide snapshots and handle actions. To
return control to the invoking terminal, start a background product owner that
retains these pipes, as the SDK does. Closing stdin after a one-off write ends
the companion.

## Framing

Each frame is one UTF-8 JSON object followed by LF (`\n`). CRLF is accepted.
Flush after each frame. JSON strings may contain JSON escapes, but a frame must
not contain pretty-printing newlines. Blank lines and malformed JSON are errors.
Input frames and menu items reject unknown fields.

The maximum input frame is **262,144 bytes including its line ending**. The
reader enforces this limit while reading, even without a newline. EOF after a
partial frame is an error; EOF between frames requests normal shutdown.

Stdout contains JSONL events in normal and protocol-check modes. Drain it
continuously and handle partial or multiple lines in each read. Drain stderr
separately; it is diagnostic output, not a second event stream. Do not forward
raw native diagnostics into product menus or public logs.

## Parent → runner

### Snapshot

The first normal-mode frame must be a complete snapshot. Each subsequent
snapshot replaces the entire menu; there is no patch or append operation.

```jsonl
{"version":1,"type":"snapshot","appId":"org.example.butler","name":"Example Butler","title":"Eb","tooltip":"Example Butler is ready","revision":1,"items":[{"kind":"label","label":"Ready"},{"kind":"action","id":"pause","label":"Pause","checked":false},{"kind":"submenu","label":"Account","items":[{"kind":"action","id":"account.open","label":"Open account settings"}]},{"kind":"separator"},{"kind":"quit","label":"Quit"}]}
```

| Field | Contract |
| --- | --- |
| `version` | Integer `1`. |
| `type` | String `"snapshot"`. |
| `appId` | Required stable identifier matching `^[a-z][a-z0-9.-]{0,63}$`, without consecutive dots. |
| `name` | Required nonempty text, at most 128 Unicode scalar values. Fixed for the process lifetime. |
| `title` | Required one or two ASCII letters or digits. Rendered as native text on macOS and a generated monogram icon on Windows/Linux. May change between snapshots. |
| `tooltip` | Optional nonempty text, at most 256 Unicode scalar values. Defaults to `name`; Linux does not display native tooltips. |
| `revision` | Required integer from `0` through `9007199254740991`. Each later snapshot must be strictly greater than the previous one. |
| `items` | Required array of menu items; an empty array is valid. |

Keep both `appId` and `name` identical after the first snapshot. Revisions may
skip numbers and restart when a new runner process starts. Invalid snapshots
terminate the protocol session rather than partially updating the current menu.
Rapid valid snapshots can be coalesced before rendering; intermediate revisions
are not guaranteed to become visible.

### Menu items

These examples are individual items, to be placed inside a snapshot's `items`
array. They are not independent top-level frames.

```jsonl
{"kind":"action","id":"refresh","label":"Refresh","enabled":true,"shortcut":"CmdOrCtrl+R"}
{"kind":"action","id":"pause","label":"Pause","checked":true}
{"kind":"label","label":"Last run completed"}
{"kind":"separator"}
{"kind":"submenu","label":"More","items":[{"kind":"action","id":"help","label":"Open help"}]}
{"kind":"quit","label":"Quit"}
```

| Kind | Required fields | Optional fields and behavior |
| --- | --- | --- |
| `action` | `id`, `label` | `enabled` defaults to `true`. `checked` creates a native check item. `shortcut` requests a native accelerator. |
| `label` | `label` | Inert, disabled text. |
| `separator` | None | Native menu separator. |
| `submenu` | `label`, `items` | A nested menu array. |
| `quit` | `label` | Ends this runner; does not emit a product action or stop a product daemon. |

Action IDs match `^[A-Za-z0-9._:-]{1,256}$` and must be unique across the
entire snapshot, including disabled items and nested menus. Reserve the
`foundation.` prefix: the TypeScript SDK rejects it. The native wire parser
currently accepts that prefix as an ordinary action ID; it does not grant
special runner behavior through an action item. Use `kind: "quit"` for Quit.

All labels are nonempty and at most 256 Unicode scalar values. Shortcuts are
nonempty and at most 64 Unicode scalar values; use ASCII accelerator names for
portability. The renderer drops shortcuts longer than 64 UTF-8 bytes, and native
backends may ignore unsupported combinations. Labels, names, tooltips, and
shortcuts cannot contain control characters or bidi controls U+202A–U+202E and
U+2066–U+2069.

A snapshot may contain at most **256 items total**, counting separators,
submenus, and their descendants. Menu arrays may have at most **eight levels**,
with the top-level array counted as level one. Include a Quit item if the human
should be able to stop the companion from its menu; it is not automatically
added to a supplied snapshot.

For optional values, omit the field when unused. The Rust wire parser also
accepts JSON `null` for `tooltip`, `checked`, and `shortcut`; the TypeScript
SDK deliberately requires omission instead. `enabled` must be a boolean when
present, including in direct integrations.

Checkmarks describe confirmed product state. A click does not commit a local
toggle: the renderer restores the snapshot's mark and emits an action. Perform
the operation in the product, read its resulting state, and send a new snapshot.

### Quit

```jsonl
{"version":1,"type":"quit"}
```

After startup, either this frame or stdin EOF ends the runner normally. The
initial frame in normal mode cannot be Quit. Empty stdin before the initial
snapshot exits successfully without creating UI or emitting `ready`/`stopped`.

## Runner → parent

The following examples show each event shape; they are not a single session's
required event sequence.

```jsonl
{"version":1,"type":"ready","pid":12345,"platform":"macos"}
{"version":1,"type":"action","id":"pause","revision":1}
{"version":1,"type":"already-running"}
{"version":1,"type":"error","code":"stale-revision"}
{"version":1,"type":"stopped"}
{"version":1,"type":"validated","revision":1}
```

| Event | Meaning |
| --- | --- |
| `ready` | The first product menu and tray properties were successfully applied. `platform` uses Rust OS names: `macos`, `linux`, or `windows`; `pid` identifies this child. |
| `action` | An enabled action from the indicated snapshot was selected. Carries only its original `id` and `revision`. |
| `already-running` | Another runner holds the same app/state lock. This child creates no UI and exits with status zero. It does not attach to or update the existing instance. |
| `error` | A safe machine-readable failure category. Treat the session as failed and observe exit; do not retry a product mutation. |
| `stopped` | Normal event-loop shutdown notification, sent on a best-effort basis. |
| `validated` | A snapshot passed the headless protocol checker. Emitted only in `--check-protocol` mode. |

`ready` follows the first successful product render, not the loading placeholder.
It does not prove that a panel displays the icon, that a user can open the menu,
or that OS approval succeeded on a different machine. There is no per-snapshot
render acknowledgment. Headless `validated` events provide no graphical
evidence; a normal graphical client should reject them. See
[platform qualification](platforms.md).

The native outbound queue holds at most 64 events. A closed output pipe or a
full queue ends the disposable renderer instead of blocking the UI or growing
memory indefinitely. Shutdown waits at most 250 ms for `stopped` to flush.
A crash, forced termination, OS loader failure, or broken pipe may prevent a
final event; the parent must always observe child exit independently.

### Revision-safe action handling

Retain the exact latest snapshot sent to this process. Before handling an
`action`, require its revision to equal that snapshot's revision, require the
ID to exist in that snapshot, and require the item to be enabled. Reject stale
events even if the same action ID exists in a newer menu. The native renderer
performs its own revision and enabled checks; the parent repeats them because
an event can already be in transit when it sends a newer snapshot.

Handle mutations with product authorization and finite deadlines. Serialize
conflicting actions, disable or defer repeats while an operation is unresolved,
and refresh from confirmed state afterward. An action has no success/failure
acknowledgment in this protocol. Never retry an uncertain mutation simply
because the renderer disconnects or an action callback times out.

### Error categories

Current categories include:

| Area | Codes |
| --- | --- |
| Invocation/state | `invalid-arguments`, `snapshot-required`, `unsafe-state-dir`, `state-unavailable`, `lock-unavailable` |
| Framing/version | `input-unavailable`, `invalid-frame`, `partial-frame`, `frame-too-large`, `unsupported-version` |
| Snapshot/menu | `invalid-snapshot`, `identity-changed`, `stale-revision`, `invalid-action`, `invalid-label`, `invalid-shortcut`, `menu-too-large`, `menu-too-deep` |
| Native/output | `tray-unavailable`, `render-model-failed`, `render-schedule-failed`, `render-tray-missing`, `render-menu-build-failed`, `render-menu-set-failed`, `render-title-failed`, `render-tooltip-failed`, `render-icon-failed`, `render-check-failed`, `output-unavailable` |

Codes contain no raw input, file paths, or product diagnostics. Show actionable
product guidance for recognized categories and a generic failure for an unknown
code. A fatal panic can report `internal-error` on stderr. OS blocking and
missing dynamic libraries can fail before the protocol starts; follow the
[installation guidance](installation.md) rather than waiting for an error frame.

## State directory and singleton scope

Use one stable absolute directory per product under the current user's private
application data directory. The direct runner rejects symbolic links or Windows
reparse points in this path, including macOS aliases such as `/tmp` and `/var`;
resolve known system aliases before supplying a temporary path. Do not use a
shared writable location. New Unix directories are created with mode `0700`;
existing state directories must belong to the current user and not be writable
by other users. The SDK additionally requires owner-only permissions.

The runner retains an OS file lock at `<state-dir>/<appId>.lock` for its
lifetime. It never removes that file on exit; an existing file alone does not
mean an instance is running. The OS releases the lock when the process exits.
Do not delete a lock file to recover from an apparent stale instance: doing so
can defeat singleton behavior. A second process using the same state directory
and app ID receives `already-running`. Different state directories are separate
singleton scopes, so do not choose a new temporary directory on every launch.

Linux also uses a validated `<state-dir>/<appId>.icons` directory for
AppIndicator's temporary PNGs. This isolates icons belonging to different
products. The runner checks for a display environment and a D-Bus session
before UI initialization, but the desktop must still provide a tray host.

## Maintainer probes

`hraness-companion --version` exits without UI and prints a plain-text line,
for example:

```text
hraness-companion 0.5.0 protocol/1
```

`hraness-companion --check-protocol` reads JSONL from stdin without creating
UI, a state directory, or a singleton lock. It emits `validated` with the
revision after each accepted snapshot, then `stopped` on Quit or EOF. It uses
the same snapshot, identity, revision, and menu validation as the renderer.
Unlike normal mode, an empty stream or an initial Quit is valid for this probe.
Do not combine either probe flag with `--state-dir`.

For example, feed these complete lines to `--check-protocol`:

```jsonl
{"version":1,"type":"snapshot","appId":"org.example.butler","name":"Example Butler","title":"Eb","revision":1,"items":[{"kind":"label","label":"Ready"},{"kind":"quit","label":"Quit"}]}
{"version":1,"type":"snapshot","appId":"org.example.butler","name":"Example Butler","title":"Eb","revision":2,"items":[{"kind":"label","label":"Updated"},{"kind":"quit","label":"Quit"}]}
{"version":1,"type":"quit"}
```

The expected event types are `validated` for revisions 1 and 2, followed by
`stopped`. No `ready` event is expected.

## Credential prompt (one-shot mode)

`hraness-companion --prompt` renders exactly one native text-entry dialog —
for example, collecting a product credential — then exits. It needs no state
directory, singleton lock, or tray, so it also works on hosts that cannot show
a menu-bar item. The SDK wraps it in `promptNative`, `promptCapability`, and
the TTY fallback `promptSecret`/`promptTui`.

```text
hraness-companion --prompt
```

Write one `prompt-request` frame to stdin and read one `prompt-result` frame
from stdout:

```jsonl
{"type":"prompt-request","version":1,"title":"Example Butler","message":"Paste the relay token","secret":true,"prefill":"existing","timeoutSeconds":120}
{"type":"prompt-result","version":1,"status":"submitted","value":"existing"}
```

| Field | Contract |
| --- | --- |
| `type` | String `"prompt-request"`. |
| `version` | Integer `1`. |
| `title` | Required nonempty display text, at most 128 Unicode scalar values. |
| `message` | Required nonempty display text, at most 512 Unicode scalar values. |
| `secret` | Optional boolean, default `true`. Masks the field when set. |
| `prefill` | Optional existing value, at most 4,096 Unicode scalar values, for editing flows. |
| `timeoutSeconds` | Optional integer `1`–`600`, default `120`. The dialog dismisses itself at this deadline. |

`title` and `message` reject control characters and bidi controls
U+202A–U+202E and U+2066–U+2069. The submitted value is bounded to 4,096
Unicode scalar values. Only the first complete frame is read; trailing bytes
after its newline are `trailing-input`, and EOF before a frame is
`prompt-required` or `partial-frame`. `invalid-prompt` covers malformed,
oversized, or unsafe requests; existing framing codes cover the rest.

| Result `status` | Meaning |
| --- | --- |
| `submitted` | The user confirmed; `value` carries the entry. |
| `cancelled` | The user cancelled or closed the dialog. |
| `timeout` | `timeoutSeconds` elapsed without a choice; no `value`. |
| `unavailable` | This host cannot render a dialog; no `value`. Fall back to a TUI prompt. |

Secret hygiene: request input travels only over stdin — never argv, and never
in diagnostics, which redact `prefill` and `value`. Keep product credentials
out of `title` and `message` as well; those strings are display-rendered and
validated, not confidential. The runner only collects and returns the value;
storage, authorization, and use remain the product's responsibility.

### Capability probe

`hraness-companion --prompt-probe` prints one frame without showing a dialog:

```jsonl
{"type":"prompt-capability","version":1,"capable":true,"detail":"macos-alert"}
```

`capable` reports whether the host can currently render a dialog — a GUI
session on macOS, an input desktop on Windows, GTK on Linux — and `detail`
names the backend or the reason it is unavailable. Gate interactive prompt
steps on it in CI, and use it to choose between the native dialog and the TUI
fallback in product flows. An `unavailable` prompt result remains the
authoritative answer whenever the two disagree.

## Wire surface versus the Rust host API

Protocol v1 exposes actions, checkmarks, shortcuts, inert labels, separators,
submenus, and Quit. Its icon is generated from `title`. It has no frames for
arbitrary images, output directories, file previews, badges, progress,
accessibility metadata, radio groups, webviews, shell commands, or networking.

The existing [Rust `Host`/`MenuModel` API](../src/lib.rs) has richer presentation
types and an [outputs-directory helper](../src/outputs.rs). Those are library
APIs for native consumers, not additional JSON fields accepted by the generic
runner. Direct language integrations use the wire subset and open richer
product interfaces from their own action handlers. Add a versioned, documented
protocol feature before depending on a richer renderer capability.
