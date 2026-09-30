# Install and run the helper

`hraness-helper` is a small native executable a product CLI runs for the few
things a terminal cannot do: show a notice or a secret prompt, and on macOS
assemble, sign and launch the product's local app. It opens no window of its
own and has no menu bar. Each call does one thing and exits. Distribution uses
a versioned executable rather than an app bundle or desktop installer; it does
not receive publisher signing credentials. Mac release executables starting
with 1.1.3 require Hraness Developer ID signatures; their release workflow
performs notarization before publication. This pipeline is prepared in
source; 1.1.2 and earlier artifacts retain their original signatures.
Locally assembled product apps keep the separate `Hraness Local Signing`
identity described in [product identity](identity.md).

`hraness-companion` is the 0.x name, kept as an alias. It answers every helper
mode the same way. Asked for the old menu bar it exits 2 and points to
`<product> tui` and `<product> status --json`. See
[the 1.0 migration guide](migration-1.0.md).

The executable is still downloaded software. Operating-system checks apply,
and some machines will not allow an unsigned executable. A successful download,
checksum, build, or process spawn is not proof that a dialog can appear. See
[platform support](platforms.md).

## What the person runs

The product owns its commands. With the control kit a product typically offers:

| Command | Meaning |
| --- | --- |
| `<product> start` / `stop` | Start or stop the product's one owner process for this user. |
| `<product> status --json` | The owner state as one JSON object, for scripts and agents. |
| `<product> tui` | An interactive view of the same state in the terminal. |
| `<product> login-item install` / `uninstall` | Opt in to starting the owner when this user signs in, or remove that registration. This is login registration, not an app installer. |
| `<product> doctor` | Platform, release and helper diagnostics with one actionable next step. |

Consult the product's `--help`; this is the shape the kit supports, not a claim
that every product version uses these exact names.

Normal users do not need Rust, Xcode or a repository checkout. The product pins
the supported release and platform artifact digest. Cached bytes must match
that digest before execution. A missing published artifact is a release
problem: report it without building source or choosing an unpinned download.

## macOS: first-launch approval

An unbundled command-line executable is not exempt from Gatekeeper. Launch
method and quarantine state affect assessment, as Apple's developer support
explains for command-line tools. [Apple Developer Technical Support](https://developer.apple.com/forums/thread/773755)

If macOS blocks the verified helper because its developer cannot be verified
or it is not notarized, tell the human which product, release and executable
were blocked. If they trust that exact download and want to proceed:

1. Attempt the normal product command so the relevant block is recorded.
2. Open **System Settings → Privacy & Security**.
3. If **Open Anyway** is available for that executable, the human may select it,
   authenticate if requested, and confirm the open action.
4. Retry the product command and check its status.

Apple documents this exception flow; the control is temporary and may be absent
under device policy. A malware or damaged-file alert is not an unidentified
publisher prompt: stop and investigate it. Do not remove quarantine attributes,
disable Gatekeeper, or claim the checksum establishes malware safety.
[Apple: Open apps safely](https://support.apple.com/en-us/102445)

The agent can diagnose and explain the next step, but the human makes the OS
trust decision. If the exception is unavailable, report a policy block and use
the product's CLI. Do not change device policy.

The helper itself does not grant access to contacts, messages, microphone,
camera or screen recording. Those remain product permissions with their own
identity and consent flow; see [the local app](identity.md) and
[permissions](permissions.md).

## Windows: reputation and application policy

The artifact is `hraness-helper.exe`, without MSI or MSIX packaging. If
SmartScreen shows an unknown-app reputation warning for the verified release,
the human can inspect **More info** and select **Run anyway** only when Windows
offers it and they trust the download. This is a user decision for that warning,
not a guarantee that unsigned software will run. [Microsoft's first-release guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/publish-first-app)

Device administrators can configure SmartScreen to block execution without an
override. Provide the release identity and digest to the administrator when
needed; do not change managed settings, disable protection, or strip download
metadata. [Microsoft: SmartScreen settings](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/available-settings)

**Smart App Control is different from SmartScreen.** Microsoft documents no
per-app exception for Smart App Control. If it blocks this unsigned helper,
the dialogs are unavailable on that machine; the product's CLI keeps working.
Turning off Smart App Control is not an installation step. [Microsoft: Smart App Control FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)

## Linux: desktop session and libraries

The dialogs use GTK 3. The published binary needs the GTK 3 runtime library
(`libgtk-3-0` on Ubuntu) and a graphical user session. 1.0 no longer needs
WebKitGTK or an AppIndicator library. [Ubuntu GTK 3 package](https://packages.ubuntu.com/noble/libgtk-3-0)

Over SSH, in containers, on servers or without a display, `--prompt-probe`
reports the dialogs unavailable and the product falls back to its terminal
prompt. That is the expected result, not a failure to repair.

## Agent handoff after a failed call

Report the product command, OS/architecture, pinned release and digest, and
the safe failure category. Keep tokens, private directory contents and raw
environment dumps out of diagnostics. Use one specific next step: human OS
approval, a missing runtime package, desktop-session setup, administrator
policy review, or a release repair.

After human approval or repair, retry once and check the product's status.
Stop and reconcile repeated or ambiguous failures; never repeatedly launch new
processes or erase state to force progress.
