//! Headless control for Hraness products.
//!
//! - [`envelope`]: the JSON envelope every `--json` command prints, the
//!   stable error codes and the exit codes (`contract/error-codes.json`).
//! - [`registry`]: verbs with an operation class, so `decide` verbs cannot
//!   ship without a human gate, and `commands --json`.
//! - [`control`] (feature `std`, Unix): one owner process per product behind
//!   a supervisor lock, with an agent socket and an admin socket.
//! - [`gate`] (feature `std`): the human gate. T1 needs a controlling
//!   terminal in the foreground; T2 needs a one-time code typed at
//!   `/dev/tty`. T3 is reserved and answers `unsupported-platform`.
//! - [`tui`] (feature `tui`): views that render to a terminal, a snapshot or
//!   JSON.
//!
//! The wire formats are documented in `docs/control.md` and
//! `docs/human-gate.md`, and shared with the TypeScript SDK through
//! `contract/`.

#![forbid(unsafe_code)]

pub mod contract;
pub mod envelope;
pub mod registry;
mod time;

#[cfg(all(feature = "std", unix))]
pub mod control;
#[cfg(feature = "std")]
mod crypto;
#[cfg(feature = "std")]
pub mod gate;
#[cfg(all(feature = "std", unix))]
mod process_identity;

#[cfg(feature = "tui")]
pub mod tui;

pub use envelope::{Audience, Envelope, ErrorBody, ErrorCode, ErrorPermission, NextStep};
pub use registry::{GateTier, OpClass, Registry, Verb};
