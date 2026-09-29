//! One owner process per product.
//!
//! The owner holds `<stateHome>/control/supervisor.lock` with `flock`,
//! writes `owner.json` and a fresh `admin.cap`, and serves two Unix
//! sockets inside the 0700 `control` directory:
//!
//! - `agent.sock` (0600) takes `{"v":1,"protocol":…,"request":…}` for the
//!   protocols the product registers, plus `hraness.control/1`
//!   `control.hello`. It refuses admin operations.
//! - `admin.sock` (0600) takes `{"v":1,"cap":<hex>,"request":…}`. The
//!   capability is defence in depth only: same-uid processes can read it,
//!   so no human gate relies on it being secret.
//!
//! Every frame is one line of JSON. See `docs/control.md`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::crypto;
use crate::envelope::{ErrorBody, ErrorCode};
use crate::process_identity;

/// The protocol of the built-in operations.
pub const CONTROL_PROTOCOL: &str = "hraness.control/1";
/// The wire version.
pub const WIRE_VERSION: u64 = 1;
/// The default largest request line, in bytes.
pub const DEFAULT_MAX_BYTES: usize = 1 << 20;
/// The default number of concurrent connections.
pub const DEFAULT_MAX_CLIENTS: usize = 32;
/// The default idle timeout of a connection.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(30);

/// Where one product's owner keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerPaths {
    pub state_home: PathBuf,
    pub dir: PathBuf,
    pub agent_sock: PathBuf,
    pub admin_sock: PathBuf,
    pub cap: PathBuf,
    pub owner_json: PathBuf,
    pub lock: PathBuf,
}

impl OwnerPaths {
    /// The layout under an explicit state home.
    pub fn in_state_home(state_home: impl Into<PathBuf>) -> Self {
        let state_home = state_home.into();
        let dir = state_home.join("control");
        Self {
            agent_sock: dir.join("agent.sock"),
            admin_sock: dir.join("admin.sock"),
            cap: dir.join("admin.cap"),
            owner_json: dir.join("owner.json"),
            lock: dir.join("supervisor.lock"),
            dir,
            state_home,
        }
    }
}

/// The state home of `product`: `$<PRODUCT>_STATE_HOME` when set, else
/// `~/Library/Application Support/<product>` on macOS and
/// `$XDG_STATE_HOME/<product>` (default `~/.local/state/<product>`)
/// elsewhere.
pub fn owner_paths(product: &str) -> Result<OwnerPaths, ErrorBody> {
    owner_paths_from(product, |k| std::env::var_os(k))
}

/// [`owner_paths`] with an explicit environment.
pub fn owner_paths_from(
    product: &str,
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<OwnerPaths, ErrorBody> {
    if !crate::envelope::valid_product_name(product) {
        return Err(ErrorBody::new(ErrorCode::Usage, "Invalid product name."));
    }
    let override_key = format!(
        "{}_STATE_HOME",
        product.to_ascii_uppercase().replace('-', "_")
    );
    let absolute = |v: std::ffi::OsString| {
        let p = PathBuf::from(v);
        p.is_absolute().then_some(p)
    };
    if let Some(p) = env(&override_key).and_then(absolute) {
        return Ok(OwnerPaths::in_state_home(p));
    }
    let home = env("HOME").and_then(absolute).ok_or_else(|| {
        ErrorBody::new(ErrorCode::Internal, "HOME is not set to an absolute path.")
    })?;
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        env("XDG_STATE_HOME")
            .and_then(absolute)
            .unwrap_or_else(|| home.join(".local/state"))
    };
    Ok(OwnerPaths::in_state_home(base.join(product)))
}

/// The contents of `owner.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerFile {
    pub schema: u32,
    pub pid: u32,
    pub boot_id: String,
    pub process_start_id: String,
    pub generation: String,
}

/// What `control.hello` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerInfo {
    pub product: String,
    pub pid: u32,
    pub generation: String,
    pub protocols: Vec<String>,
}

fn io_error(code: ErrorCode, message: &str, err: io::Error) -> ErrorBody {
    ErrorBody::new(code, message).with_detail(err.to_string())
}

fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Creates `dir` as 0700, or checks an existing one is a real directory
/// owned by us, and tightens its mode.
fn private_dir(dir: &Path) -> Result<(), ErrorBody> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| io_error(ErrorCode::Internal, "Could not create the state home.", e))?;
    }
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(io_error(
                ErrorCode::Internal,
                "Could not create the control directory.",
                e,
            ))
        }
    }
    let meta = fs::symlink_metadata(dir).map_err(|e| {
        io_error(
            ErrorCode::Internal,
            "Could not read the control directory.",
            e,
        )
    })?;
    if !meta.file_type().is_dir() || meta.uid() != uid() {
        return Err(ErrorBody::new(
            ErrorCode::PermissionDenied,
            "The control directory is not a directory owned by this user.",
        ));
    }
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
            io_error(
                ErrorCode::Internal,
                "Could not restrict the control directory.",
                e,
            )
        })?;
    }
    Ok(())
}

/// Writes `bytes` to `path` with mode 0600 through a rename, so readers
/// never see a partial file.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// Removes a leftover socket file. Refuses anything that is not a socket.
fn reclaim_socket(path: &Path) -> Result<(), ErrorBody> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(
            ErrorCode::Internal,
            "Could not inspect a socket path.",
            e,
        )),
        Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path)
            .map_err(|e| io_error(ErrorCode::Internal, "Could not remove a stale socket.", e)),
        Ok(_) => Err(ErrorBody::new(
            ErrorCode::PermissionDenied,
            "A control socket path holds something that is not a socket.",
        )
        .with_detail(path.display().to_string())),
    }
}

/// The exclusive right to be the owner, held for the life of the value.
#[derive(Debug)]
pub struct SupervisorLock {
    paths: OwnerPaths,
    _file: File,
    owner: OwnerFile,
    cap: [u8; 32],
}

impl SupervisorLock {
    /// Takes the lock without waiting, then writes `owner.json` and a new
    /// `admin.cap`. A second owner gets `control-already-running`.
    pub fn acquire(paths: &OwnerPaths) -> Result<Self, ErrorBody> {
        private_dir(&paths.dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&paths.lock)
            .map_err(|e| {
                io_error(
                    ErrorCode::Internal,
                    "Could not open the supervisor lock.",
                    e,
                )
            })?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                let mut err = ErrorBody::new(
                    ErrorCode::ControlAlreadyRunning,
                    "Another owner already holds the control lock.",
                );
                if let Ok(owner) = read_owner_file(paths) {
                    err = err.with_detail(format!("pid {}", owner.pid));
                }
                return Err(err);
            }
            Err(fs::TryLockError::Error(e)) => {
                return Err(io_error(
                    ErrorCode::Internal,
                    "Could not lock the supervisor lock.",
                    e,
                ))
            }
        }
        let pid = std::process::id();
        let (boot_id, process_start_id) = match (
            process_identity::boot_id(),
            process_identity::process_start_id(pid),
        ) {
            (Some(b), Some(s)) => (b, s),
            _ => {
                return Err(ErrorBody::new(
                    ErrorCode::Internal,
                    "Could not read this process's identity.",
                ))
            }
        };
        let random =
            |what| crypto::random_bytes::<32>().map_err(|e| io_error(ErrorCode::Internal, what, e));
        let generation = crypto::hex(&random("Could not draw a generation.")?[..16]);
        let cap = random("Could not draw an admin capability.")?;
        let owner = OwnerFile {
            schema: 1,
            pid,
            boot_id,
            process_start_id,
            generation,
        };
        write_private(&paths.cap, crypto::hex(&cap).as_bytes())
            .map_err(|e| io_error(ErrorCode::Internal, "Could not write admin.cap.", e))?;
        let json = serde_json::to_vec(&owner).expect("owner file serializes");
        write_private(&paths.owner_json, &json)
            .map_err(|e| io_error(ErrorCode::Internal, "Could not write owner.json.", e))?;
        Ok(Self {
            paths: paths.clone(),
            _file: file,
            owner,
            cap,
        })
    }

    pub fn paths(&self) -> &OwnerPaths {
        &self.paths
    }

    pub fn owner(&self) -> &OwnerFile {
        &self.owner
    }
}

impl Drop for SupervisorLock {
    fn drop(&mut self) {
        // Only files this owner wrote; the lock file itself stays.
        if read_owner_file(&self.paths).ok().as_ref() == Some(&self.owner) {
            let _ = fs::remove_file(&self.paths.owner_json);
        }
        let _ = fs::remove_file(&self.paths.cap);
    }
}

