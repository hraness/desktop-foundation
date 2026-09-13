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
desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v0.1.0" }
```

Implement `Host`, then run the event loop with the product's own
`tauri::generate_context!()`:

```rust
fn main() {
    let host = Arc::new(MyHost::new());
    desktop_foundation::run(tauri::generate_context!(), host, Options::default()).unwrap();
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
- `MenuNode::quit(title)` adds a working Quit item; inert items
  (`MenuNode::disabled`) never reach `dispatch`.

Current consumers: `oompa-menubar` in `hraness/oompa`.
