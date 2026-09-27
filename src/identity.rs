//! Local app identity on macOS: one `~/Applications/Hraness/<Product>.app`
//! per product, assembled on this Mac around an already verified executable
//! and signed with one persistent local signing identity. See
//! `docs/identity.md`.
//!
//! Nothing here runs by itself. The signing identity is created only by
//! [`ensure_signing_identity`], which a product calls after it has shown the
//! `LOCAL_SIGNING` notice. Every command goes through [`Tools`], so tests
//! inject fakes and never touch a real keychain.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::protocol::valid_app_id;
use crate::service::LoginItem;

pub const IDENTITY_NAME: &str = "Hraness Local Signing";
pub const CODESIGN: &str = "/usr/bin/codesign";
pub const SECURITY: &str = "/usr/bin/security";
pub const OPENSSL: &str = "/usr/bin/openssl";
/// Environment override: `ad-hoc` forces ad-hoc signing.
pub const SIGNING_ENV: &str = "HRANESS_SIGNING";
/// Set in the environment of the command `--launch` starts.
pub const BUNDLE_ID_ENV: &str = "HRANESS_APP_BUNDLE_ID";
pub const MAX_USAGE_CHARS: usize = 110;
const MAX_NAME_CHARS: usize = 64;
const MAX_HELPERS: usize = 8;
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ICON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARGV_FILE_BYTES: u64 = 64 * 1024;
const USAGE_KEYS: &[&str] = &[
    "NSAppleEventsUsageDescription",
    "NSContactsUsageDescription",
    "NSLocalNetworkUsageDescription",
    "NSCameraUsageDescription",
    "NSMicrophoneUsageDescription",
];

/// Why an app could not be built. Each maps to one `app-result` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppError {
    QuarantinedInput,
    IdentityUnavailable,
    SigningDeclined,
    SigningFailed,
    UnsafeDestination,
    InvalidAppRequest,
}

impl AppError {
    pub fn code(self) -> &'static str {
        match self {
            AppError::QuarantinedInput => "quarantined-input",
            AppError::IdentityUnavailable => "identity-unavailable",
            AppError::SigningDeclined => "signing-declined",
            AppError::SigningFailed => "signing-failed",
            AppError::UnsafeDestination => "unsafe-destination",
            AppError::InvalidAppRequest => "invalid-app-request",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Signing {
    /// The persistent `Hraness Local Signing` identity.
    #[default]
    Local,
    /// Ad-hoc: approvals reset after each update.
    AdHoc,
}

impl Signing {
    pub fn wire(self) -> &'static str {
        match self {
            Signing::Local => "local",
            Signing::AdHoc => "ad-hoc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperSpec {
    /// File name under `Contents/Helpers`, such as `ghostget-cookie-reader`.
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}

/// One product's app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSpec {
    pub app_id: String,
    /// Registry display name, such as `AI Charts`. Also the executable name.
    pub name: String,
    pub product_version: String,
    /// The executable copied to `Contents/MacOS/<name>`: the verified runner
    /// for SDK products, or the product's own binary for Rust products.
    pub executable: PathBuf,
    pub executable_sha256: String,
    /// A square PNG, ideally 1024 px.
    pub icon_png: Option<PathBuf>,
    /// Only the `NS*UsageDescription` keys the product needs.
    pub usage: BTreeMap<String, String>,
    pub helpers: Vec<HelperSpec>,
    pub signing: Signing,
}

impl AppSpec {
    pub fn bundle_id(&self) -> String {
        format!("app.hraness.{}", self.app_id)
    }
}

/// Runs the system tools. Tests supply a fake.
pub trait Tools {
    fn run(&self, program: &str, args: &[OsString]) -> std::io::Result<Output>;
}

/// The real `/usr/bin` tools.
pub struct SystemTools;

impl Tools for SystemTools {
    fn run(&self, program: &str, args: &[OsString]) -> std::io::Result<Output> {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .env_remove("DYLD_INSERT_LIBRARIES")
            .output()
    }
}

/// Where apps and the identity live.
pub struct Environment<'a> {
    pub home: PathBuf,
    /// The keychain that holds the identity: the login keychain by default.
    pub keychain: PathBuf,
    pub tools: &'a dyn Tools,
    /// `HRANESS_SIGNING=ad-hoc` in the product's environment.
    pub force_ad_hoc: bool,
}

impl<'a> Environment<'a> {
    pub fn for_user(home: PathBuf, tools: &'a dyn Tools) -> Self {
        let keychain = home.join("Library/Keychains/login.keychain-db");
        let force_ad_hoc = std::env::var(SIGNING_ENV).is_ok_and(|value| value == "ad-hoc");
        Environment {
            home,
            keychain,
            tools,
            force_ad_hoc,
        }
    }

    pub fn apps_dir(&self) -> PathBuf {
        self.home.join("Applications").join("Hraness")
    }

