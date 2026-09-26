//! Protocol v2 (menu kit v2) wire schema and validation.
//!
//! Mirrors `sdk/src/protocol-v2.ts` and `docs/protocol-v2.md`. v2 keeps v1's
//! framing, limits, lifecycle, revision rules and action handling; it adds a
//! template menu-bar mark and richer menu rows. Enumerated fields travel as
//! strings so each bad value gets its own stable error code.

use std::collections::HashSet;

use serde::Deserialize;

use crate::protocol::{
    decode_base64, safe_text, valid_action_id, valid_app_id, valid_icon, ProtocolError,
    WireIcon, MAX_DEPTH, MAX_ITEMS, MAX_REVISION,
};
use crate::symbols::{Symbol, SymbolFamily};
use crate::{
    AlphaIcon, Alternate, ItemState, MarkTone, MenuItem, MenuModel, MenuNode, Opens, RgbaIcon,
    Role, StatusMark,
};

pub const VERSION: u8 = 2;
pub const MAX_TEMPLATE_SIDE: u32 = 64;

/// Settings panes a `foundation.settings.<kind>` action may open.
pub const SETTINGS_KINDS: &[&str] = &[
    "full-disk-access",
    "automation",
    "contacts",
    "accessibility",
    "screen-recording",
    "camera",
    "microphone",
    "local-network",
    "incoming-connections",
    "notifications",
    "login-item",
    "gatekeeper",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFrame {
    pub version: u8,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "appId")]
    pub app_id: String,
    pub name: String,
    pub revision: u64,
    pub mark: WireMark,
    pub tooltip: Option<String>,
    pub icon: Option<WireIcon>,
    pub items: Vec<WireItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMark {
    pub symbol: String,
    #[serde(rename = "templateIcon")]
    pub template_icon: Option<WireAlphaIcon>,
    pub letters: String,
    pub tone: Option<String>,
    pub text: Option<String>,
    #[serde(rename = "accessibilityLabel")]
    pub accessibility_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireAlphaIcon {
    pub width: u32,
    pub height: u32,
    pub alpha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireAlternate {
    pub id: String,
    pub label: String,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)]
pub enum WireItem {
    Header {
        label: String,
    },
    Status {
        symbol: String,
        label: String,
        detail: Option<String>,
    },
    Label {
        label: String,
        subtitle: Option<String>,
    },
    Action {
        id: String,
        label: String,
        #[serde(default = "enabled_by_default")]
        enabled: bool,
        state: Option<String>,
        symbol: Option<String>,
        subtitle: Option<String>,
        badge: Option<String>,
        tooltip: Option<String>,
        shortcut: Option<String>,
        alternate: Option<WireAlternate>,
        role: Option<String>,
        opens: Option<String>,
    },
    Separator,
    Submenu {
        label: String,
        symbol: Option<String>,
        items: Vec<WireItem>,
    },
    Quit {
        label: String,
    },
}

fn enabled_by_default() -> bool {
    true
}

fn tone(value: Option<&str>) -> Option<MarkTone> {
    Some(match value {
        None | Some("normal") => MarkTone::Normal,
        Some("attention") => MarkTone::Attention,
        Some("error") => MarkTone::Error,
        Some("paused") => MarkTone::Paused,
        Some("offline") => MarkTone::Offline,
        Some(_) => return None,
    })
}

fn state(value: &str) -> Option<ItemState> {
    Some(match value {
        "on" => ItemState::On,
        "off" => ItemState::Off,
        "mixed" => ItemState::Mixed,
        _ => return None,
    })
}

fn opens(value: &str) -> Option<Opens> {
    Some(match value {
        "browser" => Opens::Browser,
        "finder" => Opens::Finder,
        "settings" => Opens::Settings,
        "dialog" => Opens::Dialog,
        _ => return None,
    })
}

fn role(value: &str) -> Option<Role> {
    Some(match value {
        "primary" => Role::Primary,
        "destructive" => Role::Destructive,
        _ => return None,
    })
}

fn symbol_of(name: &str, allowed: impl Fn(SymbolFamily) -> bool) -> Result<Symbol, ProtocolError> {
    Symbol::from_name(name)
        .filter(|symbol| allowed(symbol.family()))
        .ok_or(ProtocolError("invalid-symbol"))
}

fn item_symbol(name: &Option<String>) -> Result<Option<Symbol>, ProtocolError> {
    name.as_deref()
        .map(|name| symbol_of(name, |family| matches!(family, SymbolFamily::Action | SymbolFamily::Item)))
        .transpose()
}

/// `foundation.*` IDs are reserved; v2 products may use only the documented ones.
fn reserved_id_allowed(id: &str) -> bool {
    match id.strip_prefix("foundation.") {
        None => true,
        Some("login") => true,
        Some(rest) => rest
            .strip_prefix("settings.")
            .is_some_and(|kind| SETTINGS_KINDS.contains(&kind)),
    }
}

fn optional_text(value: &Option<String>, max: usize) -> bool {
    value.as_deref().map_or(true, |value| safe_text(value, max))
}

/// A validated v2 mark, decoded.
fn mark(wire: &WireMark) -> Result<StatusMark, ProtocolError> {
    let symbol = symbol_of(&wire.symbol, |family| family == SymbolFamily::Mark)?;
    let tone = tone(wire.tone.as_deref()).ok_or(ProtocolError("invalid-mark"))?;
    let letters_ok = (1..=2).contains(&wire.letters.len())
        && wire.letters.bytes().all(|byte| byte.is_ascii_alphanumeric());
    let text_ok = match &wire.text {
        None => true,
        Some(text) => {
            safe_text(text, 4) && matches!(tone, MarkTone::Attention | MarkTone::Error)
        }
    };
    if !letters_ok || !text_ok || !optional_text(&wire.accessibility_label, 128) {
        return Err(ProtocolError("invalid-mark"));
    }
    let template_icon = match &wire.template_icon {
        None => None,
        Some(icon) => {
            let sides = (1..=MAX_TEMPLATE_SIDE).contains(&icon.width)
                && (1..=MAX_TEMPLATE_SIDE).contains(&icon.height);
            let alpha = decode_base64(&icon.alpha)
                .filter(|alpha| sides && alpha.len() == icon.width as usize * icon.height as usize)
                .ok_or(ProtocolError("invalid-mark"))?;
            Some(AlphaIcon { alpha, width: icon.width, height: icon.height })
        }
    };
    Ok(StatusMark {
        symbol,
        template_icon,
        letters: wire.letters.clone(),
        tone,
        text: wire.text.clone(),
        accessibility_label: wire.accessibility_label.clone(),
    })
}

impl SnapshotFrame {
    /// Validates the whole frame and returns the decoded mark.
    pub fn validate(&self) -> Result<StatusMark, ProtocolError> {
        if self.kind != "snapshot" {
            return Err(ProtocolError("invalid-frame"));
        }
        if !valid_app_id(&self.app_id)
            || !safe_text(&self.name, 128)
            || !optional_text(&self.tooltip, 160)
            || self.icon.as_ref().is_some_and(|icon| !valid_icon(icon))
            || self.revision > MAX_REVISION
        {
            return Err(ProtocolError("invalid-snapshot"));
        }
        let mark = mark(&self.mark)?;
        walk(&self.items, 1, &mut 0, &mut HashSet::new())?;
        Ok(mark)
    }

    /// Builds the native model. Action and alternate routes embed the
    /// revision, as in v1, so a click binds to the exact offered snapshot.
    pub fn model(&self) -> MenuModel {
        let mut model = MenuModel {
            tooltip: Some(self.tooltip.clone().unwrap_or_else(|| self.name.clone())),
            icon: self.icon.as_ref().and_then(|icon| {
                crate::protocol::decode_base64(&icon.rgba).map(|rgba| RgbaIcon {
                    rgba,
                    width: icon.width,
                    height: icon.height,
                })
            }),
            nodes: nodes(&self.items, self.revision),
            ..MenuModel::default()
        };
        if let Ok(mark) = mark(&self.mark) {
            model.set_mark(mark);
        }
        model
    }

    /// Whether `id` is an enabled action or alternate in this snapshot.
    pub fn offers(&self, target: &str) -> bool {
        fn offered(items: &[WireItem], target: &str) -> bool {
            items.iter().any(|item| match item {
                WireItem::Action { id, enabled, alternate, .. } => {
                    *enabled
                        && (id == target
                            || alternate.as_ref().is_some_and(|alternate| alternate.id == target))
                }
                WireItem::Submenu { items, .. } => offered(items, target),
                _ => false,
            })
        }
        offered(&self.items, target)
    }
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
        let label_ok = match item {
            WireItem::Header { label } => safe_text(label, 48),
            WireItem::Status { symbol, label, detail } => {
                symbol_of(symbol, |family| family == SymbolFamily::Status)?;
                safe_text(label, 48) && optional_text(detail, 80)
            }
            WireItem::Label { label, subtitle } => {
                safe_text(label, 256) && optional_text(subtitle, 80)
            }
            WireItem::Action {
                id,
                label,
                state: item_state,
                symbol,
                subtitle,
                badge,
                tooltip,
                shortcut,
                alternate,
                role: item_role,
                opens: item_opens,
                ..
            } => {
                if !valid_action_id(id) || !reserved_id_allowed(id) || !ids.insert(id.clone()) {
                    return Err(ProtocolError("invalid-action"));
                }
                if item_state.as_deref().is_some_and(|value| state(value).is_none()) {
                    return Err(ProtocolError("invalid-state"));
                }
                item_symbol(symbol)?;
                if badge.as_deref().is_some_and(|value| !safe_text(value, 4)) {
                    return Err(ProtocolError("invalid-badge"));
                }
                if shortcut.as_deref().is_some_and(|value| !safe_text(value, 64)) {
                    return Err(ProtocolError("invalid-shortcut"));
                }
                if let Some(alternate) = alternate {
                    if !valid_action_id(&alternate.id)
                        || alternate.id.starts_with("foundation.")
                        || !ids.insert(alternate.id.clone())
                        || !safe_text(&alternate.label, 256)
                    {
                        return Err(ProtocolError("invalid-alternate"));
                    }
                    item_symbol(&alternate.symbol)?;
                }
                if item_role.as_deref().is_some_and(|value| role(value).is_none()) {
                    return Err(ProtocolError("invalid-role"));
                }
                if item_opens.as_deref().is_some_and(|value| opens(value).is_none()) {
                    return Err(ProtocolError("invalid-opens"));
                }
                safe_text(label, 256) && optional_text(subtitle, 80) && optional_text(tooltip, 160)
            }
            WireItem::Submenu { label, symbol, items } => {
                item_symbol(symbol)?;
                walk(items, depth + 1, count, ids)?;
                safe_text(label, 256)
            }
            WireItem::Quit { label } => safe_text(label, 256),
            WireItem::Separator => true,
        };
        if !label_ok {
            return Err(ProtocolError("invalid-label"));
        }
    }
    Ok(())
}

