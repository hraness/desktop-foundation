#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! Native renderer only: no shell, product commands, network, or webviews.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use desktop_foundation::prompt;
use desktop_foundation::protocol::{self, Event, Frame, ProtocolError, Session, VERSION};
use desktop_foundation::{
    DispatchOutcome, Host, MenuModel, Options, RefreshHandle, RenderError, RenderOperation,
};
use tauri::AppHandle;

struct Message {
    event: Event,
    flushed: Option<mpsc::SyncSender<()>>,
}

/// The UI never blocks on its parent. A parent that stops consuming events
/// loses the disposable renderer rather than accumulating an unlimited queue.
#[derive(Clone)]
struct Output {
    sender: mpsc::SyncSender<Message>,
    app: Arc<Mutex<Option<AppHandle>>>,
}

impl Output {
    fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Message>(64);
        let app = Arc::new(Mutex::new(None::<AppHandle>));
        let writer_app = app.clone();
        std::thread::spawn(move || {
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            while let Ok(message) = receiver.recv() {
                if write_event(&mut stdout, &message.event).is_err() {
                    if let Some(app) = writer_app.lock().unwrap().as_ref() {
                        app.exit(1);
                    }
                    return;
                }
                if let Some(flushed) = message.flushed {
                    let _ = flushed.try_send(());
                }
            }
        });
        Self { sender, app }
    }

    fn emit(&self, event: Event) -> bool {
        if self
            .sender
            .try_send(Message {
                event,
                flushed: None,
            })
            .is_err()
        {
            if let Some(app) = self.app.lock().unwrap().as_ref() {
                app.exit(1);
            }
            return false;
        }
        true
    }

    fn stopped(&self) {
        let (flushed, receiver) = mpsc::sync_channel(1);
        if self
            .sender
            .try_send(Message {
                event: Event::Stopped { version: VERSION },
                flushed: Some(flushed),
            })
            .is_ok()
        {
            let _ = receiver.recv_timeout(Duration::from_millis(250));
        }
    }
}

fn write_event(writer: &mut impl Write, event: &Event) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

struct Runner {
    session: Arc<Mutex<Session>>,
    input: Mutex<Option<BufReader<std::io::Stdin>>>,
    output: Output,
    icon_directory: Option<PathBuf>,
    rendered_once: AtomicBool,
}

impl Host for Runner {
    fn tray_icon_directory(&self) -> Option<PathBuf> {
        self.icon_directory.clone()
    }

    fn snapshot(&self) -> MenuModel {
        self.session
            .lock()
            .unwrap()
            .latest()
            .expect("initial validated snapshot")
            .model()
    }

    fn dispatch_result(&self, route: &str) -> DispatchOutcome {
        let event = self
            .session
            .lock()
            .unwrap()
            .latest()
            .and_then(|snapshot| snapshot.action(route));
        match event {
            Some(event) => {
                if self.output.emit(event) {
                    DispatchOutcome::Accepted
                } else {
                    DispatchOutcome::Rejected
                }
            }
            None => DispatchOutcome::Rejected,
        }
    }

    fn render_failed_at(&self, _error: RenderError, operation: RenderOperation) {
        let code = match operation {
            RenderOperation::Validate => "render-model-failed",
            RenderOperation::Schedule => "render-schedule-failed",
            RenderOperation::LookupTray => "render-tray-missing",
            RenderOperation::BuildMenu => "render-menu-build-failed",
            RenderOperation::SetMenu => "render-menu-set-failed",
            RenderOperation::SetTitle => "render-title-failed",
            RenderOperation::SetTooltip => "render-tooltip-failed",
            RenderOperation::SetIcon => "render-icon-failed",
            RenderOperation::SetChecked => "render-check-failed",
        };
        self.output.emit(Event::error(code));
        if let Some(app) = self.output.app.lock().unwrap().as_ref() {
            app.exit(1);
        }
    }

    fn model_rendered(&self, _app: &AppHandle) {
        if !self.rendered_once.swap(true, Ordering::AcqRel) {
            self.output.emit(Event::Ready {
                version: VERSION,
                pid: std::process::id(),
                platform: std::env::consts::OS,
            });
        }
    }

