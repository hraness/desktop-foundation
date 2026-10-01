# Plan: retire the menubar tray and run hraness products through a headless control owner, JSON CLI parity and a TUI

Status: proposed, 2026-09-28. Scope: shared foundations first, then every hraness product that ships or references a tray companion. Reference implementation: xcb.

## 0. Design scores

Scale is 1–10 and higher is better on every axis. For migration risk, higher means lower risk; for effort, higher means less effort.

| Design | Consistency | Agent usability | Human-approval safety | Migration risk | Effort | Total |
|---|---|---|---|---|---|---|
| D1: minimal (admin.cap plus a typed digest, `--confirm` when a human is the audience, owners only where one already exists) | 7 | 6 | 5 | 7 | 8 | 33 |
| D2: registry-first (verb registry, `commands --json`, one envelope, one-time HMAC challenge written to /dev/tty, owner in every product) | 9 | 9 | 6 | 5 | 5 | 34 |
| D3: tiered gate (T0 agent markers, T1 controlling tty, T2 typed one-time code, T3 LocalAuthentication through the helper; additive then subtractive; helper modes kept) | 8 | 7 | 8 | 9 | 6 | 38 |

D3 wins. It is the only design whose strongest gate (T3) holds against a same-uid process that drives a pty. It also keeps every non-tray helper mode that ghostget GG-8 and app assembly depend on, and it migrates additively.

Ideas grafted from D2:
- the verb registry, `commands --json`, the shared envelope and error-code registry, and op classes
- `tui --json` equal to `status --json` except for `generatedAt`
- menubar fixture parity, shared contract goldens, and the build-governance checks

Ideas grafted from D1:
- products with no authority to hold get no owner
- a contract test that pins the v0.8.1 helper argv
- the smallest workable scope for each product

### Verified and corrected claims

All reads were made against origin/main.

| Claim | Verdict | Evidence |
|---|---|---|
| `hraness-companion` has non-tray modes that must survive | Verified, with one extra mode | `desktop-foundation src/bin/hraness-companion.rs:609-641` handles `--version`, `--prompt-probe` (not listed in any design), `--assemble-app`, `--signing-identity <id>`, `--launch <abs path>`, `--notice`, `--prompt` |
| The SDK exposes `./prompt` and `./service` subpaths | Corrected | `package.json` exports only `.`, `./permissions`, `./audience`, `./cli-style`, `./menu-kit`. `service.ts` reaches users through `.` (`sdk/src/index.ts:7`). `sdk/src/prompt.ts` ships as a file and is not an export. The companion receipt is `{version:1, appId, port, token:64-hex, instance:32-hex}` over loopback HTTP with a Bearer token (`sdk/src/service.ts:12,28,52`). |
| `hraness-cli-kit` is std-only with no unsafe | Corrected | It is std-only apart from an optional `clap` feature, but already contains one `unsafe` block: `restore_default_sigpipe`, an `extern "C" signal`, at `crates/hraness-cli-kit/src/style.rs:278`. The desktop-foundation workspace has no `unsafe_code` lint. |
| xcb's `unsafe_code = "forbid"` stops a peer-credential crate | Refuted | The forbid is at `xcb Cargo.toml:13`, but workspace lints apply only to member crates, not to git dependencies. Peer credentials still go in a separate feature-gated crate so products never inherit `unsafe` by accident. |
| ghostget agent socket serves only approval and web; the admin union covers everything else | Verified | `src/control/protocol.ts:130-146` holds the admin `ControlRequest` union; `:182-184` holds `ghostget.approval/1` request, check and cancel. |
| ghostget `web.save` has no client | Verified | `service.ts:86` handles it and `validation.ts:90` validates it, but the only caller is `helper-lifecycle.test.ts:88`. |
| sponge autostart must stay recognized | Verified for Linux and Windows | `cli/watch/autostart-v1.ts:43` uses `hraness-companion-sponge-watch.desktop` and a Windows `.vbs` startup item. The macOS LaunchAgent label `app.hraness.companion.sponge-watch` was not re-read and is unverified. |
| textbutler has `retireMenuLoginItem` | Verified | `scripts/install-textbutler.ts` and its test. It retires only a regular, owner-owned `app.hraness.companion.textbutler.plist` that launches the menubar role or TextButler.app: it boots the item out, renames it aside, and never deletes it (`qualification/xcb-headless-review.json:10`). |
| textbutler still ships a tray | Corrected | Its tray is already retired. The only leftover on origin/main is an orphaned `packages/textbutler/src/__snapshots__/menubar.test.ts.snap`, which has no test. |
| Sandboxed agents cannot read the admin capability file | Refuted by design review | Claude Code and Codex sandboxes allow broad same-uid reads. No gate may depend on the capability file being secret. |
| Environment markers, `--confirm` and TTY checks separate a human from an agent | Refuted | A same-uid process can spoof all three, including by allocating a pty. Only T3 resists this. |

