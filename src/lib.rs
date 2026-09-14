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

pub mod outputs;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem as TauriMenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// The action id that quits the application. Hosts must never see it.
pub const QUIT_ACTION_ID: &str = "foundation.quit";

/// The action id that shows and focuses the companion window. Products opt in
/// with [`Options::companion_window`] and a hidden `main` window.
pub const WINDOW_SHOW_ACTION_ID: &str = "foundation.window.show";

/// Stable categories for a snapshot that could not be obtained.
///
/// The category intentionally carries no product-specific details. Products
/// can map it to a safe, user-facing model through [`Host::snapshot_failed`]
/// without leaking socket paths, credentials, or daemon output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotErrorKind {
    Unavailable,
    TimedOut,
    Protocol,
    Permission,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError {
    pub kind: SnapshotErrorKind,
}

impl SnapshotError {
    pub const fn new(kind: SnapshotErrorKind) -> Self {
        Self { kind }
    }
}

/// Result returned by the snapshot side of the host contract.
pub type SnapshotResult = Result<MenuModel, SnapshotError>;

/// Outcome of a non-blocking menu action dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The request was accepted and will be reflected by a later snapshot.
    Accepted,
    /// The action is not available in the current capability/state.
    Rejected,
    /// The caller should retry after the next refresh or backoff interval.
    Retryable,
    /// Delivery may have happened; reconcile against a fresh snapshot.
    Indeterminate,
}

const TRAY_ID: &str = "main";
const COMPANION_WINDOW_LABEL: &str = "main";
const MAX_MENU_TITLE_CHARS: usize = 256;
const MAX_MENU_BADGE_CHARS: usize = 32;
const MAX_MENU_SHORTCUT_CHARS: usize = 64;
const MAX_ACCESSIBILITY_CHARS: usize = 256;

/// One menu node. Items with no `id` are inert labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    Item { id: Option<String>, title: String, enabled: bool, icon: Option<RgbaIcon> },
    /// An interactive item with native check semantics and richer presentation
    /// metadata. This additive variant keeps the original `Item` shape source
    /// compatible for existing product adapters.
    Interactive { item: MenuItem },
    Separator,
    Submenu { title: String, items: Vec<MenuNode> },
}

impl MenuNode {
    pub fn item(id: impl Into<String>, title: impl Into<String>) -> Self {
        MenuNode::Item { id: Some(id.into()), title: title.into(), enabled: true, icon: None }
    }

    /// A menu item with a full-color icon rendered beside the title — used for
    /// file previews such as output thumbnails.
    pub fn item_with_icon(id: impl Into<String>, title: impl Into<String>, icon: RgbaIcon) -> Self {
        MenuNode::Item { id: Some(id.into()), title: title.into(), enabled: true, icon: Some(icon) }
    }

    pub fn disabled(title: impl Into<String>) -> Self {
        MenuNode::Item { id: None, title: title.into(), enabled: false, icon: None }
    }

    /// A menu item that shows and focuses the companion window.
    pub fn show_window(title: impl Into<String>) -> Self {
        MenuNode::item(WINDOW_SHOW_ACTION_ID, title)
    }

    pub fn quit(title: impl Into<String>) -> Self {
        MenuNode::item(QUIT_ACTION_ID, title)
    }

    pub fn interactive(item: MenuItem) -> Self {
        MenuNode::Interactive { item }
    }
}

/// Native presentation semantics for an interactive menu item. Tauri exposes
/// check items but has no portable progress, badge, or radio-group primitive;
/// those values are therefore retained in the model and represented in the
/// title/accessibility metadata by the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemKind {
    Action,
    Toggle { checked: bool },
    Check { checked: bool },
    Radio { selected: bool, group: Option<String> },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessibilityMetadata {
    pub label: Option<String>,
    pub value: Option<String>,
    pub hint: Option<String>,
}

