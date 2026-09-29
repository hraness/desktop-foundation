//! One-shot native notice dialog (`hraness-helper --notice`).
//!
//! A product shows this before an action causes a macOS permission prompt,
//! or to confirm a destructive action. It reads one `notice-request` frame
//! from stdin, shows one alert with the product's own button labels, writes
//! one `notice-result` frame and exits. Like `--prompt` it needs no state
//! directory or lock. See `docs/protocol.md` § Notice dialog.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::prompt::{read_single_frame, safe_display};
use crate::wire::{ProtocolError, VERSION};

pub const MAX_TITLE: usize = 128;
pub const MAX_MESSAGE: usize = 512;
pub const MAX_BUTTON: usize = 32;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
pub const MAX_TIMEOUT_SECONDS: u64 = 600;

/// Permission kinds whose System Settings pane a notice may open, with the
/// allowlisted URL. `keychain` and `developer-tools` have no pane. The same
/// table backs the `foundation.settings.<kind>` action IDs.
pub const SETTINGS_PANES: &[(&str, &str)] = &[
    (
        "full-disk-access",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
    ),
    (
        "automation",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
    ),
    (
        "contacts",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Contacts",
    ),
    (
        "accessibility",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    ),
    (
        "screen-recording",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
    ),
    (
        "camera",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
    ),
    (
        "microphone",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
    ),
    (
        "local-network",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork",
    ),
    (
        "incoming-connections",
        "x-apple.systempreferences:com.apple.Network-Settings.extension",
    ),
    (
        "notifications",
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
    ),
    (
        "login-item",
        "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
    ),
    (
        "gatekeeper",
        "x-apple.systempreferences:com.apple.preference.security?General",
    ),
];

/// The allowlisted System Settings URL for a permission kind.
pub fn settings_url(kind: &str) -> Option<&'static str> {
    SETTINGS_PANES
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, url)| *url)
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    #[serde(rename = "type")]
    kind: String,
    version: u8,
    title: String,
    message: String,
    primary: String,
    secondary: Option<String>,
    settings: Option<String>,
    #[serde(rename = "timeoutSeconds", default = "default_timeout")]
    timeout_seconds: u64,
}

/// A validated notice request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeSpec {
    pub title: String,
    pub message: String,
    /// The default button.
    pub primary: String,
    /// The cancel button (Escape), when the notice offers a choice.
    pub secondary: Option<String>,
    /// The allowlisted Settings URL to open when the primary button is chosen.
    pub settings_url: Option<&'static str>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeStatus {
    Primary,
    Secondary,
    /// The primary button was chosen and the Settings pane opened.
    Settings,
    Timeout,
    /// No GUI session: the caller falls back to its CLI copy.
    Unavailable,
}

impl NoticeStatus {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Settings => "settings",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Serialize)]
struct WireResult {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    status: &'static str,
}

fn validate(wire: WireRequest) -> Result<NoticeSpec, ProtocolError> {
    if wire.kind != "notice-request" {
        return Err(ProtocolError("invalid-notice"));
    }
    if wire.version != VERSION {
        return Err(ProtocolError("unsupported-version"));
    }
    let settings_url = match wire.settings.as_deref() {
        None => None,
        Some(kind) => Some(settings_url(kind).ok_or(ProtocolError("invalid-notice"))?),
    };
    if !safe_display(&wire.title, MAX_TITLE)
        || !safe_display(&wire.message, MAX_MESSAGE)
        || !safe_display(&wire.primary, MAX_BUTTON)
        || wire
            .secondary
            .as_deref()
            .is_some_and(|label| !safe_display(label, MAX_BUTTON))
        || wire.secondary.as_deref() == Some(wire.primary.as_str())
        || !(1..=MAX_TIMEOUT_SECONDS).contains(&wire.timeout_seconds)
    {
        return Err(ProtocolError("invalid-notice"));
    }
    Ok(NoticeSpec {
        title: wire.title,
        message: wire.message,
        primary: wire.primary,
        secondary: wire.secondary,
        settings_url,
        timeout_seconds: wire.timeout_seconds,
    })
}

/// Reads exactly one bounded `notice-request` frame.
pub fn read_spec(reader: &mut impl BufRead) -> Result<NoticeSpec, ProtocolError> {
    let frame = read_single_frame(reader, "notice-required")?;
    let wire: WireRequest =
        serde_json::from_slice(&frame).map_err(|_| ProtocolError("invalid-notice"))?;
    validate(wire)
}