Repos missing locally: credits-foundation, build-governance and blip.pet. Their steps below are marked unverified.

## 1. Goals

1. Remove every macOS menubar/tray companion (Tauri runners, `desktop/menubar/*`, `menubar*.ts`, menu-kit) from all hraness products and from desktop-foundation.
2. Give every capability that a tray offered a CLI verb, with `--json` output in one shared envelope.
3. Give every product `<product> tui`, a navigable ratatui/Ink-free text UI whose non-TTY modes `--snapshot` (text) and `--json` are deterministic and golden-tested. xcb is the reference.
4. Where a product holds long-lived authority (pending approvals, sessions, watchers), give it a headless owner, `<product> control serve`, with two Unix sockets:
   - an agent socket, carrying the narrow agent protocol;
   - an owner-only admin socket, carrying the full admin protocol.
5. Keep human decisions human-bound. Agents may read, operate safe verbs, deny, cancel and tighten. Only a human can allow, loosen, connect credentials or spend.
6. Stay within the owner's release constraints: no Apple notarization, no releases that need a 2FA human in the loop, tag-driven immutable GitHub Releases, and OIDC npm where it applies.
7. Migrate with zero lost user state. Legacy login items are renamed aside, never deleted, and no foreign process is ever signalled.

Non-goals:
- A GUI replacement.
- Windows or Linux tray parity.
- Reviving oompa, which stays archived and frozen.

## 2. Decisions and rationale

| # | Decision | Rationale |
|---|---|---|
| D-1 | Keep the desktop-foundation repo name. v0.9.0 is additive; v1.0.0 deletes the tray. | A rename breaks every `github:` and `{git, tag}` pin (the wrench to ghostget lesson). Additive first lets products move independently. |
| D-2 | Split the binary. A new `hraness-helper` asset carries every non-tray mode. `hraness-companion` stays as a byte-identical alias through v0.9.x and becomes a helper-only alias in v1.0.0, with the tray code removed. | ghostget GG-8 (`cookie-companion-resolve.ts`, `GHOSTGET_MENUBAR` as runnerBinary) and app assembly call these modes. A contract test freezes the v0.8.1 argv. |
| D-3 | Owners only where authority lives: ghostget, textbutler, spongev2 and sponge. aicharts keeps its existing collector daemon, adapted. slopcamera, valhalla and peopleblade stay per-invocation, and their gates run in-process. | This is D1's minimality. A daemon in a product with nothing to own adds attack surface and migration risk. |
| D-4 | One envelope and one error-code registry, owned by the foundation. | This is D2's consistency. Agents learn one shape across products. |
| D-5 | Every verb declares an op class: `read`, `operate` (safe, agent-allowed) or `decide` (human-bound). Actions that loosen are `decide`. Deny, cancel, pause and tighten are `operate`. | This is D2's taxonomy. It fails closed, so any verb without a class is refused at registry build. |
| D-6 | Human gate tiers come from D3. T0 recognises agent markers and refuses fast (a UX aid, not security). T1 requires a controlling tty whose foreground process group is the caller. T2 requires a typed one-time code read from `/dev/tty`. T3 is LocalAuthentication through `hraness-helper --authorize`, spawned by the owner or, in per-invocation products, by the CLI. The default is T1+T2, and T3 is available per product. Making T3 the default is an owner question (Q1). | T1 and T2 stop accidental or casual agent approvals, such as agent tool calls with no tty, but not a malicious same-uid process driving a pty. T3 resists a same-uid process when the owner spawns the helper itself and reads the result over its own child stdio, so a client cannot forge the proof. SECURITY.md in each repo states these limits plainly. |
| D-7 | The admin socket lives at `<stateHome>/control/admin.sock`, mode 0600, inside a 0700 directory. It checks peer uid where available: the Rust `peercred` feature, or the helper for Bun. It also requires a 0600 `admin.cap` capability as defence in depth, but no gate relies on that file being secret. | Filesystem modes plus a peer uid check are the real boundary. The capability file only blocks confused-deputy connects from other tools. |
| D-8 | Single ownership follows ghostget's `owner.json` (pid, bootId, processStartId, generation), generalised, plus xcb's `supervisor.lock` flock and the `ensure_daemon` spawn/confirm/reap cycle. A conflict returns `control-already-running`. | Both are proven in production. The flock prevents a split brain on crash. |
| D-9 | `tui --json` equals `status --json` apart from `generatedAt`. Every former menubar fixture state gets a `tui --snapshot` golden with the same name. | The fixtures are the existing specification of states. Parity stops a state from silently disappearing. |
| D-10 | Legacy login items are retired by the shared `retire` module, generalised from textbutler's `retireMenuLoginItem`. It requires an exact label and an owner-owned regular file, boots out only that label, renames the file to `*.retired-<ts>`, never deletes, and never signals a PID. It keeps recognising the sponge `hraness-companion-sponge-watch` and `app.hraness.companion.*` markers. | This is proven code. The rename can be undone by hand. |
| D-11 | Each product swaps from `hraness-cli-kit` to `hraness-control-kit` in a single PR, never mixing the two tags in one Cargo graph. | Cargo allows two git sources but produces duplicate types. |
| D-12 | Owner login items are opt-in (`control install`). The default is on-demand start through `ensure_owner` when a verb needs the owner. ghostget's default is Q5. | A login item is persistent config, and on-demand start avoids a background daemon for people who never use one. |
| D-13 | ghostget ships two releases. Release N adds control, parity and the TUI and keeps the menubar. Release N+1 removes the menubar. | ghostget has the most coupling (the verification claim, the package-budget ledger, help goldens, site tests). |
| D-14 | Existing decide-shaped agent verbs keep their current behaviour behind the digest until the owner answers Q4. The affected verbs are textbutler `replies send`, slopcamera `runs approve` and sponge `watch decide`. | Changing who may approve is a product decision. The registry marks them `decide-legacy` so the question is visible. |

