//! The JSON envelope, error codes and exit codes shared by every Hraness
//! CLI (`contract/envelope.schema.json`, `contract/error-codes.json`).

use std::fmt;
use std::io::Write;
use std::time::SystemTime;

use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

/// Exit status for success.
pub const EXIT_OK: u8 = 0;
/// Exit status for a failure that has no more specific status.
pub const EXIT_FAILURE: u8 = 1;
/// Exit status for a malformed command line.
pub const EXIT_USAGE: u8 = 2;
/// Exit status when a decide verb ran without a satisfied human gate.
pub const EXIT_HUMAN_REQUIRED: u8 = 3;
/// Exit status when the product's owner process is unavailable.
pub const EXIT_OWNER_UNAVAILABLE: u8 = 4;
/// Exit status for a stale revision, digest, or a second owner.
pub const EXIT_CONFLICT: u8 = 5;

/// The schema every error envelope carries.
pub const ERROR_SCHEMA: &str = "hraness.error/1";

/// A stable error code. Products add codes only as
/// [`ErrorCode::Product`], under their own `<product>.` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Usage,
    NotFound,
    PermissionDenied,
    HumanRequired,
    GateFailed,
    GateExpired,
    OwnerUnavailable,
    ControlAlreadyRunning,
    Conflict,
    DigestMismatch,
    UnsupportedPlatform,
    Internal,
    /// `<product>.<code>`, such as `ghostget.policy-locked`.
    Product(String),
}

impl ErrorCode {
    /// Every shared code, in `contract/error-codes.json` order.
    pub const SHARED: [ErrorCode; 12] = [
        ErrorCode::Usage,
        ErrorCode::NotFound,
        ErrorCode::PermissionDenied,
        ErrorCode::HumanRequired,
        ErrorCode::GateFailed,
        ErrorCode::GateExpired,
        ErrorCode::OwnerUnavailable,
        ErrorCode::ControlAlreadyRunning,
        ErrorCode::Conflict,
        ErrorCode::DigestMismatch,
        ErrorCode::UnsupportedPlatform,
        ErrorCode::Internal,
    ];

    pub fn as_str(&self) -> &str {
        match self {
            ErrorCode::Usage => "usage",
            ErrorCode::NotFound => "not-found",
            ErrorCode::PermissionDenied => "permission-denied",
            ErrorCode::HumanRequired => "human-required",
            ErrorCode::GateFailed => "gate-failed",
            ErrorCode::GateExpired => "gate-expired",
            ErrorCode::OwnerUnavailable => "owner-unavailable",
            ErrorCode::ControlAlreadyRunning => "control-already-running",
            ErrorCode::Conflict => "conflict",
            ErrorCode::DigestMismatch => "digest-mismatch",
            ErrorCode::UnsupportedPlatform => "unsupported-platform",
            ErrorCode::Internal => "internal",
            ErrorCode::Product(code) => code,
        }
    }

    /// Parses a shared code, or a product code under `product`'s prefix.
    /// Any other text is refused, so an unprefixed unknown code can never
    /// reach the wire.
    pub fn parse(code: &str, product: Option<&str>) -> Option<ErrorCode> {
        if let Some(shared) = Self::SHARED.iter().find(|c| c.as_str() == code) {
            return Some(shared.clone());
        }
        let product = product?;
        let rest = code.strip_prefix(product)?.strip_prefix('.')?;
        if valid_product_name(product) && valid_code_tail(rest) {
            Some(ErrorCode::Product(code.to_string()))
        } else {
            None
        }
    }