    fn started_with_refresh(&self, app: &AppHandle, refresh: RefreshHandle) {
        *self.output.app.lock().unwrap() = Some(app.clone());
        apply_serif(app);
        let mut input = self
            .input
            .lock()
            .unwrap()
            .take()
            .expect("stdin has one reader");
        let session = self.session.clone();
        let output = self.output.clone();
        let app = app.clone();
        std::thread::spawn(move || loop {
            let frame = match protocol::read_frame(&mut input) {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    app.exit(0);
                    return;
                }
                Err(ProtocolError(code)) => {
                    output.emit(Event::error(code));
                    app.exit(1);
                    return;
                }
            };
            match session.lock().unwrap().accept(frame) {
                Ok(true) => refresh.request(),
                Ok(false) => {
                    app.exit(0);
                    return;
                }
                Err(ProtocolError(code)) => {
                    output.emit(Event::error(code));
                    app.exit(1);
                    return;
                }
            }
        });
    }

    fn stopping(&self) {
        self.output.stopped();
    }
}

#[cfg(target_os = "macos")]
fn apply_serif(app: &AppHandle) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSFont;
    use objc2_foundation::NSString;
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.with_inner_tray_icon(|tray| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let Some(status) = tray.ns_status_item() else {
                return;
            };
            let Some(button) = status.button(mtm) else {
                return;
            };
            let font = NSFont::fontWithName_size(&NSString::from_str("Georgia"), 15.0);
            if let Some(font) = font {
                button.setFont(Some(&font));
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_serif(_app: &AppHandle) {}

fn redirected(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes directory junctions, not just symbolic links.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn private_directory(path: &Path) -> Result<(), ProtocolError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(ProtocolError("unsafe-state-dir"));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        // A Windows drive prefix alone (C:) is drive-relative. Inspect only
        // after its following root separator has made the path absolute.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if !redirected(&meta) && meta.is_dir() => (),
            Ok(_) => return Err(ProtocolError("unsafe-state-dir")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                if let Err(error) = builder.create(&current) {
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(ProtocolError("state-unavailable"));
                    }
                }
                let meta = fs::symlink_metadata(&current)
                    .map_err(|_| ProtocolError("state-unavailable"))?;
                if redirected(&meta) || !meta.is_dir() {
                    return Err(ProtocolError("unsafe-state-dir"));
                }
            }
            Err(_) => return Err(ProtocolError("state-unavailable")),
        }
    }
    #[cfg(unix)]
    {
        // Shared custody contract for the leaf: canonical parent chain,
        // realpath-verified, lstat-not-symlink, owned by the current user,
        // and `mode & 0o077 == 0`. The component walk above still owns
        // creation: it makes every missing intermediate 0700, which
        // `ensure_private_directory` alone would not apply to components it
        // did not create. The mode bound is deliberately stricter than the
        // former `mode & 0o022 == 0` leaf check — this code has only ever
        // created these directories at 0700.
        local_custody::ensure_private_directory(path).map_err(|error| {
            ProtocolError(match error.code.as_str() {
                // Transient filesystem failures keep the unavailable signal;
                // custody violations stay unsafe.
                "stat" | "create" | "chmod" | "not-found" => "state-unavailable",
                _ => "unsafe-state-dir",
            })
        })?;
    }
    Ok(())
}