## 3. Shared foundation specification

### 3.1 Distribution

desktop-foundation v0.9.0 publishes:
- **npm tgz (Release asset):** `hraness-desktop-foundation-0.9.0.tgz`, with the package still named `@hraness/desktop-foundation`.
- **Rust crates, pinned by `{git, tag = "v0.9.0"}`:**
  - `hraness-cli-kit`, unchanged API;
  - `hraness-control-kit`, new;
  - `hraness-local-app`, new.
- **Binaries:**
  - `hraness-helper-<target>` (darwin-arm64, darwin-x64), ad-hoc signed, with no notarization;
  - `hraness-companion-<target>`, byte-identical to the helper through 0.9.x.
- **Contract data:** `contract/` ships in the tgz and is readable from Rust through `include_str!` in `hraness-control-kit`. It contains:
  - `envelope.schema.json`
  - `error-codes.json`
  - `op-classes.json`
  - `helper-argv.v0.8.1.json`
  - golden fixtures

New TS subpath exports: `./control`, `./human-gate`, `./tui`, `./login`, `./helper`, `./retire`, `./registry`. Existing exports stay through 0.9.x. In v1.0.0 `./menu-kit` is removed, and root `service.ts` (the companion start, stop and status calls) is removed.

### 3.2 Envelope and errors (`contract/envelope.schema.json`)

```ts
type Envelope<T> =
  | { ok: true;  schema: `${string}.${string}/${number}`; generatedAt: string; data: T; next?: NextStep[] }
  | { ok: false; schema: "hraness.error/1"; generatedAt: string;
      error: { code: ErrorCode; message: string; detail?: string; next?: NextStep[] } };
type NextStep = { command: string; why: string; audience: "agent" | "human" };
```

Exit codes:

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | failure |
| 2 | usage |
| 3 | `human-required`: a decide verb was called without a satisfied gate |
| 4 | `owner-unavailable` |
| 5 | `conflict`: a stale `expectedRevision` or digest |

Error codes are stable kebab-case and follow the existing `CliError.code` style in cli-kit:
- `usage`
- `not-found`
- `permission-denied`
- `human-required`
- `gate-failed`
- `gate-expired`
- `owner-unavailable`
- `control-already-running`
- `conflict`
- `digest-mismatch`
- `unsupported-platform`
- `internal`

A product may add codes under its own prefix (`ghostget.*`). The registry build rejects unprefixed unknown codes.

### 3.3 TS API (`@hraness/desktop-foundation`)

