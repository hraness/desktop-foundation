//! Permission notices and recovery (`docs/permissions.md`). The Rust twin of
//! `@hraness/desktop-foundation/permissions`: rendered strings are byte for
//! byte the same, and both suites check the same golden files.
//!
//! The kit never triggers a macOS prompt. It says what macOS is about to ask
//! and why, probes state only where a probe cannot cause a prompt, explains a
//! denial in plain words, and opens the right System Settings pane when the
//! person asks.

use crate::audience::{self, Audience};
use crate::json;
use crate::style::{CliError, Style, Symbol};
use std::io::{IsTerminal as _, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Environment lookup, as everywhere in the kit.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Every macOS permission a Hraness product may need.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    FullDiskAccess,
    Automation,
    Contacts,
    Accessibility,
    ScreenRecording,
    Camera,
    Microphone,
    LocalNetwork,
    IncomingConnections,
    Notifications,
    LoginItem,
    Keychain,
    DeveloperTools,
    Gatekeeper,
}

/// What macOS does when a product needs the permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptBehavior {
    /// macOS shows a dialog that asks.
    Asks,
    /// macOS never asks; the person turns it on in System Settings.
    SettingsOnly,
    /// macOS only shows a notice.
    Notifies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

/// Why an action failed, for recovery copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryState {
    Denied,
    Unknown,
    Missing,
}

impl RecoveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            RecoveryState::Denied => "denied",
            RecoveryState::Unknown => "unknown",
            RecoveryState::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrePromptOutcome {
    /// The person pressed Enter (or the notice needs no answer).
    Continue,
    /// The person pressed s, or did not answer in time.
    Skip,
    /// Nobody can answer; the need says to go ahead.
    UnattendedProceed,
    /// Nobody can answer; the need says to stop.
    UnattendedStop,
}

/// Where the copy is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Cli,
    Dialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unattended {
    Proceed,
    Stop,
}

struct KindInfo {
    name: &'static str,
    behavior: PromptBehavior,
    pane: Option<&'static str>,
    path: Option<&'static str>,
    url: Option<&'static str>,
}

const PRIVACY: &str = "System Settings › Privacy & Security";

impl PermissionKind {
    /// Every kind, in documentation order.
    pub const ALL: [PermissionKind; 14] = [
        PermissionKind::FullDiskAccess,
        PermissionKind::Automation,
        PermissionKind::Contacts,
        PermissionKind::Accessibility,
        PermissionKind::ScreenRecording,
        PermissionKind::Camera,
        PermissionKind::Microphone,
        PermissionKind::LocalNetwork,
        PermissionKind::IncomingConnections,
        PermissionKind::Notifications,
        PermissionKind::LoginItem,
        PermissionKind::Keychain,
        PermissionKind::DeveloperTools,
        PermissionKind::Gatekeeper,
    ];

    fn info(self) -> KindInfo {
        use PermissionKind::*;
        use PromptBehavior::*;
        let privacy = |name: &'static str,
                       pane: &'static str,
                       path: &'static str,
                       url: &'static str| KindInfo {
            name,
            behavior: Asks,
            pane: Some(pane),
            path: Some(path),
            url: Some(url),
        };
        match self {
            FullDiskAccess => KindInfo {
                behavior: SettingsOnly,
                ..privacy(
                    "full-disk-access",
                    "Full Disk Access",
                    "System Settings › Privacy & Security › Full Disk Access",
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
                )
            },
            Automation => privacy(
                "automation",
                "Automation",
                "System Settings › Privacy & Security › Automation",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
            ),
            Contacts => privacy(
                "contacts",
                "Contacts",
                "System Settings › Privacy & Security › Contacts",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Contacts",
            ),
            Accessibility => privacy(
                "accessibility",
                "Accessibility",
                "System Settings › Privacy & Security › Accessibility",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
            ScreenRecording => privacy(
                "screen-recording",
                "Screen & System Audio Recording",
                "System Settings › Privacy & Security › Screen & System Audio Recording",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
            ),
            Camera => privacy(
                "camera",
                "Camera",
                "System Settings › Privacy & Security › Camera",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
            ),
            Microphone => privacy(
                "microphone",
                "Microphone",
                "System Settings › Privacy & Security › Microphone",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            ),
            LocalNetwork => privacy(
                "local-network",
                "Local Network",
                "System Settings › Privacy & Security › Local Network",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork",
            ),
            IncomingConnections => privacy(
                "incoming-connections",
                "Firewall",
                "System Settings › Network › Firewall",
                "x-apple.systempreferences:com.apple.Network-Settings.extension",
            ),
            Notifications => privacy(
                "notifications",
                "Notifications",
                "System Settings › Notifications",
                "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
            ),
            LoginItem => KindInfo {
                behavior: Notifies,
                ..privacy(
                    "login-item",
                    "Login Items & Extensions",
                    "System Settings › General › Login Items & Extensions",
                    "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
                )
            },
            Keychain => KindInfo {
                name: "keychain",
                behavior: Asks,
                pane: Some("Keychain Access"),
                path: Some("Keychain Access"),
                url: None,
            },
            DeveloperTools => KindInfo {
                name: "developer-tools",
                behavior: Asks,
                pane: None,
                path: None,
                url: None,
            },
            Gatekeeper => privacy(
                "gatekeeper",
                "Privacy & Security",
                PRIVACY,
                "x-apple.systempreferences:com.apple.preference.security?General",
            ),
        }
    }

