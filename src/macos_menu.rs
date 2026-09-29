//! macOS post-pass over the menu muda built, and the template status mark.
//!
//! muda cannot express SF Symbol images, subtitles, badges, tooltips, the
//! mixed state, section headers or Option-key alternates. After muda builds
//! the menu, this pass walks the `NSMenu` in build order and applies the
//! [`crate::menu_plan`] for each row. Every API newer than macOS 13 is
//! checked with `respondsToSelector:` first, and each missing one falls back
//! to the text form the plan already composed.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, NSObjectProtocol};
use objc2::{msg_send, sel, AnyThread, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSCellImagePosition, NSColor, NSControlStateValueMixed, NSEventModifierFlags, NSImage,
    NSImageSymbolConfiguration, NSImageView, NSMenu, NSMenuItem, NSMenuItemBadge, NSStatusItem,
    NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSData, NSPoint, NSRect, NSSize, NSString};

use crate::menu_plan::{Capabilities, PlanRow, RowPlan};
use crate::symbols::{Symbol, Tint};
use crate::{MarkTone, StatusMark};

/// Tags the tone-dot view so the next render can find and replace it.
const DOT_TAG: isize = 0x6466_646f; // "dfdo"
const DOT_SIZE: f64 = 6.0;
const MARK_HEIGHT: f64 = 18.0;

/// Detects which menu APIs this macOS offers.
pub(crate) fn capabilities() -> Capabilities {
    let item = NSMenuItem::class();
    let class_responds = |class: &AnyClass, selector| class.metaclass().responds_to(selector);
    Capabilities {
        symbols: class_responds(
            NSImage::class(),
            sel!(imageWithSystemSymbolName:accessibilityDescription:),
        ),
        subtitle: item.responds_to(sel!(setSubtitle:)),
        badge: item.responds_to(sel!(setBadge:)) && AnyClass::get(c"NSMenuItemBadge").is_some(),
        section_header: class_responds(item, sel!(sectionHeaderWithTitle:)),
    }
}

fn symbol_image(symbol: Symbol, tint: Option<Tint>) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol.sf_symbol()),
        None,
    )?;
    let color = match tint {
        Some(Tint::Green) => Some(NSColor::systemGreenColor()),
        Some(Tint::Orange) => Some(NSColor::systemOrangeColor()),
        Some(Tint::Red) => Some(NSColor::systemRedColor()),
        Some(Tint::None) | None => None,
    };
    let palette_ok = NSImageSymbolConfiguration::class()
        .metaclass()
        .responds_to(sel!(configurationWithPaletteColors:));
    match color {
        Some(color) if palette_ok => {
            let config = NSImageSymbolConfiguration::configurationWithPaletteColors(
                &NSArray::from_retained_slice(&[color]),
            );
            let tinted = image.imageWithSymbolConfiguration(&config)?;
            tinted.setTemplate(false);
            Some(tinted)
        }
        _ => {
            image.setTemplate(true);
            Some(image)
        }
    }
}

fn apply_row(
    menu: &NSMenu,
    item: &NSMenuItem,
    row: &RowPlan,
    previous: Option<&NSMenuItem>,
    mtm: MainThreadMarker,
) {
    if row.is_empty() {
        return;
    }
    if let Some(title) = &row.header {
        // A section header is a distinct item type. Insert one in place and
        // hide muda's row, which muda still owns.
        let header = NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), mtm);
        let index = menu.indexOfItem(item);
        if index >= 0 {
            menu.insertItem_atIndex(&header, index);
            item.setHidden(true);
        }
        return;
    }
    if let Some(title) = &row.title {
        item.setTitle(&NSString::from_str(title));
    }
    if let Some(symbol) = row.image {
        if let Some(image) = symbol_image(symbol, row.tint) {
            item.setImage(Some(&image));
        }
    }
    if let Some(subtitle) = &row.subtitle {
        item.setSubtitle(Some(&NSString::from_str(subtitle)));
    }
    if let Some(badge) = &row.badge {
        let badge =
            NSMenuItemBadge::initWithString(NSMenuItemBadge::alloc(), &NSString::from_str(badge));
        item.setBadge(Some(&badge));
    }
    if let Some(tooltip) = &row.tooltip {
        item.setToolTip(Some(&NSString::from_str(tooltip)));
    }
    if row.mixed {
        item.setState(NSControlStateValueMixed);
    }
    if row.alternate {
        if let Some(previous) = previous {
            // Same key equivalent plus Option: AppKit swaps the pair while ⌥ is held.
            item.setKeyEquivalent(&previous.keyEquivalent());
            item.setKeyEquivalentModifierMask(
                previous.keyEquivalentModifierMask() | NSEventModifierFlags::Option,
            );
            item.setAlternate(true);
        } else {
            item.setHidden(true);
        }
    }
}

