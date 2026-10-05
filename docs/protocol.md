# Helper protocol

`hraness-helper` is a small native executable that runs one task and exits:
the macOS local-app steps — assembling, signing and launching the product's
app — and nothing else. Any CLI language that can start a child process and
read and write pipes can use it. The product owns authorization, state and
every decision; the helper runs as the same OS user and is not a sandbox.

From 1.0, `hraness-companion` is an alias of `hraness-helper`. Every mode
below gives the same stdout, stderr and exit status under either name
(`contract/helper-argv.v0.8.1.json`, checked by `tests/helper_argv.rs`); only
`--version` prints the name of the binary that ran. The 0.x menu bar is gone:
see [Removed tray modes](#removed-tray-modes) and the
[migration guide](migration-1.0.md).

The authoritative definitions are
[the mode dispatcher](../crates/hraness-local-app/src/helper.rs) and
[the wire types](../crates/hraness-local-app/src/wire.rs). The SDK resolves
the binary with `resolveHelper` from `./helper`.

| Mode | What it does | Documented in |
| --- | --- | --- |
| `--version` | Prints one line and exits 0. | [below](#version) |
| `--assemble-app`, `--signing-identity`, `--launch` | The macOS local app. | [identity.md](identity.md) |

Any other argv prints `{"type":"error","version":1,"code":"invalid-arguments"}`
on stdout and exits 1. That includes the dialog modes removed in 2.0 —
`--notice`, `--prompt` and `--prompt-probe` — which used to render a native
alert or text-entry dialog. Products now print the equivalent terminal copy
themselves with `./permissions`.

## Framing

Each mode that reads input reads exactly one JSON frame, a single line ending
in `\n`, from stdin, and writes exactly one frame to stdout. Every frame has
`"version":1` and a `type`. A failure is one error frame and exit status 1:

```json
{"type":"error","version":1,"code":"invalid-app-request"}
```

Codes contain no raw input, file paths or product diagnostics. Show product
guidance for the codes you recognize and a generic failure for any other. A
panic reports `internal-error` on stderr. OS blocking and missing dynamic
libraries can fail before the helper starts; follow the
[installation guidance](installation.md) rather than waiting for a frame.

## Version

```text
hraness-helper 2.0.0 protocol/1,2
hraness-companion 2.0.0 protocol/1,2
```

The `protocol/1,2` suffix keeps the 0.8 line shape so existing version
parsers keep working. It no longer names a menu protocol.

## Removed tray modes

Until 0.9, `hraness-companion --state-dir <dir>` ran the menu bar and
`hraness-companion --check-protocol` checked it. From 1.0 the alias refuses
any argv starting `--state-dir`, `--check-protocol` or `--foreground`, and
no arguments at all; it draws nothing and reads nothing for these argv. It prints

```json
{"type":"error","version":1,"code":"tray-removed"}
```

on stdout, one line pointing at `<product> status --json` on
stderr, and exits **2**. Exit 2 is the usage exit code of the
[control envelope](control.md#error-codes-and-exit-codes), and no 0.x runner
ever exited 2, so a supervisor can tell this refusal apart from a crash.
`hraness-helper` answers the same argv with `invalid-arguments` and exit 1,
as it did in 0.9. The exact argv list and bytes are in
`contract/companion-alias.v1.json`.
