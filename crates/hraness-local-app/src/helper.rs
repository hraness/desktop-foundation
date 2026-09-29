//! The one-shot modes shared by `hraness-helper` and its `hraness-companion`
//! alias.
//! Argv, stdout and exit status are frozen by
//! `contract/helper-argv.v0.8.1.json`.

use std::ffi::{OsStr, OsString};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::wire::{ProtocolError, VERSION};
use crate::{identity, launch, notice, prompt};

/// Who is running the mode: the name `--version` prints and the version
/// recorded in an assembled app.
#[derive(Debug, Clone, Copy)]
pub struct Binary<'a> {
    pub name: &'a str,
    pub version: &'a str,
    /// Runner protocol versions, such as `"1,2"`.
    pub protocols: &'a str,
}

/// Runs the helper mode `args` names. `None` means the argv is not a helper
/// mode, and the caller answers `invalid-arguments` (the companion alias
/// first refuses the removed tray modes; see [`is_tray_mode`]).
pub fn dispatch(args: &[OsString], binary: Binary<'_>) -> Option<Result<(), ProtocolError>> {
    let one = |flag: &str| args.len() == 1 && args[0] == flag;
    if one("--version") {
        println!(
            "{} {} protocol/{}",
            binary.name, binary.version, binary.protocols
        );
        return Some(Ok(()));
    }
    if one("--prompt-probe") {
        return Some(prompt::probe().emit(&mut std::io::stdout()));
    }
    if one("--assemble-app") {
        return Some(assemble_app(binary.version));
    }
    if args.len() == 2 && args[0] == "--signing-identity" {
        return Some(signing_identity(&args[1]));
    }
    if args.len() == 2 && args[0] == "--launch" && Path::new(&args[1]).is_absolute() {
        return Some(launch::launch(Path::new(&args[1])));
    }
    if one("--notice") {
        return Some(
            notice::read_spec(&mut BufReader::new(std::io::stdin()))
                .and_then(|spec| notice::emit_result(&mut std::io::stdout(), notice::run(&spec))),
        );
    }
    if one("--prompt") {
        return Some(
            prompt::read_spec(&mut BufReader::new(std::io::stdin()))
                .and_then(|spec| prompt::emit_result(&mut std::io::stdout(), &prompt::run(&spec))),
        );
    }
    None
}

/// The exit status `hraness-companion` uses when asked for the menu bar,
/// which was removed in 1.0. It matches the control contract's usage code.
pub const TRAY_REMOVED_EXIT: i32 = 2;
/// The error code `hraness-companion` prints for a removed tray mode.
pub const TRAY_REMOVED_CODE: &str = "tray-removed";
/// What `hraness-companion` prints on stderr for a removed tray mode.
pub const TRAY_REMOVED_MESSAGE: &str =
    "hraness-companion: the menu bar was removed in desktop-foundation 1.0. \
Run `<product> tui` to watch a product, or `<product> status --json` from a script. \
See docs/migration-1.0.md.";

/// True for the argv refused as removed tray modes, frozen in
/// `contract/companion-alias.v1.json`: no arguments, and any argv starting
/// `--state-dir` or `--check-protocol` (the 0.x menu bar and its wire
/// self-check) or `--foreground` (the flag product CLIs passed to their
/// menu bar command). 0.x answered malformed variants of these with
/// `invalid-arguments`; 1.0 refuses them all with the clearer error. Every
/// other argv is either a helper mode or `invalid-arguments`, exactly as in
/// `hraness-helper`.
pub fn is_tray_mode(args: &[OsString]) -> bool {
    match args.first() {
        None => true,
        Some(first) => TRAY_FLAGS.iter().any(|flag| first == flag),
    }
}

const TRAY_FLAGS: [&str; 3] = ["--state-dir", "--check-protocol", "--foreground"];

/// Refuses a removed tray mode: one `tray-removed` error frame on stdout,
/// the pointer to the terminal views on stderr, exit
/// [`TRAY_REMOVED_EXIT`]. Nothing is drawn and nothing is read from stdin.
pub fn refuse_tray_mode() -> ! {
    let _ = crate::wire::write_error(&mut std::io::stdout(), VERSION, TRAY_REMOVED_CODE);
    eprintln!("{TRAY_REMOVED_MESSAGE}");
    std::process::exit(TRAY_REMOVED_EXIT);
}

