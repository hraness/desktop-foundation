//! The closed menu symbol vocabulary from `docs/protocol-v2.md`.
//!
//! The name is the API: products never send raw SF Symbol names or emoji.
//! This table maps each name to its SF Symbol, its Unicode fallback for
//! Windows, Linux and the down-level text forms, and its tint. Adding a name
//! is a reviewed change to this table, the doc table, and
//! `sdk/src/protocol-v2.ts` together.

/// Which part of the vocabulary a symbol belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolFamily {
    Status,
    Action,
    Item,
    Mark,
}

/// Status tints. Only status rows carry color; everything else is a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    None,
    Green,
    Orange,
    Red,
}

macro_rules! vocabulary {
    ($($variant:ident => $name:literal, $family:ident, $sf:literal, $fallback:expr, $tint:ident;)*) => {
        /// One name in the closed vocabulary.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Symbol { $($variant,)* }

        impl Symbol {
            /// Every symbol, in documentation order.
            pub const ALL: &'static [Symbol] = &[$(Symbol::$variant,)*];

            /// The wire name, for example `status.running`.
            pub fn name(self) -> &'static str {
                match self { $(Symbol::$variant => $name,)* }
            }

            pub fn family(self) -> SymbolFamily {
                match self { $(Symbol::$variant => SymbolFamily::$family,)* }
            }

            /// The SF Symbol drawn on macOS.
            pub fn sf_symbol(self) -> &'static str {
                match self { $(Symbol::$variant => $sf,)* }
            }

            /// The Unicode glyph used where SF Symbols are unavailable, or
            /// `None` when the item shows no glyph there.
            pub fn fallback(self) -> Option<&'static str> {
                match self { $(Symbol::$variant => $fallback,)* }
            }

            pub fn tint(self) -> Tint {
                match self { $(Symbol::$variant => Tint::$tint,)* }
            }

            /// Parses a wire name. Unknown names return `None`.
            pub fn from_name(name: &str) -> Option<Symbol> {
                match name { $($name => Some(Symbol::$variant),)* _ => None }
            }
        }
    };
}

vocabulary! {
    StatusOk => "status.ok", Status, "checkmark.circle.fill", Some("✓"), Green;
    StatusRunning => "status.running", Status, "circle.fill", Some("●"), Green;
    StatusIdle => "status.idle", Status, "circle", Some("○"), None;
    StatusPartial => "status.partial", Status, "circle.lefthalf.filled", Some("◐"), None;
    StatusSyncing => "status.syncing", Status, "arrow.triangle.2.circlepath", Some("↻"), None;
    StatusPaused => "status.paused", Status, "pause.circle", Some("⏸\u{fe0e}"), None;
    StatusAttention => "status.attention", Status, "exclamationmark.triangle.fill", Some("⚠\u{fe0e}"), Orange;
    StatusError => "status.error", Status, "xmark.octagon.fill", Some("✕"), Red;
    StatusOffline => "status.offline", Status, "circle.slash", Some("⊘"), None;
    StatusSignedOut => "status.signedOut", Status, "person.crop.circle.badge.questionmark", Some("?"), None;
    StatusLocked => "status.locked", Status, "lock.fill", Some("🔒\u{fe0e}"), None;
    ActionOpen => "action.open", Action, "arrow.up.forward.app", Some("↗"), None;
    ActionAdd => "action.add", Action, "plus.circle", Some("+"), None;
    ActionPause => "action.pause", Action, "pause.fill", Some("⏸\u{fe0e}"), None;
    ActionResume => "action.resume", Action, "play.fill", Some("▶\u{fe0e}"), None;
    ActionRefresh => "action.refresh", Action, "arrow.clockwise", Some("↻"), None;
    ActionFolder => "action.folder", Action, "folder", None, None;
    ActionCopy => "action.copy", Action, "doc.on.doc", None, None;
    ActionSettings => "action.settings", Action, "gearshape", Some("⚙\u{fe0e}"), None;
    ActionPermission => "action.permission", Action, "hand.raised", None, None;
    ActionSignIn => "action.signIn", Action, "person.crop.circle", None, None;
    ActionSignOut => "action.signOut", Action, "rectangle.portrait.and.arrow.right", None, None;
    ActionUpdate => "action.update", Action, "arrow.down.circle", Some("⤓"), None;
    ActionHelp => "action.help", Action, "questionmark.circle", None, None;
    ActionSupport => "action.support", Action, "heart", Some("♡"), None;
    ItemFile => "item.file", Item, "doc", None, None;
    ItemImage => "item.image", Item, "photo", None, None;
    ItemChat => "item.chat", Item, "bubble.left", None, None;
    ItemContact => "item.contact", Item, "person", None, None;
    ItemRoom => "item.room", Item, "person.3", None, None;
    ItemJob => "item.job", Item, "clock.arrow.circlepath", None, None;
    ItemCamera => "item.camera", Item, "camera", None, None;
    ItemChart => "item.chart", Item, "chart.bar", None, None;
    ItemAgent => "item.agent", Item, "sparkles", Some("✦"), None;
    ItemApproval => "item.approval", Item, "checkmark.seal", None, None;
    ItemKey => "item.key", Item, "key", None, None;
    MarkChat => "mark.chat", Mark, "bubble.left.and.bubble.right", None, None;
    MarkMasks => "mark.masks", Mark, "theatermasks", None, None;
    MarkDrop => "mark.drop", Mark, "drop", None, None;
    MarkDropHalf => "mark.dropHalf", Mark, "drop.halffull", None, None;
    MarkPeople => "mark.people", Mark, "person.2", None, None;
    MarkChart => "mark.chart", Mark, "chart.bar.xaxis", None, None;
    MarkCamera => "mark.camera", Mark, "camera.aperture", None, None;
    MarkShield => "mark.shield", Mark, "shield.lefthalf.filled", None, None;
    MarkAgent => "mark.agent", Mark, "sparkles", None, None;
}