/// Reads `owner.json`.
pub fn read_owner_file(paths: &OwnerPaths) -> Result<OwnerFile, ErrorBody> {
    let bytes = fs::read(&paths.owner_json)
        .map_err(|e| io_error(ErrorCode::OwnerUnavailable, "No owner.json.", e))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        ErrorBody::new(ErrorCode::OwnerUnavailable, "owner.json is not valid.")
            .with_detail(e.to_string())
    })
}

/// A request handler. It gets the `request` member and answers `result`.
pub type Handler = Arc<dyn Fn(Value) -> Result<Value, ErrorBody> + Send + Sync>;

/// What the owner serves.
#[derive(Clone)]
pub struct ServeConfig {
    pub product: String,
    /// Agent protocols, by name such as `example.approvals/1`.
    pub agent: BTreeMap<String, Handler>,
    /// Admin requests other than the built-in operations.
    pub admin: Option<Handler>,
    pub max_clients: usize,
    pub max_bytes: usize,
    pub idle: Duration,
}

impl ServeConfig {
    pub fn new(product: &str) -> Self {
        Self {
            product: product.to_string(),
            agent: BTreeMap::new(),
            admin: None,
            max_clients: DEFAULT_MAX_CLIENTS,
            max_bytes: DEFAULT_MAX_BYTES,
            idle: DEFAULT_IDLE,
        }
    }

    pub fn agent_protocol(
        mut self,
        protocol: &str,
        handler: impl Fn(Value) -> Result<Value, ErrorBody> + Send + Sync + 'static,
    ) -> Self {
        self.agent.insert(protocol.to_string(), Arc::new(handler));
        self
    }

    pub fn admin(
        mut self,
        handler: impl Fn(Value) -> Result<Value, ErrorBody> + Send + Sync + 'static,
    ) -> Self {
        self.admin = Some(Arc::new(handler));
        self
    }
}

/// Asks [`serve`] to stop. Cloning shares the flag.
#[derive(Debug, Clone, Default)]
pub struct StopToken(Arc<AtomicBool>);

impl StopToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Socket {
    Agent,
    Admin,
}

struct Shared {
    config: ServeConfig,
    info: OwnerInfo,
    cap: [u8; 32],
    stop: StopToken,
    active: AtomicUsize,
}

fn bind(path: &Path) -> Result<UnixListener, ErrorBody> {
    reclaim_socket(path)?;
    let listener = UnixListener::bind(path)
        .map_err(|e| io_error(ErrorCode::Internal, "Could not bind a control socket.", e))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| {
        io_error(
            ErrorCode::Internal,
            "Could not restrict a control socket.",
            e,
        )
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|e| io_error(ErrorCode::Internal, "Could not configure a socket.", e))?;
    Ok(listener)
}

/// Serves both sockets until `stop` is set or an admin `control.stop`
/// arrives, then removes the sockets and releases the lock. Any socket
/// file left by a crashed owner is reclaimed: holding the lock proves no
/// other owner uses it.
pub fn serve(lock: SupervisorLock, config: ServeConfig, stop: StopToken) -> Result<(), ErrorBody> {
    let paths = lock.paths.clone();
    let agent = bind(&paths.agent_sock)?;
    let admin = match bind(&paths.admin_sock) {
        Ok(l) => l,
        Err(e) => {
            let _ = fs::remove_file(&paths.agent_sock);
            return Err(e);
        }
    };
    let mut protocols: Vec<String> = config.agent.keys().cloned().collect();
    protocols.push(CONTROL_PROTOCOL.to_string());
    protocols.sort();
    let shared = Arc::new(Shared {
        info: OwnerInfo {
            product: config.product.clone(),
            pid: lock.owner.pid,
            generation: lock.owner.generation.clone(),
            protocols,
        },
        cap: lock.cap,
        config,
        stop: stop.clone(),
        active: AtomicUsize::new(0),
    });
    let mut workers = Vec::new();
    while !stop.stopped() {
        let mut idle = true;
        for (listener, kind) in [(&agent, Socket::Agent), (&admin, Socket::Admin)] {
            match listener.accept() {
                Ok((stream, _)) => {
                    idle = false;
                    let shared = Arc::clone(&shared);
                    if shared.active.fetch_add(1, Ordering::SeqCst) >= shared.config.max_clients {
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                        let _ = refuse_busy(stream);
                        continue;
                    }
                    workers.push(thread::spawn(move || {
                        let _ = connection(&shared, stream, kind);
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                    }));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }
        workers.retain(|w| !w.is_finished());
        if idle {
            thread::sleep(Duration::from_millis(15));
        }
    }
    drop(agent);
    drop(admin);
    let _ = fs::remove_file(&paths.agent_sock);
    let _ = fs::remove_file(&paths.admin_sock);
    // Connections end at their idle timeout at the latest.
    for worker in workers {
        let _ = worker.join();
    }
    drop(lock);
    Ok(())
}

fn refuse_busy(mut stream: UnixStream) -> io::Result<()> {
    let body = ErrorBody::new(ErrorCode::OwnerUnavailable, "The owner is busy. Try again.");
    stream.write_all(&response_line(Err(body)))
}

fn response_line(result: Result<Value, ErrorBody>) -> Vec<u8> {
    let value = match result {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": {"code": error.code, "message": error.message}}),
    };
    let mut line = serde_json::to_vec(&value).expect("response serializes");
    line.push(b'\n');
    line
}

fn connection(shared: &Shared, stream: UnixStream, kind: Socket) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(shared.config.idle))?;
    stream.set_write_timeout(Some(shared.config.idle))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = Vec::new();
        let limit = shared.config.max_bytes as u64 + 1;
        let n = (&mut reader).take(limit).read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        if line.last() != Some(&b'\n') {
            let body = ErrorBody::new(
                ErrorCode::Usage,
                "The request is too large or unterminated.",
            );
            writer.write_all(&response_line(Err(body)))?;
            return Ok(());
        }
        let result = handle(shared, &line, kind);
        writer.write_all(&response_line(result))?;
        if shared.stop.stopped() {
            return Ok(());
        }
    }
}

