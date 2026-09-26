# Product identity on macOS

Status: design accepted on 2026-09-26 for desktop-foundation 0.8.0. It
replaces the "unbundled companions only" rule for macOS: `AGENTS.md` and
`skills/companion/SKILL.md` change in the same pull request that ships the
implementation.

## The problem

macOS names whatever executable it sees. Today a login item shows as `node`,
`bun` or `env`, the menu bar allow-list and Activity Monitor show
`hraness-companion-aarch64-apple-darwin`, the native dialog has a generic
icon, and Rust products show bare executable names. Ad-hoc signatures change
with every build, so each upgrade resets Full Disk Access, Automation and
keychain approvals.

## Design

Each product gets a small app bundle that is built on the person's own Mac,
around the runner the SDK has already verified, and signed with one
persistent local signing identity.

- Path: `~/Applications/Hraness/<Product>.app`, where `<Product>` is the
  registry display name (`Textbutler.app`, `AI Charts.app`).
- Bundle ID: `app.hraness.<appId>`, fixed forever for that product.
- Never distributed. No zip, DMG, cask, release asset or browser download of
  the bundle exists. CI builds bundles only inside tests.
- Never quarantined. The runner arrives through `fetch`, npm or Cargo without
  `com.apple.quarantine`, and a bundle assembled locally has none, so
  Gatekeeper does not assess it and notarization is not needed.

### Bundle contents

```text
<Product>.app/Contents/
  Info.plist
  MacOS/<Product>                the verified runner, copied (never linked)
  Helpers/<helper>               optional product helpers, e.g. ghostget-cookie-reader
  Resources/AppIcon.icns         from the product's 1024 px PNG
  _CodeSignature/
```

The `.icns` file is written directly from PNG data (types `ic07` to `ic10`),
so no `iconutil` run is needed.

### Info.plist

| Key | Value |
| --- | --- |
| `CFBundleIdentifier` | `app.hraness.<appId>` |
| `CFBundleName`, `CFBundleDisplayName` | the display name |
| `CFBundleExecutable` | the display name |
| `CFBundlePackageType` | `APPL` |
| `CFBundleShortVersionString` | the product version |
| `CFBundleVersion` | `<product version>+df.<runner version>` |
| `CFBundleIconFile` | `AppIcon` |
| `LSUIElement` | `true` (no Dock icon) |
| `LSMinimumSystemVersion` | `13.0` |
| `NSHighResolutionCapable` | `true` |
| `HranessRunnerSha256` | digest of the verified runner, for `doctor` |
| `NS*UsageDescription` | only the keys the product declares (below) |

Usage descriptions name the product and give one concrete reason in one
sentence of at most 110 characters, with no jargon:

| Key | Example |
| --- | --- |
| `NSAppleEventsUsageDescription` | Textbutler sends the replies you approve through Messages. |
| `NSContactsUsageDescription` | Textbutler shows your contacts' names instead of phone numbers. Nothing leaves your Mac. |
| `NSLocalNetworkUsageDescription` | Valhalla lets room members on your network connect to this Mac. |
| `NSCameraUsageDescription` | Slopcamera uses the camera only while you record. |
| `NSMicrophoneUsageDescription` | Slopcamera records sound only while you record. |

Full Disk Access, Screen Recording and keychain prompts have no usage string;
the [permission notices](permissions.md) cover them. The bundle is signed
without the hardened runtime, so no Apple Events entitlement is needed.

### Launching through the bundle

Two things decide what macOS names:

- The menu bar item, Activity Monitor and native dialogs use the bundle of
  the running executable. Running `Contents/MacOS/<Product>` as a child with
  pipes, as the SDK does today, is enough.
- Privacy prompts and grants follow the responsible process: the process
  launchd or Launch Services started. A product owner started by
  `/usr/bin/env bun` from a LaunchAgent is attributed to `env`, and one
  started from Terminal to Terminal.

So the runner gains a supervisor mode:

```text
<Product>.app/Contents/MacOS/<Product> --launch <owner-only argv file>
```

It reads the product's command line from an owner-only file in the product's
state directory (never argv, so no values show in `ps`), spawns that command as
a child with `HRANESS_APP_BUNDLE_ID=app.hraness.<appId>` in its environment,
forwards SIGTERM and SIGINT, and exits with the child's status. It stays the
parent on purpose: `exec` would make the child its own responsible process.
The product owner then starts the menu runner from the same bundle path.

Login startup points at the bundle:

- Preferred: the LaunchAgent at
  `~/Library/LaunchAgents/app.hraness.<appId>.plist` with
  `ProgramArguments = [<bundle executable>, "--launch", <argv file>]`,
  `AssociatedBundleIdentifiers = [app.hraness.<appId>]`, `RunAtLoad`, and
  `LimitLoadToSessionType = Aqua`. Login Items then shows the product's name
  and icon.
- To evaluate during implementation: `SMAppService.mainApp` registered from
  inside the bundle, which lists the app under Open at Login. Use it only if
  it works for a locally signed, non-notarized bundle on macOS 13 through 26.

Upgrading replaces the old `app.hraness.companion.<appId>` agent: the SDK
removes the old plist after the new one is written, in the same command.

