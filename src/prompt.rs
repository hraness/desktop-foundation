//! Bounded native credential prompt for product CLIs.
//!
//! `hraness-companion --prompt` reads one `prompt-request` frame from stdin,
//! renders exactly one OS dialog, writes one `prompt-result` frame to stdout,
//! and exits. Secret values travel only over the private stdio channel: never
//! argv, logs, menu labels, or tooltips. Buffers are zeroized after use.
//!
//! `--prompt-probe` reports whether this host can render the dialog without
//! showing one, so headless callers and CI can fall back to a TUI prompt.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::protocol::{ProtocolError, MAX_FRAME_BYTES, VERSION};

pub const MAX_TITLE: usize = 128;
pub const MAX_MESSAGE: usize = 512;
pub const MAX_VALUE_CHARS: usize = 4096;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
pub const MAX_TIMEOUT_SECONDS: u64 = 600;

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

fn default_secret() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSpec {
    #[serde(rename = "type")]
    kind: String,
    version: u8,
    title: String,
    message: String,
    #[serde(default = "default_secret")]
    secret: bool,
    prefill: Option<String>,
    #[serde(rename = "timeoutSeconds", default = "default_timeout")]
    timeout_seconds: u64,
}

/// A validated prompt request. `prefill` is a secret value: it is bounded but
/// never display-validated, and it is redacted from every diagnostic surface.
pub struct PromptSpec {
    pub title: String,
    pub message: String,
    pub secret: bool,
    pub prefill: Option<String>,
    pub timeout_seconds: u64,
}

impl std::fmt::Debug for PromptSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptSpec")
            .field("title", &self.title)
            .field("message", &self.message)
            .field("secret", &self.secret)
            .field("prefill", &self.prefill.as_ref().map(|_| "<redacted>"))
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

