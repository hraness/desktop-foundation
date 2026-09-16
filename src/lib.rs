//! Product-neutral menu-bar foundation for Hraness CLI products.
//!
//! A product supplies a [`Host`] that renders a [`MenuModel`] snapshot and
//! answers action ids. The foundation owns the accessory-mode application
//! lifecycle, the status item, menu construction, and the refresh loop. The
//! host is a disposable client of the product's own daemon; it holds no
//! privilege of its own.
//!
//! The host binary runs unbundled: `cargo build` output is the artifact the
//! product CLI spawns. The shared renderer uses no application bundle or window;
//! product helpers retain their own OS permission boundaries.

pub mod outputs;
pub mod prompt;
pub mod protocol;

use std::collections::{HashMap, HashSet};
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
const MAX_MENU_NODES: usize = 256;
const MAX_MENU_DEPTH: usize = 8;
const MAX_ACTION_ID_BYTES: usize = 1024;
const MAX_ICON_DIMENSION: u32 = 512;

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

/// Presentation semantics for an interactive menu item. Toggle/check/radio
/// all use native checkmarks. Radio groups are validated for at most one
/// selection, but the host must make the selection and supply a new snapshot.
/// The renderer restores the authoritative checkmark when an item is clicked;
/// it never treats the platform's automatic toggle as daemon confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemKind {
    Action,
    Toggle { checked: bool },
    Check { checked: bool },
    Radio { selected: bool, group: Option<String> },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Optional information for future richer renderers. Tauri's portable native
/// menu API cannot set separate accessibility labels, values, or hints; these
/// fields are currently retained only. Native accessibility uses the visible
/// title and checkmark. Put essential state in those fields today.
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
    /// Rendered for actions. Native check items cannot also display this icon.
    pub icon: Option<RgbaIcon>,
    pub kind: MenuItemKind,
    pub shortcut: Option<String>,
    /// Bounded text appended to the title, not a separate native badge.
    pub badge: Option<String>,
    /// Percentage appended to the title, not a native progress control.
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
    /// `None` clears a previously rendered title.
    pub title: Option<String>,
    /// `None` clears a previously rendered icon.
    pub icon: Option<RgbaIcon>,
    /// `None` clears a previously rendered tooltip.
    pub tooltip: Option<String>,
    pub nodes: Vec<MenuNode>,
}

/// A safe category; model errors never include product labels or command IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    TooManyNodes,
    TooDeep,
    InvalidActionId,
    InvalidIcon,
    AmbiguousRadioGroup,
}

/// Safe rendering failure categories reported through [`Host::render_failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    Model(ModelError),
    Native,
}

/// The bounded operation that failed, without native error text or user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOperation {
    Validate,
    Schedule,
    LookupTray,
    BuildMenu,
    SetMenu,
    SetTitle,
    SetTooltip,
    SetIcon,
    SetChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderFailure {
    error: RenderError,
    operation: RenderOperation,
}

impl RenderFailure {
    fn native(operation: RenderOperation) -> Self {
        Self { error: RenderError::Native, operation }
    }
}