pub fn emit_result(writer: &mut impl Write, status: NoticeStatus) -> Result<(), ProtocolError> {
    let result = WireResult {
        kind: "notice-result",
        version: VERSION,
        status: status.wire(),
    };
    serde_json::to_writer(&mut *writer, &result)
        .map_err(|_| ProtocolError("output-unavailable"))?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|_| ProtocolError("output-unavailable"))
}

/// Which button the person chose, before any Settings pane is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Primary,
    Secondary,
    Timeout,
    Unavailable,
}

/// Maps a choice to the result, opening the Settings pane for a primary
/// choice when the request named one. A pane that fails to open still
/// reports `primary`, so the caller shows its own recovery copy.
pub(crate) fn resolve(
    choice: Choice,
    spec: &NoticeSpec,
    open: impl FnOnce(&str) -> bool,
) -> NoticeStatus {
    match choice {
        Choice::Primary => match spec.settings_url {
            Some(url) if open(url) => NoticeStatus::Settings,
            _ => NoticeStatus::Primary,
        },
        Choice::Secondary => NoticeStatus::Secondary,
        Choice::Timeout => NoticeStatus::Timeout,
        Choice::Unavailable => NoticeStatus::Unavailable,
    }
}

/// Shows one dialog on the calling thread, which must be the process main
/// thread. Bounded by `spec.timeout_seconds`.
pub fn run(spec: &NoticeSpec) -> NoticeStatus {
    resolve(platform::show(spec), spec, platform::open_settings)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication,
        NSApplicationActivationPolicy, NSModalResponseAbort, NSRunningApplication, NSWorkspace,
    };
    use objc2_foundation::{NSString, NSURL};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    extern "C" {
        static _dispatch_main_q: c_void;
        fn dispatch_async_f(
            queue: *const c_void,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }

    extern "C" fn abort_modal(_context: *mut c_void) {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).abortModal();
        }
    }

    pub fn show(spec: &NoticeSpec) -> Choice {
        if !crate::prompt::probe_capable() {
            return Choice::Unavailable;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return Choice::Unavailable;
        };
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        NSRunningApplication::currentApplication()
            .activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&spec.title));
        alert.setInformativeText(&NSString::from_str(&spec.message));
        alert.addButtonWithTitle(&NSString::from_str(&spec.primary));
        if let Some(secondary) = &spec.secondary {
            let button = alert.addButtonWithTitle(&NSString::from_str(secondary));
            // Escape chooses the secondary button whatever its label.
            button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        }
        static DONE: AtomicBool = AtomicBool::new(false);
        DONE.store(false, Ordering::Release);
        let timeout = spec.timeout_seconds;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(timeout));
            if !DONE.load(Ordering::Acquire) {
                unsafe { dispatch_async_f(&_dispatch_main_q, std::ptr::null_mut(), abort_modal) };
            }
        });
        let response = alert.runModal();
        DONE.store(true, Ordering::Release);
        alert.window().orderOut(None);
        if response == NSAlertFirstButtonReturn {
            Choice::Primary
        } else if response == NSAlertSecondButtonReturn {
            Choice::Secondary
        } else if response == NSModalResponseAbort {
            Choice::Timeout
        } else {
            Choice::Unavailable
        }
    }

    pub fn open_settings(url: &str) -> bool {
        let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return false;
        };
        NSWorkspace::sharedWorkspace().openURL(&url)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use gtk::prelude::*;

    const TIMEOUT_RESPONSE: u16 = 2;

    pub fn show(spec: &NoticeSpec) -> Choice {
        if !gtk::is_initialized() && gtk::init().is_err() {
            return Choice::Unavailable;
        }
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Info,
            gtk::ButtonsType::None,
            &spec.title,
        );
        dialog.set_secondary_text(Some(&spec.message));
        if let Some(secondary) = &spec.secondary {
            dialog.add_button(secondary, gtk::ResponseType::Cancel);
        }
        dialog.add_button(&spec.primary, gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Accept);
        let fired = std::rc::Rc::new(std::cell::Cell::new(false));
        let timeout = glib::timeout_add_seconds_local(
            spec.timeout_seconds.min(u32::MAX as u64) as u32,
            glib::clone!(@weak dialog, @strong fired => @default-return glib::ControlFlow::Break, move || {
                fired.set(true);
                dialog.response(gtk::ResponseType::Other(TIMEOUT_RESPONSE));
                glib::ControlFlow::Break
            }),
        );
        let response = dialog.run();
        if !fired.get() {
            timeout.remove();
        }
        dialog.close();
        match response {
            gtk::ResponseType::Accept => Choice::Primary,
            gtk::ResponseType::Other(TIMEOUT_RESPONSE) => Choice::Timeout,
            // Escape and closing the window both mean "not now".
            _ if spec.secondary.is_some() => Choice::Secondary,
            _ => Choice::Primary,
        }
    }

    /// Settings panes are macOS only.
    pub fn open_settings(_url: &str) -> bool {
        false
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::cell::Cell;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_NOTIFICATIONS,
        TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDM_CLICK_BUTTON, TDN_TIMER,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, IDCANCEL};

    const PRIMARY_ID: i32 = 100;
    const SECONDARY_ID: i32 = 101;

    struct Deadline {
        milliseconds: u64,
        fired: Cell<bool>,
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn callback(
        hwnd: HWND,
        message: TASKDIALOG_NOTIFICATIONS,
        wparam: WPARAM,
        _lparam: LPARAM,
        data: isize,
    ) -> windows::core::HRESULT {
        // For TDN_TIMER, `wparam` is the time since the dialog opened.
        let deadline = &*(data as *const Deadline);
        if message == TDN_TIMER && !deadline.fired.get() && wparam.0 as u64 >= deadline.milliseconds
        {
            deadline.fired.set(true);
            let _ = SendMessageW(
                hwnd,
                TDM_CLICK_BUTTON.0 as u32,
                Some(WPARAM(IDCANCEL.0 as usize)),
                Some(LPARAM(0)),
            );
        }
        S_OK
    }

    pub fn show(spec: &NoticeSpec) -> Choice {
        // No interactive desktop (SSH, a service, headless CI): let the
        // caller use its CLI copy instead of waiting out the timeout.
        if !crate::prompt::probe_capable() {
            return Choice::Unavailable;
        }
        let title = wide(&spec.title);
        let message = wide(&spec.message);
        let primary = wide(&spec.primary);
        let secondary = spec.secondary.as_deref().map(wide);
        let mut buttons = vec![TASKDIALOG_BUTTON {
            nButtonID: PRIMARY_ID,
            pszButtonText: PCWSTR(primary.as_ptr()),
        }];
        if let Some(secondary) = &secondary {
            buttons.push(TASKDIALOG_BUTTON {
                nButtonID: SECONDARY_ID,
                pszButtonText: PCWSTR(secondary.as_ptr()),
            });
        }
        let deadline = Deadline {
            milliseconds: spec.timeout_seconds * 1000,
            fired: Cell::new(false),
        };
        let config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER,
            pszWindowTitle: PCWSTR(title.as_ptr()),
            pszMainInstruction: PCWSTR(title.as_ptr()),
            pszContent: PCWSTR(message.as_ptr()),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: PRIMARY_ID,
            pfCallback: Some(callback),
            lpCallbackData: &deadline as *const Deadline as isize,
            ..Default::default()
        };
        let mut chosen = 0i32;
        let result =
            unsafe { TaskDialogIndirect(&config, Some(&mut chosen as *mut i32), None, None) };
        if result.is_err() {
            return Choice::Unavailable;
        }
        if deadline.fired.get() {
            return Choice::Timeout;
        }
        match chosen {
            PRIMARY_ID => Choice::Primary,
            // Escape or the close box mean "not now" when there is a choice.
            id if id == SECONDARY_ID || id == IDCANCEL.0 => {
                if spec.secondary.is_some() {
                    Choice::Secondary
                } else {
                    Choice::Primary
                }
            }
            _ => Choice::Unavailable,
        }
    }

    /// Settings panes are macOS only.
    pub fn open_settings(_url: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const DOC: &str = r#"{"type":"notice-request","version":1,"title":"Textbutler needs access to Messages","message":"macOS will ask to let Textbutler control Messages. Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.","primary":"Continue","secondary":"Not now","timeoutSeconds":120}"#;

    fn parse(json: &str) -> Result<NoticeSpec, ProtocolError> {
        read_spec(&mut Cursor::new(format!("{json}\n")))
    }

    #[test]
    fn the_documented_request_is_accepted() {
        assert!(include_str!("../../../docs/protocol.md").contains(DOC));
        let spec = parse(DOC).unwrap();
        assert_eq!(spec.primary, "Continue");
        assert_eq!(spec.secondary.as_deref(), Some("Not now"));
        assert_eq!(spec.settings_url, None);
        assert_eq!(spec.timeout_seconds, 120);
    }

    #[test]
    fn settings_kinds_map_to_the_allowlisted_panes() {
        let json = DOC.replace(
            "\"timeoutSeconds\":120",
            "\"settings\":\"full-disk-access\"",
        );
        let spec = parse(&json).unwrap();
        assert_eq!(
            spec.settings_url,
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        );
        assert_eq!(spec.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        for kind in ["keychain", "developer-tools", "https://example.com", ""] {
            let json = DOC.replace(
                "\"timeoutSeconds\":120",
                &format!("\"settings\":\"{kind}\""),
            );
            assert_eq!(parse(&json), Err(ProtocolError("invalid-notice")), "{kind}");
        }
        // docs/permissions.md lists the same kinds and panes.
        let doc = include_str!("../../../docs/permissions.md");
        for (kind, url) in SETTINGS_PANES {
            assert!(
                doc.contains(&format!("| `{kind}` |")) && doc.contains(url),
                "{kind}"
            );
        }
    }

    #[test]
    fn bad_requests_are_rejected_without_echoing_input() {
        let cases = [
            DOC.replace("notice-request", "prompt-request"),
            DOC.replace("\"primary\":\"Continue\"", "\"primary\":\"\""),
            DOC.replace(
                "\"primary\":\"Continue\"",
                &format!("\"primary\":\"{}\"", "x".repeat(33)),
            ),
            DOC.replace("\"secondary\":\"Not now\"", "\"secondary\":\"Continue\""),
            DOC.replace("\"timeoutSeconds\":120", "\"timeoutSeconds\":0"),
            DOC.replace("\"timeoutSeconds\":120", "\"timeoutSeconds\":601"),
            DOC.replace(
                "\"timeoutSeconds\":120",
                "\"timeoutSeconds\":120,\"secret\":true",
            ),
            DOC.replace("Textbutler needs access", "Textbutler\\u0007needs access"),
        ];
        for json in cases {
            assert_eq!(parse(&json), Err(ProtocolError("invalid-notice")), "{json}");
        }
        assert_eq!(
            parse(&DOC.replace("\"version\":1", "\"version\":2")),
            Err(ProtocolError("unsupported-version"))
        );
        assert_eq!(
            read_spec(&mut Cursor::new("")),
            Err(ProtocolError("notice-required"))
        );
        assert_eq!(
            read_spec(&mut Cursor::new(format!("{DOC}\n{DOC}\n"))),
            Err(ProtocolError("trailing-input"))
        );
    }

    #[test]
    fn choices_resolve_and_open_only_the_named_pane() {
        let mut spec = parse(DOC).unwrap();
        let never = |_: &str| -> bool { panic!("no pane requested") };
        assert_eq!(
            resolve(Choice::Primary, &spec, never),
            NoticeStatus::Primary
        );
        assert_eq!(
            resolve(Choice::Secondary, &spec, never),
            NoticeStatus::Secondary
        );
        assert_eq!(
            resolve(Choice::Timeout, &spec, never),
            NoticeStatus::Timeout
        );
        assert_eq!(
            resolve(Choice::Unavailable, &spec, never),
            NoticeStatus::Unavailable
        );
        spec.settings_url = settings_url("automation");
        let mut opened = None;
        assert_eq!(
            resolve(Choice::Primary, &spec, |url| {
                opened = Some(url.to_owned());
                true
            }),
            NoticeStatus::Settings
        );
        assert_eq!(opened.as_deref(), settings_url("automation"));
        assert_eq!(
            resolve(Choice::Primary, &spec, |_| false),
            NoticeStatus::Primary
        );
        assert_eq!(
            resolve(Choice::Secondary, &spec, |_| panic!("not chosen")),
            NoticeStatus::Secondary
        );
        let mut out = Vec::new();
        emit_result(&mut out, NoticeStatus::Settings).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"type\":\"notice-result\",\"version\":1,\"status\":\"settings\"}\n"
        );
    }
}
