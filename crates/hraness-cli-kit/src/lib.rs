//! Shared words and rules for Hraness command-line tools, in Rust.
//!
//! - [`audience`]: who is reading (a person, an agent, or nobody).
//! - [`permissions`]: macOS permission notices, denial recovery, Settings
//!   links and probes that never cause a prompt.
//! - [`style`]: status symbols with ASCII fallbacks, color rules, one-sentence
//!   errors with one next step, and quiet exits on closed pipes.
//! - `clap` (feature `clap`): usage errors as `✗ Unknown command "x". Did you
//!   mean "y"?` plus `→ <cli> --help`, exit 2, and JSON on stdout for
//!   `--json` and agents.
//!
//! This is the Rust twin of `@hraness/desktop-foundation`'s `audience`,
//! `permissions` and `cli-style` modules. The copy is byte for byte the same
//! and both check the same golden files. The crate uses only `std`.

pub mod audience;
#[cfg(feature = "clap")]
pub mod clap;
pub mod json;
pub mod permissions;
pub mod style;

pub use audience::Audience;
pub use style::{CliError, Output, Style, Symbol};