impl MenuModel {
    /// Validate bounded native work and unambiguous selection state. Multiple
    /// menu rows may intentionally dispatch the same stable command ID.
    /// Invalid snapshots leave the last rendered menu and its routes intact.
    pub fn validate(&self) -> Result<(), ModelError> {
        fn icon_valid(icon: &RgbaIcon) -> bool {
            icon.width > 0 && icon.height > 0
                && icon.width <= MAX_ICON_DIMENSION && icon.height <= MAX_ICON_DIMENSION
                && u64::from(icon.width) * u64::from(icon.height) * 4 == icon.rgba.len() as u64
        }
        fn walk(
            nodes: &[MenuNode], depth: usize, count: &mut usize,
            selected_groups: &mut HashSet<String>,
        ) -> Result<(), ModelError> {
            if depth > MAX_MENU_DEPTH { return Err(ModelError::TooDeep); }
            for node in nodes {
                *count += 1;
                if *count > MAX_MENU_NODES { return Err(ModelError::TooManyNodes); }
                let (id, icon) = match node {
                    MenuNode::Item { id, icon, .. } => (id, icon),
                    MenuNode::Interactive { item } => {
                        if let MenuItemKind::Radio { selected: true, group: Some(group) } = &item.kind {
                            if !selected_groups.insert(group.clone()) {
                                return Err(ModelError::AmbiguousRadioGroup);
                            }
                        }
                        (&item.id, &item.icon)
                    }
                    MenuNode::Submenu { items, .. } => {
                        walk(items, depth + 1, count, selected_groups)?;
                        continue;
                    }
                    MenuNode::Separator => continue,
                };
                if let Some(id) = id {
                    if id.is_empty() || id.len() > MAX_ACTION_ID_BYTES || id.chars().any(char::is_control) {
                        return Err(ModelError::InvalidActionId);
                    }
                }
                if icon.as_ref().is_some_and(|icon| !icon_valid(icon)) {
                    return Err(ModelError::InvalidIcon);
                }
            }
            Ok(())
        }
        if self.icon.as_ref().is_some_and(|icon| !icon_valid(icon)) {
            return Err(ModelError::InvalidIcon);
        }
        walk(&self.nodes, 0, &mut 0, &mut HashSet::new())
    }
}

