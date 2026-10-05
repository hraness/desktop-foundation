//! The Hraness local app on the person's own machine, without Tauri.
//!
//! - [`identity`] assembles and signs `~/Applications/Hraness/<Product>.app`.
//! - [`launch`] is `--launch`: the app runs the product as its child.
//! - [`service`] plans and installs per-user login items.
//! - [`helper`] dispatches every one-shot mode of `hraness-helper` and its
//!   `hraness-companion` alias, so both binaries keep one argv contract
//!   (`contract/helper-argv.v0.8.1.json`).

pub mod helper;
pub mod identity;
pub mod launch;
pub mod service;
pub mod wire;