```ts
// ./registry
export type OpClass = "read" | "operate" | "decide" | "decide-legacy";
export interface Verb<I, O> {
  path: readonly string[];            // ["approvals","decide"]
  opClass: OpClass;
  schema: string;                     // "ghostget.approval/1"
  summary: string;
  input: (argv: ParsedArgs) => I;     // throws usage
  run: (input: I, ctx: VerbContext) => Promise<O>;
  gate?: { tier: "T1T2" | "T3"; describe: (input: I) => { title: string; digest: string } };
}
export function defineRegistry(product: string, verbs: Verb<any, any>[]): Registry; // rejects missing opClass, decide without gate
export function commandsJson(reg: Registry): Envelope<{ verbs: VerbDescriptor[] }>;
export function runCli(reg: Registry, argv: string[], io: CliIO): Promise<number>; // envelope + exit codes, --json, audience

// ./control
export interface OwnerPaths { stateHome: string; dir: string; agentSock: string; adminSock: string; cap: string; ownerJson: string; lock: string }
export function ownerPaths(product: string, env?: NodeJS.ProcessEnv): OwnerPaths;
export function serveControl(opts: {
  product: string; paths: OwnerPaths;
  agent: { protocols: Record<string, (req: unknown, peer: Peer) => Promise<unknown>>; maxClients?: number; maxBytes?: number; idleMs?: number };
  admin: { handler: (req: unknown, peer: Peer) => Promise<unknown> };
  signal: AbortSignal;
}): Promise<void>;                          // flock + owner.json generation + 0700/0600 + stale-socket reclaim
export function ensureOwner(product: string, spawn: () => ChildProcess, timeoutMs?: number): Promise<OwnerInfo>; // spawn/confirm/reap
export function adminRequest<T>(paths: OwnerPaths, req: unknown): Promise<T>;
export function agentRequest<T>(paths: OwnerPaths, protocol: string, req: unknown): Promise<T>;
export function controlStatus(paths: OwnerPaths): Promise<OwnerStatus>;  // never signals; reports stale

// ./human-gate
export type GateTier = "T0" | "T1" | "T2" | "T3";
export function detectAgent(env: NodeJS.ProcessEnv, ancestry?: string[]): { agent: boolean; markers: string[] }; // T0, advisory
export function requireHuman(opts: { title: string; digest: string; tier: "T1T2" | "T3"; tty?: string }):
  Promise<{ ok: true; proof: GateProof } | { ok: false; code: "human-required" | "gate-failed" | "gate-expired" }>;
export function ownerChallenge(opts: { verb: string; digest: string; ttlMs?: number }): Challenge; // single-use, HMAC-bound to verb+digest
export function ownerAuthorize(helper: string, opts: { reason: string; digest: string }): Promise<boolean>; // T3, spawned BY the owner

// ./tui
export interface View<S> { id: string; render(state: S, width: number): string[]; keys: KeyMap<S> }
export function runTui<S>(opts: { load: () => Promise<Envelope<S>>; views: View<S>[]; mode: "interactive" | "snapshot" | "json"; width?: number }): Promise<number>;
// non-TTY → snapshot unless --json; widths 40/80/120 goldens

// ./login     (opt-in owner login item; LaunchAgent on macOS, XDG autostart on Linux)
export function installLoginItem(opts: { label: string; program: string; args: string[] }): Promise<LoginItemResult>;
export function uninstallLoginItem(label: string): Promise<LoginItemResult>;

// ./retire
export function retireLegacyLoginItem(opts: { home: string; labels: string[]; accepts: (plist: ParsedPlist) => boolean; bootout: (label: string) => Promise<void>; now?: () => Date }): Promise<"retired" | null>;

// ./helper
export function resolveHelper(opts?: { override?: string /* GHOSTGET_MENUBAR etc. */ }): Promise<string>; // prefers hraness-helper, falls back to hraness-companion alias
```

Bun has no peer-credential API. TS owners therefore rely on the 0700 directory, the 0600 socket and the capability. When `peercred: "required"` is set, the owner verifies each admin peer through `hraness-helper --peer-uid <fd>`. That mode stays opt-in until the Phase F0 spike proves it works.

### 3.4 Rust API

`hraness-control-kit`:
- It has no `unsafe` of its own, and declares `#![forbid(unsafe_code)]`.
- Default features: `std`.
- Optional features:
  - `tui`: ratatui =0.30.2, crossterm =0.29.0, ratatui-textarea =0.9.2, the same pins as xcb;
  - `clap`;
  - `peercred`, which pulls in `nix` and its `getpeereid` for macOS and `SO_PEERCRED` for Linux. Implementation must confirm the crate choice.

```rust
pub mod envelope { pub struct Envelope<T>{..} pub enum ErrorCode{..} pub fn emit<T: Serialize>(..) -> ExitCode; }
pub mod registry { pub enum OpClass{Read,Operate,Decide,DecideLegacy} pub struct Verb{..} pub fn commands_json(..) }
pub mod control {
    pub struct OwnerPaths{..} pub fn owner_paths(product:&str) -> OwnerPaths;
    pub struct SupervisorLock; impl SupervisorLock { pub fn acquire(&OwnerPaths) -> Result<Self, AlreadyRunning> }
    pub fn ensure_owner(paths:&OwnerPaths, spawn: impl FnOnce()->std::io::Result<Child>, timeout:Duration) -> Result<OwnerInfo>;
    pub fn serve(paths:&OwnerPaths, agent: AgentRouter, admin: AdminHandler, stop: StopToken) -> Result<()>;
    #[cfg(feature="peercred")] pub fn peer_uid(stream:&UnixStream) -> std::io::Result<u32>;
}
pub mod gate { pub fn detect_agent(..)->AgentMarkers; pub fn require_human(title:&str,digest:&str,tier:Tier)->Result<GateProof,GateError>;
               pub fn owner_challenge(..)->Challenge; pub fn owner_authorize(helper:&Path,reason:&str,digest:&str)->Result<bool>; }
#[cfg(feature="tui")] pub mod tui { pub trait View{..} pub enum Mode{Interactive,Snapshot,Json} pub fn run(..) -> ExitCode; pub fn render_to_string(view, width:u16) -> String /* TestBackend */ }
```