    /// The wire name: `full-disk-access`.
    pub fn as_str(self) -> &'static str {
        self.info().name
    }

    /// The kind for a wire name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    pub fn behavior(self) -> PromptBehavior {
        self.info().behavior
    }

    /// The pane's name: `Full Disk Access`.
    pub fn pane_name(self) -> Option<&'static str> {
        self.info().pane
    }

    /// Where the pane is: `System Settings › Privacy & Security › Full Disk Access`.
    pub fn settings_path(self) -> Option<&'static str> {
        self.info().path
    }

    /// The allowlisted `x-apple.systempreferences:` URL for the pane.
    pub fn settings_url(self) -> Option<&'static str> {
        self.info().url
    }

    /// True for kinds with a System Settings pane.
    pub fn has_settings_pane(self) -> bool {
        self.settings_url().is_some()
    }
}

/// The allowlist: every URL the kit and `foundation.settings.<kind>` may open.
pub fn is_allowed_settings_url(url: &str) -> bool {
    PermissionKind::ALL
        .iter()
        .any(|kind| kind.settings_url() == Some(url))
}

/// The product a notice is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRef {
    /// Display name from the portfolio registry: "Textbutler".
    pub product: String,
    /// CLI name used in next steps: "textbutler".
    pub command: String,
    /// The name macOS shows in its dialog. Defaults depend on the kind.
    pub requester: Option<String>,
}

impl ProductRef {
    pub fn new(product: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            product: product.into(),
            command: command.into(),
            requester: None,
        }
    }

    pub fn with_requester(mut self, requester: impl Into<String>) -> Self {
        self.requester = Some(requester.into());
        self
    }
}

/// One permission a product needs, with the words for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionNeed {
    pub product: ProductRef,
    pub kind: PermissionKind,
    /// "Messages", "Chrome Safe Storage".
    pub target: Option<String>,
    /// Verb phrase after "let X", no final period: "control Messages".
    pub ask: String,
    /// One sentence, at most 110 characters, ending with a period.
    pub why: String,
    /// Recovery command; defaults to `{command} doctor`.
    pub next: Option<String>,
    /// Default: proceed for kinds that only notify, stop otherwise.
    pub when_unattended: Option<Unattended>,
}

impl PermissionNeed {
    pub fn new(
        product: ProductRef,
        kind: PermissionKind,
        ask: impl Into<String>,
        why: impl Into<String>,
    ) -> Self {
        Self {
            product,
            kind,
            target: None,
            ask: ask.into(),
            why: why.into(),
            next: None,
            when_unattended: None,
        }
    }

    pub fn with_next(mut self, next: impl Into<String>) -> Self {
        self.next = Some(next.into());
        self
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_requester(mut self, requester: impl Into<String>) -> Self {
        self.product.requester = Some(requester.into());
        self
    }

    pub fn with_unattended(mut self, when: Unattended) -> Self {
        self.when_unattended = Some(when);
        self
    }

    fn next_step(&self) -> String {
        self.next
            .clone()
            .unwrap_or_else(|| format!("{} doctor", self.product.command))
    }

    fn unattended(&self) -> PrePromptOutcome {
        let when = self.when_unattended.unwrap_or(match self.kind.behavior() {
            PromptBehavior::Notifies => Unattended::Proceed,
            _ => Unattended::Stop,
        });
        match when {
            Unattended::Proceed => PrePromptOutcome::UnattendedProceed,
            Unattended::Stop => PrePromptOutcome::UnattendedStop,
        }
    }
}

/// Rendered copy for one surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedNotice {
    /// Dialog title; for the CLI, the first line without its symbol.
    pub title: String,
    /// Following CLI lines without indentation, or the dialog message.
    pub lines: Vec<String>,
    /// Pre-prompt: the confirm line. Recovery: the "press o" hint for the `→`
    /// line. Only shown when stdin and stderr are terminals.
    pub confirm: Option<String>,
    /// Recovery only: the one next step, printed after `→`.
    pub next: Option<String>,
}

impl RenderedNotice {
    /// `{"title","lines","next"}` as the TypeScript kit serializes it.
    pub fn to_json(&self) -> String {
        json::Object::new()
            .str("title", &self.title)
            .strings("lines", &self.lines)
            .opt_str("confirm", self.confirm.as_deref())
            .opt_str("next", self.next.as_deref())
            .finish()
    }
}

const TERMINALS: [(&str, &str, &str); 7] = [
    ("com.apple.Terminal", "Apple_Terminal", "Terminal"),
    ("com.googlecode.iterm2", "iTerm.app", "iTerm"),
    ("com.mitchellh.ghostty", "ghostty", "Ghostty"),
    ("com.microsoft.VSCode", "vscode", "Visual Studio Code"),
    ("dev.zed.Zed", "zed", "Zed"),
    ("dev.warp.Warp-Stable", "WarpTerminal", "Warp"),
    ("com.github.wez.wezterm", "WezTerm", "WezTerm"),
];