impl AccessibilityMetadata {
    pub fn bounded(&self) -> Self {
        Self {
            label: self.label.as_deref().map(|v| bounded_text(v, MAX_ACCESSIBILITY_CHARS)),
            value: self.value.as_deref().map(|v| bounded_text(v, MAX_ACCESSIBILITY_CHARS)),
            hint: self.hint.as_deref().map(|v| bounded_text(v, MAX_ACCESSIBILITY_CHARS)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressValue {
    /// Integer percentage in the inclusive range 0..=100.
    pub percent: u8,
}

impl ProgressValue {
    pub fn new(percent: u8) -> Self {
        Self { percent: percent.min(100) }
    }
}

/// Rich, stable menu item description used by product adapters. `id` is the
/// command identity and must remain stable across snapshots so actions can be
/// reconciled safely after refreshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: Option<String>,
    pub title: String,
    pub enabled: bool,
    pub icon: Option<RgbaIcon>,
    pub kind: MenuItemKind,
    pub shortcut: Option<String>,
    pub badge: Option<String>,
    pub progress: Option<ProgressValue>,
    pub accessibility: AccessibilityMetadata,
}

impl MenuItem {
    pub fn action(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()), title: title.into(), enabled: true, icon: None,
            kind: MenuItemKind::Action, shortcut: None, badge: None, progress: None,
            accessibility: AccessibilityMetadata::default(),
        }
    }

    pub fn toggle(id: impl Into<String>, title: impl Into<String>, checked: bool) -> Self {
        let mut item = Self::action(id, title);
        item.kind = MenuItemKind::Toggle { checked };
        item
    }

    pub fn check(id: impl Into<String>, title: impl Into<String>, checked: bool) -> Self {
        let mut item = Self::action(id, title);
        item.kind = MenuItemKind::Check { checked };
        item
    }

    pub fn radio(id: impl Into<String>, title: impl Into<String>, group: impl Into<String>, selected: bool) -> Self {
        let mut item = Self::action(id, title);
        item.kind = MenuItemKind::Radio { selected, group: Some(group.into()) };
        item
    }

    pub fn disabled(mut self) -> Self { self.enabled = false; self }
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self { self.shortcut = Some(shortcut.into()); self }
    pub fn with_badge(mut self, badge: impl Into<String>) -> Self { self.badge = Some(badge.into()); self }
    pub fn with_progress(mut self, percent: u8) -> Self { self.progress = Some(ProgressValue::new(percent)); self }
    pub fn with_icon(mut self, icon: RgbaIcon) -> Self { self.icon = Some(icon); self }
    pub fn with_accessibility(mut self, metadata: AccessibilityMetadata) -> Self { self.accessibility = metadata.bounded(); self }
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
    /// Fallible snapshot hook used by the refresh coordinator. Existing hosts
    /// keep their infallible implementation; new hosts can return a typed
    /// error and optionally provide a safe degraded model.
    fn snapshot_result(&self) -> SnapshotResult {
        Ok(self.snapshot())
    }
    /// Model to render after a failed snapshot. Returning `None` keeps the
    /// last known menu visible while the coordinator retries.
    fn snapshot_failed(&self, _error: &SnapshotError) -> Option<MenuModel> {
        None
    }
    /// Structured dispatch hook. The legacy `dispatch` method remains the
    /// compatibility default and is treated as accepted.
    fn dispatch_result(&self, id: &str) -> DispatchOutcome {
        self.dispatch(id);
        DispatchOutcome::Accepted
    }
    /// Runs once inside Tauri `setup`, after the status item exists. Products
    /// spawn sidecars here and may `handle.manage(...)` their own state.
    fn started(&self, _app: &AppHandle) {}
    /// Variant of [`Host::started`] that also exposes a cheap event
    /// invalidation handle. The default preserves existing hosts unchanged.
    fn started_with_refresh(&self, app: &AppHandle, _refresh: RefreshHandle) {
        self.started(app);
    }
    /// Runs once on `RunEvent::Exit`. Join owned work here.
    fn stopping(&self) {}
}

/// Cheap, cloneable handle for event-driven invalidation of the menu model.
/// Calling [`RefreshHandle::request`] coalesces with any refresh already in
/// flight; it never performs host work on the caller's thread.
#[derive(Clone)]
pub struct RefreshHandle {
    state: Arc<RefreshState>,
}

impl RefreshHandle {
    pub fn request(&self) {
        self.state.request();
    }