`hraness-local-app` holds macOS app assembly and launch. It is moved from `src/`, is used by the helper and ghostget, and has no Tauri dependency.

`hraness-helper` supports the following modes:

| Mode | Status |
|---|---|
| `--version` | Frozen from v0.8.1 |
| `--prompt-probe` | Frozen from v0.8.1 |
| `--assemble-app` | Frozen from v0.8.1 |
| `--signing-identity <id>` | Frozen from v0.8.1 |
| `--launch <abs>` | Frozen from v0.8.1 |
| `--notice` | Frozen from v0.8.1 |
| `--prompt` | Frozen from v0.8.1 |
| `--authorize --reason <s> --digest <hex>` | New. LAContext deviceOwnerAuthentication; prints one JSON line; exit 0 means allowed, 3 means denied. |
| `--peer-uid <fd>` | New |

`contract/helper-argv.v0.8.1.json` pins the frozen argv shapes and their stdout/exit contracts, and a test runs against both binaries.

### 3.5 Verification in the foundation itself

- `cargo test --locked`, with a `tui` feature matrix.
- `npm run check:sdk`.
- Helper argv contract test.
- Socket tests:
  - `0600`/`0700` modes;
  - stale-socket reclaim;
  - a second `serve` returns `control-already-running`;
  - `ensure_owner` reaps a child that never confirms.
- Gate tests:
  - no tty returns exit 3;
  - piped stdin cannot satisfy T2, because the code is read from `/dev/tty`;
  - a challenge is single-use and bound to both verb and digest;
  - an expired challenge returns `gate-expired`.
- T3 spike on a real Mac with an ad-hoc signed helper. This checks the claim that LAContext works without notarization. The claim is unverified: it must pass before T3 is offered at all.
- Envelope schema validation of all goldens.
- `retire` tests ported from textbutler's `install-textbutler.test.ts`, covering a foreign item, a symlink, a non-owner file and a correct retire.

## 4. Command grammar (all products)

```
<p> status            [--json]                      read     one-screen health; same data as tui
<p> commands          --json                        read     verb registry (path, opClass, schema, summary, gate tier)
<p> tui               [--snapshot|--json] [--width N]  read  interactive on TTY; non-TTY defaults to --snapshot
<p> doctor            [--json]                      read     permissions, owner, login items, legacy items found
<p> control serve     [--foreground]                —        owner products only; never forks when --foreground
<p> control status    [--json]                      read
<p> control stop      [--json]                      operate  over admin socket; never signals a pid
<p> control install|uninstall [--json]              decide   opt-in login item (persistent config)
<p> approvals list|show <id> [--json]               read     products with approvals
<p> approvals decide <id> --digest <d> deny         operate
<p> approvals decide <id> --digest <d> allow-once   decide
<p> permissions list [--json] | set <..> --expected-revision <n> <tighten|loosen>   loosen=decide
<p> <product nouns…>  (existing verbs keep their names; each gains --json and an opClass)
```

Rules:
- `--json` on a `decide` verb from a non-human audience returns exit 3 with `next: [{command, audience:"human"}]` and never prompts.
- `HRANESS_AUDIENCE` and `--confirm` only select wording. They never satisfy a gate.
- Every former menu item maps to exactly one verb. The per-product parity table lives in each repo at `docs/cli-parity.md`, and a test fails if a fixture action has no verb.

## 5. Per-product phases

Key: F = foundation, P = product. Paths are repo-relative.

### F0: desktop-foundation v0.9.0 (additive)
- **Add:**
  - `crates/hraness-control-kit/**`
  - `crates/hraness-local-app/**`
  - `src/bin/hraness-helper.rs`, which dispatches modes and adds `--authorize` and `--peer-uid`
  - `sdk/src/{control,human-gate,tui,login,retire,helper,registry}.ts` plus tests
  - `contract/**`
  - `docs/control.md`
  - `docs/human-gate.md`, which states the threat model and T1/T2 limits plainly
  - `SECURITY.md` section
- **Change:**
  - `Cargo.toml` members;
  - `package.json` exports and files;
  - the release workflow, which uploads `hraness-helper-*` and keeps `hraness-companion-*`;
  - `release-manifest.json`.
- **Keep:** the tray, `./menu-kit`, `service.ts`.

### P1: ghostget (owner product; release N, then N+1). Starts after PR #456 lands.
- **N: add**
  - `src/control/admin-socket.ts`, which moves the admin union off helper stdio onto `admin.sock` and keeps the stdio channel for the controller;
  - a `registry.ts` that maps every `ControlRequest` action to a verb;
  - `ghostget web rules set` as the first client of `web.save` (decide when loosening);
  - `ghostget tui`, plus `tui-fixtures.test.ts`, which ports every `src/control/__fixtures__/menubar/*.{json,txt}` state to `__fixtures__/tui/`;
  - `docs/cli-parity.md`.
