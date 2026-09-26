# Native runner protocol v2 (menu kit v2)

Status: specification for desktop-foundation 0.8.0. Runners and SDKs up to
0.7.x speak only [protocol v1](protocol.md). The TypeScript shapes live in
[`sdk/src/protocol-v2.ts`](../sdk/src/protocol-v2.ts); the Rust wire schema in
[`src/protocol_v2.rs`](../src/protocol_v2.rs) and the symbol table in
[`src/symbols.rs`](../src/symbols.rs) mirror them.

Protocol v2 keeps v1's framing, limits, lifecycle, revision rules, action
handling and error behavior. It changes three things:

1. The menu-bar mark is a monochrome template glyph from a closed vocabulary,
   with a dot or count only when something needs attention.
2. Menus gain headers, status rows, item symbols, subtitles, badges, tooltips,
   three-state toggles, Option-key alternates, a primary role and an `opens`
   hint, so products stop stacking grey labels and nested submenus.
3. A one-shot `--notice` dialog lets a menu explain a macOS permission prompt
   before it appears.

Everything below is additive. A v1 snapshot stays valid on a v2 runner.

## Negotiation

`hraness-companion --version` prints every protocol the runner accepts:

```text
hraness-companion 0.8.0 protocol/1,2
```

The first snapshot's `version` fixes the session version. A later frame with a
different version ends the session with `version-changed`. Runner events use
the session version, so a v2 session receives `{"version":2,"type":"action",…}`.
The SDK sends v2 only after it has read `protocol/…2` from the pinned runner;
otherwise it down-levels (see [Down-level rules](#down-level-rules)).

## Snapshot

```jsonl
{"version":2,"type":"snapshot","appId":"textbutler","name":"Textbutler","revision":7,"mark":{"symbol":"mark.chat","letters":"Tb","tone":"attention","text":"3"},"tooltip":"Textbutler · 3 replies waiting","items":[{"kind":"header","label":"Textbutler"},{"kind":"status","symbol":"status.running","label":"Running","detail":"3 chats on"},{"kind":"separator"},{"kind":"action","id":"open","label":"Open dashboard","symbol":"action.open","role":"primary","opens":"browser","shortcut":"CmdOrCtrl+O"},{"kind":"separator"},{"kind":"action","id":"chat.1","label":"Mom","symbol":"item.chat","subtitle":"Reply waiting for approval","badge":"2","alternate":{"id":"chat.1.copy","label":"Copy chat ID","symbol":"action.copy"}},{"kind":"action","id":"chats.all","label":"Show all chats (23)","opens":"browser"},{"kind":"separator"},{"kind":"action","id":"pause","label":"Pause automatic replies","symbol":"action.pause","state":"off","shortcut":"CmdOrCtrl+P"},{"kind":"action","id":"foundation.login","label":"Open at login","state":"on"},{"kind":"separator"},{"kind":"action","id":"help","label":"Help & support","symbol":"action.support","opens":"browser","alternate":{"id":"help.diagnostics","label":"Copy diagnostics","symbol":"action.copy"}},{"kind":"quit","label":"Quit Textbutler"}]}
```

| Field | Contract |
| --- | --- |
| `version` | Integer `2`. |
| `type` | `"snapshot"`. |
| `appId`, `name`, `revision`, `items` | As in v1. `appId` and `name` stay fixed for the process lifetime. |
| `mark` | Required. The menu-bar glyph (below). Replaces v1 `title`, which v2 rejects. |
| `tooltip` | Optional, 1 to 160 Unicode scalar values. May change between snapshots. |
| `icon` | Optional v1 RGBA tray art for Windows and Linux. macOS ignores it. |

### Mark

| Field | Contract |
| --- | --- |
| `symbol` | Required. A `mark.*` name from the [vocabulary](#symbol-vocabulary). |
| `templateIcon` | Optional `{ "width": W, "height": H, "alpha": "<base64>" }`, W and H `1..=64`, `alpha` decoding to exactly `W*H` bytes of 8-bit coverage. A custom product glyph drawn as a template image. When present it wins over `symbol` on every platform. |
| `letters` | Required. One or two ASCII letters or digits. Used by v1 runners and as the Windows/Linux monogram when neither `templateIcon` nor `icon` is set. |
| `tone` | Optional, default `normal`. One of `normal`, `attention`, `error`, `paused`, `offline`. |
| `text` | Optional, 1 to 4 Unicode scalar values (a count such as `3` or `99+`). Allowed only when `tone` is `attention` or `error`. |
| `accessibilityLabel` | Optional, 1 to 128 scalar values. Defaults to `name`, plus the tooltip when present. |

macOS draws `symbol` with `NSImage(systemSymbolName:)` or `templateIcon`,
always with `isTemplate = true`, so the glyph follows the menu bar's light,
dark and tinted appearance. `text` becomes the status button title to the
right of the glyph. Tones:

| Tone | macOS | Windows/Linux |
| --- | --- | --- |
| `normal` | Template glyph only. | Glyph or monogram. |
| `attention` | Template glyph plus a 5 pt system-orange dot at the lower right. | Orange dot composited. |
| `error` | Template glyph plus a 5 pt system-red dot. | Red dot composited. |
| `paused`, `offline` | Template glyph drawn dimmed (`appearsDisabled`). No dot, no color. | Glyph at 50% opacity. |

Only `attention` and `error` add color. Products show a dot or count only when
the person needs to do something. There is no permanent colored glyph.

## Menu items

Unknown fields are still rejected. All labels follow v1 text rules (nonempty,
no control or bidi characters) plus the tighter lengths below.

| Kind | Required | Optional |
| --- | --- | --- |
| `header` | `label` (≤48) | none |
| `status` | `symbol` (a `status.*` name), `label` (≤48) | `detail` (≤80) |
| `label` | `label` (≤256) | `subtitle` (≤80) |
| `action` | `id`, `label` (≤256) | `enabled`, `state`, `symbol`, `subtitle`, `badge`, `tooltip`, `shortcut`, `alternate`, `role`, `opens` |
| `separator` | none | none |
| `submenu` | `label`, `items` | `symbol` |
| `quit` | `label` | none |

Action fields:

| Field | Contract | macOS 13+ rendering |
| --- | --- | --- |
| `state` | `on`, `off` or `mixed`. Replaces v1 `checked`, which v2 rejects. Omit for a plain action. | `NSControl.StateValue`, including the mixed dash. Like v1, a click never commits the state; the product sends the confirmed state in the next snapshot. |
| `symbol` | An `action.*` or `item.*` name. | 16 pt template SF Symbol as the item image. |
| `subtitle` | 1 to 80 scalar values of secondary text. | `NSMenuItem.subtitle` on macOS 14.4+, otherwise appended as ` · subtitle`. |
| `badge` | 1 to 4 scalar values, for example `3`, `99+`, `New`. | `NSMenuItemBadge` on macOS 14+, otherwise two spaces then the badge. |
| `tooltip` | 1 to 160 scalar values. | `NSMenuItem.toolTip`. |
| `alternate` | `{ "id", "label", "symbol"? }`. `id` follows action ID rules and is unique across the snapshot. | A second item with `isAlternate = true` and the Option modifier, shown while ⌥ is held. |
| `role` | `primary` or `destructive`. | `primary` is placed where the product put it and gets bold emphasis on Windows/Linux. `destructive` adds nothing visual; the product confirms through `--notice` before acting. |
| `opens` | `browser`, `finder`, `settings` or `dialog`. | The renderer appends ` ↗` for `browser` and `…` for `settings` and `dialog`. Labels never carry these glyphs themselves. |

Selecting an alternate emits an ordinary `action` event carrying the
alternate's `id`. Alternates are hidden on Windows and Linux, so a product
never puts an essential action only in an alternate.

### Reserved foundation actions

Action IDs starting with `foundation.` are reserved. Products may include only
these, and only through the SDK helpers that build them:

| ID | Meaning |
| --- | --- |
| `foundation.login` | The standard "Open at login" toggle. The SDK handles it with its login-item code and the `LOGIN_ITEM` notice. |
| `foundation.settings.<kind>` | Opens the System Settings pane for a [permission kind](permissions.md#permission-kinds), for example `foundation.settings.full-disk-access`. The SDK handles it; the product never sees the event. |

## Symbol vocabulary

The name is the API. Products never send raw SF Symbol names or emoji; the
foundation maps each name so every product looks the same. The Unicode
fallback is used on Windows/Linux, in the down-level text forms and in
`renderMenuTree`. A dash means the item shows no glyph there. Adding a name is
a reviewed change to this table and to `sdk/src/protocol-v2.ts`.

### Status (status rows)

| Name | SF Symbol | Fallback | Tint | Use for |
| --- | --- | --- | --- | --- |
| `status.ok` | `checkmark.circle.fill` | ✓ | green | healthy or done |
| `status.running` | `circle.fill` | ● | green | active, connected |
| `status.idle` | `circle` | ○ | none | idle, nothing to do |
| `status.partial` | `circle.lefthalf.filled` | ◐ | none | partly set up |
| `status.syncing` | `arrow.triangle.2.circlepath` | ↻ | none | working or syncing |
| `status.paused` | `pause.circle` | ⏸︎ | none | paused by the person |
| `status.attention` | `exclamationmark.triangle.fill` | ⚠︎ | orange | needs the person |
| `status.error` | `xmark.octagon.fill` | ✕ | red | failed |
| `status.offline` | `circle.slash` | ⊘ | none | service or network down |
| `status.signedOut` | `person.crop.circle.badge.questionmark` | ? | none | signed out |
| `status.locked` | `lock.fill` | 🔒︎ | none | needs a macOS permission |

### Actions

| Name | SF Symbol | Fallback | Use for |
| --- | --- | --- | --- |
| `action.open` | `arrow.up.forward.app` | ↗ | open a dashboard or page |
| `action.add` | `plus.circle` | + | add something |
| `action.pause` | `pause.fill` | ⏸︎ | pause toggle |
| `action.resume` | `play.fill` | ▶︎ | resume toggle |
| `action.refresh` | `arrow.clockwise` | ↻ | refresh now |
| `action.folder` | `folder` | - | open or reveal a folder |
| `action.copy` | `doc.on.doc` | - | copy an ID or diagnostics |
| `action.settings` | `gearshape` | ⚙︎ | product settings |
| `action.permission` | `hand.raised` | - | open Privacy & Security |
| `action.signIn` | `person.crop.circle` | - | sign in |
| `action.signOut` | `rectangle.portrait.and.arrow.right` | - | sign out |
| `action.update` | `arrow.down.circle` | ⤓ | update available |
| `action.help` | `questionmark.circle` | - | docs |
| `action.support` | `heart` | ♡ | help and support |

### Content items

| Name | SF Symbol | Fallback |
| --- | --- | --- |
| `item.file` | `doc` | - |
| `item.image` | `photo` | - |
| `item.chat` | `bubble.left` | - |
| `item.contact` | `person` | - |
| `item.room` | `person.3` | - |
| `item.job` | `clock.arrow.circlepath` | - |
| `item.camera` | `camera` | - |
| `item.chart` | `chart.bar` | - |
| `item.agent` | `sparkles` | ✦ |
| `item.approval` | `checkmark.seal` | - |
| `item.key` | `key` | - |

### Menu-bar marks

Each product has its own mark so no two products share a glyph. A product
that needs a custom shape sends `templateIcon` and keeps its `mark.*` name as
the fallback.

| Name | SF Symbol | Product | `letters` |
| --- | --- | --- | --- |
| `mark.chat` | `bubble.left.and.bubble.right` | Textbutler | `Tb` |
| `mark.masks` | `theatermasks` | Ghostget (until its template ghost ships) | `Gg` |
| `mark.drop` | `drop` | Sponge | `Sp` |
| `mark.dropHalf` | `drop.halffull` | Sponge v2 | `S2` |
| `mark.people` | `person.2` | PeopleBlade | `Pb` |
| `mark.chart` | `chart.bar.xaxis` | AI Charts | `Ac` |
| `mark.camera` | `camera.aperture` | Slopcamera | `Sc` |
| `mark.shield` | `shield.lefthalf.filled` | Valhalla | `Vh` |
| `mark.agent` | `sparkles` | demos and new products | `Hc` |

## Default layout

`menuKit.layout()` in the SDK and the Rust builder produce this order. Rows in
parentheses are optional.

```text
[mark ● 3]                          template glyph, dot and count only when needed
Textbutler                          header: the product name, once
● Running · 3 chats on              1 or 2 status rows
(⚠︎ Couldn't pause replies)          transient action-error row until the next refresh
─────────
↗ Open dashboard            ⌘O      exactly one primary action
─────────
  (up to 5 recent rows)             detail in subtitle; ⌥ alternate for Copy ID or Reveal
  (Show all (23) ↗)                 overflow opens the browser or Finder, never a deep submenu
─────────
  (controls: toggles, ≤3)
✓ Open at login                     foundation.login
─────────
♡ Help & support ↗                  ⌥ Copy diagnostics
  Quit Textbutler           ⌘Q      always last
```

State-specific first rows:

| State | Status row | Primary action |
| --- | --- | --- |
| Starting | `status.syncing` "Starting Textbutler…" | none until ready (the only case without one) |
| Signed out | `status.signedOut` "Signed out" | "Sign in" (`action.signIn`, `opens: browser`) |
| Needs a permission | `status.locked` "Needs Full Disk Access" | "Open Full Disk Access settings" (`foundation.settings.full-disk-access`) |
| Service down | `status.offline` "Textbutler isn't running" | "Start Textbutler" |
| Snapshot failed | `status.attention` "Can't reach Textbutler" with detail "Retrying…" | "Open dashboard" if it still works, else "Help & support" |

## Menu lint

`lintMenu(snapshot)` returns findings. Warnings by default; every finding is
an error under `strict`. The SDK test helper and the `companion lint-menu
--strict <fixture.json>` command run it; CI runs it over every product's state
fixtures (first run, signed out, running, error, empty, maximum accounts).

| Rule | Fails when |
| --- | --- |
| `top-level-count` | More than 10 top-level rows, not counting separators and the header. |
| `depth` | An action sits inside a submenu inside a submenu (wire depth > 2), or any submenu holds the primary action. |
| `primary-count` | Not exactly one `role: primary`. A menu whose only status row uses `status.syncing` (the Starting state) may have none. |
| `status-count` | More than 2 top-level status rows, or a status row below the first separator. |
| `status-repeat` | Two status rows or a status row and its detail repeat the same text. |
| `quit` | No `quit` item, the quit item is not the last top-level item, or its label is not `Quit {name}`. |
| `header` | More than one top-level header, or a top-level header that is not `name`. |
| `raw-text` | A label, subtitle, detail or tooltip contains a path (`/`, `~/`), URL, UUID, 8+ hex characters, a `--flag`, or a CLI invocation (a known command name followed by a subcommand). |
| `sentence-case` | A label or header capitalizes a word after the first that is not in the proper-noun list: the product name, macOS, Messages, Chrome, Safari, Finder, System Settings, Keychain Access, every permission pane name in [permissions](permissions.md#permission-kinds) (such as Full Disk Access), and names the product passes in. All-capital words (ID, URL, PDF) and words with digits pass. |
| `length` | A label longer than 48 scalar values (subtitles 80, tooltips 160). Put detail in `subtitle`. |
| `glyph-in-label` | A label contains an emoji or a vocabulary fallback glyph, or ends with `↗`, `…` or `...`. Use `symbol` and `opens`. |
| `mark-text` | `mark.text` is set while `tone` is `normal`, `paused` or `offline`. |
| `shortcut-repeat` | Two items share a shortcut. |
| `empty-submenu` | A submenu has no items. |

`renderMenuTree(snapshot)` prints the stable text form used in fixtures and
pull requests: two spaces per depth, the vocabulary fallback glyph first, then
the label, ` · subtitle`, badge, the `opens` glyph, the shortcut, and `⌥ label`
on its own line under the item it belongs to.

## Down-level rules

The SDK applies these when the runner reports no `protocol/…2`, and the
Windows/Linux renderer uses the same text forms. v2 runners never down-level
on macOS. Each label is composed in full first (prefix, label, subtitle or
detail, badge, `opens` glyph) and then cut to 256 Unicode scalar values, the
last one replaced by `…`, so a down-leveled snapshot always passes v1
validation. The prefix is `– ` for a mixed state, then the symbol's fallback
glyph and a space: `– ↻ Sync now · 2 left  3 ↗`. The native renderer keeps
the badge and `opens` glyph whole and shortens the label and subtitle instead.

| v2 | v1 result |
| --- | --- |
| `mark` | `title` = `letters`; `templateIcon` and tone are dropped (the dot is lost). |
| `header` | `label` with the same text. |
| `status` | `label` = `{fallback} {label} · {detail}`. |
| `label.subtitle` | appended as ` · subtitle`. |
| `action.symbol` | the fallback glyph and a space are prefixed when the vocabulary defines one; otherwise nothing. |
| `action.subtitle` | appended as ` · subtitle`. |
| `action.badge` | two spaces then the badge. |
| `action.state` | `on` → `checked: true`; `off` → `checked: false`; `mixed` → `checked: false` and a `– ` prefix. |
| `action.opens` | ` ↗` or `…` appended to the label. |
| `action.alternate`, `action.tooltip`, `action.role`, `mark.accessibilityLabel` | dropped. |
| `submenu.symbol` | dropped. |
| `foundation.*` IDs | sent unchanged. The native v1 parser accepts them; the SDK's v1 validator accepts only the reserved IDs its own helpers inserted. |

## Notice dialog (one-shot mode)

`hraness-companion --notice` shows one native alert with product-supplied
buttons and exits. Like `--prompt`, it needs no state directory, lock or tray,
and it reads one frame from stdin and writes one frame to stdout.

```jsonl
{"type":"notice-request","version":1,"title":"Textbutler needs access to Messages","message":"macOS will ask to let Textbutler control Messages. Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.","primary":"Continue","secondary":"Not now","timeoutSeconds":120}
{"type":"notice-result","version":1,"status":"primary"}
```

| Field | Contract |
| --- | --- |
| `title` | Required, 1 to 128 scalar values. |
| `message` | Required, 1 to 512 scalar values. |
| `primary` | Required button label, 1 to 32 scalar values. The default button. |
| `secondary` | Optional button label, 1 to 32 scalar values. The cancel button (Escape). |
| `settings` | Optional [permission kind](permissions.md#permission-kinds) that has a Settings URL. When the person chooses the primary button, the runner also opens that allowlisted pane itself and returns `settings` instead of `primary`. No extra button is added. |
| `timeoutSeconds` | Optional integer 1 to 600, default 120. |

Result `status` is `primary`, `secondary`, `settings` (primary chosen and the
pane opened), `timeout` or `unavailable` (no GUI session; fall back to the CLI copy). New error code:
`invalid-notice`. Inside the [local app](identity.md) the alert shows the
product's name and icon; before that it shows the runner's generic icon.

## New error codes

`version-changed`, `invalid-mark`, `invalid-symbol`, `invalid-state`,
`invalid-alternate`, `invalid-badge`, `invalid-role`, `invalid-opens`,
`invalid-notice`, and `render-mark-failed`. The existing categories are
unchanged.

## Implementation notes for the renderer

muda 0.19 cannot express most v2 fields. The macOS renderer builds the menu
with muda as today, then runs an objc2 pass over the `NSMenu` reached through
`tray.with_inner_tray_icon` → `ns_status_item().menu()` (the pattern
`apply_serif` in `src/bin/hraness-companion.rs` already uses). It matches items
by build order and sets `image`, `subtitle`, `badge`, `toolTip`, `isAlternate`,
section headers and state, guarding each API with `respondsToSelector:` for
older macOS versions. The Georgia title font goes away with v1 marks. The
existing 256-item and 8-level limits are unchanged; the SDK reports a node
budget error with the offending count instead of letting a menu freeze.