impl Symbol {
    /// Symbols allowed on action rows, alternates and submenu parents.
    pub fn is_item_symbol(self) -> bool {
        matches!(self.family(), SymbolFamily::Action | SymbolFamily::Item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_names(prefix: &str) -> Vec<String> {
        let doc = include_str!("../docs/protocol-v2.md").replace("\r\n", "\n");
        let vocabulary = doc
            .split("\n## Symbol vocabulary\n")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("vocabulary section");
        vocabulary
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|line| line.split('`').next())
            .filter(|name| name.starts_with(prefix))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn vocabulary_matches_the_protocol_doc_tables() {
        for (prefix, family) in [
            ("status.", SymbolFamily::Status),
            ("action.", SymbolFamily::Action),
            ("item.", SymbolFamily::Item),
            ("mark.", SymbolFamily::Mark),
        ] {
            let ours: Vec<_> = Symbol::ALL
                .iter()
                .filter(|symbol| symbol.family() == family)
                .map(|symbol| symbol.name().to_owned())
                .collect();
            assert_eq!(ours, doc_names(prefix), "{prefix}");
        }
    }

    #[test]
    fn sf_symbols_and_fallbacks_match_the_doc_rows() {
        let doc = include_str!("../docs/protocol-v2.md").replace("\r\n", "\n");
        for symbol in Symbol::ALL {
            let row = doc
                .lines()
                .find(|line| line.starts_with(&format!("| `{}` |", symbol.name())))
                .expect("documented row");
            let cells: Vec<_> = row.split('|').map(str::trim).collect();
            assert_eq!(cells[2], format!("`{}`", symbol.sf_symbol()), "{}", symbol.name());
            if symbol.family() != SymbolFamily::Mark {
                assert_eq!(cells[3], symbol.fallback().unwrap_or("-"), "{}", symbol.name());
            }
        }
    }

    #[test]
    fn names_round_trip_and_unknown_names_are_rejected() {
        for symbol in Symbol::ALL {
            assert_eq!(Symbol::from_name(symbol.name()), Some(*symbol));
        }
        for name in ["", "status", "status.OK", "sparkles", "🤖", "mark.ghost"] {
            assert_eq!(Symbol::from_name(name), None, "{name}");
        }
    }
}
