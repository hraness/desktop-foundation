# Helper protocol

`hraness-helper` is a small native executable that runs one task and exits:
a dialog, a credential prompt, a capability probe, or the macOS local-app
steps. Any CLI language that can start a child process and read and write
pipes can use it. The product owns authorization, state and every decision;
the helper runs as the same OS user and is not a sandbox.

From 1.0, `hraness-companion` is an alias of `hraness-helper`. Every mode
below gives the same stdout, stderr and exit status under either name
(`contract/helper-argv.v0.8.1.json`, checked by `tests/helper_argv.rs`); only
`--version` prints the name of the binary that ran. The 0.x menu bar is gone:
see [Removed tray modes](#removed-tray-modes) and the
[migration guide](migration-1.0.md).

The authoritative definitions are
[the mode dispatcher](../crates/hraness-local-app/src/helper.rs),
[the wire types](../crates/hraness-local-app/src/wire.rs) and the
[TypeScript types](../sdk/src/notice.ts). The SDK wraps the modes in
`promptSecret`, `promptNative`, `promptCapability`, `resolveHelper` and the
permission helpers.

| Mode | What it does | Documented in |
| --- | --- | --- |
| `--version` | Prints one line and exits 0. | [below](#version) |
| `--prompt` | One text-entry dialog. | [below](#credential-prompt-one-shot-mode) |
| `--prompt-probe` | Reports whether a dialog can be shown. | [below](#capability-probe) |
| `--notice` | One alert with product buttons. | [below](#notice-dialog-one-shot-mode) |
| `--assemble-app`, `--signing-identity`, `--launch` | The macOS local app. | [identity.md](identity.md) |

Any other argv prints `{"type":"error","version":1,"code":"invalid-arguments"}`
on stdout and exits 1.

## Framing

Each mode that reads input reads exactly one JSON frame, a single line ending
in `\n`, from stdin, and writes exactly one frame to stdout. Every frame has
`"version":1` and a `type`. A failure is one error frame and exit status 1:

```json
{"type":"error","version":1,"code":"invalid-prompt"}
```

Codes contain no raw input, file paths or product diagnostics. Show product
guidance for the codes you recognize and a generic failure for any other. A
panic reports `internal-error` on stderr. OS blocking and missing dynamic
libraries can fail before the helper starts; follow the
[installation guidance](installation.md) rather than waiting for a frame.

## Version

```text
hraness-helper 1.0.0 protocol/1,2
hraness-companion 1.0.0 protocol/1,2
```

The `protocol/1,2` suffix keeps the 0.8 line shape so existing version
parsers keep working. It no longer names a menu protocol.

## Credential prompt (one-shot mode)

`hraness-helper --prompt` renders exactly one native text-entry dialog —
for example, collecting a product credential — then exits. It needs no state
directory or lock. The SDK wraps it in `promptNative`, `promptCapability`, and
the TTY fallback `promptSecret`/`promptTui`.

```text
hraness-helper --prompt
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
validated, not confidential. The helper only collects and returns the value;
storage, authorization, and use remain the product's responsibility.

### Capability probe

`hraness-helper --prompt-probe` prints one frame without showing a dialog:

```jsonl
{"type":"prompt-capability","version":1,"capable":true,"detail":"macos-alert"}
```

`capable` reports whether the host can currently render a dialog — a GUI
session on macOS, an input desktop on Windows, GTK on Linux — and `detail`
names the backend or the reason it is unavailable. Gate interactive prompt
steps on it in CI, and use it to choose between the native dialog and the TUI
fallback in product flows. An `unavailable` prompt result remains the
authoritative answer whenever the two disagree.

## Notice dialog (one-shot mode)

`hraness-helper --notice` shows one native alert with product-supplied
buttons and exits. Like `--prompt`, it needs no state directory or lock,
and it reads one frame from stdin and writes one frame to stdout.

```jsonl
{"type":"notice-request","version":1,"title":"Textbutler needs access to Messages","message":"macOS will ask to let Textbutler control Messages. Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.","primary":"Continue","secondary":"Not now","timeoutSeconds":120}
{"type":"notice-result","version":1,"status":"primary"}
```

| Field | Contract |
| --- | --- |
| `title` | Required, 1 to 128 scalar values. |
| `message` | Required, 1 to 512 scalar values. |
| `primary` | Required button label, 1 to 32 scalar values. The default button. |
| `secondary` | Optional button label, 1 to 32 scalar values. The cancel button (Escape). |
| `settings` | Optional [permission kind](permissions.md#permission-kinds) that has a Settings URL. When the person chooses the primary button, the helper also opens that allowlisted pane itself and returns `settings` instead of `primary`. No extra button is added. |
| `timeoutSeconds` | Optional integer 1 to 600, default 120. |

Result `status` is `primary`, `secondary`, `settings` (primary chosen and the
pane opened), `timeout` or `unavailable` (no GUI session; fall back to the CLI copy). Error codes:
`invalid-notice` (including a `secondary` equal to `primary`, or a `settings`
kind without a pane) and `notice-required` (empty stdin). If the pane fails
to open, the status stays `primary` so the product shows its own recovery
copy. Inside the [local app](identity.md) the alert shows the
product's name and icon; before that it shows the helper's generic icon.

Settings panes open on macOS only. On Windows the notice is a task dialog and
on Linux a GTK message dialog, both with the product's button labels; there a
`settings` notice returns `primary`. Closing a notice that has no `secondary`
counts as `primary`.

## Removed tray modes

Until 0.9, `hraness-companion --state-dir <dir>` ran the menu bar and
`hraness-companion --check-protocol` checked it. From 1.0 the alias refuses
any argv starting `--state-dir`, `--check-protocol` or `--foreground`, and
no arguments at all; it draws nothing and reads nothing for these argv. It prints

```json
{"type":"error","version":1,"code":"tray-removed"}
```

on stdout, one line naming `<product> tui` and `<product> status --json` on
stderr, and exits **2**. Exit 2 is the usage exit code of the
[control envelope](control.md#error-codes-and-exit-codes), and no 0.x runner
ever exited 2, so a supervisor can tell this refusal apart from a crash.
`hraness-helper` answers the same argv with `invalid-arguments` and exit 1,
as it did in 0.9. The exact argv list and bytes are in
`contract/companion-alias.v1.json`.