/// Applies `plan` to `menu`. The walk stops at the first structural
/// mismatch, leaving the rest as muda built it rather than decorating the
/// wrong rows.
pub(crate) fn decorate(menu: &NSMenu, plan: &[PlanRow], mtm: MainThreadMarker) {
    let items = menu.itemArray();
    if items.count() != plan.len() {
        return;
    }
    let mut previous: Option<Retained<NSMenuItem>> = None;
    for (index, row) in plan.iter().enumerate() {
        let item = items.objectAtIndex(index);
        match row {
            PlanRow::Separator => {
                if !item.isSeparatorItem() {
                    return;
                }
            }
            PlanRow::Row(row) => apply_row(menu, &item, row, previous.as_deref(), mtm),
            PlanRow::Submenu {
                row,
                items: children,
            } => {
                let Some(submenu) = item.submenu() else {
                    return;
                };
                apply_row(menu, &item, row, previous.as_deref(), mtm);
                decorate(&submenu, children, mtm);
            }
        }
        previous = Some(item);
    }
}

/// Decorates the status item's current menu.
pub(crate) fn decorate_status_menu(status: &NSStatusItem, plan: &[PlanRow], mtm: MainThreadMarker) {
    if let Some(menu) = status.menu(mtm) {
        decorate(&menu, plan, mtm);
    }
}

fn template_png(alpha: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut rgba = Vec::with_capacity(alpha.len() * 4);
    for &coverage in alpha {
        rgba.extend_from_slice(&[0, 0, 0, coverage]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

fn mark_image(mark: &StatusMark) -> Option<Retained<NSImage>> {
    let image = match &mark.template_icon {
        Some(icon) => {
            let png = template_png(&icon.alpha, icon.width, icon.height)?;
            let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))?;
            let width = MARK_HEIGHT * f64::from(icon.width) / f64::from(icon.height);
            image.setSize(NSSize::new(width, MARK_HEIGHT));
            image
        }
        None => NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(mark.symbol.sf_symbol()),
            None,
        )?,
    };
    image.setTemplate(true);
    Some(image)
}

fn remove_dot(button: &NSView) {
    for view in button.subviews().iter() {
        if view.tag() == DOT_TAG {
            view.removeFromSuperview();
        }
    }
}

/// Draws the v2 mark on the status button: a template glyph, the count as
/// the title, an orange or red dot for attention or error, and the dimmed
/// look for paused and offline. Returns `false` when no glyph could be made,
/// so the caller keeps the letters.
pub(crate) fn apply_mark(
    status: &NSStatusItem,
    mark: Option<&StatusMark>,
    accessibility: Option<&str>,
    mtm: MainThreadMarker,
) -> bool {
    let Some(button) = status.button(mtm) else {
        return false;
    };
    remove_dot(&button);
    let Some(mark) = mark else {
        button.setAppearsDisabled(false);
        return true;
    };
    let Some(image) = mark_image(mark) else {
        return false;
    };
    button.setImage(Some(&image));
    button.setImagePosition(NSCellImagePosition::ImageLeft);
    button.setTitle(&NSString::from_str(mark.text.as_deref().unwrap_or("")));
    button.setAppearsDisabled(matches!(mark.tone, MarkTone::Paused | MarkTone::Offline));
    if let Some(label) = accessibility {
        let responds: bool = button.respondsToSelector(sel!(setAccessibilityLabel:));
        if responds {
            let label = NSString::from_str(label);
            let _: () = unsafe { msg_send![&*button, setAccessibilityLabel: &*label] };
        }
    }
    let color = match mark.tone {
        MarkTone::Attention => Some(NSColor::systemOrangeColor()),
        MarkTone::Error => Some(NSColor::systemRedColor()),
        _ => None,
    };
    if let (Some(color), Some(dot)) = (color, symbol_image(Symbol::StatusRunning, None)) {
        let palette_ok = NSImageSymbolConfiguration::class()
            .metaclass()
            .responds_to(sel!(configurationWithPaletteColors:));
        let dot = if palette_ok {
            let config = NSImageSymbolConfiguration::configurationWithPaletteColors(
                &NSArray::from_retained_slice(&[color]),
            );
            dot.imageWithSymbolConfiguration(&config)
        } else {
            None
        };
        if let Some(dot) = dot {
            dot.setTemplate(false);
            let bounds = button.bounds();
            // Lower right of the glyph, which sits at the left of the button.
            let glyph = image.size();
            let inset = ((bounds.size.height - glyph.height) / 2.0).max(0.0);
            let x = (inset + glyph.width - DOT_SIZE / 2.0).min(bounds.size.width - DOT_SIZE);
            let y = if button.isFlipped() {
                bounds.size.height - inset - DOT_SIZE
            } else {
                inset
            };
            let frame = NSRect::new(
                NSPoint::new(x.max(0.0), y.max(0.0)),
                NSSize::new(DOT_SIZE, DOT_SIZE),
            );
            let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
            view.setImage(Some(&dot));
            view.setTag(DOT_TAG);
            // Below tray-icon's click target, so a click on the dot still
            // opens the menu; the target view draws nothing.
            button.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
        }
    }
    // tray-icon sizes its click target to the button after each change it
    // makes; do the same after ours.
    let frame = button.frame();
    for view in button.subviews().iter() {
        if view.tag() != DOT_TAG {
            view.setFrame(frame);
        }
    }
    true
}