fn usage(message: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::Usage, message)
}

fn op_of(request: &Value) -> Option<&str> {
    request.get("op").and_then(Value::as_str)
}

fn handle(shared: &Shared, line: &[u8], kind: Socket) -> Result<Value, ErrorBody> {
    let frame: Value =
        serde_json::from_slice(line).map_err(|_| usage("The request is not JSON."))?;
    let object = frame
        .as_object()
        .ok_or_else(|| usage("The request is not an object."))?;
    if object.get("v").and_then(Value::as_u64) != Some(WIRE_VERSION) {
        return Err(usage("Unsupported wire version."));
    }
    let request = object.get("request").cloned().unwrap_or(Value::Null);
    match kind {
        Socket::Agent => {
            if object.contains_key("cap") {
                return Err(ErrorBody::new(
                    ErrorCode::PermissionDenied,
                    "The agent socket does not take admin requests.",
                ));
            }
            let protocol = object
                .get("protocol")
                .and_then(Value::as_str)
                .ok_or_else(|| usage("The request names no protocol."))?;
            if protocol == CONTROL_PROTOCOL {
                return match op_of(&request) {
                    Some("control.hello") => Ok(serde_json::to_value(&shared.info).unwrap()),
                    _ => Err(ErrorBody::new(
                        ErrorCode::PermissionDenied,
                        "The agent socket does not take admin requests.",
                    )),
                };
            }
            let handler =
                shared.config.agent.get(protocol).ok_or_else(|| {
                    ErrorBody::new(ErrorCode::PermissionDenied, "Unknown protocol.")
                })?;
            handler(request)
        }
        Socket::Admin => {
            let cap = object
                .get("cap")
                .and_then(Value::as_str)
                .and_then(crypto::unhex)
                .unwrap_or_default();
            if !crypto::constant_time_eq(&cap, &shared.cap) {
                return Err(ErrorBody::new(
                    ErrorCode::PermissionDenied,
                    "The admin capability does not match.",
                ));
            }
            match op_of(&request) {
                Some("control.hello") => Ok(serde_json::to_value(&shared.info).unwrap()),
                Some("control.stop") => {
                    shared.stop.stop();
                    Ok(json!({"stopping": true}))
                }
                _ => match &shared.config.admin {
                    Some(handler) => handler(request),
                    None => Err(ErrorBody::new(
                        ErrorCode::NotFound,
                        "Unknown admin operation.",
                    )),
                },
            }
        }
    }
}