/// The app macOS names in a privacy prompt for this process. Inside a
/// product's local app (its launcher sets `HRANESS_APP_BUNDLE_ID`) that is
/// the product; otherwise the terminal app that started the command.
pub fn responsible_app(env: Env, product: Option<&str>) -> String {
    // An empty product name is unset, as a falsy `product` is in TS.
    if let Some(product) = product.filter(|name| !name.is_empty()) {
        if env("HRANESS_APP_BUNDLE_ID").is_some_and(|value| !value.is_empty()) {
            return product.to_owned();
        }
    }
    let bundle = env("__CFBundleIdentifier").unwrap_or_default();
    if let Some((_, _, name)) = TERMINALS.iter().find(|(id, _, _)| *id == bundle) {
        return (*name).to_owned();
    }
    let program = env("TERM_PROGRAM").unwrap_or_default();
    if let Some((_, _, name)) = TERMINALS.iter().find(|(_, id, _)| *id == program) {
        return (*name).to_owned();
    }
    if env("ZED_TERM").is_some_and(|value| !value.is_empty()) {
        return "Zed".to_owned();
    }
    "your terminal app".to_owned()
}

/// The requester for a need: explicit, else the kind's default.
pub fn requester_of(need: &PermissionNeed, env: Env) -> String {
    if let Some(requester) = need.product.requester.as_ref().filter(|r| !r.is_empty()) {
        return requester.clone();
    }
    match need.kind {
        PermissionKind::Keychain => "security".to_owned(),
        PermissionKind::IncomingConnections => need.product.command.clone(),
        // Login Items lists the program the login item runs, never the terminal.
        PermissionKind::LoginItem => need.product.product.clone(),
        _ => responsible_app(env, Some(&need.product.product)),
    }
}

fn for_product(requester: &str, need: &PermissionNeed) -> String {
    if requester == need.product.product {
        String::new()
    } else {
        format!(" for {}", need.product.product)
    }
}

/// The pre-prompt copy for one surface. Pure; `env` only resolves the
/// default requester.
pub fn render_pre_prompt(need: &PermissionNeed, surface: Surface, env: Env) -> RenderedNotice {
    let kind = need.kind;
    let (behavior, pane, path) = (
        kind.behavior(),
        kind.pane_name().unwrap_or(""),
        kind.settings_path().unwrap_or(""),
    );
    let requester = requester_of(need, env);
    let product = &need.product.product;
    let (title, detail, confirm) = if kind == PermissionKind::DeveloperTools {
        (
            format!("{product} needs Apple's command line tools to {}. macOS will offer to install them (about 1 GB).", need.ask),
            need.why.clone(),
            Some("Press Enter to continue · s to skip"),
        )
    } else if behavior == PromptBehavior::Notifies {
        let thats = if requester == *product {
            String::new()
        } else {
            format!(". That's how {product} starts in the background")
        };
        (
            format!("macOS will show a notice that {requester} can open at login{thats}."),
            format!("{} Turn it off any time in {path}.", need.why),
            None,
        )
    } else if behavior == PromptBehavior::SettingsOnly {
        (
            format!("{product} needs {pane} to {}.", need.ask),
            format!(
                "macOS doesn't ask for this. Turn on {requester} in {path}. {}",
                need.why
            ),
            Some("Press Enter to open Settings · s to skip"),
        )
    } else {
        let detail = if kind == PermissionKind::Keychain {
            format!("{} Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.", need.why)
        } else {
            format!("{} Change this any time in {path}.", need.why)
        };
        (
            format!(
                "macOS will ask to let {requester} {}{}.",
                need.ask,
                for_product(&requester, need)
            ),
            detail,
            Some("Press Enter to continue · s to skip"),
        )
    };
    if surface == Surface::Cli {
        return RenderedNotice {
            title,
            lines: vec![detail],
            confirm: confirm.map(str::to_owned),
            next: None,
        };
    }
    let heading = if behavior == PromptBehavior::SettingsOnly {
        format!("{product} needs {pane}")
    } else if kind == PermissionKind::DeveloperTools {
        format!("{product} needs Apple's command line tools")
    } else {
        format!(
            "{product} needs access to {}",
            need.target.as_deref().unwrap_or(pane)
        )
    };
    RenderedNotice {
        title: heading,
        lines: vec![format!("{title} {detail}")],
        confirm: None,
        next: None,
    }
}

