# Architecture

desktop-foundation gives Hraness CLI products a shared way to run without a
window or a menu bar: one owner process per product, short-lived commands
that print JSON, a human gate for decisions a person owns, terminal views,
and one small native helper for the few things a terminal cannot do (a
dialog, a credential prompt, the macOS local app).

```text
product CLI
  ├─ registry: verbs, operation classes, JSON envelope, exit codes
  ├─ control: one owner per product, agent + admin sockets (0700 dir)
  │    └─ ensureOwner / controlStatus (never signals a process)
  ├─ human-gate: decide verbs need a person at /dev/tty
  ├─ tui: interactive view, plain snapshot, or the status --json object
  └─ hraness-helper (native, one task, then exit)
        --notice · --prompt · --prompt-probe · --version
        --assemble-app · --signing-identity · --launch   (macOS local app)
```

`hraness-companion` is an alias of the helper. Products replacing an older
menu bar integration can follow the [migration guide](migration-1.0.md).

## Responsibilities

| Layer | Owns | Does not own |
| --- | --- | --- |
| Product CLI and owner | Accounts, data, permission checks, provider APIs, verbs and their handlers | Native binary publication |
| TypeScript SDK and Rust crates | The envelope and registry, owner sockets, the human gate, terminal views, login items, retiring old login items, pinned helper installation | Product authorization or permission escalation |
| Native helper | One dialog, prompt or local-app step per run, with bounded input | Provider credentials, shell commands, daemon APIs or product files |
| Shared release workflow | Six target builds, tests, executable assets, SDK archive, manifests and provenance | Product release gates |

The helper and the owner run as the same OS user. The sockets and the JSON
frames are application boundaries, **not an OS sandbox**. See
[control](control.md#where-the-sockets-are-and-are-not-a-boundary) and the
[human gate threat model](human-gate.md#threat-model).

## Lifecycle and storage

A product keeps its state in one owner process (`control serve`). Other
commands reach it through `agentRequest` or `adminRequest`, and
`ensureOwner` starts it on demand. `controlStatus` reads the owner files and
asks the socket; it never signals a PID. A login item for the owner is
opt-in (`./login`), and `retireLegacyLoginItem` (`./retire`) moves a 0.x
menu bar login item aside after checking it is the product's own.

The helper cache is shared by release repository, version and target. The
SDK downloads the helper once, verifies its size and SHA-256 against the
manifest in the package, and publishes it atomically.

## Distribution and trust

The versioned SDK archive embeds the exact native asset manifest. The
installer bounds downloads, checks size and SHA-256, restricts redirects,
and never extracts archives or runs an installation script.

The SDK package and its manifest are the trust root. Product maintainers pin
the admitted archive in their dependency lockfile. The installer does not
verify GitHub build attestations at runtime; release admission and the
published provenance provide that separate evidence. A SHA-256 match
establishes byte identity, not that software is harmless.

End users need the product's runtime, not a compiler, Cargo, Swift or Xcode.
The pipeline creates no installers. Starting with the next release, 1.1.3,
a separate tag-only job signs and notarizes the four raw Mac helper and
companion executables before their manifest hashes are generated.
Ordinary Mac helper builds remain ad-hoc signed. OS approval can still be needed; see
[installation guidance](installation.md).

## Platform boundary

The helper builds for macOS, Windows and Linux on x64 and arm64. The
dialogs work on each; the local app is macOS only. Terminal views and the
owner sockets need no GUI at all, so those interfaces work over SSH and
in CI. Platform-specific product features (Apple Messages, Contacts,
Mac capture helpers) stay with their products. See the
[platform contract](platforms.md).

For integrations in other languages, use the [helper protocol](protocol.md)
and the [control wire format](control.md#wire-format).