    pub fn app_path(&self, name: &str) -> PathBuf {
        self.apps_dir().join(format!("{name}.app"))
    }
}

fn os(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

// ---------------------------------------------------------------------------
// Signing identity

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum IdentityState {
    Ready { sha1: String },
    Missing,
    Unavailable,
}

/// Parses `security find-identity -p codesigning` output. The identity is
/// self-signed and untrusted, so it is listed without `-v`.
fn parse_identities(output: &str) -> Option<String> {
    let quoted = format!("\"{IDENTITY_NAME}\"");
    output.lines().find_map(|line| {
        let (_, rest) = line.trim().split_once(") ")?;
        let (hash, label) = rest.split_once(' ')?;
        (label.starts_with(&quoted)
            && hash.len() == 40
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_uppercase())
    })
}

/// Reads the identity's state without changing anything or prompting.
pub fn signing_identity_status(env: &Environment) -> IdentityState {
    let mut args = os(&["find-identity", "-p", "codesigning"]);
    args.push(env.keychain.clone().into_os_string());
    match env.tools.run(SECURITY, &args) {
        Ok(output) if output.status.success() => {
            match parse_identities(&String::from_utf8_lossy(&output.stdout)) {
                Some(sha1) => IdentityState::Ready { sha1 },
                None => IdentityState::Missing,
            }
        }
        _ => IdentityState::Unavailable,
    }
}

const OPENSSL_CONFIG: &str = "[req]\ndistinguished_name = dn\nprompt = no\n[dn]\nCN = Hraness Local Signing\n[ext]\nbasicConstraints = critical, CA:false\nkeyUsage = critical, digitalSignature\nextendedKeyUsage = critical, codeSigning\nsubjectKeyIdentifier = hash\n";

fn random_hex(bytes: usize) -> Option<String> {
    let mut buffer = vec![0u8; bytes];
    fs::File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut buffer)
        .ok()?;
    Some(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn private_temp_dir(parent: &Path) -> Option<PathBuf> {
    let dir = parent.join(format!("hraness-identity-{}", random_hex(8)?));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir).ok()?;
    Some(dir)
}

/// Creates the identity when it is missing, in `env.keychain`: an RSA 2048
/// key made on this Mac and imported non-extractable with `codesign` as its
/// only trusted application, and a self-signed code signing certificate
/// valid for 20 years. Never adds trust settings. Call this only after
/// showing the `LOCAL_SIGNING` notice; if the keychain is locked macOS asks
/// to unlock it.
///
/// The key exists unencrypted for a moment in an owner-only temporary
/// folder, which is removed before this returns.
pub fn ensure_signing_identity(env: &Environment, temp_root: &Path) -> IdentityState {
    if let ready @ IdentityState::Ready { .. } = signing_identity_status(env) {
        return ready;
    }
    let Some(dir) = private_temp_dir(temp_root) else {
        return IdentityState::Unavailable;
    };
    let created = create_identity(env, &dir);
    let _ = fs::remove_dir_all(&dir);
    if created.is_none() {
        return IdentityState::Unavailable;
    }
    signing_identity_status(env)
}

fn create_identity(env: &Environment, dir: &Path) -> Option<()> {
    let config = dir.join("openssl.cnf");
    fs::write(&config, OPENSSL_CONFIG).ok()?;
    let key = dir.join("key.pem");
    let cert = dir.join("cert.pem");
    let bundle = dir.join("identity.p12");
    let password = random_hex(16)?;
    let run = |program: &str, args: Vec<OsString>| -> Option<()> {
        env.tools
            .run(program, &args)
            .ok()
            .filter(|output| output.status.success())
            .map(|_| ())
    };
    let path = |value: &Path| value.as_os_str().to_owned();
    let mut req = os(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "7305",
        "-sha256",
        "-extensions",
        "ext",
        "-config",
    ]);
    req.extend([
        path(&config),
        "-keyout".into(),
        path(&key),
        "-out".into(),
        path(&cert),
    ]);
    run(OPENSSL, req)?;
    // 3DES/SHA-1 PKCS#12 is what `security import` reads on every macOS.
    let mut export = os(&[
        "pkcs12",
        "-export",
        "-keypbe",
        "PBE-SHA1-3DES",
        "-certpbe",
        "PBE-SHA1-3DES",
        "-macalg",
        "sha1",
        "-name",
        IDENTITY_NAME,
        "-passout",
    ]);
    export.extend([
        format!("pass:{password}").into(),
        "-inkey".into(),
        path(&key),
        "-in".into(),
        path(&cert),
        "-out".into(),
        path(&bundle),
    ]);
    run(OPENSSL, export)?;
    let _ = fs::remove_file(&key);
    let mut import = os(&["import"]);
    import.extend([path(&bundle), "-k".into(), path(&env.keychain)]);
    import.extend(os(&["-f", "pkcs12", "-x", "-T", CODESIGN, "-P"]));
    import.push(password.into());
    run(SECURITY, import)
}

// ---------------------------------------------------------------------------
// Validation

fn safe_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

/// A display name that is also a safe file name.
pub fn valid_display_name(name: &str) -> bool {
    safe_text(name, MAX_NAME_CHARS)
        && !name.starts_with('.')
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && !name.contains(['/', ':', '\\'])
}

fn valid_helper_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn validate(spec: &AppSpec) -> Result<(), AppError> {
    let version_ok = |value: &str| {
        !value.is_empty()
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    };
    let mut helper_names: Vec<&str> = spec
        .helpers
        .iter()
        .map(|helper| helper.name.as_str())
        .collect();
    helper_names.sort_unstable();
    helper_names.dedup();
    let ok = valid_app_id(&spec.app_id)
        && valid_display_name(&spec.name)
        && version_ok(&spec.product_version)
        && spec.executable.is_absolute()
        && valid_sha256(&spec.executable_sha256)
        && spec
            .icon_png
            .as_ref()
            .map_or(true, |path| path.is_absolute())
        && spec.usage.iter().all(|(key, value)| {
            USAGE_KEYS.contains(&key.as_str()) && safe_text(value, MAX_USAGE_CHARS)
        })
        && spec.helpers.len() <= MAX_HELPERS
        && helper_names.len() == spec.helpers.len()
        && spec.helpers.iter().all(|helper| {
            valid_helper_name(&helper.name)
                && helper.path.is_absolute()
                && valid_sha256(&helper.sha256)
        });
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidAppRequest)
    }
}