/// The recovery copy after a denial or an unexplained failure.
pub fn render_recovery(
    need: &PermissionNeed,
    state: RecoveryState,
    surface: Surface,
    env: Env,
) -> RenderedNotice {
    let kind = need.kind;
    let (pane, path, url) = (kind.pane_name(), kind.settings_path(), kind.settings_url());
    let requester = requester_of(need, env);
    let product = &need.product.product;
    let ask = &need.ask;
    let mut step = need.next_step();
    let mut confirm = None;
    let (title, lines): (String, Vec<String>) = if kind == PermissionKind::DeveloperTools {
        step = "xcode-select --install".to_owned();
        (
            format!("{product} needs Apple's command line tools. Nothing was installed."),
            vec![],
        )
    } else if kind == PermissionKind::Keychain && state == RecoveryState::Denied {
        (
            format!("{product} can't {ask}: the keychain request was denied."),
            vec!["Run it again and choose Always Allow when macOS asks.".to_owned()],
        )
    } else if kind == PermissionKind::Keychain && state == RecoveryState::Missing {
        let title = match &need.target {
            Some(target) => format!("{product} can't find \"{target}\" in your keychain."),
            None => format!("{product} can't {ask}: the item isn't in your keychain."),
        };
        (title, vec![])
    } else if kind == PermissionKind::Keychain {
        (
            format!("{product} couldn't {ask}. macOS may be blocking {requester}."),
            vec!["Unlock your login keychain, then retry.".to_owned()],
        )
    } else if state == RecoveryState::Denied {
        if url.is_some() {
            confirm = Some("press o to open Settings".to_owned());
        }
        (
            format!("{product} can't {ask}: macOS access is off for {requester}."),
            vec![format!("Turn on {requester} in {}.", path.unwrap_or(""))],
        )
    } else {
        (
            format!("{product} couldn't {ask}. macOS may be blocking {requester}."),
            path.map(|path| vec![format!("Check {path}.")])
                .unwrap_or_default(),
        )
    };
    if surface == Surface::Cli {
        return RenderedNotice {
            title,
            lines,
            confirm,
            next: Some(step),
        };
    }
    let heading = if kind == PermissionKind::DeveloperTools {
        format!("{product} needs Apple's command line tools")
    } else if state == RecoveryState::Denied && pane.is_some() && kind != PermissionKind::Keychain {
        format!("{} is off for {product}", pane.unwrap_or(""))
    } else {
        format!("{product} can't {ask}")
    };
    let mut message = vec![title];
    message.extend(lines);
    RenderedNotice {
        title: heading,
        lines: vec![message.join(" ")],
        confirm: None,
        next: Some(step),
    }
}

/// Which kind of CLI text [`format_notice`] prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// `🔐 …`, indented detail, and the confirm line.
    PrePrompt,
    /// `✗ …`, indented lines, and `→ next`.
    Recovery,
}

/// Plain text for a rendered CLI notice or recovery. `interactive` shows the
/// confirm or "press o" hint.
pub fn format_notice(
    notice: &RenderedNotice,
    kind: NoticeKind,
    interactive: bool,
    style: Style,
) -> String {
    let mut out = Vec::new();
    match kind {
        NoticeKind::PrePrompt => {
            out.push(style.line(Symbol::Notice, &notice.title));
            out.extend(notice.lines.iter().map(|line| format!("   {line}")));
            if let (true, Some(confirm)) = (interactive, &notice.confirm) {
                out.push(format!("   {confirm}"));
            }
        }
        NoticeKind::Recovery => {
            out.push(style.line(Symbol::Fail, &notice.title));
            out.extend(notice.lines.iter().map(|line| format!("  {line}")));
            if let Some(next) = &notice.next {
                let hint = match (interactive, &notice.confirm) {
                    (true, Some(confirm)) => format!(" · {confirm}"),
                    _ => String::new(),
                };
                out.push(format!("{}{hint}", style.line(Symbol::Next, next)));
            }
        }
    }
    out.join("\n") + "\n"
}

/// The helper `--notice` dialog request (`docs/protocol.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeRequest {
    pub title: String,
    pub message: String,
    pub primary: String,
    pub secondary: Option<String>,
    /// A permission kind whose pane the runner opens when the person chooses
    /// the primary button.
    pub settings: Option<PermissionKind>,
}

