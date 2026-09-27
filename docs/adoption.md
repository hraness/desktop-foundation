# Adopt the shared companion in a product CLI

This recipe replaces a product's tray launcher and renderer with the shared
SDK and prebuilt runner. It does not replace its daemon, provider adapters or
permission helpers. Existing consumers require individual migrations and
validation; publishing this framework does not migrate them automatically.

## 1. Inventory the current behavior

Record the existing menu commands, daemon connection, singleton identity,
startup registration, output previews, shortcuts and Quit behavior. Separate
native UI code from capability helpers. A Ghostget iMessage helper or a
Slopcamera capture helper remains necessary when it owns the actual API or OS
permission flow.

Compare the UI with the shared wire `MenuItem` in
[`sdk/src/protocol.ts`](../sdk/src/protocol.ts). It supports labels, actions,
checkmarks, shortcuts, submenus, separators and Quit. Per-item image previews
from the Rust host API are not yet part of this wire contract. Preserve a
needed feature through a supported browser view or add it to the shared
protocol before deleting its old implementation.

## 2. Pin the admitted package

Use the SDK archive from an admitted, immutable versioned GitHub release and
commit the product's dependency lockfile. The archive contains compiled
JavaScript and the matching binary manifest. Use the actual published version
from [Releases](https://github.com/hraness/desktop-foundation/releases); do not
substitute `latest`, a source checkout, or a local development build.

The product runtime must support the SDK's Node.js 22+ API requirements. Bun
compatibility must be qualified by the consuming product; it is not established
by the Node test matrix. The
customer install path must not run Cargo, Swift, Xcode, or a Tauri bundler. A
missing platform artifact is a release problem, not a reason to compile on the
customer's machine. `binary` / `binaryArgs` overrides are for maintainer tests;
production adapters use the packaged manifest.

## 3. Mount one lifecycle command

Keep the product's public `menubar` command and delegate its arguments to
`handleCompanionCommand`. The following function plugs into the existing
product argument parser; the parser passes only arguments after `menubar`.
This example uses an ES module entrypoint (`.mjs`, `.mts`, or a package with
`"type": "module"`).

```ts
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  handleCompanionCommand, openBrowser, userPaths,
} from '@hraness/desktop-foundation';

export async function menubar(args: readonly string[]) {
  return handleCompanionCommand({
    appId: 'org.example.product', name: 'Example Product', title: 'Ex',
    stateDir: join(userPaths().dataDir, 'org.example.product'),
    snapshot: async signal => [
      { kind: 'label', label: 'Example Product' },
      { kind: 'action', id: 'dashboard', label: 'Open dashboard' },
      { kind: 'quit', label: 'Quit menu companion' },
    ],
    onAction: async (id, signal) => {
      if (id === 'dashboard') {
        await openBrowser('https://example.com/dashboard');
      }
    },
  }, {
    args,
    foreground: {
      executable: process.execPath,
      args: [fileURLToPath(import.meta.url), 'menubar', '--foreground'],
    },
  });
}
```

`title` may also be a single emoji (`title: '👻'`) — macOS renders it as a
colored emoji in the menu bar. Windows and Linux trays are icon-only, so pair
an emoji title with `icon`: a `{ width, height, rgba }` record of pre-rendered
base64 pixels (at most 64×64) that ships inside the adapter's package; see
`docs/protocol.md` for the exact grammar and bounds.

Here `import.meta.url` must identify the **installed CLI entrypoint** that
routes `menubar`, not an imported helper module. Pass that entrypoint explicitly
if the adapter lives elsewhere. Preserve required product/profile arguments in
the foreground argv. Use absolute paths and an argv array, never a shell
command string. The foreground command must survive the original terminal
closing, so do not point it into a temporary checkout or package extraction.

For a compiled or bundled CLI, embed the release's pinned manifest and pass it
as `options.manifest`. The default `packagedManifest()` locates the manifest
relative to the installed SDK files, so it requires the normal npm package
layout to remain intact. Set the foreground command to the absolute compiled
executable with `['menubar', '--foreground']` and any required profile arguments;
do not prepend a JavaScript source path. Qualify this compiled entrypoint's
download, detached launch, singleton reuse and stop behavior on each supported
runtime and platform.

| Command | Result |
| --- | --- |
| `<product> menubar` or `menubar start` | Verify/install the pinned executable, start one companion, then return after confirmation. |
| `<product> menubar status` | Report running, stopped or unreachable owner state, and whether it opens at login (`--json` for the owner state object). |
| `<product> menubar doctor` | Check the platform, the helper and login startup without downloading or launching (`--json` adds artifact identity and integrity). |
| `<product> menubar stop` | Ask the authenticated owner to stop; report ambiguous failures. |
| `<product> menubar install` | Show the login item notice, then register next-login startup for the current user and save any `loginEnv` credentials. |
| `<product> menubar uninstall` | Remove owned login registration; retain the running companion and product data. |

Do not enable login startup as a side effect of ordinary installation or first
launch. Windows login registration currently needs Windows Script Host and
its VBScript feature; where policy or the OS does not provide it, retain the
manual CLI launch path. Unix login entries use LaunchAgents on macOS and XDG
autostart on Linux.

## 4. Connect product state and actions

Read the existing daemon or local data in `snapshot(signal)` using bounded IO.
Show unavailable capabilities clearly on unsupported platforms. A portable
menu does not make Apple Messages or Contacts available on Linux or Windows.

Map stable action IDs to the product's existing authorized operations. Keep
provider authentication, approval flows and account checks in those handlers.
Honor the signal, use idempotency where the product already requires it, and
refresh from confirmed state after an action. Do not retry an uncertain
mutation. Open larger UI in the product's browser interface.

When an action or CLI flow needs a credential, use the SDK's
`promptSecret(request)` — it renders a native dialog through the pinned
runner where the host supports one and falls back to a masked TTY prompt
otherwise. Pass an existing value as `prefill` for editing flows, and bound
the wait with `timeoutSeconds`. The foundation returns the entry or a status;
validation, storage and rotation stay with the product. Keep secrets out of
`title`/`message` and never pass them as arguments to any command.

## 5. Retire only the replaced paths

Once the new path passes the product's gates, remove its superseded Swift or
Tauri tray launcher, UI-only sidecars, development-build fallback, binary
packaging jobs and obsolete installer/notarization copy. Preserve native
helpers that still provide capabilities, account data and user outputs.

Inspect old per-user startup registrations and stop the old tray through its
documented control path before switching. Delete only the exact product-owned
registration that has been verified; never erase a shared LaunchAgents,
Startup, configuration or cache directory. Do not edit another running task's
processes or registrations during a source migration.

## 6. Qualify and ship the product adapter

Run the product's required checks, then exercise the installed package in a
fresh user environment: first download, repeat launch, status, harmless menu
action, state refresh, Quit and stop. Verify one menu after repeated/concurrent
starts. Test opt-in login behavior separately. Record OS/architecture, desktop,
release and digest; distinguish a native build from an observed working menu.

For OS blocks, use doctor output and the shared
[human approval instructions](installation.md). An agent explains the exact
artifact and available OS control; it does not remove quarantine, disable
security controls or grant approval for the human. A policy that blocks
unsigned software may leave only the product's already-permitted CLI/browser
mode available.

Update the product README, installation docs and marketing with the behavior
actually shipped. Claim migration and platform support only for validated
product paths; retain explicit limitations for platform-specific providers.

## Upgrade from 0.7 to 0.8

For SDK products 0.8.0 is additive: a product that only changes its pin
keeps working, and a v1 snapshot stays valid on the new runner. Rust adapters
may need small edits: `MenuNode` has new `Header` and `Status` variants and
`Submenu` gained a `symbol` field, so an exhaustive `match` or a
`MenuNode::Submenu { title, items }` pattern needs a `..` or new arms, and a
`MenuItem` or `MenuModel` built as a struct literal needs the new fields
(use the constructors and `with_*` builders instead).

1. Change the pin. SDK: install
   `releases/download/v0.8.0/hraness-desktop-foundation-0.8.0.tgz`. Rust: set
   `tag = "v0.8.0"` and refresh `Cargo.lock`.
2. Opt in to menu kit v2. Pass `mark` (for example
   `{ symbol: 'mark.chat', letters: 'Tb' }`) instead of `title`, and build
   items with `layout()` from `@hraness/desktop-foundation/menu-kit`. v2 items
   drop `title` and `checked`: use `mark.letters` and `state: 'on' | 'off' |
   'mixed'`. The SDK checks the runner's `--version` and down-levels to v1
   when the runner is older. See [protocol v2](protocol-v2.md).
3. Check every menu state in CI. Save one JSON snapshot per state and run
   `companion lint-menu --strict fixtures/*.json` (add `--proper-noun <name>`
   for product names), or call `assertMenuFixture(snapshot)` in a test.
4. Use the shared words. `@hraness/desktop-foundation/permissions` has the
   macOS notices and recovery copy, `/audience` decides whether a person or
   an agent is reading, and `/cli-style` prints `✓ ✗ ⚠ →` lines that respect
   `NO_COLOR` and pipes. Rust CLIs get the same words from the std-only
   `hraness-cli-kit` crate (0.8.1 and later), including a clap hook for
   one-line usage errors. See [permissions](permissions.md#rust-api-hraness-cli-kit).
5. Rust products get `MenuNode::header`, `MenuNode::status`, symbols,
   subtitles, badges and alternates on `MenuItem`, a `StatusMark` with a tone
   and count, `service` for login items and a single instance, and
   `notice` for the `--notice` dialog.
6. SDK login items on macOS can start the product's own app. Once the
   product has built `~/Applications/Hraness/<Name>.app`, pass
   `app: { name, argvFile }` to `handleCompanionCommand` or
   `runCompanion({ loginItem })`, so Login Items shows the product name
   instead of `bun`. If the app is missing, `install` stops with
   `app_missing`. See the README section on login items.
7. Leave local signing off for now. `identity` and the runner's
   `--assemble-app`, `--signing-identity` and `--launch` build and sign a
   product-named app on the person's Mac, but nothing calls them by default.
   Turn them on only after the clean-account check in
   [product identity](identity.md#prompts-this-raises).
