# Adopt desktop-foundation in a product CLI

This recipe gives a product one owner process, `status --json`, `tui`, a human
gate and the native helper. It does not replace the product's daemon, provider
adapters or permission helpers. Products upgrading from a menu bar release
start with the [0.9 to 1.0 migration guide](migration-1.0.md).

## 1. Inventory the current behavior

Record the product's actions, daemon connection, singleton identity, startup
registration and stop behavior. Separate UI code from capability helpers. A
Ghostget iMessage helper or a Slopcamera capture helper remains necessary when
it owns the actual API or OS permission flow.

## 2. Pin the released package

Use the SDK archive from an immutable versioned GitHub release and commit the
product's dependency lockfile. The archive contains compiled JavaScript and
the matching binary manifest. Use the actual published version from
[Releases](https://github.com/hraness/desktop-foundation/releases); do not
substitute `latest`, a source checkout, or a local development build. Rust
products pin the same tag through a Cargo git dependency.

The product runtime must support the SDK's Node.js 22+ API requirements. Bun
compatibility must be checked by the consuming product; the Node test matrix
does not establish it. The customer install path must not run Cargo, Swift or
Xcode. A missing platform artifact is a release problem, not a reason to
compile on the customer's machine.

## 3. Register verbs and run one owner

Put every product action behind a verb in `./registry` and give it an
operation class: `read`, `operate` or `decide`. Anything a person owns is
`decide` and declares a gate from `./human-gate`. Keep the product's state in
one owner process (`./control`, started on demand with `ensureOwner`); every
other command is a short-lived client that calls it through `agentRequest` or
`adminRequest` and prints the JSON envelope with `--json`. See
[control](control.md) and [the human gate](human-gate.md).

Offer `status --json` for scripts and agents and `tui` (`./tui`) for a person.
Show pending decisions in `status` rather than in a notification. Read product
state with bounded IO, show unavailable capabilities clearly on unsupported
platforms, and do not retry an uncertain mutation.

## 4. Use the helper for what a terminal cannot do

`./helper` finds and verifies `hraness-helper` from the packaged manifest.
Before an action causes a macOS permission prompt, show the notice
(`--notice`, or `prePrompt` from `./permissions`). When a flow needs a
credential, use `promptSecret(request)`: it shows a native dialog where the
host supports one and falls back to a masked terminal prompt otherwise. Keep
secrets out of `title` and `message` and never pass them as arguments.
Validation, storage and rotation stay with the product.

On macOS a product can assemble and sign its own local app so its permissions
stick across updates; see [product identity](identity.md). Leave that off
until the clean-account check there passes.

## 5. Start at login, only on request

`./login` registers an opt-in login item that starts the owner: a LaunchAgent
on macOS, XDG autostart on Linux, and a Startup script on Windows (which needs
Windows Script Host). Where policy does not allow it, keep the manual start.
Products that shipped a menu bar call `retireLegacyLoginItem` from `./retire`
so the old item stops starting at login; it moves only the exact item the
product installed and deletes nothing.

## 6. Check and ship the product adapter

Run the product's required checks, then exercise the installed package in a
fresh user environment: first download, `status --json`, a harmless verb,
`tui`, stop and a repeated start. Test opt-in login behavior separately.
Record OS/architecture, release and digest.

For OS blocks, use doctor output and the shared
[human approval instructions](installation.md). An agent explains the exact
artifact and available OS control; it does not remove quarantine, disable
security controls or grant approval for the human.

Update the product README and installation docs with the behavior actually
shipped. Claim platform support only for checked product paths, and keep
explicit limitations for platform-specific providers.