impl NoticeRequest {
    /// One JSON frame for `hraness-companion --notice`.
    pub fn to_json(&self) -> String {
        json::Object::new()
            .str("type", "notice-request")
            .raw("version", "1")
            .str("title", &self.title)
            .str("message", &self.message)
            .str("primary", &self.primary)
            .opt_str("secondary", self.secondary.as_deref())
            .opt_str("settings", self.settings.map(PermissionKind::as_str))
            .finish()
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// The `--notice` dialog request for a need, or `None` when no dialog
/// applies (login items: running the login-item command is the consent).
pub fn notice_request(need: &PermissionNeed, env: Env) -> Option<NoticeRequest> {
    let behavior = need.kind.behavior();
    if behavior == PromptBehavior::Notifies {
        return None;
    }
    let rendered = render_pre_prompt(need, Surface::Dialog, env);
    let settings = (behavior == PromptBehavior::SettingsOnly && need.kind.has_settings_pane())
        .then_some(need.kind);
    Some(NoticeRequest {
        title: clip(&rendered.title, 128),
        message: clip(&rendered.lines.join(" "), 512),
        primary: if settings.is_some() {
            "Open System Settings".to_owned()
        } else {
            "Continue".to_owned()
        },
        secondary: Some("Not now".to_owned()),
        settings,
    })
}

/// The `--json` error fields for a permission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionErrorInfo {
    /// `permission-denied`, `permission-unknown` or `permission-missing`.
    pub code: String,
    pub kind: PermissionKind,
    pub message: String,
    pub next: String,
    pub settings_url: Option<&'static str>,
}

pub fn permission_error(
    need: &PermissionNeed,
    state: RecoveryState,
    env: Env,
) -> PermissionErrorInfo {
    let recovery = render_recovery(need, state, Surface::Cli, env);
    PermissionErrorInfo {
        code: format!("permission-{}", state.as_str()),
        kind: need.kind,
        message: recovery.title,
        next: recovery.next.unwrap_or_else(|| need.next_step()),
        settings_url: need.kind.settings_url(),
    }
}

/// The whole `--json` error document:
/// `{"ok":false,"error":{…,"permission":{"kind","settingsUrl"}}}`.
pub fn permission_error_json(need: &PermissionNeed, state: RecoveryState, env: Env) -> String {
    permission_cli_error(need, state, env).render_json()
}

/// A permission failure as a [`CliError`]: its JSON form is the documented
/// shape, and its text form is the one-line version of the recovery. Use
/// [`report_permission_failure`] for the full recovery at a terminal.
pub fn permission_cli_error(need: &PermissionNeed, state: RecoveryState, env: Env) -> CliError {
    let info = permission_error(need, state, env);
    let mut error = CliError::new(info.code, info.message).with_next(info.next);
    error.permission = Some((
        info.kind.as_str().to_owned(),
        info.settings_url.map(str::to_owned),
    ));
    error
}

/// Classifies a keychain OSStatus, or the exit status of `/usr/bin/security`
/// (the OSStatus's low byte: 44 not found, 36 locked, 51 and 128 denied).
/// `None` for success and unrelated codes.
pub fn classify_keychain_status(status: i32) -> Option<RecoveryState> {
    const CODES: [(i32, RecoveryState); 4] = [
        (-128, RecoveryState::Denied),
        (-25293, RecoveryState::Denied),
        (-25308, RecoveryState::Unknown),
        (-25300, RecoveryState::Missing),
    ];
    if let Some((_, state)) = CODES.iter().find(|(code, _)| *code == status) {
        return Some(*state);
    }
    if status > 0 && status < 256 {
        return CODES
            .iter()
            .find(|(code, _)| code & 0xff == status)
            .map(|(_, state)| *state);
    }
    None
}

/// A key the person pressed at a pre-prompt or recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Skip,
    Open,
    Timeout,
}

/// What a file probe found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAccess {
    Ok,
    Denied,
    Missing,
    /// Any other failure: the probe says nothing.
    Unreadable,
}

/// Everything the kit touches, injectable for tests.
pub trait PermissionIo {
    fn env(&self, key: &str) -> Option<String>;
    fn stdin_is_tty(&self) -> bool;
    fn stderr_is_tty(&self) -> bool;
    /// Writes to stderr.
    fn write(&mut self, text: &str) -> std::io::Result<()>;
    fn read_key(&mut self, timeout: Duration) -> std::io::Result<Key>;
    /// Opens an allowlisted `x-apple.systempreferences:` URL.
    fn open_url(&mut self, url: &str) -> std::io::Result<bool>;
    /// Opens `path` for reading and closes it.
    fn file_access(&self, path: &Path) -> FileAccess {
        match std::fs::File::open(path) {
            Ok(_) => FileAccess::Ok,
            Err(error) => match error.kind() {
                std::io::ErrorKind::PermissionDenied => FileAccess::Denied,
                std::io::ErrorKind::NotFound => FileAccess::Missing,
                _ if error.raw_os_error() == Some(20) => FileAccess::Missing, // ENOTDIR
                _ => FileAccess::Unreadable,
            },
        }
    }
    /// Runs a command with no input or output and returns its exit status.
    /// `None` when it does not start or does not finish in five seconds,
    /// like the TypeScript kit's `runQuietly` (which reports both as 127).
    fn run_status(&self, argv: &[&str]) -> Option<i32> {
        let (program, args) = argv.split_first()?;
        let mut child = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Err(_) => return None,
            }
        }
    }
}

/// The real process: environment, terminal checks, stderr, and `open` on
/// macOS. On Unix a key is one keypress with no Enter: `stty` switches the
/// terminal to single-key mode for the read and restores it after, and a
/// timeout stops the reading `dd`, so nothing is left reading stdin. Ctrl-C
/// during the read restores the terminal and exits 130. Elsewhere a key is a
/// line (Enter, `s` then Enter, `o` then Enter).
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessIo;

