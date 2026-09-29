# The human gate

A `decide` verb changes something a person owns: approving a request,
loosening a permission, installing a login item. The human gate makes an agent
running in the same session stop and hand that decision back instead of making
it by accident.

TypeScript: `@hraness/desktop-foundation/human-gate`. Rust:
`hraness_control_kit::gate`.

## Tiers

| Tier | Check | Status in 0.9.0 |
|---|---|---|
| T0 | Agent markers in the environment (`CLAUDECODE`, `CODEX_SANDBOX`, ...) and agent process names among the ancestors. Advisory only: it picks wording and never grants anything. | Shipped |
| T1 | The command has a controlling terminal (`/dev/tty`) and its process group is the terminal's foreground group. | Shipped |
| T2 | The command prints a one-time code (`XXX-XXX`, from an alphabet without look-alike characters) on `/dev/tty` and reads it back from `/dev/tty` within 120 seconds. Piped stdin cannot answer it. | Shipped |
| T3 | OS owner authentication (Touch ID or password) through the helper. | Reserved. Asking for T3 returns `unsupported-platform`. |

A verb declares `T1T2` or `T3`. In 0.9.0 every gated verb uses `T1T2`.

What each outcome returns:

- No terminal, or the terminal is in the background: `human-required` (exit 3).
- Wrong code: `gate-failed` (exit 3).
- No answer in time: `gate-expired` (exit 3).
- In every case nothing changed.

A `decide` verb run with `--json` from an agent or quiet audience does not
prompt at all. It returns `human-required` with a `next` step whose audience is
`human`, such as `example approvals decide a1 --digest 3f2a allow-once`.
`HRANESS_AUDIENCE=human` and `--confirm` change wording only.

The gate shows the decision's title and digest before the code. The digest
binds the confirmation to what was shown. The owner checks the same digest
again when it applies the decision, and answers `digest-mismatch` (exit 5) if
the thing being decided changed in between.

### Owner challenges

When an owner hands a decision to a separate client, it can issue a challenge
with `ownerChallenge({verb, digest, ttlMs})`. A challenge is HMAC-bound to
the verb and digest with a per-process key, expires (five minutes by default)
and can be redeemed once. A redeemed, expired, altered or mismatched challenge
fails with `gate-failed`, `gate-expired` or `digest-mismatch`.

## Threat model

**What T1+T2 stops.** An agent that runs a gated command the way agents run
commands: without a terminal, with piped input, in the background, or with
`--json`. It also stops a person approving by reflex, because they have to read
and type a fresh code. This is the failure the gate exists for: an agent
approving its own request by accident or because a prompt told it to.

**What T1+T2 does not stop.** A determined process running as the same user.
Such a process can open a pseudo-terminal, put itself in the foreground, read
the code from the screen and type it back. It can also read `admin.cap`, talk
to the owner's sockets directly, or edit the product's files. The gate is a
guard against accidents, not a security boundary between processes of one
user. Treat anything running as your user as able to act as you.

**Deferred.** T3 (OS owner authentication through `hraness-helper
--authorize`) and peer-credential checks on the admin socket
(`hraness-helper --peer-uid`) are not in 0.9.0. T3 needs a live check that
LocalAuthentication works from an ad-hoc signed, un-notarized helper before
any product offers it. Until then the tier value is reserved and returns
`unsupported-platform`, so a product cannot silently fall back to a weaker
check.