/// A retained OS lock, never a stale PID file and never unlinked on exit.
/// Dropping/terminating the process releases the lock on every target OS.
fn lock_instance(state_dir: &Path, app_id: &str) -> Result<Option<File>, ProtocolError> {
    private_directory(state_dir)?;
    if !protocol::valid_app_id(app_id) {
        return Err(ProtocolError("invalid-snapshot"));
    }
    let path = state_dir.join(format!("{app_id}.lock"));
    if fs::symlink_metadata(&path).is_ok_and(|meta| redirected(&meta) || !meta.is_file()) {
        return Err(ProtocolError("unsafe-state-dir"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(&path)
        .map_err(|_| ProtocolError("lock-unavailable"))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // Descriptor-level custody on the lock: an owned regular file with
        // exactly one hard link and owner-only permissions. `O_NOFOLLOW` at
        // open plus this fstat check subsume the old `redirected` test — a
        // successfully opened descriptor cannot be a symlink. `owner_only`
        // asserts `mode & 0o077 == 0`, deliberately stricter than the former
        // `0o022` bound; the lock is only ever created at 0600.
        local_custody::assert_owned_fd(
            file.as_raw_fd(),
            &local_custody::OwnedPathOptions {
                kind: Some(local_custody::ObjectKind::File),
                owner_only: true,
                links: Some(1),
                ..Default::default()
            },
        )
        .map_err(|error| {
            ProtocolError(match error.code.as_str() {
                "dup" | "stat" => "lock-unavailable",
                _ => "unsafe-state-dir",
            })
        })?;
    }
    #[cfg(windows)]
    {
        let meta = file
            .metadata()
            .map_err(|_| ProtocolError("lock-unavailable"))?;
        if redirected(&meta) || !meta.is_file() {
            return Err(ProtocolError("unsafe-state-dir"));
        }
    }
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(None)
        }
        Err(_) => Err(ProtocolError("lock-unavailable")),
    }
}

fn graphical_session() -> Result<(), ProtocolError> {
    #[cfg(target_os = "linux")]
    {
        let set = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        if (!set("DISPLAY") && !set("WAYLAND_DISPLAY")) || !set("DBUS_SESSION_BUS_ADDRESS") {
            return Err(ProtocolError("tray-unavailable"));
        }
    }
    Ok(())
}

fn icon_directory(state_dir: &Path, app_id: &str) -> Result<PathBuf, ProtocolError> {
    if !protocol::valid_app_id(app_id) {
        return Err(ProtocolError("invalid-snapshot"));
    }
    // tray-icon uses its internal tray ID in PNG filenames. Each product
    // needs a separate directory even when callers share a state root.
    let directory = state_dir.join(format!("{app_id}.icons"));
    private_directory(&directory)?;
    Ok(directory)
}

fn check_protocol() -> Result<(), ProtocolError> {
    let mut input = BufReader::new(std::io::stdin());
    let mut output = std::io::stdout();
    let mut session = Session::default();
    while let Some(frame) = protocol::read_frame(&mut input)? {
        if !session.accept(frame)? {
            break;
        }
        write_event(
            &mut output,
            &Event::Validated {
                version: VERSION,
                revision: session.latest().unwrap().revision,
            },
        )
        .map_err(|_| ProtocolError("output-unavailable"))?;
    }
    write_event(&mut output, &Event::Stopped { version: VERSION })
        .map_err(|_| ProtocolError("output-unavailable"))
}

fn execute() -> Result<(), ProtocolError> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!(
            "hraness-companion {} protocol/{}",
            env!("CARGO_PKG_VERSION"),
            VERSION
        );
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--check-protocol" {
        return check_protocol();
    }
    if args.len() == 1 && args[0] == "--prompt-probe" {
        return prompt::probe().emit(&mut std::io::stdout());
    }
    if args.len() == 1 && args[0] == "--prompt" {
        let spec = prompt::read_spec(&mut BufReader::new(std::io::stdin()))?;
        return prompt::emit_result(&mut std::io::stdout(), &prompt::run(&spec));
    }
    if args.len() != 2 || args[0] != "--state-dir" {
        return Err(ProtocolError("invalid-arguments"));
    }
    let state_dir = PathBuf::from(&args[1]);
    if !state_dir.is_absolute() {
        return Err(ProtocolError("unsafe-state-dir"));
    }
    let mut input = BufReader::new(std::io::stdin());
    let Some(frame) = protocol::read_frame(&mut input)? else {
        return Ok(());
    };
    if matches!(frame, Frame::Quit { .. }) {
        return Err(ProtocolError("snapshot-required"));
    }
    let mut session = Session::default();
    session.accept(frame)?;
    let Some(_instance) = lock_instance(&state_dir, &session.latest().unwrap().app_id)? else {
        write_event(
            &mut std::io::stdout(),
            &Event::AlreadyRunning { version: VERSION },
        )
        .map_err(|_| ProtocolError("output-unavailable"))?;
        return Ok(());
    };
    graphical_session()?;
    let icon_directory = if cfg!(target_os = "linux") {
        Some(icon_directory(
            &state_dir,
            &session.latest().unwrap().app_id,
        )?)
    } else {
        None
    };
    let host = Arc::new(Runner {
        session: Arc::new(Mutex::new(session)),
        input: Mutex::new(Some(input)),
        output: Output::new(),
        icon_directory,
        rendered_once: AtomicBool::new(false),
    });
    desktop_foundation::run(
        tauri::generate_context!(),
        host,
        Options::default(),
        |builder| builder,
    )
    .map_err(|_| ProtocolError("tray-unavailable"))
}