/// Prints `{"type":"error",…}` for a failed mode and exits 1, like the
/// v0.8.1 companion.
pub fn exit_with(result: Result<(), ProtocolError>) -> ! {
    match result {
        Ok(()) => std::process::exit(0),
        Err(ProtocolError(code)) => {
            let _ = crate::wire::write_error(&mut std::io::stdout(), VERSION, code);
            std::process::exit(1);
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AppHelperWire {
    name: String,
    path: PathBuf,
    sha256: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AppRequestWire {
    #[serde(rename = "type")]
    kind: String,
    version: u8,
    app_id: String,
    name: String,
    product_version: String,
    icon_path: Option<PathBuf>,
    #[serde(default)]
    usage: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    helpers: Vec<AppHelperWire>,
    #[serde(default)]
    signing: identity::Signing,
}

fn home_dir() -> Result<PathBuf, ProtocolError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or(ProtocolError("unsafe-destination"))
}

/// `--assemble-app`: builds `~/Applications/Hraness/<Product>.app` around
/// this runner. One `app-request` line in, one `app-result` line out.
fn assemble_app(runner_version: &str) -> Result<(), ProtocolError> {
    let frame =
        prompt::read_single_frame(&mut BufReader::new(std::io::stdin()), "invalid-app-request")?;
    let wire: AppRequestWire =
        serde_json::from_slice(&frame).map_err(|_| ProtocolError("invalid-app-request"))?;
    if wire.kind != "app-request" {
        return Err(ProtocolError("invalid-app-request"));
    }
    if wire.version != VERSION {
        return Err(ProtocolError("unsupported-version"));
    }
    let executable = std::env::current_exe().map_err(|_| ProtocolError("invalid-app-request"))?;
    let bytes = std::fs::read(&executable).map_err(|_| ProtocolError("invalid-app-request"))?;
    let digest: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let spec = identity::AppSpec {
        app_id: wire.app_id,
        name: wire.name,
        product_version: wire.product_version,
        executable,
        executable_sha256: digest,
        icon_png: wire.icon_path,
        usage: wire.usage,
        helpers: wire
            .helpers
            .into_iter()
            .map(|helper| identity::HelperSpec {
                name: helper.name,
                path: helper.path,
                sha256: helper.sha256,
            })
            .collect(),
        signing: wire.signing,
    };
    let tools = identity::SystemTools;
    let env = identity::Environment::for_user(home_dir()?, &tools);
    #[derive(serde::Serialize)]
    struct AppResult {
        #[serde(rename = "type")]
        kind: &'static str,
        version: u8,
        status: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<PathBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signing: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
    }
    let result = match identity::assemble_app(&spec, &env, runner_version) {
        Ok(build) => AppResult {
            kind: "app-result",
            version: VERSION,
            status: build.status,
            path: Some(build.path),
            signing: Some(build.signing.wire()),
            code: None,
        },
        Err(error) => AppResult {
            kind: "app-result",
            version: VERSION,
            status: "failed",
            path: None,
            signing: None,
            code: Some(error.code()),
        },
    };
    let result = serde_json::to_string(&result).map_err(|_| ProtocolError("output-unavailable"))?;
    println!("{result}");
    Ok(())
}

/// `--signing-identity status|ensure`: one `signing-identity` line.
fn signing_identity(action: &OsStr) -> Result<(), ProtocolError> {
    let tools = identity::SystemTools;
    let env = identity::Environment::for_user(home_dir()?, &tools);
    let state = if action == "status" {
        identity::signing_identity_status(&env)
    } else if action == "ensure" {
        identity::ensure_signing_identity(&env, &std::env::temp_dir())
    } else {
        return Err(ProtocolError("invalid-arguments"));
    };
    let (state, sha1) = match state {
        identity::IdentityState::Ready { sha1 } => ("ready", Some(sha1)),
        identity::IdentityState::Missing => ("missing", None),
        identity::IdentityState::Unavailable => ("unavailable", None),
    };
    let mut line =
        format!("{{\"type\":\"signing-identity\",\"version\":{VERSION},\"state\":\"{state}\"");
    if let Some(sha1) = sha1 {
        line.push_str(&format!(",\"sha1\":\"{sha1}\""));
    }
    line.push('}');
    println!("{line}");
    Ok(())
}
