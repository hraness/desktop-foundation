# Headless control

One **owner** process per product holds the product's state and answers
requests on two Unix sockets. Every other command is a short-lived client
that prints a JSON envelope with `--json`. The TypeScript API is
`@hraness/desktop-foundation/control` and `/registry`; Rust products use the
`hraness-control-kit` crate. The [shared contracts](../contract) define the
request and response formats for both.

For products replacing a menu bar integration, see the
[migration guide](migration-1.0.md).

## Envelope

Every command that takes `--json` prints exactly one JSON object, on one line, on stdout.

```json
{"ok":true,"schema":"example.status/1","generatedAt":"2026-09-28T00:00:00.000Z","data":{},"next":[]}
{"ok":false,"schema":"hraness.error/1","generatedAt":"2026-09-28T00:00:00.000Z","error":{"code":"not-found","message":"No approval a1.","next":[]}}
```

- `schema` names the shape of `data` as `<product>.<noun>/<n>`.
- Product names, verb words, schema ids and product error codes follow
  `contract/names.json`, which both kits test against.
- `generatedAt` is UTC with milliseconds.
- `next` lists follow-up commands, each `{command, why, audience}` where
  `audience` is `agent` or `human`.
- Since 1.1.0, `error.permission` may name the macOS permission behind a
  failure, `{"kind":"full-disk-access","settingsUrl":"x-apple.systempreferences:…"}`.
  `settingsUrl` is absent or null when the kind has no System Settings pane.
  It is optional, and an envelope without it is byte-identical to 1.0. A
  1.0 reader rejects an envelope that carries it, so set it only toward 1.1
  readers. Build it with `errorPermission(kind, settingsUrl)` (TypeScript) or
  `ErrorBody::with_permission(ErrorPermission::new(kind).with_settings_url(url))`
  (Rust). Both keep only the known System Settings panes
  (`contract/names.json` `settingsUrls`) and leave the member out for a
  kind outside `^[a-z][a-z0-9-]*$`, with the same bytes
  (`contract/golden/error-permission-cases.json`).
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

A verb whose class depends on its input, such as `approvals decide <id>
--digest <d> <allow-once|deny>`, is registered as `decide` with an
`operateWhen` test (`operate_when` in Rust). Input that passes the test, such
as `deny`, runs as `operate` without the gate; every other input still needs a
person. `commands --json` lists the verb as `decide` with an `operateWhen`
summary.

Flags that take a value (`--digest <d>`, `--width N`) are named in the verb's
`valueFlags`, so `--digest abc` and `--digest=abc` parse the same way. The
`next` command handed to a person keeps every positional and flag, in the
`--flag=value` form, so it reads back to the same input.

Flags that take no value (`--snapshot`, `--force`) are named in `flags`. A
verb accepts only its `valueFlags`, its `flags`, `--json` and `--help`; any
other flag, such as a typo, is a usage error (exit 2) and the verb does not
run. `--help` or `-h` prints the verb's usage, summary, class and options and
exits 0 without running it or asking for a gate; before a full verb it lists
the verbs under the words given. With `--json` it prints a `hraness.help/1`
envelope holding the same descriptors as `commands --json`. The Rust side gets
the same behaviour from clap.

A verb with `output: 'raw'`, such as `control serve`, owns stdout
and returns its exit status as a number; `runCli` prints no envelope for it.

`<product> commands --json` prints the registry (path, class, schema, summary
and gate tier) so an agent can discover what it may call.

`runCli` handles `--json`, usage errors and the gate. When `detectAudience`
reports an agent, it prints the JSON envelope even without `--json`. For a
person, an error is two lines on stderr, following `CLI_MENU_STYLE.md` D5:

```
✗ Unknown command "stats". Did you mean "status"?
→ example --help
```

The first line is one sentence (plus the suggestion for a mistyped command);
the second is the first `next` step meant for a person. `HRANESS_DEBUG=1` or
`--debug` adds the code and detail. A script (the quiet audience) receives the
error envelope on stdout and `FAIL <code>: <message>` on stderr, in ASCII
whatever the locale. Help starts with `Usage:`, and `<product> help <command>`
is the same as `<command> --help` unless a product registers its own `help` verb.

A `decide` verb called by an agent (with or without `--json`), or with `--json` by a quiet audience,
returns exit 3 with a `next` step for a person and never prompts. After a
wrong or expired code, the `next` step is the same command again.
`HRANESS_AUDIENCE=human` and `--confirm` only change wording; they never satisfy
the gate. See
[human-gate.md](human-gate.md).

## Owner files

The owner keeps its files in `<stateHome>/control/`. `stateHome` is
`$<PRODUCT>_STATE_HOME` when set, else `~/Library/Application Support/<product>`
on macOS and `$XDG_STATE_HOME/<product>` (default `~/.local/state/<product>`).

| File | Mode | Purpose |
|---|---|---|
| `control/` | 0700 | The directory. The owner creates it 0700, tightens a looser mode back to 0700, and refuses a directory that belongs to another user. |
| `supervisor.lock` | 0600 | Rust owners hold an exclusive lock here for their lifetime. |
| `owner.lock.<n>` | 0600 | TypeScript owners' numbered claims. Each is written in full to a temporary file and hard-linked into place, which fails if the name exists. The highest number is the owner. A starter takes the next number only after it finds the highest claim's process gone, so starters racing over a crashed owner compete for one name and all but one get `control-already-running`. A clean stop marks its claim released in place; claims are never renamed, and a new owner removes those 16 or more numbers below its own. |
| `owner.json` | 0600 | `{schema, pid, bootId, processStartId, generation}`. |
| `agent.sock` | 0600 | Agent requests. |
| `admin.sock` | 0600 | Admin requests. |
| `admin.cap` | 0600 | 32 random bytes as hex, new for each owner generation. |

`bootId` and `processStartId` are SHA-256 digests of the boot identifier and
the process start time, so a reused pid is never mistaken for the owner.
Both are independent of time zone: on macOS the boot identifier is the
`sec`/`usec` pair of `kern.boottime`, and the start time is read with
`TZ=UTC`, so an owner started from a login item and a check from a shell
with another `TZ` agree.
A second owner gets `control-already-running` (exit 5). A socket left by an
owner that is gone is removed only after the new owner holds the claim, and
only when the path is a socket that no longer accepts connections; a socket
that still answers, or any other file at that path, is left alone and the
owner refuses to start.

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
a process can read `admin.cap`. These sockets do not perform peer-credential
checks. See [human-gate.md](human-gate.md) and
[SECURITY.md](../SECURITY.md).