    pub fn stop(&self) {
        self.state.stop();
    }
}

struct RefreshState {
    generation: AtomicU64,
    requested: AtomicBool,
    stopping: AtomicBool,
    wake: Mutex<()>,
    cv: Condvar,
}

impl RefreshState {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            requested: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            wake: Mutex::new(()),
            cv: Condvar::new(),
        }
    }

    fn request(&self) {
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.requested.store(true, Ordering::Release);
        self.cv.notify_one();
    }

    fn mark_requested(&self) {
        if self.stopping.load(Ordering::Acquire) {
            return;
        }
        self.requested.store(true, Ordering::Release);
        self.cv.notify_one();
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.cv.notify_all();
    }
}

struct RefreshController {
    host: Arc<dyn Host>,
    state: Arc<RefreshState>,
    interval: Duration,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl RefreshController {
    fn new(host: Arc<dyn Host>, interval: Duration) -> Self {
        Self {
            host,
            state: Arc::new(RefreshState::new()),
            interval,
            worker: Mutex::new(None),
        }
    }

    fn handle(&self) -> RefreshHandle {
        RefreshHandle { state: self.state.clone() }
    }

    fn start(&self, app: AppHandle) {
        if self.worker.lock().expect("refresh worker lock poisoned").is_some() {
            return;
        }
        let host = self.host.clone();
        let state = self.state.clone();
        let interval = self.interval.max(Duration::from_secs(1));
        let worker = std::thread::spawn(move || {
            // Request an initial snapshot immediately after the tray exists.
            state.request();
            loop {
                let mut guard = state.wake.lock().expect("refresh wake lock poisoned");
                while !state.requested.load(Ordering::Acquire)
                    && !state.stopping.load(Ordering::Acquire)
                {
                    let (next, _) = state
                        .cv
                        .wait_timeout(guard, interval)
                        .expect("refresh wait lock poisoned");
                    guard = next;
                    if !state.requested.load(Ordering::Acquire) {
                        state.request();
                    }
                }
                state.requested.store(false, Ordering::Release);
                drop(guard);
                if state.stopping.load(Ordering::Acquire) {
                    break;
                }

                // A single worker owns snapshots. If invalidations arrive
                // during IO, discard the stale result and immediately take a
                // newer generation instead of running concurrent snapshots.
                let mut generation = state.generation.load(Ordering::Acquire);
                loop {
                    if state.stopping.load(Ordering::Acquire) {
                        return;
                    }
                    let result = host.snapshot_result();
                    let observed = state.generation.load(Ordering::Acquire);
                    if observed != generation {
                        generation = observed;
                        continue;
                    }
                    let target = app.clone();
                    let host_for_error = host.clone();
                    let state_for_ui = state.clone();
                    let _ = app.run_on_main_thread(move || {
                        if state_for_ui.stopping.load(Ordering::Acquire)
                            || state_for_ui.generation.load(Ordering::Acquire) != generation
                        {
                            state_for_ui.mark_requested();
                            return;
                        }
                        match result {
                            Ok(model) => apply_model(&target, &model),
                            Err(error) => {
                                if let Some(model) = host_for_error.snapshot_failed(&error) {
                                    apply_model(&target, &model);
                                }
                            }
                        }
                    });
                    break;
                }
            }
        });
        *self.worker.lock().expect("refresh worker lock poisoned") = Some(worker);
    }

    fn stop(&self) {
        self.state.stop();
        if let Some(worker) = self.worker.lock().expect("refresh worker lock poisoned").take() {
            // The worker is never joined from itself, but keep this guard for
            // embedders that choose to stop from a host callback.
            if worker.thread().id() != std::thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

/// How the foundation drives one application.
#[derive(Debug, Clone)]
pub struct Options {
    /// Menu and status-item refresh interval.
    pub refresh: Duration,
    /// When true, the hidden `main` window is a companion panel: closing it
    /// hides rather than destroys it, and [`WINDOW_SHOW_ACTION_ID`] menu items
    /// show and focus it.
    pub companion_window: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { refresh: Duration::from_secs(10), companion_window: false }
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
            MenuNode::Item { id, title, enabled, icon } => {
                let item_id = match id {
                    Some(id) => id.clone(),
                    None => {
                        *inert += 1;
                        format!("foundation.inert.{}", *inert)
                    }
                };
                match icon {
                    Some(icon) => {
                        let image = tauri::image::Image::new_owned(
                            icon.rgba.clone(),
                            icon.width,
                            icon.height,
                        );
                        items.push(Box::new(tauri::menu::IconMenuItem::with_id(
                            handle,
                            item_id,
                            title,
                            *enabled,
                            Some(image),
                            None::<&str>,
                        )?));
                    }
                    None => items.push(Box::new(TauriMenuItem::with_id(
                        handle,
                        item_id,
                        title,
                        *enabled,
                        None::<&str>,
                    )?)),
                }
            }
            MenuNode::Interactive { item } => {
                let item_id = match &item.id {
                    Some(id) => id.clone(),
                    None => {
                        *inert += 1;
                        format!("foundation.inert.{}", *inert)
                    }
                };
                let title = render_item_title(item);
                let shortcut = item
                    .shortcut
                    .as_deref()
                    .map(|value| bounded_text(value, MAX_MENU_SHORTCUT_CHARS));
                let accelerator = shortcut.as_deref();
                match &item.kind {
                    MenuItemKind::Toggle { checked }
                    | MenuItemKind::Check { checked }
                    | MenuItemKind::Radio { selected: checked, .. } => {
                        items.push(Box::new(CheckMenuItem::with_id(
                            handle, item_id, title, item.enabled, *checked, accelerator,
                        )?));
                    }
                    MenuItemKind::Action => {
                        match &item.icon {
                            Some(icon) => {
                                let image = tauri::image::Image::new_owned(
                                    icon.rgba.clone(), icon.width, icon.height,
                                );
                                items.push(Box::new(tauri::menu::IconMenuItem::with_id(
                                    handle, item_id, title, item.enabled, Some(image), accelerator,
                                )?));
                            }
                            None => items.push(Box::new(TauriMenuItem::with_id(
                                handle, item_id, title, item.enabled, accelerator,
                            )?)),
                        }
                    }
                }
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

fn render_item_title(item: &MenuItem) -> String {
    let mut title = bounded_text(&item.title, MAX_MENU_TITLE_CHARS);
    if let Some(badge) = &item.badge {
        title.push_str("  [");
        title.push_str(&bounded_text(badge, MAX_MENU_BADGE_CHARS));
        title.push(']');
    }
    if let Some(progress) = item.progress {
        title.push_str(&format!("  {}%", progress.percent));
    }
    bounded_text(&title, MAX_MENU_TITLE_CHARS)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
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

fn show_companion_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(COMPANION_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Runs the application event loop. Never returns on success.
///
/// `context` is the product's `tauri::generate_context!()` result so the
/// binary keeps its own identifier, version, and embedded assets. `configure`
/// applies product builder extensions — invoke handlers, managed state — and
/// defaults to the identity.
pub fn run(
    context: tauri::Context,
    host: Arc<dyn Host>,
    options: Options,
    configure: impl FnOnce(tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry>,
) -> tauri::Result<()> {
    let dispatch_host = host.clone();
    let run_host = host.clone();
    let companion = options.companion_window;
    let controller = Arc::new(RefreshController::new(host.clone(), options.refresh));
    let refresh_handle = controller.handle();
    let event_controller = controller.clone();
    let exit_controller = controller.clone();
    let app = configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Keep setup on the UI thread cheap. The coordinator takes the
            // first snapshot asynchronously and replaces this inert menu.
            let model = MenuModel {
                nodes: vec![MenuNode::disabled("Loading status…")],
                ..MenuModel::default()
            };
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
            host.started_with_refresh(app.handle(), refresh_handle.clone());
            controller.start(app.handle().clone());
            Ok(())
        })
        .on_menu_event(move |app, event| {
            let id = event.id.0.as_str();
            if id == QUIT_ACTION_ID {
                app.exit(0);
                return;
            }
            if id == WINDOW_SHOW_ACTION_ID {
                show_companion_window(app);
                return;
            }
            if id.starts_with("foundation.inert.") {
                return;
            }
            let outcome = dispatch_host.dispatch_result(id);
            if !matches!(outcome, DispatchOutcome::Rejected) {
                event_controller.handle().request();
            }
        })
        .on_window_event(move |window, event| {
            if companion
                && window.label() == COMPANION_WINDOW_LABEL
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                // Accessory-style close: hide the panel, keep the status item.
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
    .build(context)?;
    app.run(move |_app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            exit_controller.stop();
            run_host.stopping();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_requests_advance_generation_and_coalesce() {
        let state = RefreshState::new();
        assert_eq!(state.generation.load(Ordering::Acquire), 0);
        state.request();
        state.request();
        assert_eq!(state.generation.load(Ordering::Acquire), 2);
        assert!(state.requested.load(Ordering::Acquire));
        state.requested.store(false, Ordering::Release);
        state.mark_requested();
        assert_eq!(state.generation.load(Ordering::Acquire), 2);
        assert!(state.requested.load(Ordering::Acquire));
    }

    #[test]
    fn stopped_refresh_handle_ignores_late_requests() {
        let state = Arc::new(RefreshState::new());
        let handle = RefreshHandle { state: state.clone() };
        handle.stop();
        handle.request();
        assert!(state.stopping.load(Ordering::Acquire));
        assert_eq!(state.generation.load(Ordering::Acquire), 0);
        assert!(!state.requested.load(Ordering::Acquire));
    }

    #[test]
    fn snapshot_error_is_stable_and_product_neutral() {
        let error = SnapshotError::new(SnapshotErrorKind::TimedOut);
        assert_eq!(error.kind, SnapshotErrorKind::TimedOut);
        assert_eq!(format!("{error:?}"), "SnapshotError { kind: TimedOut }");
    }

    #[test]
    fn rich_items_keep_stable_identity_and_native_toggle_state() {
        let item = MenuItem::toggle("daemon.pause", "Pause daemon", true)
            .with_shortcut("CmdOrCtrl+P")
            .with_badge("3")
            .with_progress(120);
        assert_eq!(item.id.as_deref(), Some("daemon.pause"));
        assert_eq!(item.kind, MenuItemKind::Toggle { checked: true });
        assert_eq!(item.progress, Some(ProgressValue { percent: 100 }));
        assert_eq!(render_item_title(&item), "Pause daemon  [3]  100%");
        assert!(matches!(MenuNode::interactive(item), MenuNode::Interactive { .. }));
    }

    #[test]
    fn rich_item_accessibility_metadata_is_product_neutral() {
        let metadata = AccessibilityMetadata {
            label: Some("Pause Textbutler".into()),
            value: Some("On".into()),
            hint: Some("Stops background processing".into()),
        };
        let item = MenuItem::check("pause", "Pause", false).with_accessibility(metadata.clone());
        assert_eq!(item.accessibility, metadata);
    }
}