/// The product's UI authority. `snapshot` may perform bounded local IO; it is
/// always called off the UI thread. `dispatch` must not block — the host owns
/// whatever thread does the work.
pub trait Host: Send + Sync + 'static {
    fn snapshot(&self) -> MenuModel;
    fn dispatch(&self, _id: &str) {}
    /// Optional private directory for Linux AppIndicator's generated icon
    /// files. Supply this before tray creation to isolate different products
    /// that use the same internal tray ID. Hosts own path validation and must
    /// retain the directory for the lifetime of the companion. Other platforms
    /// ignore the directory. The default preserves existing Rust consumers.
    fn tray_icon_directory(&self) -> Option<std::path::PathBuf> { None }
    /// Fallible snapshot hook used by the refresh coordinator. Existing hosts
    /// keep their infallible implementation; new hosts can return a typed
    /// error and optionally provide a safe degraded model.
    fn snapshot_result(&self) -> SnapshotResult {
        Ok(self.snapshot())
    }
    /// Cooperative cancellation for long reads. Stop work when the context is
    /// cancelled; all host IO must still have its own finite deadline. Legacy
    /// hosts keep their bounded `snapshot_result` implementation.
    fn snapshot_with_context(&self, _context: &SnapshotContext) -> SnapshotResult {
        self.snapshot_result()
    }
    /// Model to render after a failed snapshot. Returning `None` keeps the
    /// last known menu visible while the coordinator retries. This hook runs
    /// on the snapshot worker, never on the UI thread.
    fn snapshot_failed(&self, _error: &SnapshotError) -> Option<MenuModel> {
        None
    }
    /// Reports safe rendering failure categories on the snapshot worker.
    /// Keep this bounded; repeated native failures retry at the normal interval.
    fn render_failed(&self, _error: RenderError) {}
    /// Reports the failed operation while preserving legacy host behavior.
    /// Like [`Host::render_failed`], this runs on the snapshot worker.
    fn render_failed_at(&self, error: RenderError, _operation: RenderOperation) {
        self.render_failed(error);
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
    /// Runs on the UI thread only after a complete host model is applied.
    /// This excludes the loading placeholder and failed or cancelled renders.
    /// Keep this non-blocking; it is distinct from event-loop startup.
    fn model_rendered(&self, _app: &AppHandle) {}
    /// Non-blocking shutdown notification, before joining the snapshot worker.
    /// Hosts can cancel a socket read here. Legacy bounded reads are joined to
    /// completion; the foundation cannot forcibly cancel arbitrary host IO.
    fn cancel_snapshot(&self) {}
    /// Runs once after the snapshot worker stops on `RunEvent::Exit`. Join
    /// other owned work here. Do not synchronously request UI work on shutdown.
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

/// A snapshot's cooperative cancellation token. Invalidation and application
/// shutdown both cancel it. No host should wait synchronously for UI work.
#[derive(Clone)]
pub struct SnapshotContext {
    state: Arc<RefreshState>,
    generation: u64,
}

impl SnapshotContext {
    pub fn is_cancelled(&self) -> bool {
        !self.state.is_current(self.generation)
    }
}

#[derive(Default)]
struct PendingRefresh {
    generation: u64,
    requested: bool,
    stopping: bool,
    render_error: Option<RenderFailure>,
}

struct RefreshState {
    pending: Mutex<PendingRefresh>,
    cv: Condvar,
}

impl RefreshState {
    fn new() -> Self {
        Self { pending: Mutex::new(PendingRefresh::default()), cv: Condvar::new() }
    }

    fn request(&self) {
        let mut pending = self.pending.lock().expect("refresh state lock poisoned");
        if pending.stopping { return; }
        pending.generation = pending.generation.wrapping_add(1);
        pending.requested = true;
        self.cv.notify_one();
    }

    fn stop(&self) {
        let mut pending = self.pending.lock().expect("refresh state lock poisoned");
        pending.stopping = true;
        self.cv.notify_all();
    }

    fn is_current(&self, generation: u64) -> bool {
        let pending = self.pending.lock().expect("refresh state lock poisoned");
        !pending.stopping && pending.generation == generation
    }

    fn report_render_error(&self, error: RenderFailure) {
        // Wake the worker to report the error, without requesting a new render.
        // A persistent failure must not cause an immediate snapshot/retry loop.
        self.pending.lock().expect("refresh state lock poisoned").render_error = Some(error);
        self.cv.notify_one();
    }

    fn next(&self, interval: Duration) -> Option<(u64, Option<RenderFailure>)> {
        let pending = self.pending.lock().expect("refresh state lock poisoned");
        let (mut pending, timeout) = self.cv.wait_timeout_while(pending, interval, |pending| {
            !pending.requested && !pending.stopping && pending.render_error.is_none()
        }).expect("refresh state lock poisoned");
        if pending.stopping { return None; }
        // Error delivery does not consume a real pending invalidation. The
        // worker reports it first, then waits or handles that queued snapshot.
        if let Some(failure) = pending.render_error.take() {
            return Some((pending.generation, Some(failure)));
        }
        if timeout.timed_out() && !pending.requested {
            pending.generation = pending.generation.wrapping_add(1);
        }
        // Consume the request and generation under one lock. An invalidation
        // during IO now remains pending for exactly one subsequent snapshot.
        pending.requested = false;
        Some((pending.generation, pending.render_error.take()))
    }
}

#[derive(Default)]
struct PendingRender {
    scheduled: bool,
    latest: Option<(u64, MenuModel)>,
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

    fn start(&self, app: AppHandle, rendered: Arc<Mutex<RenderedMenu>>) {
        let mut owned_worker = self.worker.lock().expect("refresh worker lock poisoned");
        if owned_worker.is_some() { return; }
        let host = self.host.clone();
        let state = self.state.clone();
        let interval = self.interval.max(Duration::from_secs(1));
        *owned_worker = Some(std::thread::spawn(move || {
            let mailbox = Arc::new(Mutex::new(PendingRender::default()));
            state.request();
            while let Some((generation, render_error)) = state.next(interval) {
                if let Some(failure) = render_error {
                    host.render_failed_at(failure.error, failure.operation);
                    continue;
                }
                let context = SnapshotContext { state: state.clone(), generation };
                let result = host.snapshot_with_context(&context);
                if context.is_cancelled() { continue; }
                // Error fallbacks can themselves do IO; keep them here, never
                // inside the queued main-thread closure.
                let model = match result {
                    Ok(model) => Some(model),
                    Err(error) => host.snapshot_failed(&error),
                };
                if context.is_cancelled() { continue; }
                let Some(model) = model else { continue; };
                if let Err(error) = model.validate() {
                    host.render_failed_at(RenderError::Model(error), RenderOperation::Validate);
                    continue;
                }
                {
                    let mut pending = mailbox.lock().expect("render mailbox lock poisoned");
                    pending.latest = Some((generation, model));
                    if pending.scheduled { continue; }
                    pending.scheduled = true;
                }
                let target = app.clone();
                let state_for_ui = state.clone();
                let mailbox_for_ui = mailbox.clone();
                let rendered = rendered.clone();
                let rendered_host = host.clone();
                if app.run_on_main_thread(move || {
                    let next = {
                        let mut pending = mailbox_for_ui.lock().expect("render mailbox lock poisoned");
                        pending.scheduled = false;
                        pending.latest.take()
                    };
                    let Some((generation, model)) = next else { return; };
                    if !state_for_ui.is_current(generation) { return; }
                    match apply_model(&target, &model, &rendered) {
                        Ok(()) => rendered_host.model_rendered(&target),
                        Err(error) => state_for_ui.report_render_error(error),
                    }
                }).is_err() {
                    let mut pending = mailbox.lock().expect("render mailbox lock poisoned");
                    pending.scheduled = false;
                    pending.latest = None;
                    state.report_render_error(RenderFailure::native(RenderOperation::Schedule));
                }
            }
        }));
    }

    fn stop(&self) {
        self.state.stop();
        self.host.cancel_snapshot();
        if let Some(worker) = self.worker.lock().expect("refresh worker lock poisoned").take() {
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

#[derive(Clone)]
struct MenuRoute {
    command: Option<String>,
    check: Option<(CheckMenuItem<tauri::Wry>, bool)>,
}

#[derive(Default)]
struct RenderedMenu {
    serial: u64,
    model: Option<MenuModel>,
    routes: HashMap<String, MenuRoute>,
}

struct MenuBuild {
    serial: u64,
    sequence: usize,
    routes: HashMap<String, MenuRoute>,
}

impl MenuBuild {
    fn new(serial: u64) -> Self {
        Self { serial, sequence: 0, routes: HashMap::new() }
    }

    fn route(&mut self, command: Option<&String>, enabled: bool) -> String {
        self.sequence += 1;
        // Native identity is scoped to this rendering, while product command
        // identity remains stable. Queued events from a replaced menu cannot
        // trigger a command belonging to its replacement.
        let native_id = format!("foundation.menu.{}.{}", self.serial, self.sequence);
        self.routes.insert(native_id.clone(), MenuRoute {
            command: command.filter(|_| enabled).cloned(),
            check: None,
        });
        native_id
    }
}

fn build_items(
    handle: &AppHandle,
    nodes: &[MenuNode],
    build: &mut MenuBuild,
) -> tauri::Result<Vec<Box<dyn IsMenuItem<tauri::Wry>>>> {
    let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            MenuNode::Separator => {
                items.push(Box::new(PredefinedMenuItem::separator(handle)?));
            }
            MenuNode::Item { id, title, enabled, icon } => {
                let enabled = *enabled && id.is_some();
                let item_id = build.route(id.as_ref(), enabled);
                let title = bounded_text(title, MAX_MENU_TITLE_CHARS);
                match icon {
                    Some(icon) => {
                        let image = tauri::image::Image::new_owned(
                            icon.rgba.clone(), icon.width, icon.height,
                        );
                        items.push(Box::new(tauri::menu::IconMenuItem::with_id(
                            handle, item_id, title, enabled, Some(image), None::<&str>,
                        )?));
                    }
                    None => items.push(Box::new(TauriMenuItem::with_id(
                        handle, item_id, title, enabled, None::<&str>,
                    )?)),
                }
            }
            MenuNode::Interactive { item } => {
                let enabled = item.enabled && item.id.is_some();
                let item_id = build.route(item.id.as_ref(), enabled);
                let title = render_item_title(item);
                // Never truncate into a different shortcut. Tauri ignores an
                // invalid accelerator; drop oversized values before parsing.
                let accelerator = item.shortcut.as_deref()
                    .filter(|value| value.len() <= MAX_MENU_SHORTCUT_CHARS);
                match &item.kind {
                    MenuItemKind::Toggle { checked }
                    | MenuItemKind::Check { checked }
                    | MenuItemKind::Radio { selected: checked, .. } => {
                        let check = CheckMenuItem::with_id(
                            handle, item_id.clone(), title, enabled, *checked, accelerator,
                        )?;
                        let route = build.routes.get_mut(&item_id).expect("registered menu route");
                        route.check = Some((check.clone(), *checked));
                        if matches!(item.kind, MenuItemKind::Radio { selected: true, .. }) {
                            // Choosing the selected radio is a no-op, never a
                            // request to deselect the group's current value.
                            route.command = None;
                        }
                        items.push(Box::new(check));
                    }
                    MenuItemKind::Action => match &item.icon {
                        Some(icon) => {
                            let image = tauri::image::Image::new_owned(
                                icon.rgba.clone(), icon.width, icon.height,
                            );
                            items.push(Box::new(tauri::menu::IconMenuItem::with_id(
                                handle, item_id, title, enabled, Some(image), accelerator,
                            )?));
                        }
                        None => items.push(Box::new(TauriMenuItem::with_id(
                            handle, item_id, title, enabled, accelerator,
                        )?)),
                    },
                }
            }
            MenuNode::Submenu { title, items: children } => {
                let built = build_items(handle, children, build)?;
                let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
                    built.iter().map(|item| item.as_ref() as &dyn IsMenuItem<tauri::Wry>).collect();
                items.push(Box::new(Submenu::with_items(
                    handle, bounded_text(title, MAX_MENU_TITLE_CHARS), true, &refs,
                )?));
            }
        }
    }
    Ok(items)
}

fn render_item_title(item: &MenuItem) -> String {
    // Reserve suffix space so long titles cannot silently hide state.
    let mut suffix = String::new();
    if let Some(badge) = &item.badge {
        suffix.push_str("  [");
        suffix.push_str(&bounded_text(badge, MAX_MENU_BADGE_CHARS));
        suffix.push(']');
    }
    if let Some(progress) = item.progress {
        suffix.push_str(&format!("  {}%", progress.percent.min(100)));
    }
    let mut title = bounded_text(&item.title, MAX_MENU_TITLE_CHARS - suffix.chars().count());
    title.push_str(&suffix);
    title
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    // Labels are one line. Remove control characters that could alter native
    // menu layout or present misleading shortcut text.
    value.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).take(max_chars).collect()
}

fn build_menu(handle: &AppHandle, nodes: &[MenuNode], serial: u64)
    -> tauri::Result<(Menu<tauri::Wry>, HashMap<String, MenuRoute>)>
{
    let mut build = MenuBuild::new(serial);
    let items = build_items(handle, nodes, &mut build)?;
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        items.iter().map(|item| item.as_ref() as &dyn IsMenuItem<tauri::Wry>).collect();
    Ok((Menu::with_items(handle, &refs)?, build.routes))
}

fn apply_model(
    handle: &AppHandle, model: &MenuModel, rendered: &Mutex<RenderedMenu>,
) -> Result<(), RenderFailure> {
    let serial = {
        let mut rendered = rendered.lock().expect("rendered menu lock poisoned");
        if rendered.model.as_ref() == Some(model) { return Ok(()); }
        rendered.serial = rendered.serial.wrapping_add(1);
        rendered.serial
    };
    // Models are validated on the worker before they reach the UI thread.
    let tray = handle.tray_by_id(TRAY_ID)
        .ok_or(RenderFailure::native(RenderOperation::LookupTray))?;
    let (menu, routes) = build_menu(handle, &model.nodes, serial)
        .map_err(|_| RenderFailure::native(RenderOperation::BuildMenu))?;
    tray.set_menu(Some(menu)).map_err(|_| RenderFailure::native(RenderOperation::SetMenu))?;
    {
        let mut rendered = rendered.lock().expect("rendered menu lock poisoned");
        rendered.routes = routes;
        // If a later property update fails, retry the complete model next
        // time instead of treating partially applied native state as final.
        rendered.model = None;
    }
    tray.set_title(model.title.as_deref().map(|title| bounded_text(title, MAX_MENU_TITLE_CHARS)))
        .map_err(|_| RenderFailure::native(RenderOperation::SetTitle))?;
    tray.set_tooltip(model.tooltip.as_deref().map(|tip| bounded_text(tip, MAX_MENU_TITLE_CHARS)))
        .map_err(|_| RenderFailure::native(RenderOperation::SetTooltip))?;
    let image = model.icon.as_ref().map(|icon|
        tauri::image::Image::new_owned(icon.rgba.clone(), icon.width, icon.height)
    );
    tray.set_icon(image).map_err(|_| RenderFailure::native(RenderOperation::SetIcon))?;
    rendered.lock().expect("rendered menu lock poisoned").model = Some(model.clone());
    Ok(())
}

fn show_companion_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(COMPANION_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn loading_model(default_icon: Option<&tauri::image::Image<'_>>) -> MenuModel {
    // Windows registers the tray before the worker supplies a product model.
    // Supply a valid HICON in the initial NIM_ADD rather than depending on a
    // later icon update. This alone does not establish registration success:
    // tray-icon defers NIM_ADD failures, so readiness still awaits full render.
    #[cfg(target_os = "windows")]
    let icon = Some(default_icon.map(|image| RgbaIcon {
        rgba: image.rgba().to_vec(), width: image.width(), height: image.height(),
    }).unwrap_or_else(|| protocol::monogram("Hr")));
    #[cfg(not(target_os = "windows"))]
    let icon = { let _ = default_icon; None };
    MenuModel {
        title: Some("…".into()),
        icon,
        nodes: vec![MenuNode::disabled("Loading status…"), MenuNode::quit("Quit")],
        ..MenuModel::default()
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
    let rendered = Arc::new(Mutex::new(RenderedMenu::default()));
    let event_rendered = rendered.clone();
    let app = configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Keep setup on the UI thread cheap. The coordinator takes the
            // first snapshot asynchronously and replaces this inert menu.
            let model = loading_model(app.default_window_icon());
            let (menu, routes) = build_menu(app.handle(), &model.nodes, 1)?;
            let mut tray = TrayIconBuilder::with_id(TRAY_ID).menu(&menu);
            if let Some(directory) = host.tray_icon_directory() {
                tray = tray.temp_dir_path(directory);
            }
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
            *rendered.lock().expect("rendered menu lock poisoned") = RenderedMenu {
                serial: 1, model: Some(model), routes,
            };
            host.started_with_refresh(app.handle(), refresh_handle.clone());
            controller.start(app.handle().clone(), rendered.clone());
            Ok(())
        })
        .on_menu_event(move |app, event| {
            // Only events issued by the currently rendered tray may dispatch.
            // App/window menus and delayed events from replaced trays are ignored.
            let route = event_rendered.lock().expect("rendered menu lock poisoned")
                .routes.get(event.id.0.as_str()).cloned();
            let Some(route) = route else { return; };
            if let Some((check, checked)) = route.check {
                // Native check items auto-toggle even when the command fails.
                // The next daemon snapshot, not this click, owns the mark.
                if check.set_checked(checked).is_err() {
                    event_controller.state.report_render_error(RenderFailure::native(RenderOperation::SetChecked));
                    event_controller.handle().request();
                    return;
                }
            }
            let Some(id) = route.command else { return; };
            if id == QUIT_ACTION_ID {
                app.exit(0);
                return;
            }
            if id == WINDOW_SHOW_ACTION_ID {
                show_companion_window(app);
                return;
            }
            let _outcome = dispatch_host.dispatch_result(&id);
            // Rejected actions also reconcile state/capabilities. Never retry
            // the mutation here, including an indeterminate delivery outcome.
            event_controller.handle().request();
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
    fn loading_tray_has_a_windows_icon_before_the_first_snapshot() {
        let fallback = loading_model(None);
        assert!(fallback.validate().is_ok());
        let image = tauri::image::Image::new_owned(vec![255; 16], 2, 2);
        let configured = loading_model(Some(&image));
        assert!(configured.validate().is_ok());
        #[cfg(target_os = "windows")]
        {
            assert_eq!(fallback.icon, Some(protocol::monogram("Hr")));
            assert_eq!(configured.icon, Some(RgbaIcon { rgba: vec![255; 16], width: 2, height: 2 }));
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(fallback.icon, None);
            assert_eq!(configured.icon, None);
        }
    }

    #[test]
    fn refresh_requests_advance_generation_and_coalesce() {
        let state = RefreshState::new();
        state.request();
        state.request();
        assert_eq!(state.next(Duration::ZERO), Some((2, None)));
        assert!(!state.pending.lock().unwrap().requested);
        state.request();
        assert!(!state.is_current(2));
        assert_eq!(state.next(Duration::ZERO), Some((3, None)));
        assert!(!state.pending.lock().unwrap().requested);
    }

    #[test]
    fn stopped_refresh_handle_ignores_late_requests() {
        let state = Arc::new(RefreshState::new());
        let handle = RefreshHandle { state: state.clone() };
        handle.stop();
        handle.request();
        assert!(state.pending.lock().unwrap().stopping);
        assert_eq!(state.next(Duration::ZERO), None);
        assert_eq!(state.pending.lock().unwrap().generation, 0);
    }

    #[test]
    fn shutdown_wakes_an_idle_refresh_worker() {
        let state = Arc::new(RefreshState::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            tx.send(worker_state.next(Duration::from_secs(60))).unwrap();
        });
        state.stop();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), None);
        worker.join().unwrap();
    }

    #[test]
    fn snapshot_context_cancels_on_invalidation_or_shutdown() {
        let state = Arc::new(RefreshState::new());
        state.request();
        let (generation, _) = state.next(Duration::ZERO).unwrap();
        let context = SnapshotContext { state: state.clone(), generation };
        assert!(!context.is_cancelled());
        state.request();
        assert!(context.is_cancelled());
        let (generation, _) = state.next(Duration::ZERO).unwrap();
        let context = SnapshotContext { state: state.clone(), generation };
        state.stop();
        assert!(context.is_cancelled());
    }

    #[test]
    fn render_errors_do_not_request_an_immediate_retry_loop() {
        let state = RefreshState::new();
        let failure = RenderFailure::native(RenderOperation::SetTooltip);
        state.report_render_error(failure);
        assert!(!state.pending.lock().unwrap().requested);
        assert_eq!(state.next(Duration::ZERO), Some((0, Some(failure))));
        assert!(!state.pending.lock().unwrap().requested);
        assert_eq!(state.next(Duration::ZERO), Some((1, None)));
    }

    #[test]
    fn render_error_delivery_preserves_a_pending_snapshot() {
        let state = RefreshState::new();
        let failure = RenderFailure::native(RenderOperation::SetIcon);
        state.request();
        state.report_render_error(failure);
        assert_eq!(state.next(Duration::ZERO), Some((1, Some(failure))));
        assert!(state.pending.lock().unwrap().requested);
        assert_eq!(state.next(Duration::ZERO), Some((1, None)));
        assert!(!state.pending.lock().unwrap().requested);
    }

    #[test]
    fn render_failure_wakes_worker_without_waiting_for_refresh_interval() {
        let state = Arc::new(RefreshState::new());
        let worker_state = state.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(worker_state.next(Duration::from_secs(60))).unwrap();
        });
        let failure = RenderFailure::native(RenderOperation::SetTooltip);
        state.report_render_error(failure);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some((0, Some(failure))));
        assert!(!state.pending.lock().unwrap().requested);
        worker.join().unwrap();
    }

    #[test]
    fn operation_reports_preserve_legacy_host_failure_callback() {
        struct LegacyHost(Mutex<Option<RenderError>>);
        impl Host for LegacyHost {
            fn snapshot(&self) -> MenuModel { MenuModel::default() }
            fn render_failed(&self, error: RenderError) { *self.0.lock().unwrap() = Some(error); }
        }
        let host = LegacyHost(Mutex::new(None));
        host.render_failed_at(RenderError::Native, RenderOperation::SetTooltip);
        assert_eq!(*host.0.lock().unwrap(), Some(RenderError::Native));
    }

    #[test]
    fn native_routes_isolate_old_disabled_and_duplicate_command_rows() {
        let command = "foundation.inert.1".to_owned();
        let mut first = MenuBuild::new(1);
        let old_id = first.route(Some(&command), true);
        let mut next = MenuBuild::new(2);
        let active = next.route(Some(&command), true);
        let duplicate = next.route(Some(&command), true);
        let disabled = next.route(Some(&command), false);
        let inert = next.route(None, true);
        assert!(!next.routes.contains_key(&old_id));
        assert_ne!(active, duplicate);
        assert_eq!(next.routes[&active].command.as_ref(), Some(&command));
        assert_eq!(next.routes[&duplicate].command.as_ref(), Some(&command));
        assert_eq!(next.routes[&disabled].command, None);
        assert_eq!(next.routes[&inert].command, None);
        assert!(!next.routes.contains_key(&command));
    }

    #[test]
    fn menu_validation_allows_repeated_commands_but_bounds_native_work() {
        let mut model = MenuModel {
            nodes: vec![MenuNode::show_window("Approvals"), MenuNode::show_window("Open")],
            ..MenuModel::default()
        };
        assert_eq!(model.validate(), Ok(()));
        model.nodes = vec![MenuNode::Separator; MAX_MENU_NODES + 1];
        assert_eq!(model.validate(), Err(ModelError::TooManyNodes));
        model.nodes = vec![MenuNode::item("", "Empty ID")];
        assert_eq!(model.validate(), Err(ModelError::InvalidActionId));
        model.nodes = vec![MenuNode::item("id\ncommand", "Control characters")];
        assert_eq!(model.validate(), Err(ModelError::InvalidActionId));
        model.nodes.clear();
        for _ in 0..=MAX_MENU_DEPTH {
            model.nodes = vec![MenuNode::Submenu { title: "Nested".into(), items: model.nodes }];
        }
        assert_eq!(model.validate(), Err(ModelError::TooDeep));
    }

    #[test]
    fn invalid_icon_and_contradictory_radio_selection_are_rejected() {
        let mut model = MenuModel {
            icon: Some(RgbaIcon { width: u32::MAX, height: u32::MAX, rgba: vec![] }),
            ..MenuModel::default()
        };
        assert_eq!(model.validate(), Err(ModelError::InvalidIcon));
        model.icon = None;
        model.nodes = vec![
            MenuNode::interactive(MenuItem::radio("one", "One", "mode", true)),
            MenuNode::Submenu { title: "More".into(), items: vec![
                MenuNode::interactive(MenuItem::radio("two", "Two", "mode", true)),
            ] },
        ];
        assert_eq!(model.validate(), Err(ModelError::AmbiguousRadioGroup));
    }

    #[test]
    fn long_titles_preserve_badge_progress_and_remove_control_characters() {
        let mut item = MenuItem::action("queue", "x".repeat(400)).with_badge("12");
        // Public model fields can bypass the constructor's clamping.
        item.progress = Some(ProgressValue { percent: 255 });
        let title = render_item_title(&item);
        assert_eq!(title.chars().count(), MAX_MENU_TITLE_CHARS);
        assert!(title.ends_with("  [12]  100%"));
        assert_eq!(bounded_text("first\nsecond\tthird\0", 30), "first second third ");
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

    #[test]
    fn rich_item_labels_and_accessibility_are_bounded() {
        let long = "x".repeat(400);
        let metadata = AccessibilityMetadata {
            label: Some(long.clone()),
            value: Some(long.clone()),
            hint: Some(long.clone()),
        };
        let item = MenuItem::action("long", long.clone())
            .with_badge(long)
            .with_accessibility(metadata);
        assert!(render_item_title(&item).chars().count() <= MAX_MENU_TITLE_CHARS);
        assert_eq!(item.accessibility.label.as_ref().map(|v| v.chars().count()), Some(MAX_ACCESSIBILITY_CHARS));
    }
}
