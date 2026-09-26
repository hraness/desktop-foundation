//! Login startup and liveness for Rust menu bar products.
//!
//! The TypeScript SDK owns `menubar install/uninstall/status` for SDK
//! products. Rust products (AI Charts, Slopcamera, Valhalla, PeopleBlade)
//! use this module instead of hand-rolled LaunchAgents, so every product
//! writes the same plist, reports the same states and prints the same copy.
//!
//! Nothing here calls `launchctl`: the LaunchAgent takes effect at the next
//! login, and macOS shows its "Login item added" notice when the file
//! appears. Products print [`login_item_notice`] first. Every file operation
//! takes an explicit home directory so tests run against a temporary one.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::protocol::valid_app_id;

/// Exit code for "already running", distinct from success and failure, so
/// scripts can tell a second launch from an error.
pub const EXIT_ALREADY_RUNNING: i32 = 3;

const MAX_ARGS: usize = 64;
const MAX_ARG_BYTES: usize = 8192;
const MAX_PLIST_BYTES: u64 = 1024 * 1024;
const SETTINGS_PATH: &str = "System Settings › General › Login Items & Extensions";

/// What a product starts at login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginItem {
    /// Lowercase product ID, such as `aicharts`.
    pub app_id: String,
    /// Display name, such as `AI Charts`.
    pub name: String,
    /// The command people type, such as `aicharts`, for next-step hints.
    pub command: String,
    /// Absolute path of the program launchd starts: the bundle executable
    /// inside `~/Applications/Hraness/<Product>.app` when the product has
    /// one, so Login Items shows the product's name and icon.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// `app.hraness.<appId>` when `program` is inside the product's bundle.
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceErrorKind {
    /// The item's ID, name, paths or arguments are unusable.
    InvalidItem,
    /// A file with this name exists but this foundation did not write it,
    /// or someone edited it.
    NotOurs,
    /// The LaunchAgents folder is not a real, writable folder.
    Unwritable,
    Unsupported,
}

/// A safe error: it never carries paths or product text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceError {
    pub kind: ServiceErrorKind,
}

impl ServiceError {
    fn new(kind: ServiceErrorKind) -> Self {
        Self { kind }
    }
}

/// The rendered LaunchAgent for one item. Building it has no side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgentPlan {
    pub label: String,
    pub path: PathBuf,
    pub contents: String,
    /// Older labels this product used; install removes them when they are ours.
    pub legacy: Vec<PathBuf>,
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The ownership line, shared with the SDK's autostart files so either can
/// recognize and migrate the other's plist.
fn header(app_id: &str, body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("<!-- hraness-companion autostart {app_id} sha256:{hex} -->\n")
}

fn safe_value(value: &str) -> bool {
    !value.chars().any(|ch| ch.is_control())
}

/// Renders the LaunchAgent for `item` under `home`.
pub fn plan(item: &LoginItem, home: &Path) -> Result<LaunchAgentPlan, ServiceError> {
    let invalid = || ServiceError::new(ServiceErrorKind::InvalidItem);
    let program = item.program.to_str().ok_or_else(invalid)?;
    if !valid_app_id(&item.app_id)
        || !home.is_absolute()
        || !item.program.is_absolute()
        || item.name.is_empty()
        || item.name.chars().count() > 128
        || item.args.len() > MAX_ARGS
        || item.args.iter().any(|arg| arg.len() > MAX_ARG_BYTES)
        || ![item.name.as_str(), program, item.command.as_str()]
            .into_iter()
            .chain(item.args.iter().map(String::as_str))
            .all(safe_value)
        || item
            .bundle_id
            .as_deref()
            .is_some_and(|id| id != format!("app.hraness.{}", item.app_id))
    {
        return Err(invalid());
    }
    let label = format!("app.hraness.{}", item.app_id);
    let agents = home.join("Library").join("LaunchAgents");
    let arguments: String = std::iter::once(program)
        .chain(item.args.iter().map(String::as_str))
        .map(|arg| format!("<string>{}</string>", xml(arg)))
        .collect();
    let associated = item
        .bundle_id
        .as_deref()
        .map(|id| {
            format!(
                "<key>AssociatedBundleIdentifiers</key><array><string>{}</string></array>\n",
                xml(id)
            )
        })
        .unwrap_or_default();
    let body = format!(
        "<plist version=\"1.0\"><dict>\n<key>Label</key><string>{}</string>\n<key>ProgramArguments</key><array>{arguments}</array>\n{associated}<key>RunAtLoad</key><true/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n<key>ProcessType</key><string>Interactive</string>\n</dict></plist>\n",
        xml(&label)
    );
    Ok(LaunchAgentPlan {
        path: agents.join(format!("{label}.plist")),
        contents: header(&item.app_id, &body) + &body,
        legacy: vec![agents.join(format!("app.hraness.companion.{}.plist", item.app_id))],
        label,
    })
}