### Assembly

The runner builds bundles so the SDK and Rust products share one
implementation:

```text
hraness-companion --assemble-app
```

Request on stdin, result on stdout, one line each:

```jsonl
{"type":"app-request","version":1,"appId":"textbutler","name":"Textbutler","productVersion":"1.4.0","iconPath":"/abs/icon-1024.png","usage":{"NSAppleEventsUsageDescription":"Textbutler sends the replies you approve through Messages."},"helpers":[{"name":"textbutler-helper","path":"/abs/helper","sha256":"<hex>"}],"signing":"local"}
{"type":"app-result","version":1,"status":"built","path":"/Users/me/Applications/Hraness/Textbutler.app","signing":"local"}
```

`signing` is `local` (default) or `ad-hoc`. `status` is `built`, `unchanged`
(every input digest matches the installed bundle) or `failed` with a `code`:
`quarantined-input`, `identity-unavailable`, `signing-declined`,
`signing-failed`, `unsafe-destination`, `invalid-app-request`. Rust products
call `desktop_foundation::identity::assemble_app(&AppSpec)` directly.

Steps:

1. Verify the runner and every helper against their pinned SHA-256 digests.
2. Refuse any input that carries `com.apple.quarantine` with
   `quarantined-input`. Never remove the attribute automatically.
3. Build in `~/Applications/Hraness/.<Product>.app.staging-<random>` with
   owner-only permissions.
4. Sign helpers first, then the bundle, with
   `codesign --force --sign <identity SHA-1> --identifier <id> --timestamp=none`.
   Helpers use `app.hraness.<appId>.<helper>`.
5. Run `codesign --verify --strict` on the result.
6. Swap it in with renames. A running copy keeps its old files until restart;
   the SDK restarts the companion afterwards.

`HRANESS_SIGNING=ad-hoc` forces ad-hoc signing, and tests inject a fake
`codesign` and a temporary `HOME`. No test touches the login keychain.

## Signing identity

One identity per Mac user signs every Hraness bundle and helper: the
designated requirement stays
`identifier "app.hraness.<appId>" and certificate leaf = H"<sha1>"` across
upgrades, so privacy and keychain approvals survive them.

| Property | Value |
| --- | --- |
| Common name and label | `Hraness Local Signing` |
| Keychain | the login keychain |
| Key | RSA 2048, created on this Mac, marked non-extractable |
| Certificate | self-signed, Key Usage digitalSignature, Extended Key Usage codeSigning, valid 20 years (renewal would change the requirement and reset approvals) |
| Key access list | `/usr/bin/codesign` only |
| Keychain comment | Signs Hraness apps built on this Mac so macOS keeps their permissions after updates. Delete it to reset. |
| Selection | always by SHA-1 hash, never by name |

Runner commands, each printing one JSON line:

```text
hraness-companion --signing-identity status   {"type":"signing-identity","version":1,"state":"ready"|"missing"|"unavailable","sha1":"…"}
hraness-companion --signing-identity ensure   creates the identity when missing
```

### Prompts this raises

| Step | Prompt | Handling |
| --- | --- | --- |
| Creating the key and certificate in the unlocked login keychain | none expected | If the keychain is locked, macOS asks to unlock it. Show nothing extra; the result is `identity-unavailable` with "Unlock your login keychain, then retry." |
| First signing with the key | expected: "codesign wants to sign using key "Hraness Local Signing" in your keychain", with a password field and Always Allow | Show the `LOCAL_SIGNING` notice from [permissions](permissions.md#local_signingref) first. After Always Allow it does not return for later upgrades. A denial is `signing-declined`; the product falls back to ad-hoc and says permissions will be asked again after updates. |
| Trusting the certificate | not done | Adding trust settings (`security add-trusted-cert`) opens an administrator password dialog and edits system trust policy. The design does not need it: `codesign` signs with an untrusted self-signed identity selected by hash, and `codesign --verify` checks the signature, not the anchor. If implementation shows `codesign` refuses the identity, stop and take the decision to the maintainer; never add trust automatically. |

These prompts were reasoned from macOS keychain behavior and have not been
observed on this design. Before 0.8.0 ships, run the flow once on a clean
macOS user account and record what appears: identity creation, first signing,
an upgrade re-sign, the Login Items entry (`sfltool dumpbtm`), and one
privacy prompt through `--launch`.

### Security trade-off

Any program running as the same user can ask `codesign` to use this key, so
it could sign its own code as a Hraness app and inherit that app's approvals,
including Full Disk Access. Ad-hoc signing ties each approval to one exact
binary, which blocks that, at the cost of re-approval after every update. The
key's access list, non-extractable storage and the one-time Always Allow are
the mitigations in this design. The stronger alternative, asking for approval
on every signing, would show a password prompt at each upgrade.

## Doctor copy

```text
✓ Textbutler.app is signed by Hraness Local Signing
⚠ Textbutler.app is ad-hoc signed, so macOS asks for its permissions again after each update.
→ textbutler menubar install
```

## Windows and Linux

Unchanged. The runner keeps its current naming there; this design is macOS
only.