/// Converts validated wire items. Callers validate first; anything that
/// fails to decode here is dropped rather than rendered.
fn nodes(items: &[WireItem], revision: u64) -> Vec<MenuNode> {
    let route = |id: &str| format!("{revision}:{id}");
    items
        .iter()
        .filter_map(|item| {
            Some(match item {
                WireItem::Header { label } => MenuNode::header(label),
                WireItem::Status { symbol, label, detail } => {
                    MenuNode::status(Symbol::from_name(symbol)?, label, detail.clone())
                }
                WireItem::Label { label, subtitle } => {
                    let mut row = MenuItem::inert(label);
                    row.subtitle = subtitle.clone();
                    MenuNode::interactive(row)
                }
                WireItem::Action {
                    id,
                    label,
                    enabled,
                    state: item_state,
                    symbol,
                    subtitle,
                    badge,
                    tooltip,
                    shortcut,
                    alternate,
                    role: item_role,
                    opens: item_opens,
                } => {
                    let mut row = match item_state.as_deref().and_then(state) {
                        Some(value) => MenuItem::state(route(id), label, value),
                        None => MenuItem::action(route(id), label),
                    };
                    row.enabled = *enabled;
                    row.shortcut = shortcut.clone();
                    row.symbol = symbol.as_deref().and_then(Symbol::from_name);
                    row.subtitle = subtitle.clone();
                    row.badge = badge.clone();
                    row.tooltip = tooltip.clone();
                    row.role = item_role.as_deref().and_then(role);
                    row.opens = item_opens.as_deref().and_then(opens);
                    row.alternate = alternate.as_ref().map(|alternate| Alternate {
                        id: route(&alternate.id),
                        title: alternate.label.clone(),
                        symbol: alternate.symbol.as_deref().and_then(Symbol::from_name),
                    });
                    MenuNode::interactive(row)
                }
                WireItem::Separator => MenuNode::Separator,
                WireItem::Submenu { label, symbol, items } => MenuNode::Submenu {
                    title: label.clone(),
                    items: nodes(items, revision),
                    symbol: symbol.as_deref().and_then(Symbol::from_name),
                },
                WireItem::Quit { label } => MenuNode::quit(label),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_frame, Frame, Session};
    use crate::MenuItemKind;
    use std::io::Cursor;

    /// The documented example from docs/protocol-v2.md.
    const EXAMPLE: &str = r#"{"version":2,"type":"snapshot","appId":"textbutler","name":"Textbutler","revision":7,"mark":{"symbol":"mark.chat","letters":"Tb","tone":"attention","text":"3"},"tooltip":"Textbutler · 3 replies waiting","items":[{"kind":"header","label":"Textbutler"},{"kind":"status","symbol":"status.running","label":"Running","detail":"3 chats on"},{"kind":"separator"},{"kind":"action","id":"open","label":"Open dashboard","symbol":"action.open","role":"primary","opens":"browser","shortcut":"CmdOrCtrl+O"},{"kind":"separator"},{"kind":"action","id":"chat.1","label":"Mom","symbol":"item.chat","subtitle":"Reply waiting for approval","badge":"2","alternate":{"id":"chat.1.copy","label":"Copy chat ID","symbol":"action.copy"}},{"kind":"action","id":"chats.all","label":"Show all chats (23)","opens":"browser"},{"kind":"separator"},{"kind":"action","id":"pause","label":"Pause automatic replies","symbol":"action.pause","state":"off","shortcut":"CmdOrCtrl+P"},{"kind":"action","id":"foundation.login","label":"Open at login","state":"on"},{"kind":"separator"},{"kind":"action","id":"help","label":"Help & support","symbol":"action.support","opens":"browser","alternate":{"id":"help.diagnostics","label":"Copy diagnostics","symbol":"action.copy"}},{"kind":"quit","label":"Quit Textbutler"}]}"#;

    fn parse(json: &str) -> Result<Frame, ProtocolError> {
        read_frame(&mut Cursor::new(format!("{json}\n"))).map(Option::unwrap)
    }

    fn accept(json: &str) -> Result<bool, ProtocolError> {
        Session::default().accept(parse(json)?)
    }

    fn with(field: &str, value: &str) -> String {
        // Replaces the first `"field":<value>` in the example.
        let needle = format!("\"{field}\":");
        let start = EXAMPLE.find(&needle).unwrap() + needle.len();
        let rest = &EXAMPLE[start..];
        let end = if let Some(quoted) = rest.strip_prefix('"') {
            quoted.find('"').unwrap() + 2
        } else {
            rest.find([',', '}']).unwrap()
        };
        format!("{}{}{}", &EXAMPLE[..start], value, &rest[end..])
    }

    #[test]
    fn the_documented_example_is_accepted() {
        let doc = include_str!("../docs/protocol-v2.md");
        assert!(doc.contains(EXAMPLE), "the doc example changed; update this test");
        assert_eq!(accept(EXAMPLE), Ok(true));
    }

    #[test]
    fn example_builds_the_rich_model() {
        let mut session = Session::default();
        session.accept(parse(EXAMPLE).unwrap()).unwrap();
        let model = session.latest().unwrap().model();
        let mark = model.status_mark.as_ref().unwrap();
        assert_eq!(mark.symbol, Symbol::MarkChat);
        assert_eq!(mark.tone, MarkTone::Attention);
        assert_eq!(mark.text.as_deref(), Some("3"));
        assert_eq!(model.title, None);
        assert_eq!(model.tooltip.as_deref(), Some("Textbutler · 3 replies waiting"));
        assert_eq!(model.nodes[0], MenuNode::header("Textbutler"));
        assert_eq!(
            model.nodes[1],
            MenuNode::status(Symbol::StatusRunning, "Running", Some("3 chats on".into()))
        );
        let MenuNode::Interactive { item } = &model.nodes[5] else { panic!("chat row") };
        assert_eq!(item.id.as_deref(), Some("7:chat.1"));
        assert_eq!(item.symbol, Some(Symbol::ItemChat));
        assert_eq!(item.badge.as_deref(), Some("2"));
        assert_eq!(item.alternate.as_ref().unwrap().id, "7:chat.1.copy");
        let MenuNode::Interactive { item } = &model.nodes[9] else { panic!("login row") };
        assert_eq!(item.kind, MenuItemKind::State { state: ItemState::On });
        assert!(model.validate().is_ok());
        // 13 rows plus two Option-key alternates.
        assert_eq!(model.node_count(), 15);
    }

    #[test]
    fn alternates_and_reserved_actions_route_like_actions() {
        let mut session = Session::default();
        session.accept(parse(EXAMPLE).unwrap()).unwrap();
        let snapshot = session.latest().unwrap();
        for id in ["chat.1.copy", "help.diagnostics", "foundation.login", "open"] {
            let event = snapshot.action(&format!("7:{id}")).unwrap();
            assert_eq!(
                serde_json::to_string(&event).unwrap(),
                format!("{{\"type\":\"action\",\"version\":2,\"id\":\"{id}\",\"revision\":7}}")
            );
        }
        assert!(snapshot.action("6:open").is_none());
        assert!(snapshot.action("7:nothing").is_none());
    }

    #[test]
    fn v1_fields_are_rejected_in_v2() {
        let titled = EXAMPLE.replace("\"revision\":7,", "\"revision\":7,\"title\":\"Tb\",");
        assert_eq!(accept(&titled), Err(ProtocolError("invalid-frame")));
        let checked = EXAMPLE.replace("\"state\":\"off\"", "\"checked\":false");
        assert_eq!(accept(&checked), Err(ProtocolError("invalid-frame")));
        let no_mark = EXAMPLE.replace(
            "\"mark\":{\"symbol\":\"mark.chat\",\"letters\":\"Tb\",\"tone\":\"attention\",\"text\":\"3\"},",
            "",
        );
        assert_eq!(accept(&no_mark), Err(ProtocolError("invalid-frame")));
    }

    #[test]
    fn each_bad_field_has_its_own_code() {
        let cases = [
            (with("symbol", "\"mark.ghost\""), "invalid-symbol"),
            (with("symbol", "\"status.ok\""), "invalid-symbol"),
            (with("letters", "\"Tbx\""), "invalid-mark"),
            (with("letters", "\"é\""), "invalid-mark"),
            (with("tone", "\"loud\""), "invalid-mark"),
            (with("text", "\"12345\""), "invalid-mark"),
            (EXAMPLE.replace("\"tone\":\"attention\",", ""), "invalid-mark"),
            (EXAMPLE.replace("\"status.running\"", "\"action.open\""), "invalid-symbol"),
            (EXAMPLE.replace("\"item.chat\"", "\"status.ok\""), "invalid-symbol"),
            (EXAMPLE.replace("\"state\":\"off\"", "\"state\":\"maybe\""), "invalid-state"),
            (EXAMPLE.replace("\"badge\":\"2\"", "\"badge\":\"12345\""), "invalid-badge"),
            (EXAMPLE.replace("\"role\":\"primary\"", "\"role\":\"main\""), "invalid-role"),
            (EXAMPLE.replace("\"opens\":\"browser\",\"shortcut\"", "\"opens\":\"web\",\"shortcut\""), "invalid-opens"),
            (EXAMPLE.replace("\"chat.1.copy\"", "\"open\""), "invalid-alternate"),
            (EXAMPLE.replace("\"chat.1.copy\"", "\"foundation.login.copy\""), "invalid-alternate"),
            (EXAMPLE.replace("\"id\":\"open\"", "\"id\":\"foundation.quit\""), "invalid-action"),
            (EXAMPLE.replace("\"id\":\"open\"", "\"id\":\"foundation.settings.keychain\""), "invalid-action"),
            (EXAMPLE.replace("\"label\":\"Running\"", &format!("\"label\":\"{}\"", "x".repeat(49))), "invalid-label"),
            (EXAMPLE.replace("\"3 chats on\"", &format!("\"{}\"", "x".repeat(81))), "invalid-label"),
            (EXAMPLE.replace("\"Textbutler · 3 replies waiting\"", &format!("\"{}\"", "x".repeat(161))), "invalid-snapshot"),
        ];
        for (json, code) in cases {
            assert_eq!(accept(&json), Err(ProtocolError(code)), "{json}");
        }
    }

    #[test]
    fn settings_actions_accept_only_kinds_with_a_pane() {
        for kind in SETTINGS_KINDS {
            let json = EXAMPLE.replace("\"id\":\"open\"", &format!("\"id\":\"foundation.settings.{kind}\""));
            assert_eq!(accept(&json), Ok(true), "{kind}");
        }
        for kind in ["keychain", "developer-tools", "", "made-up"] {
            let json = EXAMPLE.replace("\"id\":\"open\"", &format!("\"id\":\"foundation.settings.{kind}\""));
            assert_eq!(accept(&json), Err(ProtocolError("invalid-action")), "{kind}");
        }
    }

    #[test]
    fn template_icons_decode_exact_alpha_coverage() {
        let icon = |w: u32, h: u32, bytes: usize| {
            let alpha = crate::protocol::tests_support::base64(&vec![200u8; bytes]);
            EXAMPLE.replace(
                "\"letters\":\"Tb\"",
                &format!("\"letters\":\"Tb\",\"templateIcon\":{{\"width\":{w},\"height\":{h},\"alpha\":\"{alpha}\"}}"),
            )
        };
        assert_eq!(accept(&icon(2, 3, 6)), Ok(true));
        let mut session = Session::default();
        session.accept(parse(&icon(2, 3, 6)).unwrap()).unwrap();
        let mark = session.latest().unwrap().model().status_mark.unwrap();
        assert_eq!(mark.template_icon.unwrap().alpha, vec![200u8; 6]);
        for (w, h, bytes) in [(2, 3, 5), (2, 3, 24), (0, 1, 0), (65, 1, 65)] {
            assert_eq!(accept(&icon(w, h, bytes)), Err(ProtocolError("invalid-mark")), "{w}x{h}");
        }
    }

    #[test]
    fn versions_are_fixed_for_the_session() {
        let mut session = Session::default();
        session.accept(parse(EXAMPLE).unwrap()).unwrap();
        let v1 = r#"{"version":1,"type":"snapshot","appId":"textbutler","name":"Textbutler","title":"Tb","revision":8,"items":[]}"#;
        assert_eq!(session.accept(parse(v1).unwrap()), Err(ProtocolError("version-changed")));
        assert_eq!(
            session.accept(parse(r#"{"version":1,"type":"quit"}"#).unwrap()),
            Err(ProtocolError("version-changed"))
        );
        assert_eq!(session.accept(parse(r#"{"version":2,"type":"quit"}"#).unwrap()), Ok(false));
        assert_eq!(session.version(), Some(2));
        let mut v1_session = Session::default();
        v1_session.accept(parse(v1).unwrap()).unwrap();
        let next = EXAMPLE.replace("\"revision\":7", "\"revision\":9");
        assert_eq!(v1_session.accept(parse(&next).unwrap()), Err(ProtocolError("version-changed")));
        assert_eq!(
            parse(&EXAMPLE.replace("\"version\":2", "\"version\":3")).err(),
            Some(ProtocolError("unsupported-version"))
        );
    }

    #[test]
    fn limits_and_depth_are_unchanged_from_v1() {
        let row = r#"{"kind":"label","label":"Row"}"#;
        let many = format!("[{}]", vec![row; MAX_ITEMS + 1].join(","));
        let json = format!(
            r#"{{"version":2,"type":"snapshot","appId":"a.b","name":"A","revision":1,"mark":{{"symbol":"mark.agent","letters":"A"}},"items":{many}}}"#
        );
        assert_eq!(accept(&json), Err(ProtocolError("menu-too-large")));
        let mut nested = row.to_owned();
        for _ in 0..MAX_DEPTH {
            nested = format!(r#"{{"kind":"submenu","label":"More","items":[{nested}]}}"#);
        }
        let json = format!(
            r#"{{"version":2,"type":"snapshot","appId":"a.b","name":"A","revision":1,"mark":{{"symbol":"mark.agent","letters":"A"}},"items":[{nested}]}}"#
        );
        assert_eq!(accept(&json), Err(ProtocolError("menu-too-deep")));
    }
}
