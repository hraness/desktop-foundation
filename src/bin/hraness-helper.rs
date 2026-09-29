#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! The one-shot helper: `--version`, `--prompt-probe`, `--assemble-app`,
//! `--signing-identity`, `--launch`, `--notice` and `--prompt`, with the
//! v0.8.1 argv, stdout and exit contract
//! (`contract/helper-argv.v0.8.1.json`). `hraness-companion` is its alias.

use hraness_local_app::helper::{self, Binary};
use hraness_local_app::wire::{self, ProtocolError, RUNNER_PROTOCOLS};

fn main() {
    // Panic payloads may include paths. Emit only a stable code.
    std::panic::set_hook(Box::new(|_| {
        let _ = wire::write_error(&mut std::io::stderr(), wire::VERSION, "internal-error");
    }));
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let binary = Binary {
        name: "hraness-helper",
        version: env!("CARGO_PKG_VERSION"),
        protocols: RUNNER_PROTOCOLS,
    };
    let result = helper::dispatch(&args, binary).unwrap_or(Err(ProtocolError("invalid-arguments")));
    helper::exit_with(result)
}
