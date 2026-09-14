# Contents

- `src/lib.rs` contains the product-neutral foundation core: `MenuNode`/`MenuModel` data model (including per-item preview icons), `Host` trait, accessory activation, status-item lifecycle, menu construction, and the refresh loop. It is built on Tauri's maintained tray and activation APIs.
- `src/outputs.rs` contains the reusable outputs-directory section: bounded newest-first listing, image thumbnails, and open/reveal dispatch for agent-dropped files.

# Guidelines

- Keep this crate product-neutral. It owns UI mechanics only — never daemon semantics, credentials, product state, paths, or command names. Product adapters live in each product's repository.
- The host binary runs unbundled: `cargo build` output is the artifact a product CLI spawns. `.app` packaging is an optional later gate, never a prerequisite.
- `Host::snapshot` may perform bounded local IO and always runs off the UI thread. `Host::dispatch` must not block.
- A product's daemon remains the sole authority. The menu-bar process is a disposable client with no privilege of its own.
- Keep secrets, raw socket paths, and environment values out of menu labels, tooltips, logs, and argv.
- Consumers pin this crate by immutable tag through a Cargo git dependency. Bump `Cargo.toml` version and tag `v*` for releases; keep tags immutable.
- Build and test with `cargo build` / `cargo test`. Keep `target/` ignored.