/// What is on disk for a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoginState {
    /// Our file, exactly as planned.
    On,
    /// No file.
    Off,
    /// Our file, but it starts an older program or arguments. Installing
    /// again updates it.
    Outdated,
    /// A file this foundation did not write, or one someone edited.
    NotOurs,
}

fn owned(path: &Path, app_id: &str) -> Result<Option<String>, ServiceError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ServiceError::new(ServiceErrorKind::Unwritable)),
    };
    if !meta.is_file() || meta.len() > MAX_PLIST_BYTES {
        return Err(ServiceError::new(ServiceErrorKind::NotOurs));
    }
    let text =
        fs::read_to_string(path).map_err(|_| ServiceError::new(ServiceErrorKind::NotOurs))?;
    let (first, body) = text
        .split_once('\n')
        .ok_or(ServiceError::new(ServiceErrorKind::NotOurs))?;
    if format!("{first}\n") != header(app_id, body) {
        return Err(ServiceError::new(ServiceErrorKind::NotOurs));
    }
    Ok(Some(text))
}

fn app_id_of(plan: &LaunchAgentPlan) -> &str {
    plan.label
        .strip_prefix("app.hraness.")
        .unwrap_or(&plan.label)
}

/// Reads the current login state without changing anything.
pub fn login_state(plan: &LaunchAgentPlan) -> LoginState {
    match owned(&plan.path, app_id_of(plan)) {
        Ok(Some(text)) if text == plan.contents => LoginState::On,
        Ok(Some(_)) => LoginState::Outdated,
        Ok(None) => LoginState::Off,
        Err(_) => LoginState::NotOurs,
    }
}

/// What `install` or `uninstall` changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Change {
    Created,
    Updated,
    Unchanged,
    Removed,
}

fn real_directory(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(ServiceError::new(ServiceErrorKind::Unwritable)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(path)
                .map_err(|_| ServiceError::new(ServiceErrorKind::Unwritable))
        }
        Err(_) => Err(ServiceError::new(ServiceErrorKind::Unwritable)),
    }
}

/// Writes the LaunchAgent (atomically, owner-only) and removes this
/// product's legacy agent when it is ours. Refuses to replace a file it did
/// not write. Takes effect at the next login.
pub fn install(plan: &LaunchAgentPlan) -> Result<Change, ServiceError> {
    if !cfg!(target_os = "macos") {
        return Err(ServiceError::new(ServiceErrorKind::Unsupported));
    }
    let app_id = app_id_of(plan);
    let parent = plan
        .path
        .parent()
        .ok_or(ServiceError::new(ServiceErrorKind::InvalidItem))?;
    if let Some(library) = parent.parent() {
        real_directory(library)?;
    }
    real_directory(parent)?;
    let existing = owned(&plan.path, app_id)?;
    if existing.as_deref() == Some(plan.contents.as_str()) {
        remove_legacy(plan);
        return Ok(Change::Unchanged);
    }
    // A per-call nonce: two installs in one process must not share a temp file.
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{}.{}.{nonce}.tmp",
        plan.label,
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file: File = options.open(&temp)?;
        file.write_all(plan.contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, &plan.path)
    };
    if write().is_err() {
        let _ = fs::remove_file(&temp);
        return Err(ServiceError::new(ServiceErrorKind::Unwritable));
    }
    remove_legacy(plan);
    Ok(if existing.is_some() {
        Change::Updated
    } else {
        Change::Created
    })
}

