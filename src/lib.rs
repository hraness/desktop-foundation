//! Product-neutral menu-bar foundation for Hraness CLI products.
//!
//! A product supplies a [`Host`] that renders a [`MenuModel`] snapshot and
//! answers action ids. The foundation owns the accessory-mode application
//! lifecycle, the status item, menu construction, and the refresh loop. The
//! host is a disposable client of the product's own daemon; it holds no
//! privilege of its own.
//!
//! The host binary runs unbundled: `cargo build` output is the artifact the
//! product CLI spawns. Packaging into a `.app` stays an optional later gate
//! for products that need TCC-bound surfaces.

use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

/// The action id that quits the application. Hosts must never see it.
pub const QUIT_ACTION_ID: &str = "foundation.quit";

const TRAY_ID: &str = "main";

/// One menu node. Items with no `id` are inert labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    Item { id: Option<String>, title: String, enabled: bool },
    Separator,
    Submenu { title: String, items: Vec<MenuNode> },
}

impl MenuNode {
    pub fn item(id: impl Into<String>, title: impl Into<String>) -> Self {
        MenuNode::Item { id: Some(id.into()), title: title.into(), enabled: true }
    }

    pub fn disabled(title: impl Into<String>) -> Self {
        MenuNode::Item { id: None, title: title.into(), enabled: false }
    }

    pub fn quit(title: impl Into<String>) -> Self {
        MenuNode::item(QUIT_ACTION_ID, title)
    }
}

/// A full-color status-item icon. `rgba` is straight (non-premultiplied)
/// RGBA8, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One complete rendered state of the status item and its menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuModel {
    /// Status-item text. Present alongside or instead of the icon.
    pub title: Option<String>,
    pub icon: Option<RgbaIcon>,
    pub tooltip: Option<String>,
    pub nodes: Vec<MenuNode>,
}

/// The product's UI authority. `snapshot` may perform bounded local IO; it is
/// always called off the UI thread. `dispatch` must not block — the host owns
/// whatever thread does the work.
pub trait Host: Send + Sync + 'static {
    fn snapshot(&self) -> MenuModel;
    fn dispatch(&self, _id: &str) {}
}

/// How the foundation drives one application.
#[derive(Debug, Clone)]
pub struct Options {
    /// Menu and status-item refresh interval.
    pub refresh: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options { refresh: Duration::from_secs(10) }
    }
}

fn build_items(
    handle: &AppHandle,
    nodes: &[MenuNode],
    inert: &mut usize,
) -> tauri::Result<Vec<Box<dyn IsMenuItem<tauri::Wry>>>> {
    let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            MenuNode::Separator => {
                items.push(Box::new(PredefinedMenuItem::separator(handle)?));
            }
            MenuNode::Item { id, title, enabled } => {
                let item_id = match id {
                    Some(id) => id.clone(),
                    None => {
                        *inert += 1;
                        format!("foundation.inert.{}", *inert)
                    }
                };
                items.push(Box::new(MenuItem::with_id(handle, item_id, title, *enabled, None::<&str>)?));
            }
            MenuNode::Submenu { title, items: children } => {
                let built = build_items(handle, children, inert)?;
                let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
                    built.iter().map(|item| item.as_ref() as &dyn IsMenuItem<tauri::Wry>).collect();
                items.push(Box::new(Submenu::with_items(handle, title, true, &refs)?));
            }
        }
    }
    Ok(items)
}

fn build_menu(handle: &AppHandle, nodes: &[MenuNode]) -> tauri::Result<Menu<tauri::Wry>> {
    let mut inert = 0usize;
    let items = build_items(handle, nodes, &mut inert)?;
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        items.iter().map(|item| item.as_ref() as &dyn IsMenuItem<tauri::Wry>).collect();
    Menu::with_items(handle, &refs)
}

fn apply_model(handle: &AppHandle, model: &MenuModel) {
    let Some(tray) = handle.tray_by_id(TRAY_ID) else { return };
    if let Ok(menu) = build_menu(handle, &model.nodes) {
        let _ = tray.set_menu(Some(menu));
    }
    if let Some(title) = &model.title {
        let _ = tray.set_title(Some(title.clone()));
    }
    if let Some(tooltip) = &model.tooltip {
        let _ = tray.set_tooltip(Some(tooltip.clone()));
    }
    if let Some(icon) = &model.icon {
        let image = tauri::image::Image::new_owned(icon.rgba.clone(), icon.width, icon.height);
        let _ = tray.set_icon(Some(image));
    }
}

fn schedule_refresh(handle: &AppHandle, host: &Arc<dyn Host>) {
    let handle = handle.clone();
    let host = host.clone();
    std::thread::spawn(move || {
        let model = host.snapshot();
        let target = handle.clone();
        let _ = handle.run_on_main_thread(move || apply_model(&target, &model));
    });
}

/// Runs the application event loop. Never returns on success.
///
/// `context` is the product's `tauri::generate_context!()` result so the
/// binary keeps its own identifier, version, and embedded assets.
pub fn run(
    context: tauri::Context,
    host: Arc<dyn Host>,
    options: Options,
) -> tauri::Result<()> {
    let dispatch_host = host.clone();
    let app = tauri::Builder::default()
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let model = host.snapshot();
            let menu = build_menu(app.handle(), &model.nodes)?;
            let mut tray = TrayIconBuilder::with_id(TRAY_ID).menu(&menu);
            if let Some(title) = &model.title {
                tray = tray.title(title.clone());
            }
            if let Some(tooltip) = &model.tooltip {
                tray = tray.tooltip(tooltip.clone());
            }
            if let Some(icon) = &model.icon {
                tray = tray.icon(tauri::image::Image::new_owned(
                    icon.rgba.clone(),
                    icon.width,
                    icon.height,
                ));
            }
            tray.build(app)?;

            let handle = app.handle().clone();
            let poll_host = host.clone();
            let refresh = options.refresh.max(Duration::from_secs(1));
            std::thread::spawn(move || loop {
                std::thread::sleep(refresh);
                let model = poll_host.snapshot();
                let target = handle.clone();
                let _ = handle.run_on_main_thread(move || apply_model(&target, &model));
            });
            Ok(())
        })
        .on_menu_event(move |app, event| {
            let id = event.id.0.as_str();
            if id == QUIT_ACTION_ID {
                app.exit(0);
                return;
            }
            if id.starts_with("foundation.inert.") {
                return;
            }
            dispatch_host.dispatch(id);
            schedule_refresh(app, &dispatch_host);
        })
        .build(context)?;
    app.run(|_, _| {});
    Ok(())
}
