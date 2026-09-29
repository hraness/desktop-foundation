#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! `hraness-companion`, kept from 1.0 as an alias of `hraness-helper`.
//!
//! Every one-shot mode (`--version`, `--prompt-probe`, `--assemble-app`,
//! `--signing-identity`, `--launch`, `--notice`, `--prompt`) runs the same
//! code as `hraness-helper`, with the same argv, stdout and exit status
//! (`contract/helper-argv.v0.8.1.json`); only `--version` prints this
//! binary's name. The menu bar was removed in 1.0: with no arguments,
//! `--state-dir …` or `--check-protocol …` the alias draws nothing, prints a
//! `tray-removed` error frame and exits 2 (`docs/migration-1.0.md`).

use hraness_local_app::helper::{self, Binary};
use hraness_local_app::wire::{self, ProtocolError, RUNNER_PROTOCOLS};

fn main() {
    // Panic payloads may include paths. Emit only a stable code.
    std::panic::set_hook(Box::new(|_| {
        let _ = wire::write_error(&mut std::io::stderr(), wire::VERSION, "internal-error");
    }));
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if helper::is_tray_mode(&args) {
        helper::refuse_tray_mode();
    }
    let binary = Binary {
        name: "hraness-companion",
        version: env!("CARGO_PKG_VERSION"),
        protocols: RUNNER_PROTOCOLS,
    };
    let result = helper::dispatch(&args, binary).unwrap_or(Err(ProtocolError("invalid-arguments")));
    helper::exit_with(result)
}
