//! The human gate for `decide` verbs.
//!
//! - **T0** [`detect_agent`]: advisory signs that an agent is running the
//!   command. It picks wording and next steps. It never satisfies or
//!   bypasses a gate.
//! - **T1**: the command has a controlling terminal and is in its
//!   foreground process group.
//! - **T2**: a one-time code is written to `/dev/tty` and must be typed
//!   back at `/dev/tty` before it expires. Piped stdin cannot answer it.
//! - **T3**: reserved for OS owner authentication. It answers
//!   `unsupported-platform` in this release.
//!
//! T1+T2 stops an agent from approving by accident or by piping text. It
//! does not stop a determined process running as the same user, which can
//! drive a pseudo-terminal. See `docs/human-gate.md`.
//!
//! [`ChallengeIssuer`] binds a gate result to one verb and one digest for
//! an owner process: single use, HMAC-bound and expiring.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::envelope::{ErrorBody, ErrorCode};
use crate::registry::GateTier;

/// How long a person has to type the code.
pub const DEFAULT_CODE_TTL: Duration = Duration::from_secs(120);
/// How long an owner challenge stays redeemable.
pub const DEFAULT_CHALLENGE_TTL: Duration = Duration::from_secs(300);

/// The result of [`detect_agent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentMarkers {
    pub agent: bool,
    pub markers: Vec<String>,
}

#[derive(Deserialize)]
struct MarkerFile {
    env: Vec<String>,
    #[serde(rename = "processNames")]
    process_names: Vec<String>,
}

fn marker_file() -> MarkerFile {
    serde_json::from_str(crate::contract::AGENT_MARKERS).expect("agent-markers.json parses")
}

/// T0: looks for agent markers in `env` and in the names of ancestor
/// processes. Advisory only.
pub fn detect_agent(env: impl Fn(&str) -> Option<OsString>, ancestry: &[String]) -> AgentMarkers {
    let file = marker_file();
    let mut markers = Vec::new();
    for key in &file.env {
        if env(key).is_some_and(|v| !v.is_empty() && v != "0") {
            markers.push(format!("env:{key}"));
        }
    }
    for name in ancestry {
        let base = name.rsplit('/').next().unwrap_or(name);
        if file.process_names.iter().any(|p| p == base) {
            let marker = format!("process:{base}");
            if !markers.contains(&marker) {
                markers.push(marker);
            }
        }
    }
    AgentMarkers {
        agent: !markers.is_empty(),
        markers,
    }
}

/// [`detect_agent`] for this process: its environment and up to 16
/// ancestors from `ps`.
pub fn detect_agent_here() -> AgentMarkers {
    detect_agent(
        |k| std::env::var_os(k),
        &ancestor_names(std::process::id(), 16),
    )
}

/// Names of the ancestors of `pid`, nearest first. Reads `ps`; never
/// signals.
pub fn ancestor_names(pid: u32, limit: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = pid;
    for _ in 0..limit {
        let Ok(out) = std::process::Command::new("ps")
            .args(["-o", "ppid=", "-p", &current.to_string()])
            .output()
        else {
            break;
        };
        let Some(parent) = String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u32>()
            .ok()
        else {
            break;
        };
        if parent <= 1 {
            break;
        }
        let Ok(out) = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", &parent.to_string()])
            .output()
        else {
            break;
        };
        names.push(String::from_utf8_lossy(&out.stdout).trim().to_string());
        current = parent;
    }
    names
}

/// What a satisfied gate returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateProof {
    pub tier: GateTier,
    pub digest: String,
    pub confirmed_at: String,
}

/// The terminal the gate talks to. The real one is `/dev/tty`; tests
/// inject their own.
pub trait Terminal {
    /// Whether this process is in the terminal's foreground group (T1).
    fn is_foreground(&self) -> std::io::Result<bool>;
    fn write(&mut self, text: &str) -> std::io::Result<()>;
    /// Reads one line, or `None` when `timeout` passes first.
    fn read_line(&mut self, timeout: Duration) -> std::io::Result<Option<String>>;
}