impl PermissionIo for ProcessIo {
    fn env(&self, key: &str) -> Option<String> {
        audience::process_env(key)
    }
    fn stdin_is_tty(&self) -> bool {
        std::io::stdin().is_terminal()
    }
    fn stderr_is_tty(&self) -> bool {
        std::io::stderr().is_terminal()
    }
    fn write(&mut self, text: &str) -> std::io::Result<()> {
        let mut err = std::io::stderr().lock();
        err.write_all(text.as_bytes())?;
        err.flush()
    }
    fn read_key(&mut self, timeout: Duration) -> std::io::Result<Key> {
        if !std::io::stdin().is_terminal() {
            return Ok(Key::Timeout);
        }
        // One read at a time, like the TS kit's `interactiveKeyBusy`: a
        // second caller times out instead of racing for the same keypress.
        static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Ok(Key::Timeout);
        }
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = Guard;
        #[cfg(unix)]
        {
            read_one_key(timeout)
        }
        #[cfg(not(unix))]
        {
            read_key_line(timeout)
        }
    }
    fn open_url(&mut self, url: &str) -> std::io::Result<bool> {
        if !cfg!(target_os = "macos") || !is_allowed_settings_url(url) {
            return Ok(false);
        }
        Ok(self.run_status(&["/usr/bin/open", url]) == Some(0))
    }
}

/// One keypress from the terminal on stdin, or `Timeout`. `stty` switches
/// the terminal to single-key mode for the read and restores it after.
/// Without `stty` (or `dd`) a line read still works — it just needs Enter.
#[cfg(unix)]
fn read_one_key(timeout: Duration) -> std::io::Result<Key> {
    use std::process::{Command, Stdio};
    let stty = |args: &[&str]| {
        Command::new("/bin/stty")
            .args(args)
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()
    };
    let saved = stty(&["-g"])
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(saved) = saved else {
        return read_key_line(timeout);
    };
    // No line buffering, no echo, and Ctrl-C arrives as a byte, so the
    // terminal is always put back before the process ends.
    let raw = stty(&["-icanon", "-echo", "-isig", "min", "1", "time", "0"])
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !raw {
        return read_key_line(timeout);
    }
    let deadline = std::time::Instant::now() + timeout;
    // `dd` reads one byte at a time so nothing is left holding stdin after a
    // timeout. Unrecognized bytes are ignored, like the TS kit's key loop.
    let read = (|| -> std::io::Result<Option<u8>> {
        while std::time::Instant::now() < deadline {
            let mut child = match Command::new("/bin/dd")
                .args(["bs=1", "count=1"])
                .stdin(Stdio::inherit())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(_) => return Ok(None),
            };
            let byte = loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        let mut buf = Vec::new();
                        if let Some(mut out) = child.stdout.take() {
                            out.read_to_end(&mut buf)?;
                        }
                        break buf.first().copied();
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(None);
                    }
                    Err(error) => return Err(error),
                }
            };
            match byte {
                // The bytes that answer the prompt: Enter, o, s, Escape and
                // Ctrl-C. Anything else does not answer; keep waiting.
                Some(byte @ (b'\n' | b'\r' | b'o' | b'O' | b's' | b'S' | 0x1b | 0x03)) => {
                    return Ok(Some(byte))
                }
                Some(_) => {}
                // dd closed without a byte: stdin is gone.
                None => return Ok(None),
            }
        }
        Ok(None)
    })();
    let _ = stty(&[&saved]);
    match read? {
        Some(0x03) => std::process::exit(130),
        Some(b'\n' | b'\r') => Ok(Key::Enter),
        Some(b'o' | b'O') => Ok(Key::Open),
        Some(_) => Ok(Key::Skip),
        None => Ok(Key::Timeout),
    }
}

/// One line from stdin, or `Timeout`. A read still waiting after a timeout
/// is kept and answers the next call, so it never swallows a later line.
fn read_key_line(timeout: Duration) -> std::io::Result<Key> {
    use std::sync::mpsc::Receiver;
    use std::sync::Mutex;
    type Pending = Receiver<std::io::Result<(usize, String)>>;
    static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let receive = match pending.take() {
        Some(receive) => receive,
        None => {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                let read = std::io::stdin().read_line(&mut line);
                let _ = send.send(read.map(|count| (count, line)));
            });
            receive
        }
    };
    match receive.recv_timeout(timeout) {
        Ok(Ok((0, _))) => Ok(Key::Skip),
        Ok(Ok((_, line))) => Ok(match line.trim().to_ascii_lowercase().as_str() {
            "" => Key::Enter,
            "o" => Key::Open,
            _ => Key::Skip,
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => {
            *pending = Some(receive);
            Ok(Key::Timeout)
        }
    }
}

/// Opens the pane for an explicit human action only. `false` when the kind
/// has no pane or `open` failed.
pub fn open_settings(kind: PermissionKind, io: &mut dyn PermissionIo) -> bool {
    match kind.settings_url() {
        Some(url) if is_allowed_settings_url(url) => io.open_url(url).unwrap_or(false),
        _ => false,
    }
}

fn io_env(io: &dyn PermissionIo) -> impl Fn(&str) -> Option<String> + '_ {
    move |key| io.env(key)
}

/// Shows the notice for the audience, waits for Enter or s when a confirm
/// applies and both stdin and stderr are terminals, and never triggers the
/// macOS prompt itself. `audience: None` detects it from `io`.
pub fn pre_prompt(
    need: &PermissionNeed,
    audience: Option<Audience>,
    io: &mut dyn PermissionIo,
) -> std::io::Result<PrePromptOutcome> {
    pre_prompt_with_timeout(need, audience, io, Duration::from_secs(120))
}