fn main() {
    // Tauri errors and panic payloads may include paths. Emit only stable codes
    // on the wire; Rust/Tauri build diagnostics remain developer-only.
    std::panic::set_hook(Box::new(|_| {
        let _ = write_event(&mut std::io::stderr(), &Event::error("internal-error"));
    }));
    if let Err(ProtocolError(code)) = execute() {
        let _ = write_event(&mut std::io::stdout(), &Event::error(code));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        // Wall-clock nanoseconds do not imply nanosecond clock resolution:
        // parallel tests can receive identical timestamps on macOS. Reserve
        // each root exclusively, and never reuse stale state from a prior PID.
        for _ in 0..1024 {
            let root = temporary.join(format!(
                "companion-lock-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&root) {
                Ok(()) => {
                    private_directory(&root).unwrap();
                    return root;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create native test fixture: {error}"),
            }
        }
        panic!("could not reserve a unique native test fixture");
    }

    #[test]
    fn retained_lock_has_distinct_contention_and_releases_on_drop() {
        let dir = fixture();
        let first = lock_instance(&dir, "test.one").unwrap().unwrap();
        assert!(lock_instance(&dir, "test.one").unwrap().is_none());
        assert!(lock_instance(&dir, "test.two").unwrap().is_some());
        drop(first);
        assert!(lock_instance(&dir, "test.one").unwrap().is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_paths_do_not_report_another_running_instance() {
        assert!(matches!(
            lock_instance(Path::new("relative"), "test"),
            Err(ProtocolError("unsafe-state-dir"))
        ));
        let dir = fixture();
        fs::create_dir(dir.join("test.lock")).unwrap();
        assert!(matches!(
            lock_instance(&dir, "test"),
            Err(ProtocolError("unsafe-state-dir"))
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn icon_directories_are_app_scoped_before_tray_creation() {
        let dir = fixture();
        let one = icon_directory(&dir, "test.one").unwrap();
        let two = icon_directory(&dir, "test.two").unwrap();
        assert_ne!(one, two);
        assert!(one.is_dir() && two.is_dir());
        assert_eq!(one, icon_directory(&dir, "test.one").unwrap());
        assert!(icon_directory(&dir, "../escape").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_directories_and_lock_files_are_refused() {
        use std::os::unix::fs::symlink;
        let dir = fixture();
        let actual = dir.join("actual");
        private_directory(&actual).unwrap();
        symlink(&actual, dir.join("redirected")).unwrap();
        assert!(matches!(
            lock_instance(&dir.join("redirected"), "test"),
            Err(ProtocolError("unsafe-state-dir"))
        ));
        symlink(&actual, dir.join("test.icons")).unwrap();
        assert!(matches!(
            icon_directory(&dir, "test"),
            Err(ProtocolError("unsafe-state-dir"))
        ));
        let actual_file = dir.join("actual-file");
        fs::write(&actual_file, b"untouched").unwrap();
        symlink(&actual_file, dir.join("test.lock")).unwrap();
        assert!(matches!(
            lock_instance(&dir, "test"),
            Err(ProtocolError("unsafe-state-dir"))
        ));
        assert_eq!(fs::read(actual_file).unwrap(), b"untouched");
        fs::remove_dir_all(dir).unwrap();
    }
}
