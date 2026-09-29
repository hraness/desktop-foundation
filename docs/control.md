# Headless control

From 0.9.0 a product can run without a menu bar. One **owner** process per
product holds the product's state and answers requests on two Unix sockets.
Every other command is a short-lived client that prints a JSON envelope with
`--json`. The TypeScript side is `@hraness/desktop-foundation/control` and
`/registry`; the Rust side is the `hraness-control-kit` crate. Both read the
same files in [`contract/`](../contract), and tests on both sides check the
same golden fixtures in `contract/golden/`.

The tray, `./menu-kit` and the companion lifecycle calls are unchanged in
0.9.x. Nothing in this document replaces them yet.

## Envelope

Every command that takes `--json` prints exactly one JSON object on stdout.

```json
{"ok":true,"schema":"example.status/1","generatedAt":"2026-09-28T00:00:00.000Z","data":{},"next":[]}
{"ok":false,"schema":"hraness.error/1","generatedAt":"2026-09-28T00:00:00.000Z","error":{"code":"not-found","message":"No approval a1.","next":[]}}
```

- `schema` names the shape of `data` as `<product>.<noun>/<n>`.
- `generatedAt` is UTC with milliseconds.
- `next` lists follow-up commands, each `{command, why, audience}` where
  `audience` is `agent` or `human`.
- The JSON Schema is `contract/envelope.schema.json`.

### Error codes and exit codes

| Exit | Codes |
|---|---|
| 0 | success |
| 1 | `not-found`, `permission-denied`, `unsupported-platform`, `internal`, and product codes |
| 2 | `usage` |
| 3 | `human-required`, `gate-failed`, `gate-expired` |
| 4 | `owner-unavailable` |
| 5 | `control-already-running`, `conflict`, `digest-mismatch` |

`contract/error-codes.json` is the list both kits check against. A product adds
codes only under its own prefix, such as `ghostget.policy-locked`; the registry
rejects any other unknown code.

## Verb registry

`defineRegistry(product, verbs)` (TypeScript) and `Registry` (Rust) take every
verb with an operation class from `contract/op-classes.json`:

- `read`: no side effects.
- `operate`: changes state an agent may change.
- `decide`: a decision a person owns. It must declare a gate, and the
  registry refuses to build without one.
- `decide-legacy`: a decision the product has always let a caller make. It is
  listed as such so the gap is visible in `commands --json`.

`<product> commands --json` prints the registry (path, class, schema, summary
and gate tier) so an agent can discover what it may call.

`runCli` handles `--json`, usage errors and the gate. A `decide` verb called
with `--json` by an agent or quiet audience returns exit 3 with a `next` step
for a person and never prompts. `HRANESS_AUDIENCE=human` and `--confirm` only
change wording; they never satisfy the gate. See
[human-gate.md](human-gate.md).

## Owner files

The owner keeps its files in `<stateHome>/control/`. `stateHome` is
`$<PRODUCT>_STATE_HOME` when set, else `~/Library/Application Support/<product>`
on macOS and `$XDG_STATE_HOME/<product>` (default `~/.local/state/<product>`).

| File | Mode | Purpose |
|---|---|---|
| `control/` | 0700 | The directory. The owner creates it 0700, tightens a looser mode back to 0700, and refuses a directory that belongs to another user. |
| `supervisor.lock` | 0600 | Rust owners hold an exclusive lock here for their lifetime. |
| `owner.lock` | 0600 | TypeScript owners create this claim with `O_EXCL`. A claim from a process that is gone is renamed aside to `owner.lock.stale-<ms>-<hex>`, never deleted. |
| `owner.json` | 0600 | `{schema, pid, bootId, processStartId, generation}`. |
| `agent.sock` | 0600 | Agent requests. |
| `admin.sock` | 0600 | Admin requests. |
| `admin.cap` | 0600 | 32 random bytes as hex, new for each owner generation. |

`bootId` and `processStartId` are SHA-256 digests of the boot identifier and
the process start time, so a reused pid is never mistaken for the owner.
A second owner gets `control-already-running` (exit 5). A socket left by an
owner that is gone is removed only after the new owner holds the claim, and
only when the path is a socket; any other file at that path is left alone and
the owner refuses to start.

`controlStatus` reads these files and probes the socket. It never sends a
signal. `ensureOwner` / `ensure_owner` starts the owner on demand, waits for
`control.hello`, and if the child never answers it kills only that child.

## Wire format

Newline-delimited JSON, one request and one response per connection.

Agent socket:

```json
{"v":1,"protocol":"example.approvals/1","request":{"op":"list"}}
```

Admin socket (`cap` is the contents of `admin.cap`):

```json
{"v":1,"cap":"<64 hex>","request":{"op":"control.stop"}}
```

Response:

```json
{"ok":true,"result":{"stopping":true}}
{"ok":false,"error":{"code":"permission-denied","message":"The agent socket does not take admin requests."}}
```

- `control.hello` (protocol `hraness.control/1`) answers on both sockets with
  `{product, pid, generation, protocols}`.
- `control.stop` is admin only.
- The agent socket refuses a request carrying `cap` and any protocol the
  owner did not register, with `permission-denied`.
- Frames larger than 1 MiB, malformed JSON and idle connections (30 s) are
  closed with an error. At most 32 agent clients are served at once.
- An exception in a handler becomes `internal` without its detail.

## Where the sockets are and are not a boundary

The sockets separate agent requests from admin requests by path and by the
capability file. Both live in a 0700 directory, so another user on the machine
cannot connect. They do not keep out a process running as the same user: such
a process can read `admin.cap`. Peer-credential checks are deferred with the
T3 gate tier. See [human-gate.md](human-gate.md) and
[SECURITY.md](../SECURITY.md).
