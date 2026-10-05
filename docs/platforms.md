# Platform contract

`desktop-foundation` gives a product CLI a control kit (one owner process,
`status --json`, a human gate) and a one-shot native helper,
`hraness-helper`. Products retain authority over accounts, daemon state,
permissions and action authorization. Product capabilities are not
automatically portable.

For products replacing a menu bar integration, see the
[migration guide](migration-1.0.md).

## What runs where

| Platform | Helper modes | Runtime constraints |
| --- | --- | --- |
| Platform | Helper modes | Runtime constraints |
| --- | --- | --- |
| macOS | The local app (`--assemble-app`, `--signing-identity`, `--launch`) | Matching Mach-O architecture; OS first-launch approval may be required. |
| Windows, Linux | `--version`; the local app modes answer an error frame | Compatible libc; matching executable target; SmartScreen or application policy may block unsigned binaries. | Native executables are distributed without `.app`, DMG, PKG, MSI,
MSIX or AppImage packaging. The release workflow is prepared to sign and
notarize Mac helper and companion executables from version 1.1.3 using a
protected tag-only environment. Ordinary builds and tests receive no
publisher credentials. Windows executables remain unsigned. OS approval
remains a separate condition; see
[installation and blocked-launch guidance](installation.md).

## Lifecycle and ownership

The product's owner process is the sole authority. Short-lived commands
(`status --json`, product verbs) reach it over the control socket; see
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

The Linux binaries link only libc and the platform C runtime — no GTK,
WebKitGTK or AppIndicator. Each release records the actual dynamic dependency
list and the build distribution alongside each Linux artifact. Build against
the oldest supported ABI baseline; do not claim a generic Linux binary runs
on every distribution or a musl-based system.

## Build evidence and desktop checks

Keep these claims separate in release notes:

| Evidence | What it proves |
| --- | --- |
| Source, contract and SDK tests | Argv, wire and lifecycle behavior exercised by those tests. |
| Native target build | The reviewed source compiles for that OS and architecture with the recorded toolchain. |
| Alias smoke | The shipped `hraness-companion` answers every one-shot argv with the same bytes as `hraness-helper`, and refuses the old menu-bar argv with exit 2. |
| Clean-install check | A fresh user environment installs the published bytes without a compiler; actual OS approval and Linux dependency behavior are recorded. |

CI compilation, simulated platform tests and probes do not establish
clean-install success. A macOS check does not qualify Linux or Windows.
Login-registration claims need a separate sign-in check.