    /// The process exit status for this code.
    pub fn exit_code(&self) -> u8 {
        match self {
            ErrorCode::Usage => EXIT_USAGE,
            ErrorCode::HumanRequired | ErrorCode::GateFailed | ErrorCode::GateExpired => {
                EXIT_HUMAN_REQUIRED
            }
            ErrorCode::OwnerUnavailable => EXIT_OWNER_UNAVAILABLE,
            ErrorCode::ControlAlreadyRunning | ErrorCode::Conflict | ErrorCode::DigestMismatch => {
                EXIT_CONFLICT
            }
            _ => EXIT_FAILURE,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if let Some(code) = ErrorCode::parse(&text, None) {
            return Ok(code);
        }
        match text.split_once('.') {
            Some((product, _)) => ErrorCode::parse(&text, Some(product)),
            None => None,
        }
        .ok_or_else(|| serde::de::Error::custom("unknown error code"))
    }
}

/// A product name: `[a-z][a-z0-9-]{0,31}`.
pub fn valid_product_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn valid_code_tail(tail: &str) -> bool {
    let bytes = tail.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'.')
}

/// Who a next step is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    Agent,
    Human,
}

/// A command the reader can run next, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextStep {
    pub command: String,
    pub why: String,
    pub audience: Audience,
}

impl NextStep {
    pub fn new(command: impl Into<String>, why: impl Into<String>, audience: Audience) -> Self {
        Self {
            command: command.into(),
            why: why.into(),
            audience,
        }
    }
}

/// The `error` member of a failed envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<NextStep>,
    /// Added in 1.1.0: the macOS permission behind the failure, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Boxed so `ErrorBody` stays small enough to return in a `Result`.
    pub permission: Option<Box<ErrorPermission>>,
}

/// The `error.permission` member: a permission kind such as
/// `full-disk-access`, and the System Settings pane that fixes it when the
/// kind has one. Added in 1.1.0. A 1.0 reader rejects an envelope that
/// carries it, so set it only toward 1.1 readers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ErrorPermission {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_url: Option<String>,
}

impl ErrorPermission {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            settings_url: None,
        }
    }

    /// Sets the System Settings link. Anything outside
    /// `x-apple.systempreferences:` is dropped, as the schema requires, so an
    /// envelope never carries a link a client should not open. From
    /// `hraness-cli-kit`, pass `PermissionErrorInfo::settings_url`, which is
    /// always one of the known panes.
    pub fn with_settings_url(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.settings_url = url.starts_with("x-apple.systempreferences:").then_some(url);
        self
    }
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            next: Vec::new(),
            permission: None,
        }
    }

    pub fn with_permission(mut self, permission: ErrorPermission) -> Self {
        self.permission = Some(Box::new(permission));
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_next(mut self, next: NextStep) -> Self {
        self.next.push(next);
        self
    }
}

impl fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorBody {}

/// `{ok:true, schema, generatedAt, data, next?}` or
/// `{ok:false, schema:"hraness.error/1", generatedAt, error}`.
#[derive(Debug, Clone, PartialEq)]
pub enum Envelope<T> {
    Ok {
        schema: String,
        generated_at: String,
        data: T,
        next: Vec<NextStep>,
    },
    Err {
        generated_at: String,
        error: ErrorBody,
    },
}

impl<T> Envelope<T> {
    /// A success envelope stamped now. `schema` is `<product>.<noun>/<n>`.
    pub fn ok(schema: impl Into<String>, data: T) -> Self {
        Envelope::Ok {
            schema: schema.into(),
            generated_at: crate::time::iso8601(SystemTime::now()),
            data,
            next: Vec::new(),
        }
    }

    /// A failure envelope stamped now.
    pub fn error(error: ErrorBody) -> Self {
        Envelope::Err {
            generated_at: crate::time::iso8601(SystemTime::now()),
            error,
        }
    }

    /// Adds a next step to a success envelope or to its error.
    pub fn with_next(mut self, step: NextStep) -> Self {
        match &mut self {
            Envelope::Ok { next, .. } => next.push(step),
            Envelope::Err { error, .. } => error.next.push(step),
        }
        self
    }

    /// Replaces the timestamp, for goldens.
    pub fn at(mut self, at: SystemTime) -> Self {
        let stamp = crate::time::iso8601(at);
        match &mut self {
            Envelope::Ok { generated_at, .. } | Envelope::Err { generated_at, .. } => {
                *generated_at = stamp
            }
        }
        self
    }