fn human_required(message: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::HumanRequired, message)
}

fn t3_unsupported() -> ErrorBody {
    ErrorBody::new(
        ErrorCode::UnsupportedPlatform,
        "Gate tier T3 (OS owner authentication) is not supported yet.",
    )
}

// No 0/O/1/I/L, so a code survives being read aloud.
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

fn one_time_code() -> Result<String, ErrorBody> {
    let bytes = crypto::random_bytes::<6>().map_err(|e| {
        ErrorBody::new(ErrorCode::Internal, "Could not draw a code.").with_detail(e.to_string())
    })?;
    // 256 % 31 bias is under 1%; acceptable for a 6-symbol typed code.
    let symbols: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    Ok(format!("{}-{}", &symbols[..3], &symbols[3..]))
}

fn normalize(code: &str) -> String {
    code.trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

/// T1+T2 on the real `/dev/tty`. No controlling terminal answers
/// `human-required`; it never falls back to stdin.
pub fn require_human(title: &str, digest: &str, tier: GateTier) -> Result<GateProof, ErrorBody> {
    if tier == GateTier::T3 {
        return Err(t3_unsupported());
    }
    #[cfg(unix)]
    {
        let mut tty = DevTty::open().map_err(|_| {
            human_required("This decision needs a person at a terminal. Nothing changed.")
        })?;
        require_human_with(&mut tty, title, digest, tier, DEFAULT_CODE_TTL)
    }
    #[cfg(not(unix))]
    {
        let _ = (title, digest);
        Err(ErrorBody::new(
            ErrorCode::UnsupportedPlatform,
            "The human gate needs a Unix terminal.",
        ))
    }
}

/// [`require_human`] on an injected terminal.
pub fn require_human_with(
    term: &mut dyn Terminal,
    title: &str,
    digest: &str,
    tier: GateTier,
    ttl: Duration,
) -> Result<GateProof, ErrorBody> {
    if tier == GateTier::T3 {
        return Err(t3_unsupported());
    }
    if !term.is_foreground().unwrap_or(false) {
        return Err(human_required(
            "This decision needs the terminal in the foreground. Nothing changed.",
        ));
    }
    let code = one_time_code()?;
    let io = |e: std::io::Error| human_required("The terminal failed.").with_detail(e.to_string());
    let clean = |s: &str| s.chars().filter(|c| !c.is_control()).collect::<String>();
    term.write(&format!(
        "\n{}\nDigest: {}\nType {} to confirm ({} s): ",
        clean(title),
        clean(digest),
        code,
        ttl.as_secs()
    ))
    .map_err(io)?;
    let started = std::time::Instant::now();
    let line = term.read_line(ttl).map_err(io)?;
    let Some(line) = line else {
        let _ = term.write("\nExpired. Nothing changed.\n");
        return Err(ErrorBody::new(
            ErrorCode::GateExpired,
            "The code expired. Nothing changed.",
        ));
    };
    if started.elapsed() > ttl {
        return Err(ErrorBody::new(
            ErrorCode::GateExpired,
            "The code expired. Nothing changed.",
        ));
    }
    if !crypto::constant_time_eq(normalize(&line).as_bytes(), normalize(&code).as_bytes()) {
        let _ = term.write("The code did not match. Nothing changed.\n");
        return Err(ErrorBody::new(
            ErrorCode::GateFailed,
            "The code did not match. Nothing changed.",
        ));
    }
    Ok(GateProof {
        tier,
        digest: digest.to_string(),
        confirmed_at: crate::time::iso8601(SystemTime::now()),
    })
}

/// T3 owner authentication. Reserved: always `unsupported-platform`.
pub fn owner_authorize(
    _helper: &std::path::Path,
    _reason: &str,
    _digest: &str,
) -> Result<bool, ErrorBody> {
    Err(t3_unsupported())
}

/// `/dev/tty`, the controlling terminal.
#[cfg(unix)]
pub struct DevTty(std::fs::File);

#[cfg(unix)]
impl DevTty {
    /// Opens the controlling terminal. Fails (ENXIO) without one.
    pub fn open() -> std::io::Result<Self> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map(DevTty)
    }
}

