//! The native decoration plan for one rendered menu.
//!
//! muda builds every row as plain text. On macOS a second pass walks the
//! built `NSMenu` in build order and applies what muda cannot express:
//! template SF Symbol images, subtitles, badges, tooltips, the mixed state,
//! section headers and Option-key alternates. This module computes that plan
//! from the model without touching AppKit, so it is tested on every platform.

use crate::symbols::{Symbol, Tint};
use crate::{compose_title, ItemState, MenuItemKind, MenuNode, TitleParts};

/// Which native APIs the running macOS offers. Each missing API falls back
/// to the text form from the down-level rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Capabilities {
    /// SF Symbol images (macOS 11 and later).
    pub symbols: bool,
    /// `NSMenuItem.subtitle` (macOS 14.4 and later).
    pub subtitle: bool,
    /// `NSMenuItemBadge` (macOS 14 and later).
    pub badge: bool,
    /// `NSMenuItem.sectionHeader(title:)` (macOS 14 and later).
    pub section_header: bool,
}

impl Capabilities {
    #[cfg(test)]
    pub const ALL: Capabilities = Capabilities {
        symbols: true,
        subtitle: true,
        badge: true,
        section_header: true,
    };
}

/// One native row, in the order muda built it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanRow {
    Separator,
    Row(RowPlan),
    Submenu { row: RowPlan, items: Vec<PlanRow> },
}

/// What the post-pass sets on one `NSMenuItem`. `None` fields are left as
/// muda built them, so v1 rows pass through untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RowPlan {
    /// The full title to set, or `None` to keep muda's.
    pub title: Option<String>,
    pub image: Option<Symbol>,
    /// Tinted (non-template) image color for status rows.
    pub tint: Option<Tint>,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub tooltip: Option<String>,
    pub mixed: bool,
    /// Replace this row with a native section header.
    pub header: Option<String>,
    /// This row is the Option-key alternate of the row before it.
    pub alternate: bool,
}

impl RowPlan {
    pub fn is_empty(&self) -> bool {
        *self == RowPlan::default()
    }
}

/// Whether the renderer builds Option-key alternates as extra native rows.
/// Only macOS can show them; elsewhere they are dropped.
pub(crate) const BUILDS_ALTERNATES: bool = cfg!(target_os = "macos");

