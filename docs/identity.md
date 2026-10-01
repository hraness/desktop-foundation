# Product identity on macOS

A product can assemble a local macOS app so dialogs and login startup use its
name and icon. A persistent signing identity gives successive builds the same
code identity; macOS decides which permissions apply to the running product.
The local app is separate from the downloaded helper's publisher signature.

The product must explicitly request assembly and signing. Before enabling this
path for users, test identity creation, signing, an update, login startup and a
privacy request in a fresh macOS user account. Automated tests with substitute
keychain and signing tools do not establish those OS interactions.

## App location and identity

The helper builds `~/Applications/Hraness/<Product>.app` on the user's Mac.
The display name identifies the product, such as `Textbutler.app` or
`AI Charts.app`, and the bundle ID is `app.hraness.<appId>`.
Keep that ID stable across updates.

The bundle is assembled locally and is not distributed as a download. Assembly
refuses inputs with `com.apple.quarantine` and does not clear that attribute.
Local assembly does not exempt software from Gatekeeper or device policy;
follow the [installation guidance](installation.md) when macOS blocks a launch.

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
| `NSContactsUsageDescription` | Textbutler shows your contacts' names instead of phone numbers. |
| `NSLocalNetworkUsageDescription` | Valhalla lets room members on your network connect to this Mac. |
| `NSCameraUsageDescription` | Slopcamera uses the camera only while you record. |
| `NSMicrophoneUsageDescription` | Slopcamera records sound only while you record. |

Full Disk Access, Screen Recording and keychain prompts have no usage string;
the [permission notices](permissions.md) cover them. The bundle is signed
without the hardened runtime, so no Apple Events entitlement is needed.

### Launching through the bundle

Running the helper from `Contents/MacOS/<Product>` supplies the app's bundle
metadata. Privacy attribution also depends on the requested permission,
launch method and macOS behavior. Check the displayed product name, Login
Items entry and privacy request using the product's actual launch path in a
fresh user account; the executable's display name alone does not establish
which process receives a permission.

The helper launches the product through a supervisor mode:

```text
<Product>.app/Contents/MacOS/<Product> --launch <owner-only argv file>
```

It reads the product's command line from an owner-only file in the product's
state directory and spawns that command as a child with
`HRANESS_APP_BUNDLE_ID=app.hraness.<appId>` in its environment. The supervisor
remains the child's parent, forwards signals and collects its exit status.
The product's helper dialogs use the same bundle path. The child
receives the stored command and arguments, so keep credentials out of that
command line. The file protects the stored launch configuration; it does not
hide the child's arguments from process inspection.

Login startup uses a LaunchAgent at
`~/Library/LaunchAgents/app.hraness.<appId>.plist`. Its `ProgramArguments` name
the bundle executable, `--launch` and the argv file. It declares
`AssociatedBundleIdentifiers = [app.hraness.<appId>]`, `RunAtLoad` and
`LimitLoadToSessionType = Aqua`. These fields describe the product and request
startup in the user's graphical session. Confirm the displayed Login Items
entry during the fresh-user check.

Upgrading replaces the old `app.hraness.companion.<appId>` agent: the SDK
removes the old plist after the new one is written, in the same command.

### Assembly

The runner builds bundles so the SDK and Rust products share one
implementation:

```text
hraness-helper --assemble-app
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
   the product restarts its owner afterwards.

`HRANESS_SIGNING=ad-hoc` forces ad-hoc signing, and tests inject a fake
`codesign` and a temporary `HOME`. No test touches the login keychain.

## Signing identity

One identity per Mac user signs every Hraness bundle and helper: the
designated requirement stays
`identifier "app.hraness.<appId>" and certificate leaf = H"<sha1>"` across
upgrades. This keeps the signing identity stable; it does not grant a
permission or guarantee that macOS will preserve an approval.

| Property | Value |
| --- | --- |
| Common name and label | `Hraness Local Signing` |
| Keychain | the login keychain |
| Key | RSA 2048, created on this Mac, marked non-extractable |
| Certificate | self-signed, Key Usage digitalSignature, Extended Key Usage codeSigning, valid 20 years |
| Key access list | `/usr/bin/codesign` only |
| Selection | always by SHA-1 hash, never by name |

Runner commands, each printing one JSON line:

```text
hraness-helper --signing-identity status   {"type":"signing-identity","version":1,"state":"ready"|"missing"|"unavailable","sha1":"…"}
hraness-helper --signing-identity ensure   creates the identity when missing
```

### Creating and using the identity

Show the shared [local signing notice](permissions.md#local_signingref) before
requesting identity creation or use. A locked keychain may require the person
to unlock it, and macOS may ask permission for `codesign` to use the key.
If creation or signing fails, report the returned error and let the person
choose the next step. The helper does not add certificate trust settings.

Assembly with `signing: "local"` requires an existing identity. A missing one
returns `identity-unavailable`; assembly does not create it automatically.
An explicit `--signing-identity ensure` call creates it when missing.
`signing: "ad-hoc"` is a separate choice whose code identity changes when the
signed executable changes, so permissions may need to be granted again after
an update.

### Signing and process trust

A persistent signing identity gives builds a consistent code identity. It
does not establish a security boundary between processes running as the same
user. A same-user process can invoke `codesign` and access user-owned
configuration; keychain access rules still govern use of the key.

Changing a certificate or using an ad-hoc signature changes code identity.
The effect on a particular privacy approval depends on macOS and the
permission involved. Neither signing mode proves a caller is trusted or
replaces the product's authorization checks.

## Launch file limits

`--launch` must run inside a Hraness app. The argv file must be an owner-only
JSON array of 1 to 64 strings, at most 64 KiB, whose first entry is an absolute
executable path. Invalid input returns `invalid-launch-file`; a child that
cannot start returns `launch-failed`. The supervisor forwards SIGTERM, SIGINT
and SIGHUP and returns the child's exit code, or 128 plus its terminating
signal.

Rust products use `identity::login_item` with `service::plan` and
`service::install` to register that launch command. `identity::doctor_line`
provides the diagnostic text below.

## Doctor copy

```text
✓ Textbutler.app is signed by Hraness Local Signing
⚠ Textbutler.app is ad-hoc signed. macOS may ask for its permissions again after an update.
→ textbutler control install
```

## Windows and Linux

Local app assembly and signing are macOS-only capabilities. Windows and Linux
products use the portable helper and their own platform permission flows.
