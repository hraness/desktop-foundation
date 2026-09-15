# desktop-foundation

Shared Rust menu-bar foundation for Hraness CLI products.

A product supplies a [`Host`](src/lib.rs) implementation that renders a
`MenuModel` snapshot and answers action ids. The foundation owns the
accessory-mode application lifecycle, the status item, menu construction,
and the refresh loop. The product's daemon stays the sole authority; the
menu-bar binary is a disposable client with no privilege of its own.

## Usage

Pin by immutable tag:

```toml
[dependencies]
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v0.4.1" }
```

Implement `Host`, then run the event loop with the product's own
`tauri::generate_context!()`:

```rust
fn main() {
    let host = Arc::new(MyHost::new());
    desktop_foundation::run(
        tauri::generate_context!(),
        host,
        Options::default(),
        |builder| builder, // or add invoke handlers / managed state here
    )
    .unwrap();
}
```

The binary runs unbundled — `cargo build` output is what a product CLI
spawns. `.app` packaging is an optional later gate for products that need
TCC-bound surfaces (camera, microphone, screen recording).

## Contract

- `snapshot()` returns the complete status-item state: title, optional
  full-color RGBA icon, tooltip, and menu nodes. Called off the UI thread
  on a refresh interval and after accepted dispatch. One worker coalesces
  invalidations and rejects superseded results. `started_with_refresh` supplies
  a `RefreshHandle` for product events; polling remains the fallback.
- `dispatch(id)` receives a menu action id. It must not block; the host
  owns whatever thread does the work.
- `started(app)` runs once after the status item exists — spawn sidecars
  and `manage` product state here. `cancel_snapshot()` signals cancellation
  before the worker joins; `stopping()` runs after it stops. Host reads must
  have finite deadlines. Use `snapshot_with_context` for cooperative cancellation.
- `MenuNode::quit(title)` adds a working Quit item; inert items
  (`MenuNode::disabled`) never reach `dispatch`.
- With `Options::companion_window`, a hidden `main` window becomes an
  on-demand panel: `MenuNode::show_window(title)` shows and focuses it, and
  closing the window hides rather than destroys it.
- `MenuNode::item_with_icon` renders a full-color preview icon beside the
  item title.

## Interactive items

Use a stable product command ID. The renderer gives each displayed row a separate
native ID, so several rows may open the same panel and old menu events cannot
activate a replacement row.

```rust
use desktop_foundation::{MenuItem, MenuNode};

let paused = true; // Read from the product's confirmed snapshot.
let pause = MenuNode::interactive(
    MenuItem::toggle("automation.pause", "Pause automatic replies", paused)
        .with_shortcut("CmdOrCtrl+P"),
);
let approvals = MenuNode::interactive(
    MenuItem::action("approvals.open", "Pending approvals")
        .with_badge("3")
        .with_shortcut("CmdOrCtrl+Shift+A"),
);
```

| Model feature | Current native presentation |
| --- | --- |
| Action and shortcut | Native menu command and accelerator |
| Check / toggle | Native checkmark; the next confirmed snapshot owns its value |
| Radio group | Native checkmarks with at most one selected item per group; the host implements selection |
| Action icon | RGBA image beside the label |
| Badge / progress | Bounded text in the label, such as `Pending approvals [3]` or `Export 42%` |
| Accessibility metadata | Retained for richer renderers; Tauri does not apply custom labels, values or hints. Native menu accessibility uses the visible label/state |
| Companion window | Show/focus the product's hidden `main` window and hide it on close |

The foundation does not render an anchored popover, searchable panel, slider,
custom progress bar, or notification center. Those remain product/native-host
work. Do not advertise metadata as a working native control.

Model validation bounds menu size, nesting, action IDs and icon buffers. Invalid
models leave the last usable menu in place. `render_failed` reports a safe error
category; `snapshot_result` and `snapshot_failed` provide typed read failures and
degraded views without copying raw daemon errors into labels.

`DispatchOutcome::Accepted` means the request was accepted, not completed.
`Indeterminate` requires reconciliation; the framework never retries a mutation.
The renderer restores checked state until the host supplies a confirmed value.

## Outputs sections

`outputs::OutputsSection` is a reusable building block for the common
"agent drops outputs into a directory" pattern:

```rust
let outputs = OutputsSection::new(state_dir.join("outputs"));

impl Host for MyHost {
    fn snapshot(&self) -> MenuModel {
        MenuModel { nodes: [product_nodes, self.outputs.nodes()].concat(), ..Default::default() }
    }
    fn dispatch(&self, id: &str) {
        if self.outputs.dispatch(id) { return; }
        // product actions
    }
}
```

It lists files newest-first, adds file-type/size labels and image thumbnails,
opens a file on click, and offers per-file Reveal in Finder in a submenu. The
folder action remains available when the folder is empty. Hidden entries,
directories, symlinks and non-UTF-8 names are skipped. Listing scans at most
10,000 entries; a larger or partially unreadable directory is explicitly labeled
as a partial scan. Ties are ordered by filename.

Actions refer only to files offered by the latest snapshot and verify size,
timestamp and (on Unix) device/inode/change-time before opening. Replaced or
removed files require a refreshed menu. This is stale-action protection, not a
sandbox for other software running as the same user. Thumbnails have separate
encoded-byte, dimension and allocation limits.

Current consumers: Oompa, Ghostget, PeopleBlade, Slopcamera, AI Charts and Valhalla.
Textbutler uses a separate native Swift control client.

## Validation and releases

Run `cargo test --locked` and `cargo build --locked` on macOS. On managed Hraness
hosts run these through the installed `oompa-host-run` mac-native lane. Tests use
synthetic output files and do not open Finder or connect to product daemons.

Release from the reviewed, validated tree by updating both Cargo version records
and pushing a new immutable `v*` tag. Never move existing tags. Consumers update
both their Git dependency tag and the exact Cargo lockfile commit; they retain
their own native and source delivery gates.