    /// The exit status this envelope maps to.
    pub fn exit_code(&self) -> u8 {
        match self {
            Envelope::Ok { .. } => EXIT_OK,
            Envelope::Err { error, .. } => error.code.exit_code(),
        }
    }
}

impl<T: Serialize> Serialize for Envelope<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Envelope::Ok {
                schema,
                generated_at,
                data,
                next,
            } => {
                let mut s = serializer.serialize_struct("Envelope", 5)?;
                s.serialize_field("ok", &true)?;
                s.serialize_field("schema", schema)?;
                s.serialize_field("generatedAt", generated_at)?;
                s.serialize_field("data", data)?;
                if !next.is_empty() {
                    s.serialize_field("next", next)?;
                }
                s.end()
            }
            Envelope::Err {
                generated_at,
                error,
            } => {
                let mut s = serializer.serialize_struct("Envelope", 4)?;
                s.serialize_field("ok", &false)?;
                s.serialize_field("schema", ERROR_SCHEMA)?;
                s.serialize_field("generatedAt", generated_at)?;
                s.serialize_field("error", error)?;
                s.end()
            }
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Envelope<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Wire<T> {
            ok: bool,
            schema: String,
            generated_at: String,
            data: Option<T>,
            #[serde(default)]
            next: Vec<NextStep>,
            error: Option<ErrorBody>,
        }
        use serde::de::Error;
        let wire = Wire::<T>::deserialize(deserializer)?;
        match (wire.ok, wire.data, wire.error) {
            (true, Some(data), None) => Ok(Envelope::Ok {
                schema: wire.schema,
                generated_at: wire.generated_at,
                data,
                next: wire.next,
            }),
            (false, None, Some(error)) if wire.schema == ERROR_SCHEMA && wire.next.is_empty() => {
                Ok(Envelope::Err {
                    generated_at: wire.generated_at,
                    error,
                })
            }
            _ => Err(D::Error::custom("not a hraness envelope")),
        }
    }
}