- **N: change**
  - `resolveHelper` in `src/cookie-companion-resolve.ts`, with the `GHOSTGET_MENUBAR` override kept as an alias;
  - help goldens;
  - package-budget ledger;
  - `package.json` files;
  - `package-artifact.ts`.
- **N+1: remove**
  - `src/control/menubar-cli.ts` and `.test.ts`
  - `src/control/menubar-fixtures.test.ts`
  - `src/control/menubar-icon.ts`
  - `src/control/__fixtures__/menubar/**`
  - `docs/menubar-release.md`
  - `website/public/icons/menubar.svg`
  - Ghostget.app tray role
- **N+1: regenerate**
  - verification claim `menu-bar-snapshot-read-only`, replaced by `tui-snapshot-read-only` with its AGENTS.md quote updated, along with `docs/assurance.md`;
  - `site.test.ts:575-576`;
  - legacy retire of the `app.hraness.companion.ghostget` login item.
- **Keep:** app assembly through `hraness-helper --assemble-app`; the GG-8 cookie paths.

### P2: textbutler (owner already exists: `daemon.sock` through `@hraness/local-custody`)
- **Add:**
  - an admin/agent split on the existing daemon, or a documented single socket plus op-class gate. Choose the split only if `local-custody` supports two listeners, which is unverified;
  - `registry.ts`, `tui`, `commands --json`.
- **Remove:** `packages/textbutler/src/__snapshots__/menubar.test.ts.snap`, which is orphaned.
- **Change:**
  - `scripts/install-textbutler.ts` imports `retireLegacyLoginItem` from `./retire` and deletes its local copy, keeping its tests as regression tests;
  - `cli-style.ts`, `permission-copy.ts` and `permission-prompt.ts` move to the 0.9.0 exports.
- **Mark:** `replies send` as `decide-legacy` (Q4).

### P3: spongev2 (owner: `service run`, HTTP over a Unix socket)
- **Add:**
  - `control serve` as the canonical name, with `service run` kept as an alias for one minor release;
  - an admin socket beside the existing one;
  - `registry.ts`, `tui` with fixture parity from `src/menubar/__fixtures__/menubar/*`.
- **Remove:** `src/menubar/**`, including `index.ts`, `menubar.test.ts` and the fixtures.
- **Change:** menubar branches in `src/cli.ts` and `src/cli.test.ts`; `src/permissions.ts` imports; the `package.json` pin.

### P4: sponge (owner: `watch run` file seam, `status.json` plus `requests.json`)
- **Add:**
  - `control serve` wrapping `watch run`, where the socket replaces `requests.json` writes for admin actions and the file seam stays read-only for one release;
  - `registry.ts`, `tui` with parity from `cli/__fixtures__/menubar/*`.
- **Remove:**
  - `cli/menubar.ts` and its test
  - `cli/menubar-fixtures.test.ts`
  - `cli/menubar-icon.ts`
  - `cli/companion-manifest.ts` and its test, after `watch` stops using the companion runner
  - `cli/__fixtures__/menubar/**`
- **Keep:** `cli/watch/autostart-v1.ts`, recognising `hraness-companion-sponge-watch` for retire and uninstall.
- **Mark:** `watch decide` as `decide-legacy` (Q4).
- **CI:** keep the single `Required` aggregate.

### P5: aicharts (collector daemon; no new owner)
- **Add:** `aicharts status|tui|commands --json` reading `collector-status.json` and `last-cycle.json` through `hraness-control-kit` with the `tui` feature, plus TestBackend goldens at widths 40/80/120.
- **Remove:**
  - `desktop/menubar/**`: `Cargo.toml`, `build.rs`, `tauri.conf.json`, `frontend/`, `icons/`, `src/*`, `fixtures/*`
  - `crates/aicharts-cli/src/menubar.rs`
  - `scripts/menubar{,-build}.ts` and `scripts/menubar.test.ts`
  - the workspace member entry
- **Retire:** the aicharts menubar login item.
- **CI:** single `Required` aggregate; drop the menubar job from the aggregate's needs in the same PR.

### P6: slopcamera (per-invocation; no owner)
- **Add:** `status|tui|commands --json`, and in-process gates for `decide` verbs.
- **Remove:**
  - `desktop/menubar/**`
  - `apps/desktop/cli/menubar{,-status}.ts` and their tests
  - `docs/menubar-release.md`
- **Change:** the release workflow, then re-review `release-workflow.test.ts`, whose reviewed digest `5b89df98` must be recomputed and reviewed in the same PR.
- **Mark:** `runs approve` as `decide-legacy` (Q4).
- **CI:** single `Required` aggregate.

### P7: valhalla (per-invocation; no owner)
- **Add:** `status|tui|commands --json`.
- **Remove:**
  - `desktop/menubar/**`
  - the `release.yml` menubar job
  - the menubar entries in `publish_release.py` `asset_names`
