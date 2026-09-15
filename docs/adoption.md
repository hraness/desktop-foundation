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
| `<product> menubar status --json` | Report running, stopped or unreachable owner state. |
| `<product> menubar doctor --json` | Inspect artifact identity/integrity and platform guidance without downloading or launching. |
| `<product> menubar stop` | Ask the authenticated owner to stop; report ambiguous failures. |
| `<product> menubar install` | Explicitly register next-login startup for the current user. |
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
