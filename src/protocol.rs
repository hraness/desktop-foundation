//! Bounded JSON Lines contract for the product-neutral native runner.
//!
//! Protocol data never contains executable code or filesystem operations.
//! The owning CLI receives action IDs and decides how to handle them.

use std::collections::HashSet;
use std::io::BufRead;

use serde::{Deserialize, Serialize};

use crate::{MenuItem, MenuItemKind, MenuModel, MenuNode, RgbaIcon};

pub const VERSION: u8 = 1;
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_ITEMS: usize = 256;
pub const MAX_DEPTH: usize = 8;
pub const MAX_REVISION: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Frame {
    Snapshot {
        version: u8,
        #[serde(rename = "appId")]
        app_id: String,
        name: String,
        title: String,
        tooltip: Option<String>,
        revision: u64,
        items: Vec<WireItem>,
    },
    Quit {
        version: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WireItem {
    Action {
        id: String,
        label: String,
        #[serde(default = "enabled_by_default")]
        enabled: bool,
        checked: Option<bool>,
        shortcut: Option<String>,
    },
    Label {
        label: String,
    },
    Separator,
    Submenu {
        label: String,
        items: Vec<WireItem>,
    },
    Quit {
        label: String,
    },
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    Ready {
        version: u8,
        pid: u32,
        platform: &'static str,
    },
    Validated {
        version: u8,
        revision: u64,
    },
    Action {
        version: u8,
        id: String,
        revision: u64,
    },
    Error {
        version: u8,
        code: &'static str,
    },
    AlreadyRunning {
        version: u8,
    },
    Stopped {
        version: u8,
    },
}

impl Event {
    pub fn error(code: &'static str) -> Self {
        Self::Error {
            version: VERSION,
            code,
        }
    }
}

/// Safe machine-readable categories: never copy input into an error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolError(pub &'static str);

/// Limits allocation even if stdin never supplies a newline. Oversized and
/// partial frames are rejected; the caller must close rather than resync.
pub fn read_frame(reader: &mut impl BufRead) -> Result<Option<Frame>, ProtocolError> {
    let mut frame = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .map_err(|_| ProtocolError("input-unavailable"))?;
        if bytes.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(ProtocolError("partial-frame"))
            };
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
            return serde_json::from_slice(&frame)
                .map(Some)
                .map_err(|_| ProtocolError("invalid-frame"));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub app_id: String,
    pub name: String,
    pub title: String,
    pub tooltip: Option<String>,
    pub revision: u64,
    pub items: Vec<WireItem>,
}

fn safe_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"._-".contains(&ch))
}

fn valid_action_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"._:-".contains(&ch))
}

