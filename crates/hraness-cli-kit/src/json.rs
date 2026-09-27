//! Just enough JSON to write the kit's documents without a serializer
//! dependency. The output matches `JSON.stringify` byte for byte, so the Rust
//! and TypeScript kits check the same golden files.

use std::fmt::Write as _;

/// `value` as a JSON string literal, escaped exactly as `JSON.stringify` does.
pub fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// A JSON object written field by field, in order.
#[derive(Debug, Default)]
pub struct Object {
    body: String,
}

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(&mut self, key: &str) {
        if !self.body.is_empty() {
            self.body.push(',');
        }
        self.body.push_str(&string(key));
        self.body.push(':');
    }

    /// A string field.
    pub fn str(mut self, key: &str, value: &str) -> Self {
        self.key(key);
        self.body.push_str(&string(value));
        self
    }

    /// A string field, or `null` when absent.
    pub fn str_or_null(mut self, key: &str, value: Option<&str>) -> Self {
        self.key(key);
        match value {
            Some(value) => self.body.push_str(&string(value)),
            None => self.body.push_str("null"),
        }
        self
    }

    /// A string field only when present.
    pub fn opt_str(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(value) => self.str(key, value),
            None => self,
        }
    }

    /// A field whose value is already JSON (a number, `true`, an object).
    pub fn raw(mut self, key: &str, json: &str) -> Self {
        self.key(key);
        self.body.push_str(json);
        self
    }

    /// An array of strings.
    pub fn strings(mut self, key: &str, values: &[String]) -> Self {
        self.key(key);
        self.body.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.body.push(',');
            }
            self.body.push_str(&string(value));
        }
        self.body.push(']');
        self
    }

    pub fn finish(self) -> String {
        format!("{{{}}}", self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_json_stringify() {
        assert_eq!(string("plain"), "\"plain\"");
        assert_eq!(string("say \"hi\" \\ now"), "\"say \\\"hi\\\" \\\\ now\"");
        assert_eq!(string("a\nb\tc\r\u{8}\u{c}"), "\"a\\nb\\tc\\r\\b\\f\"");
        assert_eq!(string("\u{1b}[0m\u{0}"), "\"\\u001b[0m\\u0000\"");
        // Non-ASCII, slashes and U+2028 pass through as JSON.stringify leaves them.
        assert_eq!(string("› é / \u{2028}"), "\"› é / \u{2028}\"");
        assert_eq!(
            Object::new()
                .str("a", "x")
                .str_or_null("b", None)
                .opt_str("c", None)
                .raw("d", "1")
                .strings("e", &["y".into()])
                .finish(),
            "{\"a\":\"x\",\"b\":null,\"d\":1,\"e\":[\"y\"]}"
        );
    }
}
