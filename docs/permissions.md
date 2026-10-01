# Permission notices and recovery

Use `@hraness/desktop-foundation/permissions` to explain a macOS permission
request and help a person recover from a denial. It runs independently of the
native helper. Rust products use the std-only `hraness-cli-kit` crate with the
matching [API and permission wording](#rust-api-hraness-cli-kit).

The kit never triggers a macOS prompt by itself. It says what macOS is about
to ask and why, probes state only where a probe cannot cause a prompt,
explains a denial in plain words, and opens the right System Settings pane
when the person asks.

## Rules every product follows

- Before any system prompt, show the notice for that prompt: on stderr at a
  terminal, or through the `--notice` dialog when there is no terminal.
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

The URLs are the allowlist: `openPermissionSettings` and the notice dialog open only
these. `keychain` and `developer-tools` have no pane, so they cannot be a
notice's `settings`. Verify the `incoming-connections` and `gatekeeper` URLs on macOS 26
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
export type Surface = 'cli' | 'dialog';

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

/** The --json error object for a permission failure. */
export function permissionError(need: PermissionNeed, state: 'denied' | 'unknown' | 'missing'): {
  code: 'permission-denied' | 'permission-unknown' | 'permission-missing';
  kind: PermissionKind; message: string; next: string; settingsUrl: string | null;
};
```

`NoticeRequest` and `NoticeResult` come from `sdk/src/notice.ts`.
`PermissionKind` is defined there too, because the wire uses it.

The shipped kit also exports these helpers. Every function that resolves a
default requester takes an optional last `env` argument, and `responsibleApp`
takes an optional product name, because the local app's bundle ID alone does
not carry the display name:

```ts
export function responsibleApp(env?: NodeJS.ProcessEnv, product?: string): string;
export function requesterOf(need: PermissionNeed, env?: NodeJS.ProcessEnv): string;
export function hasSettingsPane(kind: PermissionKind): kind is SettingsPermissionKind;
export function isAllowedSettingsUrl(url: string): boolean;
/** Plain text for a CLI notice (🔐 …) or recovery (✗ … → …), with the confirm or "press o" hint only when interactive. */
export function formatNotice(notice: RenderedNotice, options: { kind: 'pre-prompt' | 'recovery'; interactive: boolean; style?: CliStyle }): string;
/** The --notice dialog request for a need, or null for login items. */
export function permissionNoticeRequest(need: PermissionNeed, env?: NodeJS.ProcessEnv): NoticeRequest | null;
/** Prints the CLI recovery on stderr and opens Settings when the person presses o. */
export function reportPermissionFailure(need: PermissionNeed, state: RecoveryState, options?: { audience?: Audience; io?: PermissionIO }): Promise<void>;
/** The whole JSON error document from § JSON error shape. */
export function permissionErrorJson(need: PermissionNeed, state: RecoveryState, env?: NodeJS.ProcessEnv): { ok: false; error: … };
/** OSStatus or `/usr/bin/security` exit status → denied | unknown | missing. */
export function classifyKeychainStatus(status: number): RecoveryState | undefined;
export function defaultPermissionIO(): PermissionIO;
```

`RenderedNotice` from `renderRecovery` also carries `next`, the one step
printed after `→`; its `confirm` is the "press o to open Settings" hint.

Keychain Access has no pane, so keychain copy never offers Settings. A keychain `missing` recovery reads `✗ {product} can't find "{target}" in your
keychain.`

`sdk/test/golden/permissions/*.txt` holds the rendered copy for every preset,
surface and state, for a product as its own requester and before its local
app exists. The Rust `hraness-cli-kit` tests read the same files, and also check
this page's kind table, preset copy and JSON sample. Regenerate
them with `UPDATE_GOLDEN=1 npm run check:sdk` and review the diff.

### Requester defaults

`requester` is the name in the macOS dialog. When the product omits it:

| Kind | Default requester |
| --- | --- |
| `keychain` | The executable that calls the keychain. The preset takes it as a parameter (`security` today; the signed helper later). |
| `incoming-connections` | The listening executable's name. |
| `login-item` | The product. Login Items lists the program the login item runs, never the terminal; `install` passes that program's name until the local app exists. |
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
`. That's how {product} starts in the background`.

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

### Dialog (`--notice`, when a prompt is coming and there is no terminal)

| Behavior | Title | Message | Buttons |
| --- | --- | --- | --- |
| asks | `{product} needs access to {target or pane}` | the CLI notice's first two lines as one paragraph, without the symbol | primary "Continue", secondary "Not now" |
| settings-only | `{product} needs {pane}` | the CLI notice's first two lines | primary "Open System Settings" with `settings: {kind}` (choosing it opens the pane), secondary "Not now" |
| notifies | no dialog: running the product's login-item command is the consent. | | |

## Presets

Presets fill `kind`, `target`, `ask`, `why` and the requester rule. Products
pass their `ProductRef` and any parameter shown. The rendered copy below uses
Textbutler, Ghostget, Wordcell, PeopleBlade, Valhalla, Slopcamera and algal
as examples, with the requester equal to the product (the local app is
installed). Before that, the requester is the terminal app or executable and
`{forProduct}` names the product.

### `LOGIN_ITEM(ref)`

kind `login-item`; `why`: "It starts in the background when you log in and shows no window or icon."

```text
🔐 macOS will show a notice that Textbutler can open at login.
   It starts in the background when you log in and shows no window or icon. Turn it off any time in System Settings › General › Login Items & Extensions.
```

Before the local app (requester `bun`):

```text
🔐 macOS will show a notice that bun can open at login. That's how Textbutler starts in the background.
   It starts in the background when you log in and shows no window or icon. Turn it off any time in System Settings › General › Login Items & Extensions.
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
   Hraness uses this key to give the apps it builds on this Mac a stable signing identity. Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.
   Press Enter to continue · s to skip
```

Skipping returns control to the product. If the product chooses ad-hoc
signing, macOS may ask for permissions again after an update; see
[product identity](identity.md#creating-and-using-the-identity).

## JSON error shape

With `--json` or an agent audience, a permission failure is:

```json
{"ok":false,"error":{"code":"permission-denied","message":"Textbutler can't read your Messages: macOS access is off for Textbutler.","next":"textbutler doctor","permission":{"kind":"full-disk-access","settingsUrl":"x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"}}}
```

## Rust API (`hraness-cli-kit`)

The crate uses only `std`; the optional `clap` feature adds the
usage-error hook for clap 4 CLIs. Pin it by tag like the rest of this
repository:

```toml
hraness-cli-kit = { git = "https://github.com/hraness/desktop-foundation", tag = "v1.1.2", features = ["clap"] }
```

`desktop_foundation::cli_kit`, `::audience` and `::permissions` re-export it
for products that already depend on the `desktop-foundation` crate. Every function that
resolves a default requester takes `env: &dyn Fn(&str) -> Option<String>`
(`audience::process_env` for the real environment), so tests never touch the
process.

```rust
pub mod audience {
    pub enum Audience { Human, Agent, Quiet }
    pub const AGENT_MARKERS: [&str; 6];
    pub fn detect(env: &dyn Fn(&str) -> Option<String>, stderr_is_tty: bool) -> Audience;
    pub fn detect_current() -> Audience;
    pub fn process_env(name: &str) -> Option<String>;
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
    pub enum Surface { Cli, Dialog }
    pub enum Unattended { Proceed, Stop }
    pub enum NoticeKind { PrePrompt, Recovery }

    pub struct ProductRef { pub product: String, pub command: String, pub requester: Option<String> }
    pub struct PermissionNeed {
        pub product: ProductRef, pub kind: PermissionKind, pub target: Option<String>,
        pub ask: String, pub why: String, pub next: Option<String>, pub when_unattended: Option<Unattended>,
    }
    pub struct RenderedNotice { pub title: String, pub lines: Vec<String>, pub confirm: Option<String>, pub next: Option<String> }
    pub struct NoticeRequest { pub title: String, pub message: String, pub primary: String,
        pub secondary: Option<String>, pub settings: Option<PermissionKind> }   // .to_json()

    pub trait PermissionIo {
        fn env(&self, key: &str) -> Option<String>;
        fn stdin_is_tty(&self) -> bool;
        fn stderr_is_tty(&self) -> bool;
        fn write(&mut self, text: &str) -> std::io::Result<()>;            // stderr
        fn read_key(&mut self, timeout: Duration) -> std::io::Result<Key>;
        fn open_url(&mut self, url: &str) -> std::io::Result<bool>;
        fn file_access(&self, path: &Path) -> FileAccess { … }             // default: open for reading
        fn run_status(&self, argv: &[&str]) -> Option<i32> { … }           // default: std::process
    }
    pub enum Key { Enter, Skip, Open, Timeout }
    pub struct ProcessIo;   // the real process; reads a line for Enter, s or o

    impl PermissionKind {
        pub const ALL: [PermissionKind; 14];
        pub fn as_str(self) -> &'static str;          // "full-disk-access"
        pub fn parse(name: &str) -> Option<Self>;
        pub fn behavior(self) -> PromptBehavior;
        pub fn pane_name(self) -> Option<&'static str>;
        pub fn settings_path(self) -> Option<&'static str>;
        pub fn settings_url(self) -> Option<&'static str>;
        pub fn has_settings_pane(self) -> bool;
    }
    pub fn is_allowed_settings_url(url: &str) -> bool;
    pub fn responsible_app(env: Env, product: Option<&str>) -> String;
    pub fn requester_of(need: &PermissionNeed, env: Env) -> String;
    pub fn render_pre_prompt(need: &PermissionNeed, surface: Surface, env: Env) -> RenderedNotice;
    pub fn render_recovery(need: &PermissionNeed, state: RecoveryState, surface: Surface, env: Env) -> RenderedNotice;
    pub fn format_notice(notice: &RenderedNotice, kind: NoticeKind, interactive: bool, style: Style) -> String;
    pub fn notice_request(need: &PermissionNeed, env: Env) -> Option<NoticeRequest>;
    pub fn pre_prompt(need: &PermissionNeed, audience: Option<Audience>, io: &mut dyn PermissionIo) -> std::io::Result<PrePromptOutcome>;
    pub fn report_permission_failure(need: &PermissionNeed, state: RecoveryState, audience: Option<Audience>, io: &mut dyn PermissionIo) -> std::io::Result<()>;
    pub fn permission_status(kind: PermissionKind, target: Option<&str>, io: &dyn PermissionIo) -> PermissionState;
    pub fn open_settings(kind: PermissionKind, io: &mut dyn PermissionIo) -> bool;
    pub fn permission_error_json(need: &PermissionNeed, state: RecoveryState, env: Env) -> String;
    pub fn permission_cli_error(need: &PermissionNeed, state: RecoveryState, env: Env) -> CliError;
    pub fn classify_keychain_status(status: i32) -> Option<RecoveryState>;

    pub mod presets {
        pub fn login_item(r: ProductRef) -> PermissionNeed;
        pub fn chrome_safe_storage(r: ProductRef, browser: Option<&str>, caller: &str, why: Option<&str>) -> PermissionNeed;
        pub fn messages_fda(r: ProductRef, why: Option<&str>) -> PermissionNeed;
        pub fn automation(r: ProductRef, app: &str, why: &str) -> PermissionNeed;
        pub fn contacts(r: ProductRef, why: Option<&str>) -> PermissionNeed;
        pub fn local_network(r: ProductRef, why: &str) -> PermissionNeed;
        pub fn incoming_connections(r: ProductRef, listener: &str, why: &str) -> PermissionNeed;
        pub fn xcode_tools(r: ProductRef, skip_effect: &str) -> PermissionNeed;
        pub fn screen_recording(r: ProductRef, why: &str) -> PermissionNeed;
        pub fn local_signing(r: ProductRef) -> PermissionNeed;
    }
}

pub mod style {
    pub enum Symbol { Ok, Fail, Warn, Next, On, Off, Skip, Progress, Notice }  // .glyph(), .ascii()
    pub struct Style { pub color: bool, pub ascii: bool }   // detect(env, is_tty), stdout(), stderr(), symbol(), line()
    pub struct CliError { pub code, pub message, pub detail, pub next, pub permission, pub exit_code }
        // render_human(style), render_json(), report(json, audience) -> exit code
    pub struct Output;       // result, detail, warn, next ("Next:" for people only), error
    pub fn sentence(text: &str, keep: &[&str]) -> String;
    pub fn check_summary(passed: usize, warnings: usize, failures: usize) -> String;
    pub fn format_bytes(bytes: u64) -> String;
    pub fn write_stdout(text: &str) -> bool;         // ignores a closed pipe
    pub fn exit_quietly_on_broken_pipe();            // `| head -1` exits 0, not a panic
    pub fn restore_default_sigpipe();                // read-only listings only
}

pub mod clap {   // feature "clap"
    pub struct UsageOptions { pub cli: Option<String>, pub aliases: Vec<(String, String)>, pub audience: Option<Audience> }
    pub fn usage_error(error: &clap::Error, root: &clap::Command, args: &[String], options: &UsageOptions) -> Option<CliError>;
    pub fn exit_on_parse_error(error: clap::Error, root: &clap::Command, args: &[String], options: &UsageOptions) -> i32;
    pub fn cap_help_width(command: clap::Command, width: usize) -> clap::Command;   // every level
    pub fn help_lines_over(command: &clap::Command, width: usize) -> Vec<(String, String)>;
    pub fn closest(input: &str, candidates: &[(String, String)]) -> Option<String>;
}
```

Rendered strings are byte-identical between TypeScript and Rust. Both suites
check the same golden files for every preset, surface and state.

The clap hook turns clap's multi-line usage errors into the contract's form:

```text
✗ Unknown command "stauts". Did you mean "status"?
→ textbutler --help
```

with exit 2, or `{"ok":false,"error":{"code":"usage","message":…,"next":…}}`
on stdout for `--json` and agents. Suggestions come from the command's
visible subcommands and aliases (one edit per three letters, or a unique
prefix); `UsageOptions::alias("status", "proxy status")` suggests a command
that lives in a group. Help and version requests, and a group run without its
subcommand, print to stdout and exit 0.

Keep help within 100 columns: enable clap's `wrap_help` feature, parse with
`cap_help_width(Cli::command(), 100)` (clap's own `max_term_width` covers one
command, not its subcommands), and test with `help_lines_over` plus a run
under `COLUMNS=200`.
