# Install and operate a CLI companion

A companion is a small native executable launched by a product CLI. It places a
menu in the macOS menu bar, Windows notification area, or a compatible Linux
panel. Detailed interfaces open in the user's browser. Distribution uses a
versioned executable rather than an app bundle or desktop installer; it does
not use Apple notarization or publisher signing credentials.

The executable is still downloaded software. Operating-system checks apply,
and some machines will not allow an unsigned executable. A successful download,
checksum, build, or process spawn is not proof that the user can see a working
menu. See [platform support and qualification](platforms.md).

## Product command contract

Products adopting the SDK expose the following command shape. Replace
`<product>` with the product CLI; consult its `menubar --help` before using these
commands. This is an adoption contract, not a claim that every existing product
version has migrated.

| Command | Meaning |
| --- | --- |
| `<product> menubar` or `menubar start` | Resolve the product's pinned companion release, verify and cache its native executable if needed, and start one companion for this user and product. |
| `<product> menubar status` | Report whether the companion is running. Daemon status is a separate product state. |
| `<product> menubar doctor` | Report platform, release, session and launch diagnostics with an actionable next step. |
| `<product> menubar stop` | Stop this product's companion. Preserve product data and daemon behavior. |
| `<product> menubar install` | Opt in to launching the companion when this user signs in. This is login registration, not an app installer. |
| `<product> menubar uninstall` | Remove that login registration. Preserve product configuration, accounts and outputs. |

Starting twice must keep one menu. A normal interactive start remains available
until Quit, stop, logout or process exit. Login registration is explicit and must
respect Quit; a supervisor must not immediately resurrect a companion the user
has quit. If the product offers a distinct daemon stop action, label it as such.

Normal users do not need Rust, Xcode or a repository checkout. The product pins
the supported runner version and platform artifact digest. Cached bytes must
match that digest before execution. A missing published artifact is a release
problem: report it without building source or choosing an unpinned download.

## macOS: first-launch approval

An unbundled command-line executable is not exempt from Gatekeeper. Launch
method and quarantine state affect assessment, as Apple's developer support
explains for command-line tools. [Apple Developer Technical Support](https://developer.apple.com/forums/thread/773755)

If macOS blocks the verified companion because its developer cannot be verified
or it is not notarized, tell the human which product, release and executable
were blocked. If they trust that exact download and want to proceed:

1. Attempt the normal product launch so the relevant block is recorded.
2. Open **System Settings → Privacy & Security**.
3. If **Open Anyway** is available for that executable, the human may select it,
   authenticate if requested, and confirm the open action.
4. Retry the product command and check the menu and status.

Apple documents this exception flow; the control is temporary and may be absent
under device policy. A malware or damaged-file alert is not an unidentified
publisher prompt: stop and investigate it. Do not remove quarantine attributes,
disable Gatekeeper, or claim the checksum establishes malware safety.
[Apple: Open apps safely](https://support.apple.com/en-us/102445)

The agent can diagnose and explain the next step, but the human makes the OS
trust decision. If the exception is unavailable, report a policy block and use
the product's available CLI/browser interface. Do not change device policy.

The companion itself does not grant access to contacts, messages, microphone,
camera or screen recording. Those remain product/provider permissions with
their own supported identity and consent flow. Never move a protected action
into the tray runner just to avoid its permission requirement.

## Windows: reputation and application policy

The artifact is `hraness-companion.exe`, without MSI or MSIX packaging. If
SmartScreen shows an unknown-app reputation warning for the verified release,
the human can inspect **More info** and select **Run anyway** only when Windows
offers it and they trust the download. This is a user decision for that warning,
not a guarantee that unsigned software will run. [Microsoft's first-release guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/publish-first-app)

Device administrators can configure SmartScreen to block execution without an
override. Provide the release identity and digest to the administrator when
needed; do not change managed settings, disable protection, or strip download
metadata. [Microsoft: SmartScreen settings](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/available-settings)

**Smart App Control is different from SmartScreen.** Microsoft documents no
per-app exception for Smart App Control. If it blocks this unsigned companion,
the unsigned distribution path is unavailable on that machine. Keep any
already-permitted CLI/browser mode usable and report the limitation. Turning
off Smart App Control is not an installation step. [Microsoft: Smart App Control FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)

The notification-area icon may be hidden in the taskbar's overflow. Check there
before diagnosing a missing process. Remote services and noninteractive
sessions are not substitutes for the signed-in user's desktop session.

## Linux: desktop session and libraries

The current renderer uses Tauri's GTK/AppIndicator backend. It needs the runtime
libraries linked by the published binary, a graphical user session, and a panel
that displays AppIndicator/StatusNotifierItem entries. A source build additionally
needs the development packages in [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/).

For a compatible Ubuntu installation, the principal runtime package names are
`libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`; their package dependencies
supply related libraries. Consult the artifact's supported distribution and
loader error before selecting packages. Do not give end users the entire
compiler/development dependency list. [Ubuntu WebKitGTK package](https://packages.ubuntu.com/noble/libwebkit2gtk-4.1-0), [Ubuntu AppIndicator package](https://packages.ubuntu.com/noble/libayatana-appindicator3-1)

GNOME sessions may need a compatible AppIndicator extension. Offer the
distribution-supported extension or the documented
[GNOME AppIndicator/KStatusNotifierItem extension](https://extensions.gnome.org/extension/615/appindicator-support/)
as a user choice; do not silently install or change shell extensions. The
presence of a display or D-Bus session alone does not prove a panel host exists.

For SSH, containers, servers, missing libraries, or a desktop without a tray
host, report that the native companion is unavailable. Continue with the
product's supported CLI/browser controls. Do not open an invisible background
window, install another desktop environment, or treat headless mode as a
successful tray launch.

## Agent handoff after a failed launch

Report the product command, OS/architecture, pinned release and digest, the
safe failure category, and whether a native menu was actually observed. Keep
tokens, private directory contents and raw environment dumps out of diagnostics.
Use one specific next step: human OS approval, a missing runtime package,
desktop-session setup, administrator policy review, or a release repair.

After human approval or repair, retry once and verify status plus a harmless
menu action. Stop and reconcile repeated or ambiguous failures; never repeatedly
launch new processes or erase singleton state to force progress.