#[cfg(target_os = "macos")]
fn quarantined(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return true;
    };
    let name = c"com.apple.quarantine";
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    size >= 0
}

#[cfg(not(target_os = "macos"))]
fn quarantined(_path: &Path) -> bool {
    false
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reads a regular, non-symlink input file within `max` bytes.
fn read_input(path: &Path, max: u64) -> Result<Vec<u8>, AppError> {
    let meta = fs::symlink_metadata(path).map_err(|_| AppError::InvalidAppRequest)?;
    if !meta.is_file() || meta.len() > max {
        return Err(AppError::InvalidAppRequest);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(max + 1).read_to_end(&mut bytes))
        .map_err(|_| AppError::InvalidAppRequest)?;
    if bytes.len() as u64 > max {
        return Err(AppError::InvalidAppRequest);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Bundle contents

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The digest of every input, stored in Info.plist so an unchanged request
/// is recognized without rebuilding.
fn inputs_digest(
    spec: &AppSpec,
    icon: Option<&[u8]>,
    signing: Signing,
    runner_version: &str,
) -> String {
    let mut hasher = Sha256::new();
    let mut field = |value: &[u8]| {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    };
    for value in [
        &spec.app_id,
        &spec.name,
        &spec.product_version,
        &spec.executable_sha256,
    ] {
        field(value.as_bytes());
    }
    field(runner_version.as_bytes());
    field(signing.wire().as_bytes());
    field(icon.map(sha256_hex).unwrap_or_default().as_bytes());
    for (key, value) in &spec.usage {
        field(key.as_bytes());
        field(value.as_bytes());
    }
    for helper in &spec.helpers {
        field(helper.name.as_bytes());
        field(helper.sha256.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Renders Info.plist. Deterministic for one request.
pub fn info_plist(spec: &AppSpec, runner_version: &str, inputs: &str, has_icon: bool) -> String {
    let mut entries: Vec<(String, String)> = vec![
        (
            "CFBundleIdentifier".into(),
            format!("<string>{}</string>", xml(&spec.bundle_id())),
        ),
        (
            "CFBundleName".into(),
            format!("<string>{}</string>", xml(&spec.name)),
        ),
        (
            "CFBundleDisplayName".into(),
            format!("<string>{}</string>", xml(&spec.name)),
        ),
        (
            "CFBundleExecutable".into(),
            format!("<string>{}</string>", xml(&spec.name)),
        ),
        ("CFBundlePackageType".into(), "<string>APPL</string>".into()),
        (
            "CFBundleInfoDictionaryVersion".into(),
            "<string>6.0</string>".into(),
        ),
        (
            "CFBundleShortVersionString".into(),
            format!("<string>{}</string>", xml(&spec.product_version)),
        ),
        (
            "CFBundleVersion".into(),
            format!(
                "<string>{}+df.{}</string>",
                xml(&spec.product_version),
                xml(runner_version)
            ),
        ),
        ("LSUIElement".into(), "<true/>".into()),
        (
            "LSMinimumSystemVersion".into(),
            "<string>13.0</string>".into(),
        ),
        ("NSHighResolutionCapable".into(), "<true/>".into()),
        (
            "HranessRunnerSha256".into(),
            format!("<string>{}</string>", spec.executable_sha256),
        ),
        (
            "HranessInputsSha256".into(),
            format!("<string>{inputs}</string>"),
        ),
    ];
    if has_icon {
        entries.push(("CFBundleIconFile".into(), "<string>AppIcon</string>".into()));
    }
    for (key, value) in &spec.usage {
        entries.push((key.clone(), format!("<string>{}</string>", xml(value))));
    }
    let body: String = entries
        .iter()
        .map(|(key, value)| format!("\t<key>{key}</key>\n\t{value}\n"))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{body}</dict>\n</plist>\n"
    )
}

/// Reads one `<string>` value from an Info.plist this module wrote.
pub fn plist_string(plist: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = &plist[plist.find(&marker)? + marker.len()..];
    let rest = rest.trim_start().strip_prefix("<string>")?;
    let value = &rest[..rest.find("</string>")?];
    Some(
        value
            .replace("&quot;", "\"")
            .replace("&gt;", ">")
            .replace("&lt;", "<")
            .replace("&amp;", "&"),
    )
}

/// An `.icns` file with PNG entries at 128, 256, 512 and 1024 px, written
/// directly so no `iconutil` run is needed.
pub fn icns_from_png(png: &[u8]) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::new(Cursor::new(png))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    if image.width() != image.height() || image.width() < 128 {
        return None;
    }
    let mut entries = Vec::new();
    for (kind, size) in [
        (b"ic07", 128u32),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
    ] {
        let resized = image.resize_exact(size, size, image::imageops::FilterType::Triangle);
        let mut data = Cursor::new(Vec::new());
        resized.write_to(&mut data, image::ImageFormat::Png).ok()?;
        let data = data.into_inner();
        entries.extend_from_slice(kind);
        entries.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
        entries.extend_from_slice(&data);
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&((entries.len() + 8) as u32).to_be_bytes());
    icns.extend_from_slice(&entries);
    Some(icns)
}

// ---------------------------------------------------------------------------
// Assembly

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppBuild {
    /// `built` or `unchanged`.
    pub status: &'static str,
    pub path: PathBuf,
    pub signing: Signing,
}

fn real_dir(path: &Path, create: bool) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(AppError::UnsafeDestination),
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(path)
                .map_err(|_| AppError::UnsafeDestination)
        }
        Err(_) => Err(AppError::UnsafeDestination),
    }
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), AppError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .map_err(|_| AppError::UnsafeDestination)
}