fn shortened(value: &str, max: usize) -> String {
    let value: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if value.chars().count() <= max {
        return value;
    }
    let mut cut: String = value.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Builds the plan for `nodes`. `alternates` must match what the builder
/// emitted: one extra native row after each item that has an alternate.
pub(crate) fn plan(nodes: &[MenuNode], caps: Capabilities, alternates: bool) -> Vec<PlanRow> {
    let mut rows = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            MenuNode::Separator => rows.push(PlanRow::Separator),
            MenuNode::Item { .. } => rows.push(PlanRow::Row(RowPlan::default())),
            MenuNode::Header { title } => rows.push(PlanRow::Row(RowPlan {
                header: caps.section_header.then(|| shortened(title, 256)),
                ..RowPlan::default()
            })),
            MenuNode::Status {
                symbol,
                title,
                detail,
            } => {
                let native_image = caps.symbols;
                let native_subtitle = caps.subtitle && detail.is_some();
                rows.push(PlanRow::Row(RowPlan {
                    title: Some(compose_title(TitleParts {
                        prefix: if native_image {
                            None
                        } else {
                            symbol.fallback()
                        },
                        title,
                        subtitle: if native_subtitle {
                            None
                        } else {
                            detail.as_deref()
                        },
                        ..TitleParts::default()
                    })),
                    image: native_image.then_some(*symbol),
                    tint: native_image.then(|| symbol.tint()),
                    subtitle: if native_subtitle {
                        detail.as_deref().map(|d| shortened(d, 80))
                    } else {
                        None
                    },
                    ..RowPlan::default()
                }));
            }
            MenuNode::Interactive { item } => {
                let mixed = matches!(
                    item.kind,
                    MenuItemKind::State {
                        state: ItemState::Mixed
                    }
                );
                // A preview thumbnail from muda wins over the symbol.
                let image = item.symbol.filter(|_| caps.symbols && item.icon.is_none());
                let subtitle = item.subtitle.as_deref().filter(|_| caps.subtitle);
                let badge = item.badge.as_deref().filter(|_| caps.badge);
                let touched = image.is_some()
                    || subtitle.is_some()
                    || badge.is_some()
                    || mixed
                    || item.tooltip.is_some();
                rows.push(PlanRow::Row(if touched {
                    RowPlan {
                        // The native state shows the dash; the image replaces the
                        // fallback glyph; native subtitle and badge leave the title.
                        title: Some(compose_title(TitleParts {
                            state: None,
                            prefix: if image.is_some() {
                                None
                            } else {
                                item.symbol.and_then(Symbol::fallback)
                            },
                            title: &item.title,
                            subtitle: if subtitle.is_some() {
                                None
                            } else {
                                item.subtitle.as_deref()
                            },
                            badge: if badge.is_some() {
                                None
                            } else {
                                item.badge.as_deref()
                            },
                            progress: item.progress.map(|progress| progress.percent),
                            opens: item.opens,
                        })),
                        image,
                        subtitle: subtitle.map(|value| shortened(value, 80)),
                        badge: badge.map(|value| shortened(value, 32)),
                        tooltip: item.tooltip.as_deref().map(|value| shortened(value, 160)),
                        mixed: mixed && caps.symbols,
                        ..RowPlan::default()
                    }
                } else {
                    RowPlan::default()
                }));
                if alternates {
                    if let Some(alternate) = &item.alternate {
                        let image = alternate.symbol.filter(|_| caps.symbols);
                        rows.push(PlanRow::Row(RowPlan {
                            title: Some(compose_title(TitleParts {
                                prefix: if image.is_some() {
                                    None
                                } else {
                                    alternate.symbol.and_then(Symbol::fallback)
                                },
                                title: &alternate.title,
                                ..TitleParts::default()
                            })),
                            image,
                            alternate: true,
                            ..RowPlan::default()
                        }));
                    }
                }
            }
            MenuNode::Submenu { items, symbol, .. } => rows.push(PlanRow::Submenu {
                row: RowPlan {
                    image: symbol.filter(|_| caps.symbols),
                    ..RowPlan::default()
                },
                items: plan(items, caps, alternates),
            }),
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Alternate, MenuItem, Opens};

    fn rows(nodes: &[MenuNode], caps: Capabilities) -> Vec<RowPlan> {
        plan(nodes, caps, true)
            .into_iter()
            .map(|row| match row {
                PlanRow::Row(row) => row,
                PlanRow::Submenu { row, .. } => row,
                PlanRow::Separator => RowPlan::default(),
            })
            .collect()
    }

    fn sample() -> Vec<MenuNode> {
        vec![
            MenuNode::header("Textbutler"),
            MenuNode::status(Symbol::StatusRunning, "Running", Some("3 chats on".into())),
            MenuNode::Separator,
            MenuNode::interactive(
                MenuItem::action("open", "Open dashboard")
                    .with_symbol(Symbol::ActionOpen)
                    .opens(Opens::Browser)
                    .primary(),
            ),
            MenuNode::interactive(
                MenuItem::action("chat", "Mom")
                    .with_symbol(Symbol::ItemChat)
                    .with_subtitle("Reply waiting for approval")
                    .with_badge("2")
                    .with_tooltip("Last message 2 minutes ago")
                    .with_alternate(
                        Alternate::new("chat.copy", "Copy chat ID").with_symbol(Symbol::ActionCopy),
                    ),
            ),
            MenuNode::interactive(MenuItem::state(
                "pause",
                "Pause automatic replies",
                ItemState::Mixed,
            )),
            MenuNode::interactive(MenuItem::action("plain", "Plain v1 row")),
            MenuNode::quit("Quit Textbutler"),
        ]
    }

    #[test]
    fn modern_macos_moves_detail_into_native_fields() {
        let rows = rows(&sample(), Capabilities::ALL);
        assert_eq!(rows.len(), 9, "one extra native row for the alternate");
        assert_eq!(rows[0].header.as_deref(), Some("Textbutler"));
        assert_eq!(rows[1].title.as_deref(), Some("Running"));
        assert_eq!(rows[1].subtitle.as_deref(), Some("3 chats on"));
        assert_eq!(rows[1].image, Some(Symbol::StatusRunning));
        assert_eq!(rows[1].tint, Some(Tint::Green));
        assert_eq!(rows[3].title.as_deref(), Some("Open dashboard ↗"));
        assert_eq!(rows[3].image, Some(Symbol::ActionOpen));
        assert_eq!(rows[4].title.as_deref(), Some("Mom"));
        assert_eq!(
            rows[4].subtitle.as_deref(),
            Some("Reply waiting for approval")
        );
        assert_eq!(rows[4].badge.as_deref(), Some("2"));
        assert_eq!(
            rows[4].tooltip.as_deref(),
            Some("Last message 2 minutes ago")
        );
        assert!(rows[5].alternate);
        assert_eq!(rows[5].title.as_deref(), Some("Copy chat ID"));
        assert_eq!(rows[5].image, Some(Symbol::ActionCopy));
        assert!(rows[6].mixed);
        assert_eq!(rows[6].title.as_deref(), Some("Pause automatic replies"));
        assert!(rows[7].is_empty(), "plain rows keep muda's title");
        assert!(rows[8].is_empty());
    }

    #[test]
    fn older_macos_keeps_the_text_forms() {
        let caps = Capabilities {
            symbols: true,
            ..Capabilities::default()
        };
        let rows = rows(&sample(), caps);
        assert_eq!(rows[0].header, None);
        assert_eq!(rows[1].title.as_deref(), Some("Running · 3 chats on"));
        assert_eq!(rows[1].subtitle, None);
        assert_eq!(
            rows[4].title.as_deref(),
            Some("Mom · Reply waiting for approval  2")
        );
        assert_eq!(rows[4].badge, None);
        let bare = plan(&sample(), Capabilities::default(), true);
        let PlanRow::Row(status) = &bare[1] else {
            panic!("status row")
        };
        assert_eq!(status.title.as_deref(), Some("● Running · 3 chats on"));
        assert_eq!(status.image, None);
    }

    #[test]
    fn alternates_add_rows_only_when_built() {
        assert_eq!(plan(&sample(), Capabilities::ALL, true).len(), 9);
        assert_eq!(plan(&sample(), Capabilities::ALL, false).len(), 8);
        let nested = vec![MenuNode::Submenu {
            title: "More".into(),
            symbol: Some(Symbol::ActionSettings),
            items: sample(),
        }];
        let built = plan(&nested, Capabilities::ALL, true);
        let PlanRow::Submenu { row, items } = &built[0] else {
            panic!("submenu")
        };
        assert_eq!(row.image, Some(Symbol::ActionSettings));
        assert_eq!(items.len(), 9);
    }

    #[test]
    fn long_native_fields_are_bounded() {
        let item = MenuItem::action("x", "Row")
            .with_subtitle("s".repeat(200))
            .with_tooltip("t".repeat(400))
            .with_badge("b".repeat(40));
        let rows = rows(&[MenuNode::interactive(item)], Capabilities::ALL);
        assert_eq!(rows[0].subtitle.as_ref().unwrap().chars().count(), 80);
        assert!(rows[0].subtitle.as_ref().unwrap().ends_with('…'));
        assert_eq!(rows[0].tooltip.as_ref().unwrap().chars().count(), 160);
        assert_eq!(rows[0].badge.as_ref().unwrap().chars().count(), 32);
    }
}