impl Snapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if !valid_app_id(&self.app_id)
            || !safe_text(&self.name, 128)
            || self.title.is_empty()
            || self.title.len() > 2
            || !self.title.bytes().all(|ch| ch.is_ascii_alphanumeric())
            || self
                .tooltip
                .as_ref()
                .is_some_and(|value| !safe_text(value, 256))
            || self.revision > MAX_REVISION
        {
            return Err(ProtocolError("invalid-snapshot"));
        }
        fn walk(
            items: &[WireItem],
            depth: usize,
            count: &mut usize,
            ids: &mut HashSet<String>,
        ) -> Result<(), ProtocolError> {
            if depth > MAX_DEPTH {
                return Err(ProtocolError("menu-too-deep"));
            }
            for item in items {
                *count += 1;
                if *count > MAX_ITEMS {
                    return Err(ProtocolError("menu-too-large"));
                }
                let label = match item {
                    WireItem::Action {
                        id,
                        label,
                        shortcut,
                        ..
                    } => {
                        if !valid_action_id(id) || !ids.insert(id.clone()) {
                            return Err(ProtocolError("invalid-action"));
                        }
                        if shortcut.as_ref().is_some_and(|value| !safe_text(value, 64)) {
                            return Err(ProtocolError("invalid-shortcut"));
                        }
                        label
                    }
                    WireItem::Label { label } | WireItem::Quit { label } => label,
                    WireItem::Submenu { label, items } => {
                        walk(items, depth + 1, count, ids)?;
                        label
                    }
                    WireItem::Separator => continue,
                };
                if !safe_text(label, 256) {
                    return Err(ProtocolError("invalid-label"));
                }
            }
            Ok(())
        }
        walk(&self.items, 1, &mut 0, &mut HashSet::new())
    }

    pub fn model(&self) -> MenuModel {
        fn nodes(items: &[WireItem], revision: u64) -> Vec<MenuNode> {
            items
                .iter()
                .map(|item| match item {
                    WireItem::Action {
                        id,
                        label,
                        enabled,
                        checked,
                        shortcut,
                    } => {
                        // Embedding revision in the route binds a native event to
                        // the exact offered snapshot, not the newest same-named action.
                        let mut item = MenuItem::action(format!("{revision}:{id}"), label);
                        item.enabled = *enabled;
                        item.shortcut = shortcut.clone();
                        if let Some(checked) = checked {
                            item.kind = MenuItemKind::Check { checked: *checked };
                        }
                        MenuNode::interactive(item)
                    }
                    WireItem::Label { label } => MenuNode::disabled(label),
                    WireItem::Separator => MenuNode::Separator,
                    WireItem::Submenu { label, items } => MenuNode::Submenu {
                        title: label.clone(),
                        items: nodes(items, revision),
                    },
                    WireItem::Quit { label } => MenuNode::quit(label),
                })
                .collect()
        }
        MenuModel {
            title: if cfg!(target_os = "macos") {
                Some(self.title.clone())
            } else {
                None
            },
            icon: if cfg!(target_os = "macos") {
                None
            } else {
                Some(monogram(&self.title))
            },
            tooltip: Some(self.tooltip.clone().unwrap_or_else(|| self.name.clone())),
            nodes: nodes(&self.items, self.revision),
        }
    }

    pub fn action(&self, route: &str) -> Option<Event> {
        let (revision, id) = route.split_once(':')?;
        if revision.parse::<u64>().ok()? != self.revision {
            return None;
        }
        fn offered(items: &[WireItem], target: &str) -> bool {
            items.iter().any(|item| match item {
                WireItem::Action { id, enabled, .. } => *enabled && id == target,
                WireItem::Submenu { items, .. } => offered(items, target),
                _ => false,
            })
        }
        offered(&self.items, id).then(|| Event::Action {
            version: VERSION,
            id: id.into(),
            revision: self.revision,
        })
    }
}

#[derive(Default)]
pub struct Session {
    latest: Option<Snapshot>,
}

impl Session {
    pub fn latest(&self) -> Option<&Snapshot> {
        self.latest.as_ref()
    }

    /// `false` means the parent requested a clean shutdown. A bad frame does
    /// not replace the authoritative snapshot; callers terminate the stream.
    pub fn accept(&mut self, frame: Frame) -> Result<bool, ProtocolError> {
        match frame {
            Frame::Quit { version } => {
                if version != VERSION {
                    return Err(ProtocolError("unsupported-version"));
                }
                Ok(false)
            }
            Frame::Snapshot {
                version,
                app_id,
                name,
                title,
                tooltip,
                revision,
                items,
            } => {
                if version != VERSION {
                    return Err(ProtocolError("unsupported-version"));
                }
                let snapshot = Snapshot {
                    app_id,
                    name,
                    title,
                    tooltip,
                    revision,
                    items,
                };
                snapshot.validate()?;
                if let Some(previous) = &self.latest {
                    if previous.app_id != snapshot.app_id || previous.name != snapshot.name {
                        return Err(ProtocolError("identity-changed"));
                    }
                    if previous.revision >= snapshot.revision {
                        return Err(ProtocolError("stale-revision"));
                    }
                }
                self.latest = Some(snapshot);
                Ok(true)
            }
        }
    }
}