fn nonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{count}-{}",
        std::process::id(),
        random_hex(4).unwrap_or_default()
    )
}

fn codesign(
    env: &Environment,
    identity: &str,
    identifier: &str,
    path: &Path,
) -> Result<(), AppError> {
    let mut args = os(&[
        "--force",
        "--sign",
        identity,
        "--identifier",
        identifier,
        "--timestamp=none",
    ]);
    if identity != "-" {
        args.push("--keychain".into());
        args.push(env.keychain.clone().into_os_string());
    }
    args.push(path.as_os_str().to_owned());
    let output = env
        .tools
        .run(CODESIGN, &args)
        .map_err(|_| AppError::SigningFailed)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if identity != "-"
        && [
            "canceled",
            "cancelled",
            "denied",
            "user interaction is not allowed",
        ]
        .iter()
        .any(|word| stderr.contains(word))
    {
        Err(AppError::SigningDeclined)
    } else {
        Err(AppError::SigningFailed)
    }
}

fn verify(env: &Environment, path: &Path) -> bool {
    let mut args = os(&["--verify", "--strict"]);
    args.push(path.as_os_str().to_owned());
    env.tools
        .run(CODESIGN, &args)
        .is_ok_and(|output| output.status.success())
}

/// Builds (or confirms) `~/Applications/Hraness/<name>.app`. With local
/// signing the identity must already exist: a missing one is
/// `identity-unavailable`, and the product decides whether to create it
/// (after its notice) or fall back to ad-hoc.
pub fn assemble_app(
    spec: &AppSpec,
    env: &Environment,
    runner_version: &str,
) -> Result<AppBuild, AppError> {
    validate(spec)?;
    let signing = if env.force_ad_hoc {
        Signing::AdHoc
    } else {
        spec.signing
    };
    // 1. Read and verify every input against its pinned digest.
    let inputs: Vec<&Path> = std::iter::once(spec.executable.as_path())
        .chain(spec.helpers.iter().map(|helper| helper.path.as_path()))
        .chain(spec.icon_png.as_deref())
        .collect();
    // 2. Never build from, or clear, a quarantined download.
    if inputs.iter().any(|path| quarantined(path)) {
        return Err(AppError::QuarantinedInput);
    }
    let executable = read_input(&spec.executable, MAX_INPUT_BYTES)?;
    if sha256_hex(&executable) != spec.executable_sha256 {
        return Err(AppError::InvalidAppRequest);
    }
    let mut helpers = Vec::new();
    for helper in &spec.helpers {
        let bytes = read_input(&helper.path, MAX_INPUT_BYTES)?;
        if sha256_hex(&bytes) != helper.sha256 {
            return Err(AppError::InvalidAppRequest);
        }
        helpers.push((helper, bytes));
    }
    let icon_png = spec
        .icon_png
        .as_deref()
        .map(|path| read_input(path, MAX_ICON_BYTES))
        .transpose()?;
    let digest = inputs_digest(spec, icon_png.as_deref(), signing, runner_version);
    let plist = info_plist(spec, runner_version, &digest, icon_png.is_some());

    real_dir(&env.home.join("Applications"), true)?;
    let apps = env.apps_dir();
    real_dir(&apps, true)?;
    let destination = env.app_path(&spec.name);
    if let Ok(meta) = fs::symlink_metadata(&destination) {
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(AppError::UnsafeDestination);
        }
        let installed =
            fs::read_to_string(destination.join("Contents/Info.plist")).unwrap_or_default();
        if plist_string(&installed, "HranessInputsSha256").as_deref() == Some(digest.as_str())
            && verify(env, &destination)
        {
            return Ok(AppBuild {
                status: "unchanged",
                path: destination,
                signing,
            });
        }
    }
    // Resizing is the slow step, so it waits until a build is needed.
    let icns = match &icon_png {
        Some(png) => Some(icns_from_png(png).ok_or(AppError::InvalidAppRequest)?),
        None => None,
    };
    let identity = match signing {
        Signing::AdHoc => "-".to_owned(),
        Signing::Local => match signing_identity_status(env) {
            IdentityState::Ready { sha1 } => sha1,
            _ => return Err(AppError::IdentityUnavailable),
        },
    };

    // 3. Stage with owner-only permissions.
    let staging = apps.join(format!(".{}.app.staging-{}", spec.name, nonce()));
    let result = stage_and_sign(
        spec,
        env,
        &staging,
        &identity,
        &executable,
        &helpers,
        icns.as_deref(),
        &plist,
    );
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    // 6. Swap in with renames; a running copy keeps its old files.
    let old = apps.join(format!(".{}.app.old-{}", spec.name, nonce()));
    let had_old = destination.exists();
    if had_old && fs::rename(&destination, &old).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return Err(AppError::UnsafeDestination);
    }
    if fs::rename(&staging, &destination).is_err() {
        if had_old {
            let _ = fs::rename(&old, &destination);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(AppError::UnsafeDestination);
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(AppBuild {
        status: "built",
        path: destination,
        signing,
    })
}

#[allow(clippy::too_many_arguments)]
fn stage_and_sign(
    spec: &AppSpec,
    env: &Environment,
    staging: &Path,
    identity: &str,
    executable: &[u8],
    helpers: &[(&HelperSpec, Vec<u8>)],
    icns: Option<&[u8]>,
    plist: &str,
) -> Result<(), AppError> {
    real_dir(staging, true)?;
    let contents = staging.join("Contents");
    for dir in ["", "MacOS", "Resources"] {
        real_dir(&contents.join(dir), true)?;
    }
    write_file(&contents.join("Info.plist"), plist.as_bytes(), 0o600)?;
    write_file(&contents.join("PkgInfo"), b"APPL????", 0o600)?;
    let main = contents.join("MacOS").join(&spec.name);
    write_file(&main, executable, 0o700)?;
    if let Some(icns) = icns {
        write_file(&contents.join("Resources/AppIcon.icns"), icns, 0o600)?;
    }
    // 4. Helpers first, then the bundle.
    if !helpers.is_empty() {
        real_dir(&contents.join("Helpers"), true)?;
    }
    for (helper, bytes) in helpers {
        let path = contents.join("Helpers").join(&helper.name);
        write_file(&path, bytes, 0o700)?;
        codesign(
            env,
            identity,
            &format!("{}.{}", spec.bundle_id(), helper.name),
            &path,
        )?;
    }
    // The copy is re-read so a swapped file in the staging folder is caught.
    if sha256_hex(&read_input(&main, MAX_INPUT_BYTES)?) != spec.executable_sha256 {
        return Err(AppError::UnsafeDestination);
    }
    codesign(env, identity, &spec.bundle_id(), staging)?;
    // 5. Verify the signed result.
    if !verify(env, staging) {
        return Err(AppError::SigningFailed);
    }
    Ok(())
}

/// Reads which identity signed an installed app, for `doctor`.
pub fn signed_by(env: &Environment, app: &Path) -> Option<Signing> {
    let mut args = os(&["-dvv"]);
    args.push(app.as_os_str().to_owned());
    let output = env
        .tools
        .run(CODESIGN, &args)
        .ok()
        .filter(|output| output.status.success())?;
    // codesign -d writes its report to stderr.
    let report = String::from_utf8_lossy(&output.stderr);
    if report
        .lines()
        .any(|line| line.trim() == format!("Authority={IDENTITY_NAME}"))
    {
        Some(Signing::Local)
    } else if report.lines().any(|line| line.trim() == "Signature=adhoc") {
        Some(Signing::AdHoc)
    } else {
        None
    }
}

/// The `doctor` line for an app's signature, with an ASCII fallback.
pub fn doctor_line(name: &str, command: &str, signing: Option<Signing>, ascii: bool) -> String {
    let (ok, warn, next) = if ascii {
        ("OK", "WARN", "->")
    } else {
        ("✓", "⚠", "→")
    };
    match signing {
        Some(Signing::Local) => format!("{ok} {name}.app is signed by {IDENTITY_NAME}\n"),
        Some(Signing::AdHoc) => format!(
            "{warn} {name}.app is ad-hoc signed, so macOS asks for its permissions again after each update.\n{next} {command} menubar install\n"
        ),
        None => format!("{warn} {name}.app isn't set up yet.\n{next} {command} menubar install\n"),
    }
}

// ---------------------------------------------------------------------------
// Launching through the bundle

/// The login item that starts a product through its app, so Login Items
/// and privacy prompts show the product's name: `<Product>.app/Contents/
/// MacOS/<Product> --launch <argv file>`.
pub fn login_item(
    env: &Environment,
    app_id: &str,
    name: &str,
    command: &str,
    argv_file: &Path,
) -> LoginItem {
    LoginItem {
        app_id: app_id.to_owned(),
        name: name.to_owned(),
        command: command.to_owned(),
        program: env.app_path(name).join("Contents/MacOS").join(name),
        args: vec![
            "--launch".to_owned(),
            argv_file.to_string_lossy().into_owned(),
        ],
        bundle_id: Some(format!("app.hraness.{app_id}")),
    }
}

/// Writes the owner-only argv file `--launch` reads, atomically.
pub fn write_argv_file(path: &Path, argv: &[String]) -> Result<(), AppError> {
    if argv.is_empty()
        || argv.len() > 64
        || !Path::new(&argv[0]).is_absolute()
        || argv.iter().any(|arg| arg.contains('\0'))
    {
        return Err(AppError::InvalidAppRequest);
    }
    let parent = path.parent().ok_or(AppError::UnsafeDestination)?;
    real_dir(parent, false)?;
    let json = serde_json::to_vec(argv).map_err(|_| AppError::InvalidAppRequest)?;
    // `read_argv_file` ignores a bigger file, and the app would start nothing.
    if json.len() as u64 > MAX_ARGV_FILE_BYTES {
        return Err(AppError::InvalidAppRequest);
    }
    let temp = parent.join(format!(".launch-argv-{}.tmp", nonce()));
    write_file(&temp, &json, 0o600)?;
    fs::rename(&temp, path).map_err(|_| {
        let _ = fs::remove_file(&temp);
        AppError::UnsafeDestination
    })
}

/// Reads an argv file: owner-only, owned by this user, a JSON array of 1 to
/// 64 strings whose first entry is an absolute path.
pub fn read_argv_file(path: &Path) -> Option<Vec<String>> {
    let meta = fs::symlink_metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return None;
        }
    }
    if !meta.is_file() || meta.len() > MAX_ARGV_FILE_BYTES {
        return None;
    }
    let argv: Vec<String> = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (!argv.is_empty() && argv.len() <= 64 && Path::new(&argv[0]).is_absolute()).then_some(argv)
}