/// Writes the envelope as one JSON line and returns its exit status. A
/// write failure (such as a closed pipe) returns [`EXIT_FAILURE`].
pub fn emit<T: Serialize>(writer: &mut impl Write, envelope: &Envelope<T>) -> u8 {
    let written = serde_json::to_writer(&mut *writer, envelope)
        .map_err(std::io::Error::from)
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush());
    match written {
        Ok(()) => envelope.exit_code(),
        Err(_) => EXIT_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_codes_and_exits_match_the_contract() {
        let contract: serde_json::Value =
            serde_json::from_str(crate::contract::ERROR_CODES).unwrap();
        let listed = contract["codes"].as_array().unwrap();
        assert_eq!(listed.len(), ErrorCode::SHARED.len());
        for (entry, code) in listed.iter().zip(ErrorCode::SHARED.iter()) {
            assert_eq!(entry["code"], code.as_str());
            assert_eq!(entry["exit"], code.exit_code() as u64, "{code}");
        }
    }

    #[test]
    fn product_codes_need_their_own_prefix() {
        assert_eq!(
            ErrorCode::parse("ghostget.policy-locked", Some("ghostget")),
            Some(ErrorCode::Product("ghostget.policy-locked".into()))
        );
        assert_eq!(
            ErrorCode::parse("ghostget.policy-locked", Some("sponge")),
            None
        );
        assert_eq!(ErrorCode::parse("policy-locked", Some("ghostget")), None);
        assert_eq!(ErrorCode::parse("ghostget.", Some("ghostget")), None);
        assert_eq!(ErrorCode::parse("Ghostget.x", Some("Ghostget")), None);
        assert_eq!(ErrorCode::parse("usage", None), Some(ErrorCode::Usage));
    }

    #[test]
    fn envelopes_serialize_in_contract_order() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_790_553_600_000);
        let ok = Envelope::ok("example.status/1", serde_json::json!({"n": 1}))
            .with_next(NextStep::new("example tui", "See it", Audience::Human))
            .at(at);
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"ok":true,"schema":"example.status/1","generatedAt":"2026-09-28T00:00:00.000Z","data":{"n":1},"next":[{"command":"example tui","why":"See it","audience":"human"}]}"#
        );
        let err: Envelope<()> =
            Envelope::error(ErrorBody::new(ErrorCode::HumanRequired, "Needs a person.")).at(at);
        assert_eq!(err.exit_code(), EXIT_HUMAN_REQUIRED);
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"ok":false,"schema":"hraness.error/1","generatedAt":"2026-09-28T00:00:00.000Z","error":{"code":"human-required","message":"Needs a person."}}"#
        );
    }

    #[test]
    fn envelope_goldens_round_trip() {
        let goldens = [
            include_str!("../../../contract/golden/envelope-ok.json"),
            include_str!("../../../contract/golden/envelope-error.json"),
            include_str!("../../../contract/golden/envelope-human-required.json"),
            include_str!("../../../contract/golden/envelope-product-code.json"),
            include_str!("../../../contract/golden/envelope-permission.json"),
            include_str!("../../../contract/golden/commands.json"),
        ];
        for text in goldens {
            let env: Envelope<serde_json::Value> = serde_json::from_str(text).unwrap();
            let want: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(serde_json::to_value(&env).unwrap(), want);
        }
        let bad = r#"{"ok":false,"schema":"x.y/1","generatedAt":"2026-09-28T00:00:00.000Z","error":{"code":"usage","message":"m"}}"#;
        assert!(serde_json::from_str::<Envelope<()>>(bad).is_err());
        let unknown = r#"{"ok":false,"schema":"hraness.error/1","generatedAt":"2026-09-28T00:00:00.000Z","error":{"code":"nope","message":"m"}}"#;
        assert!(serde_json::from_str::<Envelope<()>>(unknown).is_err());
    }

    #[test]
    fn permission_is_optional_and_additive() {
        let env: Envelope<()> = Envelope::error(
            ErrorBody::new(ErrorCode::PermissionDenied, "No access.").with_permission(
                ErrorPermission::new("full-disk-access").with_settings_url(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
                ),
            ),
        );
        let text = serde_json::to_string(&env).unwrap();
        assert!(text.contains(r#""permission":{"kind":"full-disk-access","settingsUrl":"x-apple.systempreferences:"#), "{text}");
        assert_eq!(serde_json::from_str::<Envelope<()>>(&text).unwrap(), env);
        // No pane: settingsUrl is left out, and a null reads as none.
        let keychain = ErrorBody::new(ErrorCode::PermissionDenied, "No access.")
            .with_permission(ErrorPermission::new("keychain"));
        assert_eq!(
            serde_json::to_string(&keychain).unwrap(),
            r#"{"code":"permission-denied","message":"No access.","permission":{"kind":"keychain"}}"#
        );
        let null: ErrorBody = serde_json::from_str(
            r#"{"code":"permission-denied","message":"m","permission":{"kind":"keychain","settingsUrl":null}}"#,
        )
        .unwrap();
        assert_eq!(null.permission.unwrap().settings_url, None);
        // A link outside System Settings is dropped.
        assert_eq!(
            ErrorPermission::new("keychain")
                .with_settings_url("https://example.com/")
                .settings_url,
            None
        );
        // Without it, the 1.0.0 bytes are unchanged.
        assert!(
            !serde_json::to_string(&ErrorBody::new(ErrorCode::Usage, "m"))
                .unwrap()
                .contains("permission")
        );
    }

    #[test]
    fn emit_returns_the_exit_status() {
        let mut out = Vec::new();
        let env: Envelope<()> = Envelope::error(ErrorBody::new(ErrorCode::Conflict, "Stale."));
        assert_eq!(emit(&mut out, &env), EXIT_CONFLICT);
        assert!(out.ends_with(b"\n"));
    }
}