/// [`pre_prompt`] with a custom answer timeout (default 120 seconds).
pub fn pre_prompt_with_timeout(
    need: &PermissionNeed,
    audience: Option<Audience>,
    io: &mut dyn PermissionIo,
    timeout: Duration,
) -> std::io::Result<PrePromptOutcome> {
    let (notice, style, interactive, audience) = {
        let env = io_env(io);
        let audience = audience.unwrap_or_else(|| audience::detect(&env, io.stderr_is_tty()));
        let notice = render_pre_prompt(need, Surface::Cli, &env);
        let style = Style::detect(&env, io.stderr_is_tty());
        (
            notice,
            style,
            io.stdin_is_tty() && io.stderr_is_tty(),
            audience,
        )
    };
    let unattended = need.unattended();
    match audience {
        Audience::Quiet => return Ok(unattended),
        Audience::Agent => {
            let mut message = vec![notice.title.clone()];
            message.extend(notice.lines.iter().cloned());
            let line = json::Object::new()
                .str("type", "permission-notice")
                .str("product", &need.product.product)
                .str("kind", need.kind.as_str())
                .str("message", &message.join(" "))
                .finish();
            io.write(&(line + "\n"))?;
            return Ok(unattended);
        }
        Audience::Human => {}
    }
    io.write(&format_notice(
        &notice,
        NoticeKind::PrePrompt,
        interactive,
        style,
    ))?;
    if notice.confirm.is_none() {
        return Ok(PrePromptOutcome::Continue);
    }
    if !interactive {
        return Ok(unattended);
    }
    let key = io.read_key(timeout)?;
    if matches!(key, Key::Skip | Key::Timeout) {
        return Ok(PrePromptOutcome::Skip);
    }
    if need.kind.behavior() == PromptBehavior::SettingsOnly || key == Key::Open {
        open_settings(need.kind, io);
    }
    Ok(PrePromptOutcome::Continue)
}

/// Prints the recovery for a failure on stderr (human and quiet audiences)
/// and opens Settings when the person presses o. Prints nothing for agents:
/// print [`permission_error_json`] on stdout for them instead.
pub fn report_permission_failure(
    need: &PermissionNeed,
    state: RecoveryState,
    audience: Option<Audience>,
    io: &mut dyn PermissionIo,
) -> std::io::Result<()> {
    let (recovery, style, interactive) = {
        let env = io_env(io);
        let audience = audience.unwrap_or_else(|| audience::detect(&env, io.stderr_is_tty()));
        if audience == Audience::Agent {
            return Ok(());
        }
        let recovery = render_recovery(need, state, Surface::Cli, &env);
        let style = Style::detect(&env, io.stderr_is_tty()).for_audience(audience);
        let interactive = audience == Audience::Human && io.stdin_is_tty() && io.stderr_is_tty();
        (recovery, style, interactive)
    };
    io.write(&format_notice(
        &recovery,
        NoticeKind::Recovery,
        interactive,
        style,
    ))?;
    if interactive
        && recovery.confirm.is_some()
        && io.read_key(Duration::from_secs(30))? == Key::Open
    {
        open_settings(need.kind, io);
    }
    Ok(())
}

/// Only `~/Library` data outside other apps' containers and cloud folders is
/// guarded by Full Disk Access alone. Documents, Desktop, Downloads, iCloud
/// Drive, removable volumes and app containers each raise their own prompt.
fn is_silent_probe_path(path: &Path, home: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(home.join("Library")) else {
        return false;
    };
    match rest.components().next() {
        None => false,
        // APFS names are case-insensitive: `containers` is `Containers`.
        Some(Component::Normal(first)) => !first.to_str().is_some_and(|first| {
            [
                "Containers",
                "Group Containers",
                "Mobile Documents",
                "CloudStorage",
                "Daemon Containers",
            ]
            .iter()
            .any(|name| first.eq_ignore_ascii_case(name))
        }),
        Some(_) => false,
    }
}

fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// The real home directory when `HOME` is missing or relative, like the
/// TypeScript kit's `homedir()` fallback. `std::env::home_dir` is deprecated
/// only between Rust 1.29 and 1.86; this crate's MSRV is older, so the allow
/// stays until the MSRV passes 1.86.
fn real_home() -> Option<PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir().filter(|home| home.is_absolute())
}