- **Keep:** `qualify_macos.py`, which runs in the desktop job, not the menubar job; confirm this before deleting.
- **CI:** single `Required` aggregate.

### P8: peopleblade (per-invocation; references only)
- **Change:**
  - references in `docs/AGENT_INTERFACES.md`, `docs/CLI_RELEASE.md`, `docs/OPERATIONS.md`, `skills/peopleblade/SKILL.md`, `scripts/build-cli-package.ts` and `scripts/production-control.test.ts`;
  - the `package.json` pin, then regenerate `bun.lock` and `THIRD_PARTY_NOTICES.md`.
- **Add:** `commands --json` and `tui --snapshot` only if a status verb exists.

### P9: cleanup lane
- accounts-cli: pins and docs.
- credits-foundation: missing locally, so its scope is unverified; inspect with `gh api` before starting.
- gobstopper: its roadmap mentions the tray.
- jungle: remove the menubar surfaces from inventories and pins.
- support-foundation v0.8.0: drop companion support copy.
- oompa: archived and frozen; no change.
- blip.pet: Q3.

### F1: desktop-foundation v1.0.0 (subtractive)
- **Remove:**
  - the Tauri tray runner code paths in the `hraness-companion` binary, which becomes a helper-only alias;
  - `MenuModel`, `run` and `Host` tray pieces;
  - `sdk/src/menu-kit.ts`;
  - the companion start/stop/status calls in `sdk/src/service.ts`;
  - the Tauri dependencies.
- **Keep:** `hraness-companion` as an alias until no pin references it, tracked by the jungle inventory.
- **Precondition:** every product pin has moved to at least 0.9.0 and no origin/main imports `./menu-kit` or `serveCompanion`. A grep gate in CI across the inventory proves this.

### F2: build-governance v0.5.0 (missing locally; unverified)
- Add checks that each product has:
  - `commands --json` validating against `contract/envelope.schema.json`;
  - every verb carrying an op class;
  - `tui --json` equal to `status --json` modulo `generatedAt`;
  - no `desktop/menubar` or `menubar*.ts` paths after that product's removal PR.

## 6. Sequencing and lanes

```
F0 desktop-foundation v0.9.0  ──┬─ Lane A (owner products, serial by risk):  P2 textbutler → P3 spongev2 → P4 sponge → P1 ghostget N → ghostget N+1
                                ├─ Lane B (read-only trays, parallel):       P5 aicharts ‖ P6 slopcamera ‖ P7 valhalla
                                └─ Lane C (refs/cleanup, parallel):          P8 peopleblade ‖ P9 accounts-cli, credits-foundation, gobstopper, jungle
all lanes merged ─→ F1 desktop-foundation v1.0.0 ─→ F2 build-governance v0.5.0 ─→ support-foundation v0.8.0 ─→ jungle inventory final pin
```

- Lane A starts with textbutler because its tray is already gone and its daemon already exists, which makes it the cheapest check of the owner API.
- ghostget goes last in Lane A because of PR #456 and its two-release rule.
- Lanes B and C need only F0 and can run as parallel worktrees.
- Within a repo, the order is always:
  1. an additive PR (verbs, TUI, parity, still pinned to 0.9.0);
  2. a subtractive PR (remove the tray, then retire legacy items);
  3. release.
- A registry or envelope change after F0 is a foundation patch release (0.9.x), and dependent lanes re-pin before continuing.

## 7. Release and pin steps per repo

| Repo | Pin form | Release path |
|---|---|---|
| desktop-foundation | — | Bump `package.json`, `Cargo.toml` (workspace and crates) and `release-manifest.json`. Annotated tag `v0.9.0`, then the tag workflow builds the tgz, the helper and alias binaries (ad-hoc signed, no notarization) and the contract, and creates an immutable GitHub Release. v1.0.0 follows the same path. |
| ghostget | tgz URL in `package.json`; `{git, tag}` for any Rust | Bump PR, annotated tag, tag Release workflow with OIDC npm. Re-measure the package budget in every PR. Merge serially by hand, per the existing FV discipline. |
| textbutler | tgz URL | Bump PR, annotated tag, Release (a draft race is expected; rerun if it fails). Promotion follows the existing control-epoch digest path, which the owner runs. |
| spongev2 | tgz URL | Tag Release. |
| sponge | tgz URL | Tag Release; production health smoke after deploy. |
| aicharts | `hraness-control-kit = { git, tag = "v0.9.0", features = ["tui"] }` | Tag Release. The Worker is untouched. The CLI release only. |
| slopcamera | tgz URL (TS) and `{git, tag}` if the Rust menubar crate stays until removal | Re-review the release-workflow digest, then tag Release. |
| valhalla | `{git, tag}` | `publish_release.py` asset list updated, then tag Release. |
| peopleblade | tgz URL | Bump, tag Release with OIDC npm (CLI), then the admission window check. |
| accounts-cli, credits-foundation, gobstopper | as found | Tag Release where the pin changes. |
| support-foundation | tgz URL | v0.8.0 tag Release. |
| build-governance | tgz URL | v0.5.0 tag Release after F1. |
| jungle | inventory pins | One PR per wave that updates pins to immutable tags. |

