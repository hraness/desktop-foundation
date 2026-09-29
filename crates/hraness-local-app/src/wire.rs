//! The primitives the one-shot helper modes share with the menu-bar
//! runner protocol. `desktop_foundation::protocol` re-exports them.

use std::io::Write;

/// The protocol version every one-shot helper frame uses.
pub const VERSION: u8 = 1;
/// The largest frame any mode reads from stdin.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
/// Runner protocol versions `--version` reports. `desktop_foundation`'s
/// tests check this against its `SUPPORTED_VERSIONS`.
pub const RUNNER_PROTOCOLS: &str = "1,2";

/// Safe machine-readable categories: never copy input into an error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolError(pub &'static str);

pub fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.contains("..")
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || b".-".contains(&ch))
}

/// `{"type":"error","version":<v>,"code":"<code>"}` plus a newline: the
/// same bytes the runner's `Event::Error` serializes to.
pub fn write_error(writer: &mut impl Write, version: u8, code: &str) -> std::io::Result<()> {
    #[derive(serde::Serialize)]
    struct Error<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        version: u8,
        code: &'a str,
    }
    serde_json::to_writer(
        &mut *writer,
        &Error {
            kind: "error",
            version,
            code,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_lines_keep_the_runner_field_order() {
        let mut out = Vec::new();
        write_error(&mut out, 1, "invalid-arguments").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"type\":\"error\",\"version\":1,\"code\":\"invalid-arguments\"}\n"
        );
    }

    #[test]
    fn app_ids_are_short_lowercase_identifiers() {
        assert!(valid_app_id("textbutler"));
        assert!(valid_app_id("app.hraness-1"));
        for bad in ["", "Text", "1abc", "a..b", "a/b", &"a".repeat(65)] {
            assert!(!valid_app_id(bad), "{bad}");
        }
    }
}
