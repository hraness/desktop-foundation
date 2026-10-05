//! The primitives the one-shot helper modes share: the frame version, the
//! frame size limit and the error line.

use std::io::{BufRead, Write};

/// The protocol version every one-shot helper frame uses.
pub const VERSION: u8 = 1;
/// The largest frame any mode reads from stdin.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
/// The protocol versions `--version` reports. Frozen by
/// `contract/helper-argv.v0.8.1.json`; 1.0 removed the menu-bar runner these
/// once named, and the line keeps its shape so version parsers still work.
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
/// same bytes the 0.x menu-bar runner printed for an error.
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

/// Reads exactly one bounded line. Trailing bytes after the first line are
/// rejected so a piped request cannot smuggle a second one. `missing` is the
/// error code for empty input.
#[doc(hidden)]
pub fn read_single_frame(
    reader: &mut impl BufRead,
    missing: &'static str,
) -> Result<Vec<u8>, ProtocolError> {
    let mut frame = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .map_err(|_| ProtocolError("input-unavailable"))?;
        if bytes.is_empty() {
            return Err(if frame.is_empty() {
                ProtocolError(missing)
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
            // A second request already buffered in the same write is smuggling;
            // never block waiting for EOF — callers may hold the pipe open.
            if !reader
                .fill_buf()
                .map_err(|_| ProtocolError("input-unavailable"))?
                .is_empty()
            {
                return Err(ProtocolError("trailing-input"));
            }
            return Ok(frame);
        }
    }
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