/// Builds `nodes` as plain AppKit rows the way muda does (text titles, one
/// extra row per alternate), runs the post-pass, and prints one line per
/// native row. Must run on the main thread. Used by `tests/macos_menu.rs`.
#[doc(hidden)]
pub fn native_menu_dump(nodes: &[crate::MenuNode]) -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    fn build(nodes: &[crate::MenuNode], mtm: MainThreadMarker) -> Retained<NSMenu> {
        let menu = NSMenu::new(mtm);
        let plain = |title: &str| {
            let item = NSMenuItem::new(mtm);
            item.setTitle(&NSString::from_str(title));
            item
        };
        for node in nodes {
            match node {
                crate::MenuNode::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                crate::MenuNode::Item { title, .. } | crate::MenuNode::Header { title } => {
                    menu.addItem(&plain(title))
                }
                crate::MenuNode::Status { title, .. } => menu.addItem(&plain(title)),
                crate::MenuNode::Interactive { item } => {
                    let row = plain(&crate::render_item_title(item));
                    if let Some(shortcut) = &item.shortcut {
                        // Enough of muda's accelerator parsing for the test.
                        if let Some(key) = shortcut.strip_prefix("CmdOrCtrl+") {
                            row.setKeyEquivalent(&NSString::from_str(&key.to_lowercase()));
                            row.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
                        }
                    }
                    menu.addItem(&row);
                    if let Some(alternate) = &item.alternate {
                        menu.addItem(&plain(&alternate.title));
                    }
                }
                crate::MenuNode::Submenu { title, items, .. } => {
                    let row = plain(title);
                    row.setSubmenu(Some(&build(items, mtm)));
                    menu.addItem(&row);
                }
            }
        }
        menu
    }
    fn dump(menu: &NSMenu, depth: usize, out: &mut String) {
        for item in menu.itemArray().iter() {
            if item.isHidden() {
                continue;
            }
            out.push_str(&"  ".repeat(depth));
            if item.isSeparatorItem() {
                out.push_str("---\n");
                continue;
            }
            let caps = capabilities();
            if caps.section_header && item.isSectionHeader() {
                out.push_str("[header] ");
            }
            if item.isAlternate() {
                out.push_str("[⌥] ");
            }
            if item.image().is_some() {
                out.push_str("[image] ");
            }
            if item.state() == NSControlStateValueMixed {
                out.push_str("[mixed] ");
            }
            out.push_str(&item.title().to_string());
            if let Some(subtitle) = caps.subtitle.then(|| item.subtitle()).flatten() {
                out.push_str(&format!(" | subtitle: {subtitle}"));
            }
            if let Some(badge) = caps
                .badge
                .then(|| item.badge())
                .flatten()
                .and_then(|badge| badge.stringValue())
            {
                out.push_str(&format!(" | badge: {badge}"));
            }
            if let Some(tip) = item.toolTip() {
                out.push_str(&format!(" | tooltip: {tip}"));
            }
            let key = item.keyEquivalent().to_string();
            let mask = item.keyEquivalentModifierMask();
            if item.isAlternate() || !key.is_empty() {
                let option = if mask.contains(NSEventModifierFlags::Option) {
                    "⌥"
                } else {
                    ""
                };
                let command = if mask.contains(NSEventModifierFlags::Command) {
                    "⌘"
                } else {
                    ""
                };
                out.push_str(&format!(" | key: {option}{command}{key}"));
            }
            out.push('\n');
            if let Some(submenu) = item.submenu() {
                dump(&submenu, depth + 1, out);
            }
        }
    }
    let menu = build(nodes, mtm);
    let plan = crate::menu_plan::plan(nodes, capabilities(), true);
    decorate(&menu, &plan, mtm);
    let mut out = String::new();
    dump(&menu, 0, &mut out);
    Some(out)
}