fn exchange(sock: &Path, frame: &Value, timeout: Duration) -> Result<Value, ErrorBody> {
    let unavailable = |e: io::Error| {
        io_error(
            ErrorCode::OwnerUnavailable,
            "The owner is not answering.",
            e,
        )
    };
    let mut stream = UnixStream::connect(sock).map_err(unavailable)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(unavailable)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(unavailable)?;
    let mut line = serde_json::to_vec(frame).expect("frame serializes");
    line.push(b'\n');
    stream.write_all(&line).map_err(unavailable)?;
    let mut reply = Vec::new();
    BufReader::new(stream)
        .take(DEFAULT_MAX_BYTES as u64 + 1)
        .read_until(b'\n', &mut reply)
        .map_err(unavailable)?;
    let value: Value = serde_json::from_slice(&reply).map_err(|_| {
        ErrorBody::new(
            ErrorCode::OwnerUnavailable,
            "The owner answered something unreadable.",
        )
    })?;
    if value.get("ok") == Some(&Value::Bool(true)) {
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
    let error = value.get("error");
    let code = error
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .and_then(|c| serde_json::from_value::<ErrorCode>(json!(c)).ok())
        .unwrap_or(ErrorCode::Internal);
    let message = error
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The owner refused the request.");
    Err(ErrorBody::new(code, message))
}

/// Sends one admin request.
pub fn admin_request(paths: &OwnerPaths, request: Value) -> Result<Value, ErrorBody> {
    let cap = fs::read_to_string(&paths.cap)
        .map_err(|e| io_error(ErrorCode::OwnerUnavailable, "No admin capability.", e))?;
    exchange(
        &paths.admin_sock,
        &json!({"v": WIRE_VERSION, "cap": cap.trim(), "request": request}),
        DEFAULT_IDLE,
    )
}

/// Sends one agent request.
pub fn agent_request(
    paths: &OwnerPaths,
    protocol: &str,
    request: Value,
) -> Result<Value, ErrorBody> {
    exchange(
        &paths.agent_sock,
        &json!({"v": WIRE_VERSION, "protocol": protocol, "request": request}),
        DEFAULT_IDLE,
    )
}

fn hello(paths: &OwnerPaths) -> Result<OwnerInfo, ErrorBody> {
    let value = exchange(
        &paths.agent_sock,
        &json!({"v": WIRE_VERSION, "protocol": CONTROL_PROTOCOL, "request": {"op": "control.hello"}}),
        Duration::from_secs(2),
    )?;
    serde_json::from_value(value).map_err(|_| {
        ErrorBody::new(
            ErrorCode::OwnerUnavailable,
            "The owner's hello is unreadable.",
        )
    })
}

/// What `control status` reports. Reading it never signals any process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerStatus {
    /// An owner answered `control.hello`.
    pub running: bool,
    /// `owner.json` names a process that no longer exists in this boot.
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<OwnerInfo>,
}

/// Reports whether an owner is running.
pub fn control_status(paths: &OwnerPaths) -> OwnerStatus {
    if let Ok(info) = hello(paths) {
        return OwnerStatus {
            running: true,
            stale: false,
            owner: Some(info),
        };
    }
    let stale = match read_owner_file(paths) {
        Ok(f) => !process_identity::matches(f.pid, &f.boot_id, &f.process_start_id),
        Err(_) => false,
    };
    OwnerStatus {
        running: false,
        stale,
        owner: None,
    }
}

