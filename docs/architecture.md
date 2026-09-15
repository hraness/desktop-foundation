# Architecture

The shared companion separates desktop presentation from product authority.
One versioned native executable serves every product; each product runs its own
instance and supplies its menu through the TypeScript SDK. This avoids a
separate native build and desktop installer for every product.

```text
product menubar
  └─ SDK: verify binary, start/reuse the product's foreground owner
       ├─ product code: read daemon state, authorize and perform actions
       ├─ authenticated loopback service: status and stop
       └─ hraness-companion: native tray + per-product OS lock
             snapshots → JSON lines → menu
             action ID + revision → product handler
```

## Responsibilities

| Layer | Owns | Does not own |
| --- | --- | --- |
| Product CLI/daemon | Accounts, data, permission checks, provider APIs, browser routes, snapshot content and action handlers | Native tray rendering or shared binary publication |
| TypeScript SDK | Pinned binary installation, process lifecycle, status/stop, callback deadlines, optional login registration | Product authorization or permission escalation |
| Native runner | Menu rendering, events, OS singleton lock, exit on Quit or parent disconnect | Provider credentials, arbitrary shell commands, daemon APIs or product filesystem access through the protocol |
| Shared release workflow | Six target builds, tests, executable assets, SDK archive, manifests and provenance | Product release gates or interactive desktop qualification |

The runner executes as the same OS user. The JSON protocol is an application
boundary, **not an OS sandbox**. Keep product secrets out of labels and action
IDs; do not assume another program running as that user is isolated from the
companion's files or processes.

## Menu and action contract

[`CompanionOptions`](../sdk/src/client.ts) supplies a stable `appId`, product
name, one or two alphanumeric title characters, a private `stateDir`, and
`snapshot(signal)` / `onAction(id, signal)` callbacks. Use a lowercase app ID
of at most 64 characters, starting with a letter and containing letters,
digits, dots or hyphens, with no consecutive dots.

The wire menu supports labels, actions, checkmarks, shortcuts, submenus,
separators and Quit. It does not currently expose every feature of the older
Rust `MenuModel`, such as per-item image previews. Use the browser for forms,
account management and larger views. A product may stay on the Rust host API
until its required features have a supported shared-runner equivalent.

The SDK validates and copies snapshots, assigns revisions and dispatches only
enabled action IDs from the current revision. Reads and mutations have bounded
waits and cancellation signals. Product callbacks must also use finite IO
deadlines, honor cancellation and reconcile ambiguous results. A timed-out
action is not automatically retried. Refresh from confirmed product state;
never toggle a displayed value as proof that a mutation completed.

## Lifecycle and storage

[`handleCompanionCommand`](../sdk/src/commands.ts) starts a detached owner by
re-entering the installed product CLI's private `--foreground` branch. The
initial CLI returns after status is confirmed. A native OS lock prevents a
second runner for the same product and state directory. Quit and `stop` end
the companion; stopping the product daemon is a separate product action.

The owner exposes only authenticated loopback `status` and `stop` operations.
An unpredictable token is kept in an instance-specific receipt in the product's
state directory, not in argv. Each owner removes only its own receipt.
Status distinguishes a responding owner from an unreachable receipt. Lifecycle
code never signals a PID copied from that receipt. Use a stable state directory
under `userPaths().dataDir`, with a distinct subdirectory for each product.

Executable caches are shared by release repository, version and target.
Autostart entries are per product and per user. Login registration is explicit
and takes effect at the next sign-in. Unregistering it leaves the running
companion, shared cache and product data intact.

## Distribution and trust

The versioned SDK archive embeds the exact native asset manifest. First start
downloads the matching raw executable from the named GitHub release; subsequent
starts verify the cached bytes. The installer bounds downloads, checks size and
SHA-256, restricts redirects, and publishes the verified file atomically. It
does not extract archives or run an installation shell script.

The SDK package and its manifest are the trust root. Product maintainers pin
the admitted archive in their dependency lockfile. The installer does not
independently verify GitHub build attestations at runtime; release admission
and the published provenance provide that separate evidence. SHA-256 matching
establishes byte identity, not that software is harmless.

End users need the product's supported runtime, not a compiler, Cargo, Swift
or Xcode. The pipeline does not create `.app` bundles, desktop installers or
Apple notarization submissions. OS approval can still be necessary and can
be unavailable under device policy. Only the human makes that trust decision;
see [installation guidance](installation.md).

## Platform and migration boundary

The native surface targets macOS menu bars, Windows notification areas and
compatible Linux AppIndicator desktops on x64 and arm64. Linux runtime
libraries and a tray host remain prerequisites. Builds and protocol probes do
not prove that a visible menu works on a clean desktop; see the
[platform evidence contract](platforms.md).

This portability does not move Ghostget's Apple Messages/Contacts APIs,
Slopcamera's Mac capture helpers, or any other platform-specific capability to
Windows or Linux. Provider authentication, permissions and delivery requirements
remain with their existing owners. The shared runner is available for adoption;
this repository does not establish that every existing product has migrated.
Follow the [product adoption recipe](adoption.md) for each consumer.

For integrations in other languages, use the [versioned JSON-lines protocol](protocol.md).
