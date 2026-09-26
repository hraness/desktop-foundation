# Permission notices and recovery

Status: specification for desktop-foundation 0.8.0. The TypeScript kit ships
in the SDK (`sdk/src/permissions.ts`, `sdk/src/audience.ts`); the Rust mirror
ships as the pure `hraness-cli-kit` crate in this repository, re-exported as
`desktop_foundation::permissions`. Neither needs the native runner, so any CLI
can use them.

The kit never triggers a macOS prompt by itself. It says what macOS is about
to ask and why, probes state only where a probe cannot cause a prompt,
explains a denial in plain words, and opens the right System Settings pane
when the person asks.

## Rules every product follows

- Before any system prompt, show the notice for that prompt: on stderr at a
  terminal, or through the `--notice` dialog when a menu action causes it.
- After a denial, say it was a denial (never "not found", "signed out" or a raw
  `EPERM`), name the exact pane and give one next step.
- Name the product. `requester` is whatever macOS will actually show, so the
  copy stays true before and after the [local app](identity.md) exists.
- Confirm with Enter only when macOS will show a blocking dialog that asks for
  a decision. A login-item notice is informational and needs no confirmation.
- Never wait for input without a terminal on both stdin and stderr.

## Permission kinds

| Kind | macOS behavior | Pane name | Settings path | `settingsUrl` |
| --- | --- | --- | --- | --- |
| `full-disk-access` | settings-only: macOS never asks; the person turns it on | Full Disk Access | System Settings › Privacy & Security › Full Disk Access | `x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles` |
| `automation` | asks (Allow / Don't Allow) | Automation | System Settings › Privacy & Security › Automation | `x-apple.systempreferences:com.apple.preference.security?Privacy_Automation` |
| `contacts` | asks | Contacts | System Settings › Privacy & Security › Contacts | `x-apple.systempreferences:com.apple.preference.security?Privacy_Contacts` |
| `accessibility` | asks, then points to Settings | Accessibility | System Settings › Privacy & Security › Accessibility | `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` |
| `screen-recording` | asks, then points to Settings | Screen & System Audio Recording | System Settings › Privacy & Security › Screen & System Audio Recording | `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture` |
| `camera` | asks | Camera | System Settings › Privacy & Security › Camera | `x-apple.systempreferences:com.apple.preference.security?Privacy_Camera` |
| `microphone` | asks | Microphone | System Settings › Privacy & Security › Microphone | `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone` |
| `local-network` | asks (macOS 15+) | Local Network | System Settings › Privacy & Security › Local Network | `x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork` |
| `incoming-connections` | asks when the firewall is on | Firewall | System Settings › Network › Firewall | `x-apple.systempreferences:com.apple.Network-Settings.extension` |
| `notifications` | asks | Notifications | System Settings › Notifications | `x-apple.systempreferences:com.apple.Notifications-Settings.extension` |
| `login-item` | notifies ("Login item added" / "Background items added") | Login Items & Extensions | System Settings › General › Login Items & Extensions | `x-apple.systempreferences:com.apple.LoginItems-Settings.extension` |
| `keychain` | asks, with a password field and Always Allow | Keychain Access | Keychain Access | none |
| `developer-tools` | asks to install Apple's command line tools | none | none | none |
| `gatekeeper` | blocks a quarantined download | Privacy & Security | System Settings › Privacy & Security | `x-apple.systempreferences:com.apple.preference.security?General` |

The URLs are the allowlist: `openPermissionSettings`, the
`foundation.settings.<kind>` menu action and the notice dialog open only
these. `keychain` and `developer-tools` have no pane, so they have no
`foundation.settings.<kind>` action and cannot be a notice's `settings`. Verify the `incoming-connections` and `gatekeeper` URLs on macOS 26
when the kit lands; fall back to the Privacy & Security pane if either fails.

## TypeScript API

```ts
export type Audience = 'human' | 'agent' | 'quiet';
export type PermissionKind =
  | 'full-disk-access' | 'automation' | 'contacts' | 'accessibility'
  | 'screen-recording' | 'camera' | 'microphone' | 'local-network'
  | 'incoming-connections' | 'notifications' | 'login-item' | 'keychain'
  | 'developer-tools' | 'gatekeeper';
export type PromptBehavior = 'asks' | 'settings-only' | 'notifies';
export type PermissionState = 'granted' | 'denied' | 'not-determined' | 'unknown';
export type PrePromptOutcome = 'continue' | 'skip' | 'unattended-proceed' | 'unattended-stop';
export type Surface = 'cli' | 'menu' | 'dialog';

export interface ProductRef {
  product: string;      // display name from the portfolio registry: "Textbutler"
  command: string;      // CLI name used in next steps: "textbutler"
  requester?: string;   // the name macOS shows; default depends on the kind (below)
}

export interface PermissionNeed extends ProductRef {
  kind: PermissionKind;
  target?: string;      // "Messages", "Chrome Safe Storage"
  ask: string;          // verb phrase after "let X", no final period: "control Messages"
  why: string;          // one sentence, at most 110 characters, ending with a period
  next?: string;        // recovery command; default `${command} doctor`
  whenUnattended?: 'proceed' | 'stop'; // default: 'proceed' for notifies, 'stop' otherwise
}

export interface RenderedNotice {
  title: string;        // dialog title; first line of the CLI form
  lines: string[];      // CLI lines without the symbol, or the dialog message parts
  confirm?: string;     // "Press Enter to continue · s to skip" when a confirm applies
}

/** Everything the kit touches, injectable for tests. */
export interface PermissionIO {
  env: NodeJS.ProcessEnv;
  stdinIsTTY: boolean;
  stderrIsTTY: boolean;
  write(text: string): void;                   // stderr
  readKey(timeoutSeconds: number): Promise<'enter' | 's' | 'o' | 'timeout'>;
  openUrl(url: string): Promise<boolean>;      // `open` with an allowlisted URL
  notice?(request: NoticeRequest): Promise<NoticeResult>;   // runner --notice
  fileAccess?(path: string): Promise<'ok' | 'denied' | 'missing'>;
  run?(argv: readonly string[]): Promise<{ status: number }>;
}

export function detectAudience(input?: { env?: NodeJS.ProcessEnv; stderrIsTTY?: boolean }): Audience;
export function responsibleApp(env?: NodeJS.ProcessEnv): string;
export function behaviorOf(kind: PermissionKind): PromptBehavior;
export function paneName(kind: PermissionKind): string | null;
export function settingsPath(kind: PermissionKind): string | null;
export function settingsUrl(kind: PermissionKind): string | null;

export function renderPrePrompt(need: PermissionNeed, surface: Surface): RenderedNotice;
export function renderRecovery(need: PermissionNeed, state: 'denied' | 'unknown' | 'missing', surface: Surface): RenderedNotice;

/** Shows the notice for the audience, waits for Enter/s when it applies, never triggers the prompt. */
export function prePrompt(need: PermissionNeed, options?: { audience?: Audience; io?: PermissionIO; timeoutSeconds?: number }): Promise<PrePromptOutcome>;
/** Read-only probe; returns 'unknown' when no probe is safe. */
export function permissionStatus(kind: PermissionKind, target?: string, io?: PermissionIO): Promise<PermissionState>;
/** Opens the pane for an explicit human action only. Returns false when the kind has no pane. */
export function openPermissionSettings(kind: PermissionKind, io?: PermissionIO): Promise<boolean>;

/** Menu rows: a status.locked row plus the foundation.settings.<kind> action when a pane exists. */
export function permissionMenuItems(need: PermissionNeed, state: PermissionState): MenuItemV2[];
/** The --json error object for a permission failure. */
export function permissionError(need: PermissionNeed, state: 'denied' | 'unknown' | 'missing'): {
  code: 'permission-denied' | 'permission-unknown' | 'permission-missing';
  kind: PermissionKind; message: string; next: string; settingsUrl: string | null;
};
```

`MenuItemV2`, `NoticeRequest` and `NoticeResult` come from
`sdk/src/protocol-v2.ts`. `PermissionKind` is defined there too, because the
wire uses it.

### Requester defaults

`requester` is the name in the macOS dialog. When the product omits it:

| Kind | Default requester |
| --- | --- |
| `keychain` | The executable that calls the keychain. The preset takes it as a parameter (`security` today; the signed helper later). |
| `incoming-connections` | The listening executable's name. |
| everything else | `responsibleApp(env)` |

`responsibleApp(env)` returns the product name when `HRANESS_APP_BUNDLE_ID`
is set (the local app's launcher sets it for its children). Otherwise it maps
`__CFBundleIdentifier`, then `TERM_PROGRAM`: `com.apple.Terminal` or
`Apple_Terminal` → Terminal, `com.googlecode.iterm2` or `iTerm.app` → iTerm,
`com.mitchellh.ghostty` or `ghostty` → Ghostty, `com.microsoft.VSCode` or
`vscode` → Visual Studio Code, `dev.zed.Zed` or `ZED_TERM` set → Zed,
`dev.warp.Warp-Stable` or `WarpTerminal` → Warp, `com.github.wez.wezterm` or
`WezTerm` → WezTerm. Anything else returns "your terminal app".

### Status probes

Probes never cause a prompt.

| Kind | Probe | Result |
| --- | --- | --- |
| `full-disk-access` | `fileAccess` on the target file (Messages: `~/Library/Messages/chat.db`) | `ok` → granted, `denied` (EPERM/EACCES) → denied, `missing` → unknown |
| `developer-tools` | `/usr/bin/xcode-select -p` exit status (this does not open the install dialog; `xcrun`, `swiftc` and `git` shims do) | 0 → granted, otherwise not-determined |
| `login-item` | the product's LaunchAgent or login item record | present → granted, absent → not-determined |
| `keychain` | none | always unknown. Classify the keychain call's own result for `renderRecovery` instead: `errSecUserCanceled` (-128) or `errSecAuthFailed` (-25293) → `denied`, `errSecInteractionNotAllowed` (-25308) → `unknown` with "Unlock your login keychain, then retry.", `errSecItemNotFound` (-25300) → `missing` |
| all others | none in TypeScript | unknown |

The file probe answers for the process that runs it. A CLI in Terminal learns
about Terminal's access, not the local app's. Products that read protected
data from the local app run the probe there too.

## Audience

`detectAudience()` decides who is reading, in this order:

1. `HRANESS_AUDIENCE` set to `human`, `agent` or `quiet` (`off` means `quiet`)
   wins. Other values are ignored.
2. Any agent marker set to a nonempty value → `agent`: `AI_AGENT`,
   `CLAUDECODE`, `CODEX_SANDBOX`, `CODEX_SANDBOX_NETWORK_DISABLED`,
   `CURSOR_AGENT`, `GEMINI_CLI`. The list lives only in `audience.ts` and its
   Rust twin, and only exact names count (a prefix such as `CODEX_` also
   matches human configuration like `CODEX_HOME`).
3. stderr is a terminal → `human`.
4. Otherwise → `quiet`.

support-foundation keeps reading `HRANESS_SUPPORT_AUDIENCE` when
`HRANESS_AUDIENCE` is unset.

| Output | human | agent | quiet |
| --- | --- | --- | --- |
| Command result | text | JSON when the command has a `--json` form | text, no color |
| Errors | text on stderr | the `--json` error object on stdout | text on stderr |
| Permission notice | stderr, with confirm when it applies | one JSON line on stderr: `{"type":"permission-notice","product","kind","message"}` | nothing |
| `Next:` hint | stderr | the `next` field in JSON | nothing |
| Progress | stderr, redrawn in place | nothing | nothing |
| Support and credits invitations | human copy | their existing JSON line | nothing |

`--json` always wins for the command result. `HRANESS_AUDIENCE=human` turns an
agent session back into text.

## Copy templates

`{requester}` and `{product}` are names, `{ask}` and `{why}` come from the
need, and `{path}` is the kind's settings path. `{forProduct}` is empty when
the requester is the product, otherwise ` for {product}`.

### CLI notice (stderr, human audience)

asks:
```text
🔐 macOS will ask to let {requester} {ask}{forProduct}.
   {why} Change this any time in {path}.
   Press Enter to continue · s to skip
```

asks, `keychain` (no pane):
```text
🔐 macOS will ask to let {requester} {ask}{forProduct}.
   {why} Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.
   Press Enter to continue · s to skip
```

settings-only:
```text
🔐 {product} needs {pane} to {ask}.
   macOS doesn't ask for this. Turn on {requester} in {path}. {why}
   Press Enter to open Settings · s to skip
```

notifies (no confirm line):
```text
🔐 macOS will show a notice that {requester} can open at login{thatsProduct}.
   {why} Turn it off any time in {path}.
```

`{thatsProduct}` is empty when the requester is the product, otherwise
`. That's {product}'s menu bar`.

The third line appears only when stdin and stderr are both terminals. Enter
continues, `s` skips, and for settings-only Enter opens the pane first. With no
answer after `timeoutSeconds` (default 120) the outcome is `skip`.

### CLI recovery (stderr)

denied, with a pane:
```text
✗ {product} can't {ask}: macOS access is off for {requester}.
  Turn on {requester} in {path}.
→ {next} · press o to open Settings
```

denied, `keychain`:
```text
✗ {product} can't {ask}: the keychain request was denied.
  Run it again and choose Always Allow when macOS asks.
→ {next}
```

unknown:
```text
✗ {product} couldn't {ask}. macOS may be blocking {requester}.
  Check {path}.
→ {next}
```

missing, `developer-tools`:
```text
✗ {product} needs Apple's command line tools. Nothing was installed.
→ xcode-select --install
```

"press o to open Settings" appears only when stdin and stderr are terminals
and the kind has a pane. The symbols follow the CLI style contract, including
its ASCII fallback (`🔐` → `NOTE`, `✗` → `FAIL`, `→` → `->`).

### Menu

`permissionMenuItems` returns a `status` row with `status.locked` and, when
the kind has a pane, an action with `symbol: action.permission`,
`opens: settings` and ID `foundation.settings.{kind}`:

| State | Status row | Action |
| --- | --- | --- |
| not-determined, unknown | "Needs {pane}", detail "To {ask}" | "Open {pane} settings" |
| denied | "{pane} is off", detail "Turn on {requester} to {ask}" | "Open {pane} settings" |
| granted | no rows | none |

When the product is signed out or stopped, the permission rows follow the
status row that explains that.

### Dialog (`--notice`, when a menu action is about to cause a prompt)

| Behavior | Title | Message | Buttons |
| --- | --- | --- | --- |
| asks | `{product} needs access to {target or pane}` | the CLI notice's first two lines as one paragraph, without the symbol | primary "Continue", secondary "Not now" |
| settings-only | `{product} needs {pane}` | the CLI notice's first two lines | primary "Open System Settings" with `settings: {kind}` (choosing it opens the pane), secondary "Not now" |
| notifies | no dialog: turning on "Open at login" is the consent. The toggle's subtitle reads "macOS shows a notice when you turn this on". | | |

## Presets

Presets fill `kind`, `target`, `ask`, `why` and the requester rule. Products
pass their `ProductRef` and any parameter shown. The rendered copy below uses
Textbutler, Ghostget, Wordcell, PeopleBlade, Valhalla, Slopcamera and algal
as examples, with the requester equal to the product (the local app is
installed). Before that, the requester is the terminal app or executable and
`{forProduct}` names the product.

### `LOGIN_ITEM(ref)`

kind `login-item`; `why`: "Its menu bar icon opens when you log in. Nothing else runs in the background."

```text
🔐 macOS will show a notice that Textbutler can open at login.
   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.
```

Before the local app (requester `bun`):

```text
🔐 macOS will show a notice that bun can open at login. That's Textbutler's menu bar.
   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.
```

### `CHROME_SAFE_STORAGE(ref, { browser = 'Chrome', caller, why? })`

kind `keychain`; target `{browser} Safe Storage`; ask `use "{browser} Safe Storage" from your keychain`; requester `caller`; default `why`: "{product} uses it to read the {browser} sign-in you already have and never stores it."

```text
🔐 macOS will ask to let security use "Chrome Safe Storage" from your keychain for Ghostget.
   Ghostget uses it to read the Chrome sign-in you already have and never stores it. Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.
   Press Enter to continue · s to skip
```

A denial is `permission-denied` with kind `keychain`, never "no matching
cookies". Safari cookies use `full-disk-access` with target `Safari`.

### `MESSAGES_FDA(ref, { why? })`

kind `full-disk-access`; target Messages; ask `read your Messages`; default `why`: "Only the chats you pick are read."

```text
🔐 Textbutler needs Full Disk Access to read your Messages.
   macOS doesn't ask for this. Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access. Only the chats you pick are read.
   Press Enter to open Settings · s to skip
```

### `AUTOMATION(ref, app, why)`

kind `automation`; target `app`; ask `control {app}`.

```text
🔐 macOS will ask to let Textbutler control Messages.
   Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.
   Press Enter to continue · s to skip
```

### `CONTACTS(ref, { why? })`

kind `contacts`; ask `see your contacts`; default `why`: "{product} reads names and numbers on this Mac."

```text
🔐 macOS will ask to let PeopleBlade see your contacts.
   PeopleBlade reads names and numbers on this Mac. Change this any time in System Settings › Privacy & Security › Contacts.
   Press Enter to continue · s to skip
```

### `LOCAL_NETWORK(ref, why)`

kind `local-network`; ask `find and connect to devices on your local network`.

```text
🔐 macOS will ask to let Valhalla find and connect to devices on your local network.
   Room members on your network connect to this Mac. Change this any time in System Settings › Privacy & Security › Local Network.
   Press Enter to continue · s to skip
```

### `INCOMING_CONNECTIONS(ref, { listener, why })`

kind `incoming-connections`; ask `accept incoming network connections`; requester `listener`.

```text
🔐 macOS will ask to let vhalla accept incoming network connections for Valhalla.
   Choose Allow so room members can reach this Mac. Change this any time in System Settings › Network › Firewall.
   Press Enter to continue · s to skip
```

### `XCODE_TOOLS(ref, { skipEffect })`

kind `developer-tools`; this preset has its own template because there is no
pane and the dialog installs software:

```text
🔐 algal needs Apple's command line tools to build a small helper. macOS will offer to install them (about 1 GB).
   Nothing is installed unless you agree in that window. Or skip: {skipEffect}.
   Press Enter to continue · s to skip
```

Products call `permissionStatus('developer-tools')` first and show this only
when the tools are missing. Unattended, the outcome is `unattended-stop` and
the product prints the `developer-tools` recovery instead of opening the
dialog. Compiler output is captured, never inherited.

### `SCREEN_RECORDING(ref, why)`

kind `screen-recording`; ask `record your screen`.

```text
🔐 macOS will ask to let Slopcamera record your screen.
   Slopcamera only captures the window you pick. Change this any time in System Settings › Privacy & Security › Screen & System Audio Recording.
   Press Enter to continue · s to skip
```

### `LOCAL_SIGNING(ref)`

kind `keychain`; target `Hraness Local Signing`; requester `codesign`; ask
`use your "Hraness Local Signing" key`. Shown before the first signing on a Mac
(see [identity](identity.md#signing-identity)).

```text
🔐 macOS will ask to let codesign use your "Hraness Local Signing" key for Textbutler.
   Hraness signs its apps on this Mac with it so they keep their permissions after updates. Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.
   Press Enter to continue · s to skip
```

Skipping falls back to ad-hoc signing and prints `⚠ Textbutler will ask for
its permissions again after each update.`

## JSON error shape

With `--json` or an agent audience, a permission failure is:

```json
{"ok":false,"error":{"code":"permission-denied","message":"Textbutler can't read your Messages: macOS access is off for Textbutler.","next":"textbutler doctor","permission":{"kind":"full-disk-access","settingsUrl":"x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"}}}
```

## Rust API (`hraness-cli-kit`)

```rust
pub mod audience {
    pub enum Audience { Human, Agent, Quiet }
    pub fn detect(env: &dyn Fn(&str) -> Option<String>, stderr_is_tty: bool) -> Audience;
    pub fn detect_current() -> Audience;
}

pub mod permissions {
    #[non_exhaustive]
    pub enum PermissionKind { FullDiskAccess, Automation, Contacts, Accessibility, ScreenRecording,
        Camera, Microphone, LocalNetwork, IncomingConnections, Notifications, LoginItem, Keychain,
        DeveloperTools, Gatekeeper }
    pub enum PromptBehavior { Asks, SettingsOnly, Notifies }
    pub enum PermissionState { Granted, Denied, NotDetermined, Unknown }
    pub enum RecoveryState { Denied, Unknown, Missing }
    pub enum PrePromptOutcome { Continue, Skip, UnattendedProceed, UnattendedStop }
    pub enum Surface { Cli, Menu, Dialog }

    pub struct ProductRef<'a> { pub product: &'a str, pub command: &'a str, pub requester: Option<&'a str> }
    pub struct PermissionNeed<'a> {
        pub product: ProductRef<'a>, pub kind: PermissionKind, pub target: Option<&'a str>,
        pub ask: &'a str, pub why: &'a str, pub next: Option<&'a str>, pub when_unattended: Option<Unattended>,
    }
    pub enum Unattended { Proceed, Stop }
    pub struct RenderedNotice { pub title: String, pub lines: Vec<String>, pub confirm: Option<String> }

    pub trait PermissionIo {
        fn env(&self, key: &str) -> Option<String>;
        fn stdin_is_tty(&self) -> bool;
        fn stderr_is_tty(&self) -> bool;
        fn write(&mut self, text: &str) -> std::io::Result<()>;
        fn read_key(&mut self, timeout: std::time::Duration) -> std::io::Result<Key>;
        fn open_url(&mut self, url: &str) -> std::io::Result<bool>;
    }
    pub enum Key { Enter, Skip, Open, Timeout }

    impl PermissionKind {
        pub fn as_str(self) -> &'static str;          // "full-disk-access"
        pub fn behavior(self) -> PromptBehavior;
        pub fn pane_name(self) -> Option<&'static str>;
        pub fn settings_path(self) -> Option<&'static str>;
        pub fn settings_url(self) -> Option<&'static str>;
    }
    pub fn responsible_app(env: &dyn Fn(&str) -> Option<String>) -> String;
    pub fn render_pre_prompt(need: &PermissionNeed, surface: Surface) -> RenderedNotice;
    pub fn render_recovery(need: &PermissionNeed, state: RecoveryState, surface: Surface) -> RenderedNotice;
    pub fn pre_prompt(need: &PermissionNeed, audience: Audience, io: &mut dyn PermissionIo) -> std::io::Result<PrePromptOutcome>;
    pub fn permission_status(kind: PermissionKind, target: Option<&std::path::Path>) -> PermissionState;
    pub fn open_settings(kind: PermissionKind, io: &mut dyn PermissionIo) -> std::io::Result<bool>;

    pub mod presets {
        pub fn login_item(r: ProductRef) -> PermissionNeed;
        pub fn chrome_safe_storage(r: ProductRef, browser: &str, caller: &str) -> PermissionNeed;
        pub fn messages_fda(r: ProductRef) -> PermissionNeed;
        pub fn automation(r: ProductRef, app: &str, why: &str) -> PermissionNeed;
        pub fn contacts(r: ProductRef) -> PermissionNeed;
        pub fn local_network(r: ProductRef, why: &str) -> PermissionNeed;
        pub fn incoming_connections(r: ProductRef, listener: &str, why: &str) -> PermissionNeed;
        pub fn xcode_tools(r: ProductRef, skip_effect: &str) -> PermissionNeed;
        pub fn screen_recording(r: ProductRef, why: &str) -> PermissionNeed;
        pub fn local_signing(r: ProductRef) -> PermissionNeed;
    }
}
```

Rendered strings are byte-identical between TypeScript and Rust. Both suites
check the same golden files for every preset, surface and state.