fn remove_legacy(plan: &LaunchAgentPlan) {
    for path in &plan.legacy {
        if let Ok(Some(_)) = owned(path, app_id_of(plan)) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Removes this product's LaunchAgent (and legacy one) when they are ours.
/// Does not stop a running menu bar.
pub fn uninstall(plan: &LaunchAgentPlan) -> Result<Change, ServiceError> {
    let app_id = app_id_of(plan);
    let current = owned(&plan.path, app_id)?;
    let mut removed = false;
    if current.is_some() {
        fs::remove_file(&plan.path).map_err(|_| ServiceError::new(ServiceErrorKind::Unwritable))?;
        removed = true;
    }
    for path in &plan.legacy {
        if let Ok(Some(_)) = owned(path, app_id) {
            removed |= fs::remove_file(path).is_ok();
        }
    }
    Ok(if removed {
        Change::Removed
    } else {
        Change::Unchanged
    })
}

/// Held for the life of a running menu bar. A second launch finds the lock
/// taken and exits with [`EXIT_ALREADY_RUNNING`].
pub struct InstanceLock {
    _file: File,
}

fn lock_path(state_dir: &Path, app_id: &str) -> Result<PathBuf, ServiceError> {
    if !valid_app_id(app_id) || !state_dir.is_absolute() {
        return Err(ServiceError::new(ServiceErrorKind::InvalidItem));
    }
    let path = state_dir.join(format!("{app_id}.menubar.lock"));
    if fs::symlink_metadata(&path)
        .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(ServiceError::new(ServiceErrorKind::NotOurs));
    }
    Ok(path)
}

fn open_lock(path: &Path) -> Result<File, ServiceError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| ServiceError::new(ServiceErrorKind::Unwritable))
}

impl InstanceLock {
    /// `Ok(None)` means another copy is running.
    pub fn acquire(state_dir: &Path, app_id: &str) -> Result<Option<InstanceLock>, ServiceError> {
        real_directory(state_dir)?;
        let file = open_lock(&lock_path(state_dir, app_id)?)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(InstanceLock { _file: file })),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                Ok(None)
            }
            Err(_) => Err(ServiceError::new(ServiceErrorKind::Unwritable)),
        }
    }
}

/// Whether a menu bar holding [`InstanceLock`] is running. `None` when it
/// cannot be told (no state folder yet counts as not running).
pub fn is_running(state_dir: &Path, app_id: &str) -> Option<bool> {
    let path = lock_path(state_dir, app_id).ok()?;
    if !path.exists() {
        return Some(false);
    }
    let file = open_lock(&path).ok()?;
    match fs2::FileExt::try_lock_shared(&file) {
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&file);
            Some(false)
        }
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Some(true)
        }
        Err(_) => None,
    }
}

/// One product's login and liveness state, for `menubar status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceStatus {
    pub login: LoginState,
    pub running: Option<bool>,
}

/// Output symbols, with the ASCII fallback for `TERM=dumb` and non-UTF-8
/// terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    Unicode,
    Ascii,
}

impl Glyphs {
    fn get(self, unicode: &'static str, ascii: &'static str) -> &'static str {
        match self {
            Glyphs::Unicode => unicode,
            Glyphs::Ascii => ascii,
        }
    }
}

impl ServiceStatus {
    pub fn read(plan: &LaunchAgentPlan, state_dir: &Path, app_id: &str) -> Self {
        Self {
            login: login_state(plan),
            running: is_running(state_dir, app_id),
        }
    }