No step needs Apple notarization or an interactive 2FA release approval. Where a repo already gates promotion on an owner-run digest (textbutler, aicharts wrangler, sloptrade-style runtimes), that gate is not changed by this plan.

## 8. Verification gates (all must pass on the integration candidate)

1. **Foundation:** everything in section 3.5, including the helper argv contract test against both `hraness-helper` and `hraness-companion`.
2. **Per-product parity:** every former menubar fixture state has a `tui --snapshot` golden at widths 40/80/120 and a `tui --json` golden. `tui --json` equals `status --json` modulo `generatedAt`.
3. **Registry:** `commands --json` validates against the schema. No verb lacks an op class. Every `decide` verb has a gate. `docs/cli-parity.md` covers every former menu action.
4. **Gate behaviour:**
   - In a sandbox with a private HOME, an empty PATH and no tty (the xcb `cli_ux.rs` pattern), every `decide` verb exits 3 with `human-required` and writes nothing.
   - Every `operate` deny or cancel verb succeeds.
5. **Owner, for owner products:**
   - a second `control serve` returns `control-already-running`;
   - a killed owner is reclaimed through the flock;
   - `control stop` works through the admin socket;
   - `control status` never signals;
   - socket modes are checked;
   - the agent socket refuses admin actions.
6. **Retire:** fixture HOMEs with a foreign item, a symlink, a non-owner file and the correct item. Only the correct item is renamed, and nothing is deleted.
7. **Removal:** grep gate showing no `menubar`, `menu-kit`, `serveCompanion` or `tauri` paths in the product after its subtractive PR. Package-content listing shows no tray binary.
8. **CI:** each repo's `Required` aggregate is green, with the menubar job removed from its needs in the same PR.
9. **Live qualification, after release:**
   - `status --json` and `tui --snapshot` from the released artifact on a clean HOME;
   - legacy login item retire on the owner's machine, with a `doctor --json` receipt;
   - the T3 prompt shown once by the owner, if T3 is enabled.
10. **Independent agent review** of each subtractive PR and of F1.

## 9. Rollback

- **Foundation:** 0.9.0 is additive, so rolling back means re-pinning to v0.8.1. v1.0.0 is the only breaking release. Products stay on 0.9.x until F1 has shipped with a clean grep gate, and a re-pin to 0.9.x restores the alias binary.
- **Product additive PRs:** revert the PR. No state format changes, because owners reuse the existing state directories and `owner.json` shape.
- **Product subtractive PRs:** release the previous tag. The tray binary and its fixtures come back with it. Retired login items are recovered by renaming `*.retired-<ts>` back and running `launchctl bootstrap`. `doctor` prints that exact command, and nothing is deleted.
- **sponge file seam and spongev2 `service run`:** both stay readable or aliased for one minor release, so an older CLI talking to a newer owner still works.
- **ghostget:** release N+1 can be rolled back to N, which has both the menubar and control. The verification claim is regenerated from the rolled-back docs.
- **A gate regression**, such as a `decide` verb reachable without a gate, is a stop-ship. Revert the product release. There is no hotfix-forward exception.

## 10. Decisions on the open questions (2026-09-28, Claude taking over Devin `menubar`)

Ben's direction (full removal; headless owner + clients; full CLI parity; human decisions stay human-bound; no notarization; no 2FA release steps) settles these without a new owner round:

- **Q1 → T1+T2 is the gate for this program. T3 (LocalAuthentication via `hraness-helper --authorize`) is a follow-up spike, not shipped in 0.9.0.** `--authorize` and `--peer-uid` helper modes are NOT added in F0; the gate API keeps a `T3` tier value reserved (returns `unsupported-platform`). SECURITY/human-gate docs state plainly that T1+T2 stops accidental agent approvals, not a determined same-uid process.
- **Q2 → no OS notification by default.** Pending approvals surface through `status`, `tui`, the requesting agent's `next` steps, and `doctor`. The existing `--notice` helper mode stays available; a product may opt in later.
- **Q3 → blip.pet out of scope.** It is a native AppKit menu-bar pet whose tray *is* the product, not a control surface, and Ben called it unused. No change.
- **Q4 → keep current behaviour (`decide-legacy`).** Changing who may approve is a separate product decision; the registry makes it visible.
- **Q5 → on-demand owner start** (`ensureOwner`); `control install` is opt-in.
- **Q6 → oompa is archived on GitHub; frozen, no change.**
- **Scope trim:** `admin.cap` stays (defence in depth), `peercred` Rust feature is deferred with T3. The TS owner relies on 0700 dir + 0600 socket + cap.