/// Tiny built-in alphanumeric glyphs avoid shipping fonts or relying on a
/// machine font installation for the Windows/Linux icon-only tray surfaces.
/// macOS uses its native text title (with Georgia when available).
pub fn monogram(title: &str) -> RgbaIcon {
    const GLYPHS: [[u8; 7]; 36] = [
        [14, 17, 17, 31, 17, 17, 17],
        [30, 17, 17, 30, 17, 17, 30],
        [14, 17, 16, 16, 16, 17, 14],
        [30, 17, 17, 17, 17, 17, 30],
        [31, 16, 16, 30, 16, 16, 31],
        [31, 16, 16, 30, 16, 16, 16],
        [14, 17, 16, 23, 17, 17, 15],
        [17, 17, 17, 31, 17, 17, 17],
        [31, 4, 4, 4, 4, 4, 31],
        [7, 2, 2, 2, 2, 18, 12],
        [17, 18, 20, 24, 20, 18, 17],
        [16, 16, 16, 16, 16, 16, 31],
        [17, 27, 21, 21, 17, 17, 17],
        [17, 25, 21, 19, 17, 17, 17],
        [14, 17, 17, 17, 17, 17, 14],
        [30, 17, 17, 30, 16, 16, 16],
        [14, 17, 17, 17, 21, 18, 13],
        [30, 17, 17, 30, 20, 18, 17],
        [15, 16, 16, 14, 1, 1, 30],
        [31, 4, 4, 4, 4, 4, 4],
        [17, 17, 17, 17, 17, 17, 14],
        [17, 17, 17, 17, 17, 10, 4],
        [17, 17, 17, 21, 21, 21, 10],
        [17, 17, 10, 4, 10, 17, 17],
        [17, 17, 10, 4, 4, 4, 4],
        [31, 1, 2, 4, 8, 16, 31],
        [14, 17, 19, 21, 25, 17, 14],
        [4, 12, 4, 4, 4, 4, 14],
        [14, 17, 1, 2, 4, 8, 31],
        [30, 1, 1, 14, 1, 1, 30],
        [2, 6, 10, 18, 31, 2, 2],
        [31, 16, 16, 30, 1, 1, 30],
        [14, 16, 16, 30, 17, 17, 14],
        [31, 1, 2, 4, 8, 8, 8],
        [14, 17, 17, 14, 17, 17, 14],
        [14, 17, 17, 15, 1, 1, 14],
    ];
    const LOWERCASE: [[u8; 7]; 26] = [
        [0, 0, 14, 1, 15, 17, 15],
        [16, 16, 30, 17, 17, 17, 30],
        [0, 0, 14, 16, 16, 17, 14],
        [1, 1, 15, 17, 17, 17, 15],
        [0, 0, 14, 17, 31, 16, 14],
        [6, 9, 8, 28, 8, 8, 8],
        [0, 15, 17, 17, 15, 1, 14],
        [16, 16, 30, 17, 17, 17, 17],
        [4, 0, 12, 4, 4, 4, 14],
        [2, 0, 6, 2, 2, 18, 12],
        [16, 16, 18, 20, 24, 20, 18],
        [12, 4, 4, 4, 4, 4, 14],
        [0, 0, 26, 21, 21, 21, 21],
        [0, 0, 30, 17, 17, 17, 17],
        [0, 0, 14, 17, 17, 17, 14],
        [0, 30, 17, 17, 30, 16, 16],
        [0, 15, 17, 17, 15, 1, 1],
        [0, 0, 22, 25, 16, 16, 16],
        [0, 0, 15, 16, 14, 1, 30],
        [8, 8, 28, 8, 8, 9, 6],
        [0, 0, 17, 17, 17, 19, 13],
        [0, 0, 17, 17, 17, 10, 4],
        [0, 0, 17, 17, 21, 21, 10],
        [0, 0, 17, 10, 4, 10, 17],
        [0, 17, 17, 17, 15, 1, 14],
        [0, 0, 31, 2, 4, 8, 31],
    ];
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 1..31 {
        for x in 1..31 {
            if (x == 1 || x == 30) && (y == 1 || y == 30) {
                continue;
            }
            rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].copy_from_slice(&[35, 39, 48, 255]);
        }
    }
    let letters: Vec<u8> = title
        .bytes()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(2)
        .collect();
    let origin = if letters.len() < 2 { 11 } else { 5 };
    for (letter, ch) in letters.into_iter().enumerate() {
        let glyph = if ch.is_ascii_lowercase() {
            &LOWERCASE[usize::from(ch - b'a')]
        } else if ch.is_ascii_digit() {
            &GLYPHS[26 + usize::from(ch - b'0')]
        } else {
            &GLYPHS[usize::from(ch - b'A')]
        };
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                for dy in 0..2 {
                    for dx in 0..2 {
                        let x = origin + letter * 12 + col * 2 + dx;
                        let y = 9 + row * 2 + dy;
                        rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                            .copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    RgbaIcon {
        rgba,
        width: 32,
        height: 32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(revision: u64) -> Frame {
        Frame::Snapshot {
            version: 1,
            app_id: "test.app".into(),
            name: "Test".into(),
            title: "AI".into(),
            tooltip: None,
            revision,
            items: vec![
                WireItem::Action {
                    id: "pause:all".into(),
                    label: "Pause".into(),
                    enabled: true,
                    checked: Some(true),
                    shortcut: None,
                },
                WireItem::Quit {
                    label: "Quit".into(),
                },
            ],
        }
    }

    #[test]
    fn framing_bounds_unterminated_input_and_rejects_partial_json() {
        assert_eq!(
            read_frame(&mut Cursor::new(vec![b' '; MAX_FRAME_BYTES + 1])),
            Err(ProtocolError("frame-too-large"))
        );
        assert_eq!(
            read_frame(&mut Cursor::new(b"{\"type\":\"quit\",\"version\":1}")),
            Err(ProtocolError("partial-frame"))
        );
        assert!(matches!(
            read_frame(&mut Cursor::new(b"{\"type\":\"quit\",\"version\":1}\r\n")),
            Ok(Some(Frame::Quit { version: 1 }))
        ));
    }

    #[test]
    fn invalid_protocol_does_not_replace_the_authoritative_snapshot() {
        let mut session = Session::default();
        session.accept(frame(3)).unwrap();
        assert_eq!(
            session.accept(frame(3)),
            Err(ProtocolError("stale-revision"))
        );
        let mut next = frame(4);
        if let Frame::Snapshot { app_id, .. } = &mut next {
            *app_id = "other.app".into();
        }
        assert_eq!(session.accept(next), Err(ProtocolError("identity-changed")));
        assert_eq!(session.latest().unwrap().revision, 3);
        assert_eq!(
            session.accept(Frame::Quit { version: 2 }),
            Err(ProtocolError("unsupported-version"))
        );
    }

    #[test]
    fn actions_are_bound_to_revision_and_enabled_ids() {
        let mut session = Session::default();
        session.accept(frame(3)).unwrap();
        let first = session.latest().unwrap().clone();
        assert_eq!(
            first.action("3:pause:all"),
            Some(Event::Action {
                version: 1,
                id: "pause:all".into(),
                revision: 3
            })
        );
        assert!(first.action("2:pause:all").is_none());
        assert!(first.action("3:unknown").is_none());
        let mut next = frame(4);
        if let Frame::Snapshot { items, .. } = &mut next {
            if let WireItem::Action { enabled, .. } = &mut items[0] {
                *enabled = false;
            }
        }
        session.accept(next).unwrap();
        assert!(session.latest().unwrap().action("3:pause:all").is_none());
        assert!(session.latest().unwrap().action("4:pause:all").is_none());
        assert_ne!(first.model(), session.latest().unwrap().model());
    }

    #[test]
    fn menus_reject_duplicates_deep_trees_controls_and_unsafe_ids() {
        for value in ["../outside", "", "a/b", "a\\b"] {
            assert!(!valid_app_id(value));
        }
        let mut nested = WireItem::Label {
            label: "safe".into(),
        };
        for _ in 0..MAX_DEPTH {
            nested = WireItem::Submenu {
                label: "Nested".into(),
                items: vec![nested],
            };
        }
        let mut next = frame(1);
        if let Frame::Snapshot { items, .. } = &mut next {
            *items = vec![nested];
        }
        assert_eq!(
            Session::default().accept(next),
            Err(ProtocolError("menu-too-deep"))
        );
        let mut next = frame(1);
        if let Frame::Snapshot { items, .. } = &mut next {
            items.push(items[0].clone());
        }
        assert_eq!(
            Session::default().accept(next),
            Err(ProtocolError("invalid-action"))
        );
        let mut next = frame(1);
        if let Frame::Snapshot { tooltip, .. } = &mut next {
            *tooltip = Some("bad\u{202e}text".into());
        }
        assert_eq!(
            Session::default().accept(next),
            Err(ProtocolError("invalid-snapshot"))
        );
    }

    #[test]
    fn schema_rejects_unknown_effects_and_accepts_optional_fields() {
        let good = b"{\"version\":1,\"type\":\"snapshot\",\"appId\":\"app.test\",\"name\":\"Test\",\"title\":\"Tb\",\"revision\":0,\"items\":[{\"kind\":\"action\",\"id\":\"x\",\"label\":\"X\"}]}\n";
        let mut session = Session::default();
        assert!(session
            .accept(read_frame(&mut Cursor::new(good)).unwrap().unwrap())
            .unwrap());
        assert!(read_frame(&mut Cursor::new(
            b"{\"version\":1,\"type\":\"quit\",\"command\":\"arbitrary\"}\n"
        ))
        .is_err());
    }

    #[test]
    fn all_glyphs_produce_a_bounded_visible_icon() {
        for ch in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars() {
            let icon = monogram(&format!("{ch}{ch}"));
            assert_eq!(icon.rgba.len(), 4096);
            assert!(icon
                .rgba
                .chunks_exact(4)
                .any(|px| px == [255, 255, 255, 255]));
            assert!(MenuModel {
                icon: Some(icon),
                ..MenuModel::default()
            }
            .validate()
            .is_ok());
        }
        assert_ne!(monogram("Sl"), monogram("SL"));
    }
}