    /// The human status: one line each for running and login, then one
    /// next step when something is off.
    pub fn human(&self, item: &LoginItem, glyphs: Glyphs) -> String {
        let ok = glyphs.get("✓", "OK");
        let off = glyphs.get("○", "-");
        let warn = glyphs.get("⚠", "WARN");
        let next = glyphs.get("→", "->");
        let name = &item.name;
        let command = &item.command;
        let mut lines = vec![match self.running {
            Some(true) => format!("{ok} {name} is in your menu bar"),
            Some(false) => format!("{off} {name} isn't in your menu bar"),
            None => format!("{warn} Couldn't tell whether {name} is running"),
        }];
        lines.push(match self.login {
            LoginState::On => format!("{ok} Opens at login"),
            LoginState::Off => format!("{off} Doesn't open at login"),
            LoginState::Outdated => format!("{warn} Opens at login with an older version"),
            LoginState::NotOurs => {
                format!("{warn} A login item for {name} was changed outside {command}")
            }
        });
        let hint = match (self.login, self.running) {
            (LoginState::Outdated | LoginState::NotOurs, _) => {
                Some(format!("{command} menubar install"))
            }
            (_, Some(false)) => Some(format!("{command} menubar start")),
            (LoginState::Off, _) => Some(format!("{command} menubar install")),
            _ => None,
        };
        if let Some(hint) = hint {
            lines.push(format!("{next} {hint}"));
        }
        lines.join("\n") + "\n"
    }
}

/// The line a product prints on stderr before writing its LaunchAgent,
/// matching the SDK's `LOGIN_ITEM` notice. `requester` is the name macOS
/// shows: the product once it launches through its local app, otherwise the
/// executable's own name.
pub fn login_item_notice(product: &str, requester: &str, glyphs: Glyphs) -> String {
    let lock = glyphs.get("🔐", "NOTE");
    let thats = if requester == product {
        String::new()
    } else {
        format!(". That's {product}'s menu bar")
    };
    format!(
        "{lock} macOS will show a notice that {requester} can open at login{thats}.\n   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in {SETTINGS_PATH}.\n"
    )
}

/// What `menubar install` prints after it wrote the LaunchAgent.
pub fn install_result(item: &LoginItem, change: Change, glyphs: Glyphs) -> String {
    let ok = glyphs.get("✓", "OK");
    let next = glyphs.get("→", "->");
    let name = &item.name;
    match change {
        Change::Unchanged => format!("{ok} {name} already opens at login\n"),
        _ => format!(
            "{ok} {name} will open at login\n{next} {} menubar start to open it now\n",
            item.command
        ),
    }
}

/// What `menubar uninstall` prints.
pub fn uninstall_result(item: &LoginItem, change: Change, glyphs: Glyphs) -> String {
    let ok = glyphs.get("✓", "OK");
    let name = &item.name;
    match change {
        Change::Removed => {
            format!("{ok} {name} won't open at login. Quit it from its menu when you're done.\n")
        }
        _ => format!("{ok} {name} already doesn't open at login\n"),
    }
}

