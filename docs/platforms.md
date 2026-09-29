# Platform contract

`desktop-foundation` gives a product CLI a control kit (one owner process,
`status --json`, `tui`, a human gate) and a one-shot native helper,
`hraness-helper`. Products retain authority over accounts, daemon state,
permissions and action authorization. The helper's dialogs are portable;
product capabilities are not automatically portable.

1.0 removed the 0.x menu bar on every platform. There is no tray icon,
notification-area item or panel indicator to qualify. See
[the migration guide](migration-1.0.md).

## What runs where

| Platform | Helper modes | Runtime constraints |
| --- | --- | --- |
| macOS | Notice and prompt (`NSAlert`), and the local app (`--assemble-app`, `--signing-identity`, `--launch`) | User's graphical session for dialogs; matching Mach-O architecture; OS first-launch approval may be required. |
| Windows | Notice (task dialog) and prompt (Win32 dialog) | User's interactive desktop for dialogs; matching executable target; SmartScreen or application policy may block unsigned binaries. |
| Linux | Notice and prompt (GTK 3 dialogs) | Compatible glibc and GTK 3; a graphical session for dialogs. |

The local app modes are macOS only; on Windows and Linux they answer with an
error frame. Native executables are distributed without `.app`, DMG, PKG, MSI,
MSIX or AppImage packaging. Builds use no publisher signing credentials or
notarization submission. OS approval remains a separate condition; see
[installation and blocked-launch guidance](installation.md).

## Lifecycle and ownership

The product's owner process is the sole authority. Short-lived commands
(`status --json`, `tui`, product verbs) reach it over the control socket; see
[control](control.md). The helper holds no state, takes no lock and exits after
one request. It never carries shell commands to execute.

Each product has one owner per user, independent of other products. Optional
login registration targets the user's interactive session, never a machine
service.

The shared foundation must not absorb messaging history, account credentials,
provider sessions, browser authentication, or camera/microphone/screen-recording
permission ownership. For example, an iMessage provider remains macOS-specific
even when its product runs on Windows or Linux.

## Linux libraries

The Linux binaries link GTK 3 for the dialogs and nothing from WebKitGTK or
AppIndicator. Each release records the actual dynamic dependency list and the
build distribution alongside each Linux artifact. Build against the oldest
supported ABI baseline; do not claim a generic Linux binary runs on every
distribution or a musl-based system.

## Build evidence and desktop checks

Keep these claims separate in release notes:

| Evidence | What it proves |
| --- | --- |
| Source, contract and SDK tests | Argv, wire and lifecycle behavior exercised by those tests. |
| Native target build | The reviewed source compiles for that OS and architecture with the recorded toolchain. |
| Alias smoke | The shipped `hraness-companion` answers every one-shot argv with the same bytes as `hraness-helper`, and refuses the old menu-bar argv with exit 2. |
| Prompt smoke | The dialog capability probe and a bounded, auto-dismissed notice and prompt agree on that host. |
| Clean-install check | A fresh user environment installs the published bytes without a compiler; actual OS approval and Linux dependency behavior are recorded. |

CI compilation, simulated platform tests and probes do not establish
clean-install success. A macOS check does not qualify Linux or Windows.
Login-registration claims need a separate sign-in check.

### Dialog capability

`--prompt-probe` reports capability before showing anything: a GUI session on
macOS (`macos-alert`), an input desktop on Windows (`win32-dialog`), GTK
initialization on Linux (`gtk-dialog`). CI runs `scripts/prompt-smoke.mjs` on
every leg: a capable host must return `timeout` from the bounded auto-dismiss
dialog, an incapable host must return `unavailable`, and any other pairing
fails the leg. Products should treat an `unavailable` result as the
authoritative answer and fall back to a terminal prompt.
