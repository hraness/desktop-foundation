//! desktop-foundation 1.0: the one-shot helper for Hraness CLI products.
//!
//! The menu bar and tray runner were removed in 1.0 (see
//! `docs/migration-1.0.md`). This crate now builds two executables that run
//! the same code, `hraness-helper` and its `hraness-companion` alias, and
//! re-exports the crates products link directly:
//!
//! - [`local_app`] (`hraness-local-app`): the macOS local app, login items
//!   and the signed-helper machinery.
//! - [`cli_kit`] (`hraness-cli-kit`): audience detection and permission copy.
//!
//! The headless control surface lives in `hraness-control-kit`, which
//! products depend on by the same tag.

pub use hraness_local_app as local_app;
pub use hraness_local_app::{identity, service};

pub use hraness_cli_kit as cli_kit;
pub use hraness_cli_kit::{audience, permissions};
