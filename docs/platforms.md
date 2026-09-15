# Platform contract

`desktop-foundation` shares the menu model, renderer and lifecycle. The
`@hraness/desktop-foundation` SDK connects a product CLI to the unbundled
`hraness-companion` runner over JSON lines. Products retain authority over
accounts, daemon state, permissions and action authorization. Native menu
rendering is portable; product capabilities are not automatically portable.

## Common surface

Use a compact tray icon, status labels, action rows, checkmarks, bounded
submenus, separators and Quit. The existing Rust model also carries item icons,
shortcuts, radio selection, and textual badge/progress values. These are native
menu presentations, not web widgets. Use the browser for forms, search, account
onboarding and larger views. See [the model and host contract](../README.md).

| Platform | Surface | Runtime constraints |
| --- | --- | --- |
| macOS | Native menu-bar status item in accessory mode | User's graphical session; matching Mach-O architecture; OS first-launch approval may be required. |
| Windows | Native notification-area icon and context menu | User's interactive desktop; matching executable target; SmartScreen or application policy may block unsigned binaries. |
| Linux | GTK/AppIndicator menu in a compatible desktop panel | Compatible glibc and dynamic libraries; graphical session and D-Bus; a panel host that displays indicators. |

Native executables are distributed without `.app`, DMG, PKG, MSI, MSIX or
AppImage packaging. Builds use no publisher signing credentials or notarization
submission. OS approval remains a separate condition; see
[installation and blocked-launch guidance](installation.md).

## Design for the actual common denominator

- Put product identity and important state in the menu. Windows does not show
  tray text titles, Linux title display depends on the panel, and Tauri's Linux
  backend does not support tray tooltips. A two-letter serif **image** can be
  portable; native title text alone cannot guarantee that appearance.
- Always attach a menu. Some Linux hosts do not display an icon without one.
  Keep a menu attached when refreshing it; the pinned Linux backend supports
  replacement but cannot remove an attached menu with `set_menu(None)`.
- Theme-aware macOS template icons are a platform enhancement. Test icon
  contrast on Windows and Linux rather than assuming macOS template behavior.

Display constraints come from the [pinned Tauri tray builder API](https://docs.rs/tauri/2.11.5/tauri/tray/struct.TrayIconBuilder.html).
Menu replacement behavior is verified against the [pinned Linux implementation](https://docs.rs/crate/tray-icon/0.24.2/source/src/platform_impl/gtk/mod.rs).

Menu activation is the portable action path. Do not make double-click, hover,
left-click customization or tray coordinates essential: Tauri does not emit
tray mouse events on Linux. [Tauri system tray documentation](https://v2.tauri.app/learn/system-tray/)

Keep the full status in visible row text/checkmarks, including disconnected,
stale or pending states. Accessibility metadata retained by the model is not
equivalent to a custom accessible native control. Shortcuts are menu shortcuts,
not registered global hotkeys. Native display of item icons varies by host.

## Lifecycle and ownership

The product CLI owns daemon connections and turns confirmed product state into
menu snapshots. The runner owns native UI only. JSON-lines action events carry
stable IDs; they do not contain shell commands to execute. The product maps IDs
to allowed actions using its own authorization and finite deadlines. EOF,
disconnect and Quit must release UI resources and the per-user/product
singleton. Do not auto-retry an action whose result is unknown.

Each product has one companion per user, independent of other products.
Repeated launches must converge to the existing instance. Optional login
registration targets the user's interactive session, never a machine service.
Quitting the menu must have explicit behavior distinct from stopping a daemon.

The shared foundation must not absorb messaging history, account credentials,
provider sessions, browser authentication, or camera/microphone/screen-recording
permission ownership. For example, an iMessage provider remains macOS-specific
even when its product can show a status menu on Windows or Linux.

## Linux support boundary

A StatusNotifierItem, a watcher and a visualizing host are separate components
of the desktop protocol. A live session bus is not evidence that any panel will
render the item. [freedesktop.org Status Notifier design](https://specifications.freedesktop.org/status-notifier-item/0.1/basic-design.html)

The current Tauri dependency graph retains WebKitGTK/GTK runtime dependencies
even though this runner creates no webview windows. Publish the actual dynamic
dependency list and minimum distribution alongside each Linux artifact. Build
against the oldest supported ABI baseline; do not claim a generic Linux binary
runs on every distribution or a musl-based system.

Upstream `tray-icon` now offers a separate KSNI/D-Bus backend that can avoid GTK,
libappindicator and libxdo when GTK features are disabled. That is a possible
future dependency reduction, not the current Tauri-backed implementation.
Do not advertise those missing dependencies until the backend is migrated and
qualified. [Upstream tray-icon backends](https://github.com/tauri-apps/tray-icon)

## Build evidence and desktop qualification

Keep these claims separate in release notes:

| Evidence | What it proves |
| --- | --- |
| Source/model/SDK tests | Protocol validation and lifecycle behavior exercised by those tests. |
| Native target build | The reviewed source compiles for that OS and architecture with the recorded toolchain. |
| Executable protocol probe | The downloaded bytes execute and report the expected runner protocol on that host. |
| Interactive desktop smoke | An icon/menu is visible, a harmless action works, state refreshes, starting twice keeps one menu, and Quit/stop removes it. |
| Clean-install smoke | A fresh user environment installs the published bytes without a compiler; actual OS approval and Linux dependency behavior are recorded. |

CI compilation, simulated platform tests and protocol probes do not establish
interactive desktop or clean-install success. A macOS validation does not
qualify Linux or Windows. A release should identify the exact version, digest,
OS/architecture and desktop environment for each completed proof; unavailable
targets remain explicitly unqualified. Login-registration claims need a separate
sign-in/restart check, including Quit behavior.

### Windows ARM64 qualification boundary

GitHub's hosted Windows ARM64 image currently presents an interactive
explorer-owned taskbar that cannot complete `Shell_NotifyIcon`: the bounded
fixture probe fails with `E_FAIL` across apartments and icon variants while the
identical probe passes on hosted x64 Windows. The ARM64 leg therefore runs the
desktop fixture in capability mode — a proven-incapable host records
`tray-unsupported` evidence and the job continues with compilation, unit and
headless protocol gates, skipping only the interactive tray smokes. Interactive
tray qualification for Windows is carried by the x64 leg; the exercised code
path is architecture-independent Win32. If the hosted image regains a
functional notification area, the capability probe resumes the ARM64
interactive gates automatically. Until then released ARM64 executables are
build- and protocol-qualified, not interactive-desktop-qualified.
