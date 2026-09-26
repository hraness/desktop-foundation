// Protocol v2 (menu kit v2) shapes, shared by the SDK and the Rust runner.
// Types only: nothing here is used at runtime yet. The contract, limits and
// rendering rules are in docs/protocol-v2.md; the permission kinds are
// described in docs/permissions.md. Keep the symbol unions in sync with the
// vocabulary tables in docs/protocol-v2.md (sdk/test/protocol-v2.test.ts
// checks this).

export type ProtocolVersionV2 = 2;

/** Symbols for `status` rows. */
export type StatusSymbol =
  | 'status.ok' | 'status.running' | 'status.idle' | 'status.partial' | 'status.syncing'
  | 'status.paused' | 'status.attention' | 'status.error' | 'status.offline'
  | 'status.signedOut' | 'status.locked';

/** Symbols for actions. */
export type ActionSymbol =
  | 'action.open' | 'action.add' | 'action.pause' | 'action.resume' | 'action.refresh'
  | 'action.folder' | 'action.copy' | 'action.settings' | 'action.permission'
  | 'action.signIn' | 'action.signOut' | 'action.update' | 'action.help' | 'action.support';

/** Symbols for content rows (recent chats, files, rooms). */
export type ItemSymbol =
  | 'item.file' | 'item.image' | 'item.chat' | 'item.contact' | 'item.room' | 'item.job'
  | 'item.camera' | 'item.chart' | 'item.agent' | 'item.approval' | 'item.key';

/** Menu-bar marks, one per product. */
export type MarkSymbol =
  | 'mark.chat' | 'mark.masks' | 'mark.drop' | 'mark.dropHalf' | 'mark.people'
  | 'mark.chart' | 'mark.camera' | 'mark.shield' | 'mark.agent';

/** Every name in the closed vocabulary. */
export type MenuSymbol = StatusSymbol | ActionSymbol | ItemSymbol | MarkSymbol;

/** Menu-bar tone. Only `attention` and `error` add a colored dot. */
export type MarkTone = 'normal' | 'attention' | 'error' | 'paused' | 'offline';

/** Template glyph: `alpha` is standard base64 of exactly `width * height` coverage bytes (1..=64 per side). */
export interface AlphaIcon { width: number; height: number; alpha: string }

/** v1 full-color tray art for Windows/Linux, unchanged from protocol v1. */
export interface TrayIconV2 { width: number; height: number; rgba: string }

export interface StatusMark {
  symbol: MarkSymbol;
  templateIcon?: AlphaIcon;
  /** One or two ASCII letters or digits: v1 title and Windows/Linux monogram. */
  letters: string;
  tone?: MarkTone;
  /** 1 to 4 scalar values; allowed only with tone `attention` or `error`. */
  text?: string;
  accessibilityLabel?: string;
}

export type ItemState = 'on' | 'off' | 'mixed';
export type ActionRole = 'primary' | 'destructive';
export type ActionOpens = 'browser' | 'finder' | 'settings' | 'dialog';

export interface AlternateItemV2 { id: string; label: string; symbol?: ActionSymbol | ItemSymbol }

export interface ActionItemV2 {
  kind: 'action';
  id: string;
  label: string;
  enabled?: boolean;
  state?: ItemState;
  symbol?: ActionSymbol | ItemSymbol;
  subtitle?: string;
  badge?: string;
  tooltip?: string;
  shortcut?: string;
  alternate?: AlternateItemV2;
  role?: ActionRole;
  opens?: ActionOpens;
}
export interface HeaderItemV2 { kind: 'header'; label: string }
export interface StatusItemV2 { kind: 'status'; symbol: StatusSymbol; label: string; detail?: string }
export interface LabelItemV2 { kind: 'label'; label: string; subtitle?: string }
export interface SeparatorItemV2 { kind: 'separator' }
export interface SubmenuItemV2 { kind: 'submenu'; label: string; symbol?: ActionSymbol | ItemSymbol; items: readonly MenuItemV2[] }
export interface QuitItemV2 { kind: 'quit'; label: string }

export type MenuItemV2 =
  | ActionItemV2 | HeaderItemV2 | StatusItemV2 | LabelItemV2
  | SeparatorItemV2 | SubmenuItemV2 | QuitItemV2;

export interface SnapshotV2 {
  version: ProtocolVersionV2;
  type: 'snapshot';
  appId: string;
  name: string;
  revision: number;
  mark: StatusMark;
  tooltip?: string;
  icon?: TrayIconV2;
  items: readonly MenuItemV2[];
}

export type RunnerEventV2 =
  | { version: ProtocolVersionV2; type: 'ready'; pid: number; platform: string }
  | { version: ProtocolVersionV2; type: 'already-running' | 'stopped' }
  | { version: ProtocolVersionV2; type: 'validated'; revision: number }
  | { version: ProtocolVersionV2; type: 'action'; id: string; revision: number }
  | { version: ProtocolVersionV2; type: 'error'; code: string };

/** Permission kinds; the wire uses them in `foundation.settings.<kind>` and notice requests. */
export type PermissionKind =
  | 'full-disk-access' | 'automation' | 'contacts' | 'accessibility'
  | 'screen-recording' | 'camera' | 'microphone' | 'local-network'
  | 'incoming-connections' | 'notifications' | 'login-item' | 'keychain'
  | 'developer-tools' | 'gatekeeper';

/** Kinds with a System Settings pane (keychain and developer-tools have none). */
export type SettingsPermissionKind = Exclude<PermissionKind, 'keychain' | 'developer-tools'>;

/** Reserved action IDs the SDK helpers insert and handle. */
export type FoundationActionId = 'foundation.login' | `foundation.settings.${SettingsPermissionKind}`;

/** One-shot `hraness-companion --notice` request. */
export interface NoticeRequest {
  type: 'notice-request';
  version: 1;
  title: string;
  message: string;
  primary: string;
  secondary?: string;
  settings?: SettingsPermissionKind;
  timeoutSeconds?: number;
}
export type NoticeStatus = 'primary' | 'secondary' | 'settings' | 'timeout' | 'unavailable';
export interface NoticeResult { type: 'notice-result'; version: 1; status: NoticeStatus }