/// Read-only probe that never causes a prompt; `Unknown` when no probe is
/// safe. `FullDiskAccess` takes a target (`Messages`, `Safari` or an absolute
/// path outside app containers), `LoginItem` takes the product's app ID, and
/// `DeveloperTools` runs `xcode-select -p`, which never opens the install
/// dialog.
pub fn permission_status(
    kind: PermissionKind,
    target: Option<&str>,
    io: &dyn PermissionIo,
) -> PermissionState {
    let home = io
        .env("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .or_else(real_home);
    match kind {
        PermissionKind::FullDiskAccess => {
            let (Some(home), Some(target)) = (home, target) else {
                return PermissionState::Unknown;
            };
            let path = match target {
                "Messages" => home.join("Library/Messages/chat.db"),
                "Safari" => home.join("Library/Safari"),
                path if Path::new(path).is_absolute() => match normalize(Path::new(path)) {
                    Some(path) => path,
                    None => return PermissionState::Unknown,
                },
                _ => return PermissionState::Unknown,
            };
            if !is_silent_probe_path(&path, &home) {
                return PermissionState::Unknown;
            }
            match io.file_access(&path) {
                FileAccess::Ok => PermissionState::Granted,
                FileAccess::Denied => PermissionState::Denied,
                FileAccess::Missing | FileAccess::Unreadable => PermissionState::Unknown,
            }
        }
        PermissionKind::DeveloperTools => match io.run_status(&["/usr/bin/xcode-select", "-p"]) {
            Some(0) => PermissionState::Granted,
            // A run that could not happen reads as not-determined, like the
            // TS kit's 127.
            _ => PermissionState::NotDetermined,
        },
        PermissionKind::LoginItem => {
            let (Some(home), Some(id)) = (home, target) else {
                return PermissionState::Unknown;
            };
            let valid = id.len() <= 64
                && id.starts_with(|c: char| c.is_ascii_lowercase())
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
                && !id.contains("..");
            if !valid {
                return PermissionState::Unknown;
            }
            for label in [
                format!("app.hraness.{id}"),
                format!("app.hraness.companion.{id}"),
            ] {
                let plist = home
                    .join("Library/LaunchAgents")
                    .join(format!("{label}.plist"));
                match io.file_access(&plist) {
                    FileAccess::Ok => return PermissionState::Granted,
                    // A probe that errors says nothing, like a throw in the
                    // TS kit's `probeFile`.
                    FileAccess::Unreadable => return PermissionState::Unknown,
                    _ => {}
                }
            }
            PermissionState::NotDetermined
        }
        _ => PermissionState::Unknown,
    }
}

/// Presets fill `kind`, `target`, `ask`, `why` and the requester rule.
/// Products pass their [`ProductRef`] and the parameters shown.
pub mod presets {
    use super::*;

    /// "Open at login": notifies, so no confirm and it proceeds unattended.
    pub fn login_item(r: ProductRef) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::LoginItem,
            "open at login",
            "It starts in the background when you log in and shows no window or icon.",
        )
        .with_unattended(Unattended::Proceed)
    }

    /// A browser's cookie key in the keychain. `browser` defaults to Chrome;
    /// `caller` is the executable macOS names (`security` today).
    pub fn chrome_safe_storage(
        r: ProductRef,
        browser: Option<&str>,
        caller: &str,
        why: Option<&str>,
    ) -> PermissionNeed {
        let browser = browser.unwrap_or("Chrome");
        let why = why.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "{} uses it to read the {browser} sign-in you already have and never stores it.",
                r.product
            )
        });
        PermissionNeed::new(
            r,
            PermissionKind::Keychain,
            format!("use \"{browser} Safe Storage\" from your keychain"),
            why,
        )
        .with_requester(caller)
        .with_target(format!("{browser} Safe Storage"))
    }

    pub fn messages_fda(r: ProductRef, why: Option<&str>) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::FullDiskAccess,
            "read your Messages",
            why.unwrap_or("Only the chats you pick are read."),
        )
        .with_target("Messages")
    }

    pub fn automation(r: ProductRef, app: &str, why: &str) -> PermissionNeed {
        PermissionNeed::new(r, PermissionKind::Automation, format!("control {app}"), why)
            .with_target(app)
    }

    pub fn contacts(r: ProductRef, why: Option<&str>) -> PermissionNeed {
        let why = why
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} reads names and numbers on this Mac.", r.product));
        PermissionNeed::new(r, PermissionKind::Contacts, "see your contacts", why)
    }

    pub fn local_network(r: ProductRef, why: &str) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::LocalNetwork,
            "find and connect to devices on your local network",
            why,
        )
    }

    pub fn incoming_connections(r: ProductRef, listener: &str, why: &str) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::IncomingConnections,
            "accept incoming network connections",
            why,
        )
        .with_requester(listener)
    }

    /// Apple's command line tools. `skip_effect` says what happens without
    /// them ("algal answers without Apple Intelligence").
    pub fn xcode_tools(r: ProductRef, skip_effect: &str) -> PermissionNeed {
        let effect = skip_effect.trim_end_matches('.');
        PermissionNeed::new(
            r,
            PermissionKind::DeveloperTools,
            "build a small helper",
            format!("Nothing is installed unless you agree in that window. Or skip: {effect}."),
        )
        .with_unattended(Unattended::Stop)
    }

    pub fn screen_recording(r: ProductRef, why: &str) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::ScreenRecording,
            "record your screen",
            why,
        )
    }

    /// The local signing key, before the first signing on a Mac.
    pub fn local_signing(r: ProductRef) -> PermissionNeed {
        PermissionNeed::new(
            r,
            PermissionKind::Keychain,
            "use your \"Hraness Local Signing\" key",
            "Hraness signs its apps on this Mac with it so they keep their permissions after updates.",
        )
        .with_requester("codesign")
        .with_target("Hraness Local Signing")
    }
}