/// Returns the running owner, or spawns one and waits until it answers.
/// A child that exits or does not answer within `timeout` is killed and
/// reaped; nothing else is ever signalled. When two callers race, the
/// loser's child exits with `control-already-running` and this still
/// returns the winner.
pub fn ensure_owner(
    paths: &OwnerPaths,
    spawn: impl FnOnce() -> io::Result<Child>,
    timeout: Duration,
) -> Result<OwnerInfo, ErrorBody> {
    if let Ok(info) = hello(paths) {
        return Ok(info);
    }
    let mut child = spawn()
        .map_err(|e| io_error(ErrorCode::OwnerUnavailable, "Could not start the owner.", e))?;
    let deadline = Instant::now() + timeout;
    let mut exited = false;
    loop {
        if let Ok(info) = hello(paths) {
            return Ok(info);
        }
        if !exited {
            match child.try_wait() {
                Ok(Some(_)) => exited = true,
                Ok(None) => {}
                Err(_) => exited = true,
            }
        }
        if Instant::now() >= deadline {
            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
            return Err(ErrorBody::new(
                ErrorCode::OwnerUnavailable,
                "The owner did not start in time.",
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::AtomicU32;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            // Short: macOS limits socket paths to 104 bytes.
            let p = PathBuf::from("/tmp").join(format!(
                "hck-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Another test's fork can hold a copy of a just-released lock fd
    /// until its exec closes it (O_CLOEXEC), so re-acquiring retries.
    fn acquire(paths: &OwnerPaths) -> SupervisorLock {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match SupervisorLock::acquire(paths) {
                Ok(lock) => return lock,
                Err(e)
                    if e.code == ErrorCode::ControlAlreadyRunning && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    fn start(paths: &OwnerPaths, config: ServeConfig) -> (StopToken, thread::JoinHandle<()>) {
        let lock = acquire(paths);
        let stop = StopToken::new();
        let s = stop.clone();
        let handle = thread::spawn(move || serve(lock, config, s).unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while hello(paths).is_err() {
            assert!(Instant::now() < deadline, "owner never answered");
            thread::sleep(Duration::from_millis(10));
        }
        (stop, handle)
    }

    fn mode(p: &Path) -> u32 {
        fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    #[test]
    fn paths_follow_the_platform_and_override() {
        let env = |k: &str| match k {
            "HOME" => Some("/h".into()),
            "XDG_STATE_HOME" => Some("/x".into()),
            _ => None,
        };
        let p = owner_paths_from("example", env).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(
                p.state_home,
                Path::new("/h/Library/Application Support/example")
            );
        } else {
            assert_eq!(p.state_home, Path::new("/x/example"));
        }
        assert_eq!(p.agent_sock, p.state_home.join("control/agent.sock"));
        let over = |k: &str| (k == "MY_APP_STATE_HOME").then(|| "/s".into());
        assert_eq!(
            owner_paths_from("my-app", over).unwrap().state_home,
            Path::new("/s")
        );
        let relative = |k: &str| (k == "HOME").then(|| "rel".into());
        assert!(owner_paths_from("x", relative).is_err());
        assert!(owner_paths_from("Bad", env).is_err());
    }

    #[test]
    fn serves_with_private_modes_and_answers_both_sockets() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let config = ServeConfig::new("example")
            .agent_protocol("example.approvals/1", |req| Ok(json!({"echo": req})))
            .admin(|req| Ok(json!({"admin": req})));
        let (stop, handle) = start(&paths, config);
        assert_eq!(mode(&paths.dir), 0o700);
        assert_eq!(mode(&paths.agent_sock), 0o600);
        assert_eq!(mode(&paths.admin_sock), 0o600);
        assert_eq!(mode(&paths.cap), 0o600);
        assert_eq!(mode(&paths.owner_json), 0o600);

        let owner = read_owner_file(&paths).unwrap();
        assert_eq!(owner.pid, std::process::id());
        assert!(process_identity::matches(
            owner.pid,
            &owner.boot_id,
            &owner.process_start_id
        ));

        assert_eq!(
            agent_request(&paths, "example.approvals/1", json!({"op": "list"})).unwrap(),
            json!({"echo": {"op": "list"}})
        );
        let unknown = agent_request(&paths, "other/1", json!({})).unwrap_err();
        assert_eq!(unknown.code, ErrorCode::PermissionDenied);
        let stop_from_agent =
            agent_request(&paths, CONTROL_PROTOCOL, json!({"op": "control.stop"})).unwrap_err();
        assert_eq!(stop_from_agent.code, ErrorCode::PermissionDenied);
        let smuggled = exchange(
            &paths.agent_sock,
            &json!({"v": 1, "protocol": "example.approvals/1", "cap": "00", "request": {}}),
            DEFAULT_IDLE,
        )
        .unwrap_err();
        assert_eq!(
            smuggled.message,
            "The agent socket does not take admin requests."
        );

        let bad_cap = exchange(
            &paths.admin_sock,
            &json!({"v": 1, "cap": "0".repeat(64), "request": {"op": "control.stop"}}),
            DEFAULT_IDLE,
        )
        .unwrap_err();
        assert_eq!(bad_cap.code, ErrorCode::PermissionDenied);
        assert_eq!(
            admin_request(&paths, json!({"op": "x"})).unwrap(),
            json!({"admin": {"op": "x"}})
        );
        let status = control_status(&paths);
        assert!(status.running);
        assert_eq!(
            status.owner.unwrap().protocols,
            vec!["example.approvals/1", CONTROL_PROTOCOL]
        );

        assert_eq!(
            admin_request(&paths, json!({"op": "control.stop"})).unwrap(),
            json!({"stopping": true})
        );
        handle.join().unwrap();
        assert!(stop.stopped());
        assert!(!paths.agent_sock.exists());
        assert!(!paths.owner_json.exists());
        assert!(!paths.cap.exists());
        assert!(!control_status(&paths).running);
    }

    #[test]
    fn a_second_owner_gets_control_already_running() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let first = SupervisorLock::acquire(&paths).unwrap();
        let second = SupervisorLock::acquire(&paths).unwrap_err();
        assert_eq!(second.code, ErrorCode::ControlAlreadyRunning);
        assert_eq!(second.code.exit_code(), crate::envelope::EXIT_CONFLICT);
        drop(first);
        acquire(&paths);
    }

    #[test]
    fn stale_sockets_are_reclaimed_and_other_files_refused() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        private_dir(&paths.dir).unwrap();
        // A crashed owner's socket: bound, then the listener dropped.
        drop(UnixListener::bind(&paths.agent_sock).unwrap());
        drop(UnixListener::bind(&paths.admin_sock).unwrap());
        let (stop, handle) = start(&paths, ServeConfig::new("example"));
        stop.stop();
        handle.join().unwrap();

        fs::write(&paths.agent_sock, b"not a socket").unwrap();
        let lock = acquire(&paths);
        let err = serve(lock, ServeConfig::new("example"), StopToken::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(fs::read(&paths.agent_sock).unwrap(), b"not a socket");
    }

    #[test]
    fn a_wide_control_directory_is_tightened_and_a_symlink_refused() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        fs::create_dir_all(&paths.dir).unwrap();
        fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o755)).unwrap();
        drop(acquire(&paths));
        assert_eq!(mode(&paths.dir), 0o700);

        let t2 = Temp::new();
        let linked = OwnerPaths::in_state_home(&t2.0);
        std::os::unix::fs::symlink(&paths.dir, &linked.dir).unwrap();
        assert_eq!(
            SupervisorLock::acquire(&linked).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }

    #[test]
    fn oversized_and_malformed_requests_are_refused() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let mut config =
            ServeConfig::new("example").agent_protocol("example.a/1", |_| Ok(Value::Null));
        config.max_bytes = 128;
        let (stop, handle) = start(&paths, config);
        let big = agent_request(&paths, "example.a/1", json!("x".repeat(200))).unwrap_err();
        assert_eq!(big.code, ErrorCode::Usage);
        let old = exchange(&paths.agent_sock, &json!({"v": 2}), DEFAULT_IDLE).unwrap_err();
        assert_eq!(old.code, ErrorCode::Usage);
        stop.stop();
        handle.join().unwrap();
    }

    #[test]
    fn ensure_owner_reaps_a_child_that_never_confirms() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let pid = Arc::new(AtomicU32::new(0));
        let p = Arc::clone(&pid);
        let err = ensure_owner(
            &paths,
            || {
                let child = Command::new("sleep").arg("30").spawn()?;
                p.store(child.id(), Ordering::SeqCst);
                Ok(child)
            },
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OwnerUnavailable);
        assert_eq!(
            err.code.exit_code(),
            crate::envelope::EXIT_OWNER_UNAVAILABLE
        );
        // Reaped: no process with that PID remains in this boot.
        assert!(process_identity::process_start_id(pid.load(Ordering::SeqCst)).is_none());
    }

    #[test]
    fn ensure_owner_returns_a_running_owner_without_spawning() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let (stop, handle) = start(&paths, ServeConfig::new("example"));
        let info =
            ensure_owner(&paths, || panic!("must not spawn"), Duration::from_secs(1)).unwrap();
        assert_eq!(info.pid, std::process::id());
        stop.stop();
        handle.join().unwrap();
    }

    #[test]
    fn ensure_owner_waits_for_a_spawned_owner() {
        let t = Temp::new();
        let paths = OwnerPaths::in_state_home(&t.0);
        let p2 = paths.clone();
        let stop = StopToken::new();
        let s2 = stop.clone();
        let mut server = None;
        let info = ensure_owner(
            &paths,
            || {
                server = Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    let lock = acquire(&p2);
                    serve(lock, ServeConfig::new("example"), s2).unwrap();
                }));
                Command::new("true").spawn()
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(info.product, "example");
        stop.stop();
        server.unwrap().join().unwrap();
    }
}