#[cfg(unix)]
impl Terminal for DevTty {
    fn is_foreground(&self) -> std::io::Result<bool> {
        let foreground = rustix::termios::tcgetpgrp(&self.0)?;
        Ok(foreground == rustix::process::getpgrp())
    }

    fn write(&mut self, text: &str) -> std::io::Result<()> {
        use std::io::Write;
        self.0.write_all(text.as_bytes())?;
        self.0.flush()
    }

    fn read_line(&mut self, timeout: Duration) -> std::io::Result<Option<String>> {
        use rustix::event::{poll, PollFd, PollFlags, Timespec};
        use std::io::Read;
        let deadline = std::time::Instant::now() + timeout;
        let mut line = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let spec = Timespec {
                tv_sec: left.as_secs() as _,
                tv_nsec: left.subsec_nanos() as _,
            };
            let mut fds = [PollFd::new(&self.0, PollFlags::IN)];
            match poll(&mut fds, Some(&spec)) {
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
            let mut buf = [0u8; 128];
            let n = self.0.read(&mut buf)?;
            if n == 0 {
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            line.extend_from_slice(&buf[..n]);
            if line.contains(&b'\n') || line.len() > 256 {
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
        }
    }
}

/// A challenge an owner hands a client, redeemable once for one verb and
/// one digest before it expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Challenge {
    pub id: String,
    pub verb: String,
    pub digest: String,
    pub expires_at_ms: u64,
    pub mac: String,
}

/// Issues and redeems [`Challenge`]s with a per-process key.
pub struct ChallengeIssuer {
    key: [u8; 32],
    used: Mutex<HashMap<String, u64>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ChallengeIssuer {
    pub fn new() -> Result<Self, ErrorBody> {
        let key = crypto::random_bytes::<32>().map_err(|e| {
            ErrorBody::new(ErrorCode::Internal, "Could not draw a key.").with_detail(e.to_string())
        })?;
        Ok(Self {
            key,
            used: Mutex::new(HashMap::new()),
        })
    }

    fn mac(&self, id: &str, verb: &str, digest: &str, expires_at_ms: u64) -> String {
        let message = format!("hraness.challenge/1\0{id}\0{verb}\0{digest}\0{expires_at_ms}");
        crypto::hex(&crypto::hmac_sha256(&self.key, message.as_bytes()))
    }

    /// A new challenge for `verb` on `digest`.
    pub fn owner_challenge(
        &self,
        verb: &str,
        digest: &str,
        ttl: Duration,
    ) -> Result<Challenge, ErrorBody> {
        let id = crypto::hex(&crypto::random_bytes::<16>().map_err(|e| {
            ErrorBody::new(ErrorCode::Internal, "Could not draw an id.").with_detail(e.to_string())
        })?);
        let expires_at_ms = now_ms() + ttl.as_millis() as u64;
        let mac = self.mac(&id, verb, digest, expires_at_ms);
        Ok(Challenge {
            id,
            verb: verb.to_string(),
            digest: digest.to_string(),
            expires_at_ms,
            mac,
        })
    }

    /// Redeems `challenge` for `verb` on `digest`. Fails with
    /// `digest-mismatch` for another verb or digest, `gate-failed` for a
    /// forged or reused challenge, and `gate-expired` after its TTL.
    pub fn redeem(&self, challenge: &Challenge, verb: &str, digest: &str) -> Result<(), ErrorBody> {
        self.redeem_at(challenge, verb, digest, now_ms())
    }

    fn redeem_at(
        &self,
        c: &Challenge,
        verb: &str,
        digest: &str,
        now: u64,
    ) -> Result<(), ErrorBody> {
        let want = self.mac(&c.id, &c.verb, &c.digest, c.expires_at_ms);
        if !crypto::constant_time_eq(want.as_bytes(), c.mac.as_bytes()) {
            return Err(ErrorBody::new(
                ErrorCode::GateFailed,
                "The challenge is not valid.",
            ));
        }
        if c.verb != verb || c.digest != digest {
            return Err(ErrorBody::new(
                ErrorCode::DigestMismatch,
                "The challenge is for a different decision.",
            ));
        }
        if now > c.expires_at_ms {
            return Err(ErrorBody::new(
                ErrorCode::GateExpired,
                "The challenge expired.",
            ));
        }
        let mut used = self.used.lock().unwrap_or_else(|p| p.into_inner());
        used.retain(|_, expires| *expires >= now);
        if used.insert(c.id.clone(), c.expires_at_ms).is_some() {
            return Err(ErrorBody::new(
                ErrorCode::GateFailed,
                "The challenge was already used.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Fake {
        foreground: bool,
        out: String,
        answers: VecDeque<Option<String>>,
    }

    impl Fake {
        fn new(foreground: bool, answer: Option<&str>) -> Self {
            Self {
                foreground,
                out: String::new(),
                answers: VecDeque::from([answer.map(str::to_string)]),
            }
        }
        fn code(&self) -> String {
            let at = self.out.find("Type ").unwrap() + 5;
            self.out[at..at + 7].to_string()
        }
    }

    impl Terminal for Fake {
        fn is_foreground(&self) -> std::io::Result<bool> {
            Ok(self.foreground)
        }
        fn write(&mut self, text: &str) -> std::io::Result<()> {
            self.out.push_str(text);
            Ok(())
        }
        fn read_line(&mut self, _: Duration) -> std::io::Result<Option<String>> {
            Ok(self.answers.pop_front().flatten())
        }
    }

    /// Echoes the code it was shown, as a person would.
    struct Person(Fake);
    impl Terminal for Person {
        fn is_foreground(&self) -> std::io::Result<bool> {
            Ok(true)
        }
        fn write(&mut self, text: &str) -> std::io::Result<()> {
            self.0.write(text)
        }
        fn read_line(&mut self, _: Duration) -> std::io::Result<Option<String>> {
            Ok(Some(format!("{}\n", self.0.code().to_lowercase())))
        }
    }

    #[test]
    fn the_right_code_passes() {
        let mut p = Person(Fake::new(true, None));
        let proof =
            require_human_with(&mut p, "Allow a1", "3f2a", GateTier::T1T2, DEFAULT_CODE_TTL)
                .unwrap();
        assert_eq!(proof.digest, "3f2a");
        assert!(p.0.out.contains("Digest: 3f2a"));
    }

    #[test]
    fn background_wrong_and_silent_answers_fail() {
        let mut bg = Fake::new(false, Some("x"));
        let e =
            require_human_with(&mut bg, "t", "d", GateTier::T1T2, DEFAULT_CODE_TTL).unwrap_err();
        assert_eq!(e.code, ErrorCode::HumanRequired);
        assert!(
            bg.out.is_empty(),
            "no code is shown to a background process"
        );

        let mut wrong = Fake::new(true, Some("AAA-AAA\n"));
        let e =
            require_human_with(&mut wrong, "t", "d", GateTier::T1T2, DEFAULT_CODE_TTL).unwrap_err();
        assert_eq!(e.code, ErrorCode::GateFailed);
        assert_eq!(e.code.exit_code(), 3);

        let mut silent = Fake::new(true, None);
        let e = require_human_with(&mut silent, "t", "d", GateTier::T1T2, DEFAULT_CODE_TTL)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::GateExpired);
    }

    #[test]
    fn control_characters_in_the_title_are_dropped() {
        let mut p = Person(Fake::new(true, None));
        require_human_with(&mut p, "a\x1b[2Jb", "d", GateTier::T1T2, DEFAULT_CODE_TTL).unwrap();
        assert!(p.0.out.contains("a[2Jb"));
        assert!(!p.0.out.contains('\x1b'));
    }

    #[test]
    fn t3_is_reserved() {
        let mut p = Person(Fake::new(true, None));
        let e = require_human_with(&mut p, "t", "d", GateTier::T3, DEFAULT_CODE_TTL).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnsupportedPlatform);
        assert_eq!(
            require_human("t", "d", GateTier::T3).unwrap_err().code,
            ErrorCode::UnsupportedPlatform
        );
        assert!(owner_authorize(std::path::Path::new("/x"), "r", "d").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn without_a_controlling_terminal_it_is_human_required() {
        if DevTty::open().is_ok() {
            eprintln!("skipped: this test run has a controlling terminal");
            return;
        }
        let e = require_human("t", "d", GateTier::T1T2).unwrap_err();
        assert_eq!(e.code, ErrorCode::HumanRequired);
        assert_eq!(e.code.exit_code(), 3);
    }

    #[test]
    fn codes_use_the_unambiguous_alphabet() {
        for _ in 0..50 {
            let c = one_time_code().unwrap();
            assert_eq!(c.len(), 7);
            assert!(normalize(&c).bytes().all(|b| ALPHABET.contains(&b)));
        }
        assert_eq!(normalize(" ab c-d9 \n"), "ABCD9");
    }

    #[test]
    fn challenges_are_single_use_bound_and_expiring() {
        let issuer = ChallengeIssuer::new().unwrap();
        let c = issuer
            .owner_challenge("approvals decide", "3f2a", Duration::from_secs(60))
            .unwrap();
        assert_eq!(
            issuer
                .redeem(&c, "approvals decide", "ffff")
                .unwrap_err()
                .code,
            ErrorCode::DigestMismatch
        );
        assert_eq!(
            issuer.redeem(&c, "policy set", "3f2a").unwrap_err().code,
            ErrorCode::DigestMismatch
        );
        let mut forged = c.clone();
        forged.digest = "ffff".into();
        assert_eq!(
            issuer
                .redeem(&forged, "approvals decide", "ffff")
                .unwrap_err()
                .code,
            ErrorCode::GateFailed
        );
        let mut extended = c.clone();
        extended.expires_at_ms += 1_000_000;
        assert_eq!(
            issuer
                .redeem(&extended, "approvals decide", "3f2a")
                .unwrap_err()
                .code,
            ErrorCode::GateFailed
        );
        issuer.redeem(&c, "approvals decide", "3f2a").unwrap();
        assert_eq!(
            issuer
                .redeem(&c, "approvals decide", "3f2a")
                .unwrap_err()
                .code,
            ErrorCode::GateFailed
        );

        let other = ChallengeIssuer::new().unwrap();
        assert_eq!(
            other
                .redeem(&c, "approvals decide", "3f2a")
                .unwrap_err()
                .code,
            ErrorCode::GateFailed
        );

        let late = issuer
            .owner_challenge("approvals decide", "3f2a", Duration::from_millis(10))
            .unwrap();
        assert_eq!(
            issuer
                .redeem_at(&late, "approvals decide", "3f2a", late.expires_at_ms + 1)
                .unwrap_err()
                .code,
            ErrorCode::GateExpired
        );
    }

    #[test]
    fn agent_markers_are_advisory_and_from_the_contract() {
        let env = |k: &str| (k == "CLAUDECODE").then(|| OsString::from("1"));
        let m = detect_agent(env, &["/usr/local/bin/codex".into(), "zsh".into()]);
        assert!(m.agent);
        assert_eq!(m.markers, vec!["env:CLAUDECODE", "process:codex"]);
        let none = detect_agent(|k| (k == "AI_AGENT").then(|| OsString::from("0")), &[]);
        assert!(!none.agent);
    }
}