impl Drop for PromptSpec {
    fn drop(&mut self) {
        if let Some(value) = &mut self.prefill {
            value.zeroize();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStatus {
    Submitted,
    Cancelled,
    Timeout,
    Unavailable,
}

impl PromptStatus {
    fn wire(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One dialog outcome. `value` exists only on `Submitted` and is redacted.
#[derive(Serialize)]
pub struct PromptResult {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

impl std::fmt::Debug for PromptResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptResult")
            .field("version", &self.version)
            .field("status", &self.status)
            .field("value", &self.value.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl Drop for PromptResult {
    fn drop(&mut self) {
        if let Some(value) = &mut self.value {
            value.zeroize();
        }
    }
}

impl PromptResult {
    pub fn submitted(value: String) -> Self {
        Self {
            kind: "prompt-result",
            version: VERSION,
            status: PromptStatus::Submitted.wire(),
            value: Some(value),
        }
    }

    pub fn of(status: PromptStatus) -> Self {
        Self {
            kind: "prompt-result",
            version: VERSION,
            status: status.wire(),
            value: None,
        }
    }

    pub fn status(&self) -> PromptStatus {
        match self.status {
            "submitted" => PromptStatus::Submitted,
            "cancelled" => PromptStatus::Cancelled,
            "timeout" => PromptStatus::Timeout,
            _ => PromptStatus::Unavailable,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PromptCapability {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    capable: bool,
    detail: &'static str,
}

impl PromptCapability {
    pub fn emit(&self, writer: &mut impl Write) -> Result<(), ProtocolError> {
        serde_json::to_writer(&mut *writer, self)
            .map_err(|_| ProtocolError("output-unavailable"))?;
        writer
            .write_all(b"\n")
            .and_then(|()| writer.flush())
            .map_err(|_| ProtocolError("output-unavailable"))
    }
}

fn safe_display(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn validate(wire: WireSpec) -> Result<PromptSpec, ProtocolError> {
    if wire.kind != "prompt-request" {
        return Err(ProtocolError("invalid-prompt"));
    }
    if wire.version != VERSION {
        return Err(ProtocolError("unsupported-version"));
    }
    if !safe_display(&wire.title, MAX_TITLE) || !safe_display(&wire.message, MAX_MESSAGE) {
        return Err(ProtocolError("invalid-prompt"));
    }
    if wire
        .prefill
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_VALUE_CHARS)
        || !(1..=MAX_TIMEOUT_SECONDS).contains(&wire.timeout_seconds)
    {
        return Err(ProtocolError("invalid-prompt"));
    }
    Ok(PromptSpec {
        title: wire.title,
        message: wire.message,
        secret: wire.secret,
        prefill: wire.prefill,
        timeout_seconds: wire.timeout_seconds,
    })
}

/// Reads exactly one bounded `prompt-request` frame. Trailing bytes after the
/// first line are rejected so a piped spec cannot smuggle a second request.
pub fn read_spec(reader: &mut impl BufRead) -> Result<PromptSpec, ProtocolError> {
    let mut frame = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .map_err(|_| ProtocolError("input-unavailable"))?;
        if bytes.is_empty() {
            return Err(if frame.is_empty() {
                ProtocolError("prompt-required")
            } else {
                ProtocolError("partial-frame")
            });
        }
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1);
        let take = end.unwrap_or(bytes.len());
        if frame.len() + take > MAX_FRAME_BYTES {
            return Err(ProtocolError("frame-too-large"));
        }
        frame.extend_from_slice(&bytes[..take]);
        reader.consume(take);
        if end.is_some() {
            let wire: WireSpec =
                serde_json::from_slice(&frame).map_err(|_| ProtocolError("invalid-prompt"))?;
            let spec = validate(wire)?;
            // A second request already buffered in the same write is smuggling;
            // never block waiting for EOF — callers may hold the pipe open.
            if !reader
                .fill_buf()
                .map_err(|_| ProtocolError("input-unavailable"))?
                .is_empty()
            {
                return Err(ProtocolError("trailing-input"));
            }
            return Ok(spec);
        }
    }
}

pub fn emit_result(writer: &mut impl Write, result: &PromptResult) -> Result<(), ProtocolError> {
    serde_json::to_writer(&mut *writer, result).map_err(|_| ProtocolError("output-unavailable"))?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|_| ProtocolError("output-unavailable"))
}

pub fn probe() -> PromptCapability {
    platform::probe()
}

/// Renders one dialog on the calling thread. Callers must run this on the
/// process main thread; every platform implementation is modal and bounded by
/// `spec.timeout_seconds`.
pub fn run(spec: &PromptSpec) -> PromptResult {
    let result = platform::run(spec);
    if result.status() == PromptStatus::Submitted {
        match &result.value {
            Some(value) if value.chars().count() <= MAX_VALUE_CHARS => result,
            _ => PromptResult::of(PromptStatus::Unavailable),
        }
    } else {
        result
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication,
        NSApplicationActivationPolicy, NSModalResponseAbort, NSRunningApplication,
        NSSecureTextField, NSTextField,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    extern "C" {
        // libSystem exports the main queue as a global; dispatch_get_main_queue
        // is a header inline that returns its address.
        static _dispatch_main_q: c_void;
        fn dispatch_async_f(
            queue: *const c_void,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *mut c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *mut c_void);
    }

    pub fn probe() -> PromptCapability {
        let session = unsafe { CGSessionCopyCurrentDictionary() };
        if session.is_null() {
            PromptCapability {
                kind: "prompt-capability",
                version: VERSION,
                capable: false,
                detail: "no-gui-session",
            }
        } else {
            unsafe { CFRelease(session) };
            PromptCapability {
                kind: "prompt-capability",
                version: VERSION,
                capable: true,
                detail: "macos-alert",
            }
        }
    }

    extern "C" fn abort_modal(_context: *mut c_void) {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).abortModal();
        }
    }

    pub fn run(spec: &PromptSpec) -> PromptResult {
        if !probe().capable {
            return PromptResult::of(PromptStatus::Unavailable);
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return PromptResult::of(PromptStatus::Unavailable);
        };
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        NSRunningApplication::currentApplication()
            .activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&spec.title));
        alert.setInformativeText(&NSString::from_str(&spec.message));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(280.0, 24.0));
        let field = if spec.secret {
            NSSecureTextField::initWithFrame(NSSecureTextField::alloc(mtm), frame).into_super()
        } else {
            NSTextField::initWithFrame(NSTextField::alloc(mtm), frame)
        };
        if let Some(prefill) = &spec.prefill {
            field.setStringValue(&NSString::from_str(prefill));
        }
        alert.setAccessoryView(Some(&field));
        alert.window().setInitialFirstResponder(Some(&field));
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
            PromptResult::submitted(field.stringValue().to_string())
        } else if response == NSAlertSecondButtonReturn {
            PromptResult::of(PromptStatus::Cancelled)
        } else if response == NSModalResponseAbort {
            PromptResult::of(PromptStatus::Timeout)
        } else {
            PromptResult::of(PromptStatus::Unavailable)
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const IDC_MESSAGE: i32 = 100;
    const IDC_VALUE: i32 = 101;
    const WIDTH: i32 = 384;
    const HEIGHT: i32 = 176;
    const TIMER_ID: usize = 1;
    // winuser.h values the windows crate gates behind extra features.
    const EM_SETLIMITTEXT_MSG: u32 = 0xC5;

    struct State {
        spec_title: Vec<u16>,
        spec_message: Vec<u16>,
        spec_prefill: Option<Vec<u16>>,
        secret: bool,
        edit: Cell<HWND>,
        outcome: Cell<Option<PromptStatus>>,
        value: RefCell<String>,
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn probe() -> PromptCapability {
        // An interactive desktop is required to host the dialog.
        let capable = unsafe {
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
                .map(|desktop| {
                    let _ = CloseDesktop(desktop);
                    true
                })
                .unwrap_or(false)
        };
        PromptCapability {
            kind: "prompt-capability",
            version: VERSION,
            capable,
            detail: if capable {
                "win32-dialog"
            } else {
                "no-input-desktop"
            },
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        match message {
            WM_CREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                let state = &*(create.lpCreateParams as *const State);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                let instance = HINSTANCE(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().0);
                let font = GetStockObject(DEFAULT_GUI_FONT);
                let label = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("static"),
                    PCWSTR(state.spec_message.as_ptr()),
                    WS_CHILD | WS_VISIBLE,
                    12,
                    12,
                    WIDTH - 24,
                    56,
                    Some(hwnd),
                    Some(HMENU(IDC_MESSAGE as *mut c_void)),
                    Some(instance),
                    None,
                )
                .unwrap_or_default();
                if label != HWND::default() {
                    let _ = SendMessageW(
                        label,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
                let style = WINDOW_STYLE(
                    WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_BORDER.0
                        | WS_TABSTOP.0
                        | ES_AUTOHSCROLL as u32
                        | if state.secret { ES_PASSWORD as u32 } else { 0 },
                );
                let edit = CreateWindowExW(
                    WINDOW_EX_STYLE(WS_EX_CLIENTEDGE.0),
                    w!("edit"),
                    PCWSTR::null(),
                    style,
                    12,
                    72,
                    WIDTH - 24,
                    24,
                    Some(hwnd),
                    Some(HMENU(IDC_VALUE as *mut c_void)),
                    Some(instance),
                    None,
                )
                .unwrap_or_default();
                state.edit.set(edit);
                if edit != HWND::default() {
                    let _ = SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    let _ = SendMessageW(
                        edit,
                        EM_SETLIMITTEXT_MSG,
                        Some(WPARAM(MAX_VALUE_CHARS)),
                        Some(LPARAM(0)),
                    );
                    if let Some(prefill) = &state.spec_prefill {
                        let _ = SetWindowTextW(edit, PCWSTR(prefill.as_ptr()));
                    }
                    let _ = SetFocus(Some(edit));
                }
                for (id, text, x, default) in [
                    (IDOK.0, w!("OK"), WIDTH - 176, true),
                    (IDCANCEL.0, w!("Cancel"), WIDTH - 92, false),
                ] {
                    let button = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("button"),
                        text,
                        WINDOW_STYLE(
                            WS_CHILD.0
                                | WS_VISIBLE.0
                                | WS_TABSTOP.0
                                | if default {
                                    BS_DEFPUSHBUTTON as u32
                                } else {
                                    BS_PUSHBUTTON as u32
                                },
                        ),
                        x,
                        HEIGHT - 64,
                        80,
                        26,
                        Some(hwnd),
                        Some(HMENU(id as *mut c_void)),
                        Some(instance),
                        None,
                    )
                    .unwrap_or_default();
                    if button != HWND::default() {
                        let _ = SendMessageW(
                            button,
                            WM_SETFONT,
                            Some(WPARAM(font.0 as usize)),
                            Some(LPARAM(1)),
                        );
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                if state.is_null() {
                    return DefWindowProcW(hwnd, message, wparam, lparam);
                }
                match (wparam.0 & 0xffff) as i32 {
                    value if value == IDOK.0 => {
                        let edit = (*state).edit.get();
                        if edit != HWND::default() {
                            let mut buffer = vec![0u16; MAX_VALUE_CHARS + 1];
                            let read = GetWindowTextW(edit, &mut buffer) as usize;
                            *(*state).value.borrow_mut() =
                                String::from_utf16_lossy(&buffer[..read]);
                        }
                        (*state).outcome.set(Some(PromptStatus::Submitted));
                        let _ = DestroyWindow(hwnd);
                        LRESULT(0)
                    }
                    value if value == IDCANCEL.0 => {
                        (*state).outcome.set(Some(PromptStatus::Cancelled));
                        let _ = DestroyWindow(hwnd);
                        LRESULT(0)
                    }
                    _ => DefWindowProcW(hwnd, message, wparam, lparam),
                }
            }
            WM_TIMER => {
                if !state.is_null() {
                    (*state).outcome.set(Some(PromptStatus::Timeout));
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_CLOSE => {
                if !state.is_null() && (*state).outcome.get().is_none() {
                    (*state).outcome.set(Some(PromptStatus::Cancelled));
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    pub fn run(spec: &PromptSpec) -> PromptResult {
        if !probe().capable {
            return PromptResult::of(PromptStatus::Unavailable);
        }
        let state = State {
            spec_title: wide(&spec.title),
            spec_message: wide(&spec.message),
            spec_prefill: spec.prefill.as_deref().map(wide),
            secret: spec.secret,
            edit: Cell::new(HWND::default()),
            outcome: Cell::new(None),
            value: std::cell::RefCell::new(String::new()),
        };
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let class = w!("HranessCompanionPrompt");
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().0);
            if REGISTERED.get().is_none() {
                let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
                let wndclass = WNDCLASSW {
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance,
                    lpszClassName: class,
                    hCursor: cursor,
                    ..Default::default()
                };
                let _ = REGISTERED.set(RegisterClassW(&wndclass) != 0);
            }
            let x = (GetSystemMetrics(SM_CXSCREEN) - WIDTH) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - HEIGHT) / 2;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_DLGMODALFRAME.0),
                class,
                PCWSTR(state.spec_title.as_ptr()),
                WINDOW_STYLE(WS_POPUP.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_VISIBLE.0),
                x.max(0),
                y.max(0),
                WIDTH,
                HEIGHT,
                None,
                None,
                Some(instance),
                Some(&state as *const State as *const c_void),
            );
            if hwnd.is_err() {
                return PromptResult::of(PromptStatus::Unavailable);
            }
            let hwnd = hwnd.unwrap_or_default();
            let millis = spec
                .timeout_seconds
                .saturating_mul(1000)
                .min(u32::MAX as u64 - 1) as u32;
            SetTimer(Some(hwnd), TIMER_ID, millis, None);
            let mut message = MSG::default();
            loop {
                let got = GetMessageW(&mut message, None, 0, 0);
                // BOOL(-1) is an API error; BOOL(0) is WM_QUIT. Both end the loop.
                if got.0 <= 0 {
                    break;
                }
                if message.message == WM_KEYDOWN && message.wParam.0 == VK_RETURN.0 as usize {
                    // Route Enter to the default button inside a plain window.
                    let _ = SendMessageW(
                        hwnd,
                        WM_COMMAND,
                        Some(WPARAM(IDOK.0 as usize)),
                        Some(LPARAM(0)),
                    );
                    continue;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let status = state.outcome.get().unwrap_or(PromptStatus::Cancelled);
        if status == PromptStatus::Submitted {
            PromptResult::submitted(std::mem::take(&mut *state.value.borrow_mut()))
        } else {
            PromptResult::of(status)
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use gtk::prelude::*;

    const TIMEOUT_RESPONSE: u16 = 2;

    pub fn probe() -> PromptCapability {
        let capable = gtk::is_initialized() || gtk::init().is_ok();
        PromptCapability {
            kind: "prompt-capability",
            version: VERSION,
            capable,
            detail: if capable {
                "gtk-dialog"
            } else {
                "no-gtk-session"
            },
        }
    }

    pub fn run(spec: &PromptSpec) -> PromptResult {
        if !gtk::is_initialized() && gtk::init().is_err() {
            return PromptResult::of(PromptStatus::Unavailable);
        }
        let dialog = gtk::Dialog::with_buttons(
            Some(&spec.title),
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            &[
                ("OK", gtk::ResponseType::Ok),
                ("Cancel", gtk::ResponseType::Cancel),
            ],
        );
        let area = dialog.content_area();
        let label = gtk::Label::new(Some(&spec.message));
        label.set_xalign(0.0);
        label.set_line_wrap(true);
        label.set_max_width_chars(48);
        area.pack_start(&label, false, false, 8);
        let entry = gtk::Entry::new();
        entry.set_visibility(!spec.secret);
        entry.set_activates_default(true);
        entry.set_max_length(MAX_VALUE_CHARS as i32);
        entry.set_width_chars(42);
        if let Some(prefill) = &spec.prefill {
            entry.set_text(prefill);
        }
        area.pack_start(&entry, false, false, 8);
        dialog.set_default_response(gtk::ResponseType::Ok);
        dialog.show_all();
        entry.grab_focus();
        let timeout = glib::timeout_add_seconds_local(
            spec.timeout_seconds.min(u32::MAX as u64) as u32,
            glib::clone!(@weak dialog => @default-return glib::ControlFlow::Break, move || {
                dialog.response(gtk::ResponseType::Other(TIMEOUT_RESPONSE));
                glib::ControlFlow::Break
            }),
        );
        let response = dialog.run();
        timeout.remove();
        let result = match response {
            gtk::ResponseType::Ok => {
                let mut value = entry.text().to_string();
                let result = PromptResult::submitted(std::mem::take(&mut value));
                value.zeroize();
                result
            }
            gtk::ResponseType::Other(TIMEOUT_RESPONSE) => PromptResult::of(PromptStatus::Timeout),
            _ => PromptResult::of(PromptStatus::Cancelled),
        };
        dialog.close();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn spec(extra: &str) -> Vec<u8> {
        format!(
            "{{\"type\":\"prompt-request\",\"version\":1,\"title\":\"Product\",\"message\":\"Enter it\"{extra}}}\n"
        )
        .into_bytes()
    }

    #[test]
    fn spec_accepts_bounds_and_defaults() {
        let parsed = read_spec(&mut Cursor::new(spec(""))).unwrap();
        assert!(parsed.secret);
        assert_eq!(parsed.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        let parsed = read_spec(&mut Cursor::new(spec(
            ",\"secret\":false,\"prefill\":\"cur\",\"timeoutSeconds\":600",
        )))
        .unwrap();
        assert!(!parsed.secret);
        assert_eq!(parsed.prefill.as_deref(), Some("cur"));
        assert_eq!(parsed.timeout_seconds, 600);
    }

    #[test]
    fn spec_rejects_unsafe_text_and_bad_bounds() {
        for bad in [",\"timeoutSeconds\":0", ",\"timeoutSeconds\":601"] {
            assert!(matches!(
                read_spec(&mut Cursor::new(spec(bad))),
                Err(ProtocolError("invalid-prompt"))
            ));
        }
        let oversized = format!(",\"prefill\":\"{}\"", "x".repeat(MAX_VALUE_CHARS + 1));
        assert!(matches!(
            read_spec(&mut Cursor::new(spec(&oversized))),
            Err(ProtocolError("invalid-prompt"))
        ));
        let control =
            "{\"type\":\"prompt-request\",\"version\":1,\"title\":\"bad\u{202e}\",\"message\":\"x\"}\n"
                .as_bytes()
                .to_vec();
        assert!(matches!(
            read_spec(&mut Cursor::new(control)),
            Err(ProtocolError("invalid-prompt"))
        ));
        let missing = b"{\"type\":\"prompt-request\",\"version\":1,\"title\":\"x\"}\n";
        assert!(matches!(
            read_spec(&mut Cursor::new(missing)),
            Err(ProtocolError("invalid-prompt"))
        ));
        let trailing = spec("");
        let mut both = trailing.clone();
        both.extend_from_slice(&trailing);
        assert!(matches!(
            read_spec(&mut Cursor::new(both)),
            Err(ProtocolError("trailing-input"))
        ));
        assert!(matches!(
            read_spec(&mut Cursor::new(b"")),
            Err(ProtocolError("prompt-required"))
        ));
        let mut wrong = spec("");
        wrong[9] = b'x';
        assert!(matches!(
            read_spec(&mut Cursor::new(wrong)),
            Err(ProtocolError("invalid-prompt"))
        ));
    }

    #[test]
    fn secrets_never_render_in_diagnostics() {
        let spec = read_spec(&mut Cursor::new(spec(",\"prefill\":\"topsecret\""))).unwrap();
        let debug = format!("{spec:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("topsecret"));
        let result = PromptResult::submitted("hunter2".into());
        let debug = format!("{result:?}");
        assert!(!debug.contains("hunter2"));
        let mut wire = Vec::new();
        emit_result(&mut wire, &result).unwrap();
        let wire = String::from_utf8(wire).unwrap();
        assert!(wire.contains("\"status\":\"submitted\""));
        assert!(wire.contains("\"value\":\"hunter2\""));
        let mut wire = Vec::new();
        emit_result(&mut wire, &PromptResult::of(PromptStatus::Timeout)).unwrap();
        assert!(String::from_utf8(wire)
            .unwrap()
            .contains("\"status\":\"timeout\""));
    }

    #[test]
    fn capability_frame_is_bounded() {
        let mut wire = Vec::new();
        probe().emit(&mut wire).unwrap();
        let wire = String::from_utf8(wire).unwrap();
        assert!(wire.contains("\"type\":\"prompt-capability\""));
        assert!(wire.contains("\"capable\":"));
    }
}
