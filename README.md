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
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v0.3.0" }
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
  on a refresh interval and after every dispatch.
- `dispatch(id)` receives a menu action id. It must not block; the host
  owns whatever thread does the work.
- `started(app)` runs once after the status item exists — spawn sidecars
  and `manage` product state here. `stopping()` runs on exit.
- `MenuNode::quit(title)` adds a working Quit item; inert items
  (`MenuNode::disabled`) never reach `dispatch`.
- With `Options::companion_window`, a hidden `main` window becomes an
  on-demand panel: `MenuNode::show_window(title)` shows and focuses it, and
  closing the window hides rather than destroys it.
- `MenuNode::item_with_icon` renders a full-color preview icon beside the
  item title.

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

It lists files newest-first (bounded, hidden files and directories skipped),
labels each by filename stem — the agent's description — thumbnails image
files, opens a file on click, and adds a "Reveal Outputs Folder" item.
Dispatch resolves entries by a name hash, so refresh races cannot open the
wrong file.

Current consumers: `oompa-menubar` in `hraness/oompa`, `ghostget-desktop`
in `hraness/ghostget`, `peopleblade-menubar` in `hraness/peopleblade`.