/// The bundle ID of the app this executable runs from, when it runs from a
/// Hraness app.
pub fn own_bundle_id(executable: &Path) -> Option<String> {
    let contents = executable.parent()?.parent()?;
    if contents.file_name()? != "Contents" || executable.parent()?.file_name()? != "MacOS" {
        return None;
    }
    let plist = fs::read_to_string(contents.join("Info.plist")).ok()?;
    plist_string(&plist, "CFBundleIdentifier")
        .filter(|id| id.strip_prefix("app.hraness.").is_some_and(valid_app_id))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[derive(Default)]
    struct FakeTools {
        calls: RefCell<Vec<(String, Vec<String>)>>,
        identity: RefCell<Option<&'static str>>,
        codesign_stderr: Option<&'static str>,
        verify_ok: bool,
    }

    const SHA1: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

    impl Tools for FakeTools {
        fn run(&self, program: &str, args: &[OsString]) -> std::io::Result<Output> {
            let args: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            self.calls
                .borrow_mut()
                .push((program.to_owned(), args.clone()));
            let output = |code: i32, stdout: String, stderr: &str| Output {
                status: ExitStatus::from_raw(code << 8),
                stdout: stdout.into_bytes(),
                stderr: stderr.as_bytes().to_vec(),
            };
            Ok(match (program, args[0].as_str()) {
                (SECURITY, "find-identity") => match *self.identity.borrow() {
                    Some(sha1) => output(0, format!("\nPolicy: Code Signing\n  Matching identities\n  1) {sha1} \"{IDENTITY_NAME}\" (CSSMERR_TP_NOT_TRUSTED)\n     1 identities found\n"), ""),
                    None => output(0, "\nPolicy: Code Signing\n  Matching identities\n     0 identities found\n".into(), ""),
                },
                (SECURITY, "import") => {
                    *self.identity.borrow_mut() = Some(SHA1);
                    output(0, String::new(), "")
                }
                (OPENSSL, _) => output(0, String::new(), ""),
                (CODESIGN, "--verify") => output(if self.verify_ok { 0 } else { 1 }, String::new(), ""),
                (CODESIGN, "--force") => match self.codesign_stderr {
                    Some(stderr) => output(1, String::new(), stderr),
                    None => output(0, String::new(), ""),
                },
                _ => output(1, String::new(), ""),
            })
        }
    }

    fn temp_home() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "foundation-identity-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn env<'a>(home: &Path, tools: &'a FakeTools) -> Environment<'a> {
        Environment {
            home: home.to_owned(),
            keychain: home.join("test.keychain-db"),
            tools,
            force_ad_hoc: false,
        }
    }

    fn png(size: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(size, size, image::Rgba([20, 20, 20, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn spec(home: &Path) -> AppSpec {
        let executable = home.join("runner");
        fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        let helper = home.join("helper");
        fs::write(&helper, b"helper").unwrap();
        let icon = home.join("icon.png");
        fs::write(&icon, png(256)).unwrap();
        AppSpec {
            app_id: "aicharts".into(),
            name: "AI Charts".into(),
            product_version: "1.4.0".into(),
            executable_sha256: sha256_hex(&fs::read(&executable).unwrap()),
            executable,
            icon_png: Some(icon),
            usage: BTreeMap::from([(
                "NSAppleEventsUsageDescription".into(),
                "AI Charts sends the replies you approve through Messages.".into(),
            )]),
            helpers: vec![HelperSpec {
                name: "aicharts-helper".into(),
                sha256: sha256_hex(b"helper"),
                path: helper,
            }],
            signing: Signing::Local,
        }
    }

    #[test]
    fn identity_status_parses_untrusted_listings() {
        let home = temp_home();
        let tools = FakeTools::default();
        assert_eq!(
            signing_identity_status(&env(&home, &tools)),
            IdentityState::Missing
        );
        *tools.identity.borrow_mut() = Some(SHA1);
        assert_eq!(
            signing_identity_status(&env(&home, &tools)),
            IdentityState::Ready { sha1: SHA1.into() }
        );
        let (program, args) = tools.calls.borrow()[0].clone();
        assert_eq!(program, SECURITY);
        assert_eq!(
            args,
            vec![
                "find-identity".to_owned(),
                "-p".into(),
                "codesigning".into(),
                home.join("test.keychain-db").to_string_lossy().into_owned()
            ]
        );
        assert_eq!(parse_identities("  1) ZZZ \"Hraness Local Signing\""), None);
        assert_eq!(
            parse_identities(&format!("  1) {SHA1} \"Someone Else\"")),
            None
        );
        assert_eq!(
            serde_json::to_string(&IdentityState::Ready { sha1: SHA1.into() }).unwrap(),
            format!("{{\"state\":\"ready\",\"sha1\":\"{SHA1}\"}}")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ensure_creates_once_in_the_given_keychain_and_cleans_up() {
        let home = temp_home();
        let tools = FakeTools::default();
        let env = env(&home, &tools);
        let temp = home.join("tmp");
        fs::create_dir(&temp).unwrap();
        assert_eq!(
            ensure_signing_identity(&env, &temp),
            IdentityState::Ready { sha1: SHA1.into() }
        );
        let calls = tools.calls.borrow().clone();
        let programs: Vec<&str> = calls
            .iter()
            .map(|(program, args)| {
                if program == SECURITY {
                    args[0].as_str()
                } else {
                    program.as_str()
                }
            })
            .collect();
        assert_eq!(
            programs,
            vec!["find-identity", OPENSSL, OPENSSL, "import", "find-identity"]
        );
        let import = &calls[3].1;
        let keychain = home.join("test.keychain-db").to_string_lossy().into_owned();
        assert!(import
            .windows(2)
            .any(|pair| pair == ["-k", keychain.as_str()]));
        assert!(import.windows(2).any(|pair| pair == ["-T", CODESIGN]));
        assert!(import.contains(&"-x".to_owned()), "non-extractable");
        assert!(!calls
            .iter()
            .any(|(_, args)| args.iter().any(|arg| arg.contains("add-trusted-cert"))));
        assert_eq!(
            fs::read_dir(&temp).unwrap().count(),
            0,
            "temporary key material removed"
        );
        // Already ready: nothing else runs.
        tools.calls.borrow_mut().clear();
        assert_eq!(
            ensure_signing_identity(&env, &temp),
            IdentityState::Ready { sha1: SHA1.into() }
        );
        assert_eq!(tools.calls.borrow().len(), 1);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn assembles_signs_helpers_first_and_recognizes_unchanged() {
        let home = temp_home();
        let tools = FakeTools {
            verify_ok: true,
            ..Default::default()
        };
        *tools.identity.borrow_mut() = Some(SHA1);
        let env = env(&home, &tools);
        let spec = spec(&home);
        let build = assemble_app(&spec, &env, "0.8.0").unwrap();
        assert_eq!(build.status, "built");
        assert_eq!(build.path, home.join("Applications/Hraness/AI Charts.app"));
        let contents = build.path.join("Contents");
        assert_eq!(
            fs::read(contents.join("MacOS/AI Charts")).unwrap(),
            fs::read(&spec.executable).unwrap()
        );
        assert_eq!(
            fs::read(contents.join("Helpers/aicharts-helper")).unwrap(),
            b"helper"
        );
        let icns = fs::read(contents.join("Resources/AppIcon.icns")).unwrap();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
            icns.len()
        );
        let plist = fs::read_to_string(contents.join("Info.plist")).unwrap();
        assert_eq!(
            plist_string(&plist, "CFBundleIdentifier").as_deref(),
            Some("app.hraness.aicharts")
        );
        assert_eq!(
            plist_string(&plist, "CFBundleExecutable").as_deref(),
            Some("AI Charts")
        );
        assert_eq!(
            plist_string(&plist, "CFBundleVersion").as_deref(),
            Some("1.4.0+df.0.8.0")
        );
        assert_eq!(
            plist_string(&plist, "HranessRunnerSha256"),
            Some(spec.executable_sha256.clone())
        );
        assert!(plist.contains("<key>LSUIElement</key>\n\t<true/>"));
        assert!(plist.contains("<key>NSAppleEventsUsageDescription</key>"));
        let signs: Vec<Vec<String>> = tools
            .calls
            .borrow()
            .iter()
            .filter(|(p, a)| p == CODESIGN && a[0] == "--force")
            .map(|(_, a)| a.clone())
            .collect();
        assert_eq!(signs.len(), 2);
        assert!(signs[0].contains(&"app.hraness.aicharts.aicharts-helper".to_owned()));
        assert!(signs[1].contains(&"app.hraness.aicharts".to_owned()));
        assert!(signs
            .iter()
            .all(|args| args.windows(2).any(|pair| pair == ["--sign", SHA1])
                && args.contains(&"--timestamp=none".to_owned())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&build.path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        // Same inputs: unchanged, nothing signed again.
        tools.calls.borrow_mut().clear();
        assert_eq!(
            assemble_app(&spec, &env, "0.8.0").unwrap().status,
            "unchanged"
        );
        assert!(!tools.calls.borrow().iter().any(|(_, a)| a[0] == "--force"));
        // A new runner version rebuilds in place and leaves no staging behind.
        assert_eq!(assemble_app(&spec, &env, "0.8.1").unwrap().status, "built");
        let leftovers: Vec<_> = fs::read_dir(env.apps_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![OsString::from("AI Charts.app")]);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failures_map_to_codes_and_leave_the_old_app() {
        let home = temp_home();
        let spec = spec(&home);
        // No identity yet.
        let tools = FakeTools {
            verify_ok: true,
            ..Default::default()
        };
        assert_eq!(
            assemble_app(&spec, &env(&home, &tools), "0.8.0"),
            Err(AppError::IdentityUnavailable)
        );
        // Ad-hoc needs no identity; forced by the environment too.
        let mut forced = env(&home, &tools);
        forced.force_ad_hoc = true;
        let build = assemble_app(&spec, &forced, "0.8.0").unwrap();
        assert_eq!(build.signing, Signing::AdHoc);
        assert!(tools
            .calls
            .borrow()
            .iter()
            .any(|(_, a)| a.windows(2).any(|pair| pair == ["--sign", "-"])));
        // Declined and failed signing keep the installed app.
        for (stderr, error) in [
            (
                "errSecAuthFailed: User canceled the operation",
                AppError::SigningDeclined,
            ),
            ("errSecInternalComponent", AppError::SigningFailed),
        ] {
            let tools = FakeTools {
                verify_ok: true,
                codesign_stderr: Some(stderr),
                ..Default::default()
            };
            *tools.identity.borrow_mut() = Some(SHA1);
            assert_eq!(
                assemble_app(&spec, &env(&home, &tools), "0.9.0"),
                Err(error)
            );
            assert!(home
                .join("Applications/Hraness/AI Charts.app/Contents/MacOS/AI Charts")
                .exists());
        }
        // A failed verification is a signing failure.
        let tools = FakeTools {
            verify_ok: false,
            ..Default::default()
        };
        *tools.identity.borrow_mut() = Some(SHA1);
        assert_eq!(
            assemble_app(&spec, &env(&home, &tools), "0.9.0"),
            Err(AppError::SigningFailed)
        );
        // A tampered input is refused.
        let tools = FakeTools {
            verify_ok: true,
            ..Default::default()
        };
        let wrong = AppSpec {
            executable_sha256: "0".repeat(64),
            ..spec.clone()
        };
        assert_eq!(
            assemble_app(&wrong, &forced_env(&home, &tools), "0.8.0"),
            Err(AppError::InvalidAppRequest)
        );
        let leftovers = fs::read_dir(home.join("Applications/Hraness"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 1, "no staging folders left");
        fs::remove_dir_all(home).unwrap();
    }

    fn forced_env<'a>(home: &Path, tools: &'a FakeTools) -> Environment<'a> {
        Environment {
            force_ad_hoc: true,
            ..env(home, tools)
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quarantined_inputs_are_refused_and_never_cleared() {
        use std::os::unix::ffi::OsStrExt;
        let home = temp_home();
        let spec = spec(&home);
        let path = std::ffi::CString::new(spec.executable.as_os_str().as_bytes()).unwrap();
        let value = b"0081;00000000;Test;";
        let set = unsafe {
            libc::setxattr(
                path.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(set, 0);
        let tools = FakeTools {
            verify_ok: true,
            ..Default::default()
        };
        assert_eq!(
            assemble_app(&spec, &forced_env(&home, &tools), "0.8.0"),
            Err(AppError::QuarantinedInput)
        );
        assert!(quarantined(&spec.executable), "attribute left in place");
        assert!(tools.calls.borrow().is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn requests_are_validated() {
        let home = temp_home();
        let good = spec(&home);
        let cases: Vec<Box<dyn Fn(&mut AppSpec)>> = vec![
            Box::new(|s| s.name = "../Evil".into()),
            Box::new(|s| s.name = ".Hidden".into()),
            Box::new(|s| s.name = "A/B".into()),
            Box::new(|s| s.app_id = "AI".into()),
            Box::new(|s| s.product_version = "1 0".into()),
            Box::new(|s| {
                s.usage
                    .insert("NSCalendarsUsageDescription".into(), "x".into());
            }),
            Box::new(|s| {
                s.usage
                    .insert("NSCameraUsageDescription".into(), "x".repeat(111));
            }),
            Box::new(|s| s.helpers[0].name = "../helper".into()),
            Box::new(|s| s.helpers.push(s.helpers[0].clone())),
            Box::new(|s| s.executable = PathBuf::from("runner")),
        ];
        for mutate in cases {
            let mut bad = good.clone();
            mutate(&mut bad);
            assert_eq!(validate(&bad), Err(AppError::InvalidAppRequest), "{bad:?}");
        }
        assert_eq!(validate(&good), Ok(()));
        assert!(icns_from_png(&png(64)).is_none(), "too small");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn launch_files_and_login_items_point_at_the_app() {
        let home = temp_home();
        let tools = FakeTools::default();
        let env = env(&home, &tools);
        let argv = home.join("launch.json");
        write_argv_file(&argv, &["/usr/bin/true".into(), "menubar".into()]).unwrap();
        assert_eq!(
            read_argv_file(&argv),
            Some(vec!["/usr/bin/true".into(), "menubar".into()])
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&argv, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                read_argv_file(&argv),
                None,
                "group-readable files are refused"
            );
        }
        assert_eq!(
            write_argv_file(&argv, &["relative".into()]),
            Err(AppError::InvalidAppRequest)
        );
        // Never write a file `read_argv_file` would ignore at login.
        let mut big = vec!["/usr/bin/true".to_owned()];
        big.extend(std::iter::repeat("a".repeat(8000)).take(10));
        assert_eq!(
            write_argv_file(&argv, &big),
            Err(AppError::InvalidAppRequest)
        );
        big.truncate(8);
        write_argv_file(&argv, &big).unwrap();
        assert_eq!(read_argv_file(&argv), Some(big));
        let item = login_item(&env, "aicharts", "AI Charts", "aicharts", &argv);
        assert_eq!(
            item.program,
            home.join("Applications/Hraness/AI Charts.app/Contents/MacOS/AI Charts")
        );
        assert_eq!(item.args[0], "--launch");
        let plan = crate::service::plan(&item, &home).unwrap();
        assert!(plan
            .contents
            .contains("<string>app.hraness.aicharts</string></array>"));
        // own_bundle_id only trusts a Hraness Info.plist next to MacOS/.
        let macos = home.join("X.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        let spec = spec(&home);
        fs::write(
            home.join("X.app/Contents/Info.plist"),
            info_plist(&spec, "0.8.0", "x", false),
        )
        .unwrap();
        assert_eq!(
            own_bundle_id(&macos.join("X")),
            Some("app.hraness.aicharts".into())
        );
        assert_eq!(own_bundle_id(&home.join("runner")), None);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn doctor_copy() {
        assert_eq!(
            doctor_line("Textbutler", "textbutler", Some(Signing::Local), false),
            "✓ Textbutler.app is signed by Hraness Local Signing\n"
        );
        assert_eq!(
            doctor_line("Textbutler", "textbutler", Some(Signing::AdHoc), false),
            "⚠ Textbutler.app is ad-hoc signed, so macOS asks for its permissions again after each update.\n→ textbutler menubar install\n"
        );
        assert_eq!(
            doctor_line("Textbutler", "textbutler", None, true),
            "WARN Textbutler.app isn't set up yet.\n-> textbutler menubar install\n"
        );
    }
}