/// One sentence and a next step for each error.
pub fn error_message(item: &LoginItem, error: ServiceError, glyphs: Glyphs) -> String {
    let fail = glyphs.get("✗", "FAIL");
    let next = glyphs.get("→", "->");
    let name = &item.name;
    let command = &item.command;
    match error.kind {
        ServiceErrorKind::NotOurs => format!(
            "{fail} {name}'s login item was changed outside {command}, so it was left alone.\n  Remove it in {SETTINGS_PATH}, then try again.\n{next} {command} menubar install\n"
        ),
        ServiceErrorKind::Unwritable => format!(
            "{fail} Couldn't write {name}'s login item. Check that your Library folder is writable.\n{next} {command} doctor\n"
        ),
        ServiceErrorKind::InvalidItem => format!("{fail} {name} can't set up its login item from this install.\n{next} {command} doctor\n"),
        ServiceErrorKind::Unsupported => format!("{fail} Opening at login is set up this way on macOS only.\n"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn temp_home() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "foundation-service-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn item(home: &Path) -> LoginItem {
        LoginItem {
            app_id: "aicharts".into(),
            name: "AI Charts".into(),
            command: "aicharts".into(),
            program: home.join("Applications/Hraness/AI Charts.app/Contents/MacOS/AI Charts"),
            args: vec!["menubar".into(), "--foreground".into()],
            bundle_id: Some("app.hraness.aicharts".into()),
        }
    }

    #[test]
    fn the_plan_points_at_the_bundle_and_is_owned() {
        let home = PathBuf::from("/Users/example");
        let plan = plan(&item(&home), &home).unwrap();
        assert_eq!(plan.label, "app.hraness.aicharts");
        assert_eq!(
            plan.path,
            home.join("Library/LaunchAgents/app.hraness.aicharts.plist")
        );
        assert_eq!(
            plan.legacy,
            vec![home.join("Library/LaunchAgents/app.hraness.companion.aicharts.plist")]
        );
        let (first, body) = plan.contents.split_once('\n').unwrap();
        assert!(first.starts_with("<!-- hraness-companion autostart aicharts sha256:"));
        assert_eq!(format!("{first}\n"), header("aicharts", body));
        assert!(body.contains("<array><string>/Users/example/Applications/Hraness/AI Charts.app/Contents/MacOS/AI Charts</string><string>menubar</string><string>--foreground</string></array>"));
        assert!(body.contains("<key>AssociatedBundleIdentifiers</key><array><string>app.hraness.aicharts</string></array>"));
        assert!(body.contains("<key>LimitLoadToSessionType</key><string>Aqua</string>"));
    }

    #[test]
    fn unsafe_items_are_rejected() {
        let home = PathBuf::from("/Users/example");
        let good = item(&home);
        let cases: Vec<Box<dyn Fn(&mut LoginItem)>> = vec![
            Box::new(|item| item.app_id = "AI Charts".into()),
            Box::new(|item| item.program = PathBuf::from("relative/aicharts")),
            Box::new(|item| item.args.push("line\nbreak".into())),
            Box::new(|item| item.bundle_id = Some("app.hraness.other".into())),
            Box::new(|item| item.args = vec!["x".into(); MAX_ARGS + 1]),
        ];
        for mutate in cases {
            let mut bad = good.clone();
            mutate(&mut bad);
            assert_eq!(
                plan(&bad, &home),
                Err(ServiceError::new(ServiceErrorKind::InvalidItem))
            );
        }
        assert!(plan(&good, Path::new("relative")).is_err());
        let escaped = LoginItem {
            args: vec!["<&>".into()],
            ..good
        };
        assert!(plan(&escaped, &home)
            .unwrap()
            .contents
            .contains("<string>&lt;&amp;&gt;</string>"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_update_uninstall_and_migration_in_a_temporary_home() {
        let home = temp_home();
        let spec = item(&home);
        let plan = plan(&spec, &home).unwrap();
        assert_eq!(login_state(&plan), LoginState::Off);
        // An older SDK agent for the same product is ours and migrates.
        let legacy_body = "<plist version=\"1.0\"><dict></dict></plist>\n";
        fs::create_dir_all(plan.path.parent().unwrap()).unwrap();
        fs::write(
            &plan.legacy[0],
            header("aicharts", legacy_body) + legacy_body,
        )
        .unwrap();
        assert_eq!(install(&plan), Ok(Change::Created));
        assert!(!plan.legacy[0].exists());
        assert_eq!(login_state(&plan), LoginState::On);
        assert_eq!(install(&plan), Ok(Change::Unchanged));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&plan.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let moved = self::plan(
            &LoginItem {
                args: vec!["menubar".into()],
                ..spec.clone()
            },
            &home,
        )
        .unwrap();
        assert_eq!(login_state(&moved), LoginState::Outdated);
        assert_eq!(install(&moved), Ok(Change::Updated));
        assert_eq!(uninstall(&moved), Ok(Change::Removed));
        assert_eq!(uninstall(&moved), Ok(Change::Unchanged));
        assert_eq!(login_state(&moved), LoginState::Off);
        // A hand-edited file is left alone.
        fs::write(&plan.path, "<plist><dict></dict></plist>\n").unwrap();
        assert_eq!(login_state(&plan), LoginState::NotOurs);
        assert_eq!(
            install(&plan),
            Err(ServiceError::new(ServiceErrorKind::NotOurs))
        );
        assert_eq!(
            uninstall(&plan),
            Err(ServiceError::new(ServiceErrorKind::NotOurs))
        );
        assert!(plan.path.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn the_instance_lock_reports_liveness_and_second_launches() {
        let state = temp_home();
        assert_eq!(is_running(&state, "aicharts"), Some(false));
        let lock = InstanceLock::acquire(&state, "aicharts")
            .unwrap()
            .expect("first launch");
        assert_eq!(is_running(&state, "aicharts"), Some(true));
        assert!(
            InstanceLock::acquire(&state, "aicharts").unwrap().is_none(),
            "second launch"
        );
        drop(lock);
        assert_eq!(is_running(&state, "aicharts"), Some(false));
        assert!(InstanceLock::acquire(&state, "aicharts").unwrap().is_some());
        assert!(InstanceLock::acquire(Path::new("relative"), "aicharts").is_err());
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn human_copy_is_plain_with_one_next_step() {
        let home = PathBuf::from("/Users/example");
        let spec = item(&home);
        let status =
            |login, running| ServiceStatus { login, running }.human(&spec, Glyphs::Unicode);
        assert_eq!(
            status(LoginState::On, Some(true)),
            "✓ AI Charts is in your menu bar\n✓ Opens at login\n"
        );
        assert_eq!(
            status(LoginState::Off, Some(false)),
            "○ AI Charts isn't in your menu bar\n○ Doesn't open at login\n→ aicharts menubar start\n"
        );
        assert_eq!(
            status(LoginState::Off, Some(true)),
            "✓ AI Charts is in your menu bar\n○ Doesn't open at login\n→ aicharts menubar install\n"
        );
        assert_eq!(
            status(LoginState::Outdated, None),
            "⚠ Couldn't tell whether AI Charts is running\n⚠ Opens at login with an older version\n→ aicharts menubar install\n"
        );
        assert_eq!(
            ServiceStatus { login: LoginState::NotOurs, running: Some(true) }.human(&spec, Glyphs::Ascii),
            "OK AI Charts is in your menu bar\nWARN A login item for AI Charts was changed outside aicharts\n-> aicharts menubar install\n"
        );
        assert_eq!(
            serde_json::to_string(&ServiceStatus {
                login: LoginState::On,
                running: Some(true)
            })
            .unwrap(),
            r#"{"login":"on","running":true}"#
        );
    }

    #[test]
    fn notices_and_results_match_the_permission_templates() {
        let home = PathBuf::from("/Users/example");
        let spec = item(&home);
        assert_eq!(
            login_item_notice("AI Charts", "AI Charts", Glyphs::Unicode),
            "🔐 macOS will show a notice that AI Charts can open at login.\n   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.\n"
        );
        assert_eq!(
            login_item_notice("AI Charts", "aicharts", Glyphs::Ascii),
            "NOTE macOS will show a notice that aicharts can open at login. That's AI Charts's menu bar.\n   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.\n"
        );
        assert_eq!(
            install_result(&spec, Change::Created, Glyphs::Unicode),
            "✓ AI Charts will open at login\n→ aicharts menubar start to open it now\n"
        );
        assert_eq!(
            install_result(&spec, Change::Unchanged, Glyphs::Unicode),
            "✓ AI Charts already opens at login\n"
        );
        assert_eq!(
            uninstall_result(&spec, Change::Removed, Glyphs::Unicode),
            "✓ AI Charts won't open at login. Quit it from its menu when you're done.\n"
        );
        assert_eq!(
            error_message(&spec, ServiceError::new(ServiceErrorKind::NotOurs), Glyphs::Unicode),
            "✗ AI Charts's login item was changed outside aicharts, so it was left alone.\n  Remove it in System Settings › General › Login Items & Extensions, then try again.\n→ aicharts menubar install\n"
        );
    }
}
