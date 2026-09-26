// Protocol v2 (menu kit v2) shapes, shared by the SDK and the Rust runner,
// plus the SDK's v2 validator, symbol table and down-level rules. The
// contract, limits and rendering rules are in docs/protocol-v2.md; the
// permission kinds are described in docs/permissions.md. Keep the symbol
// unions and MENU_SYMBOLS in sync with the vocabulary tables in
// docs/protocol-v2.md (sdk/test/protocol-v2.test.ts checks this).

import { assertAppId, assertTrayIcon, MAX_FRAME_BYTES, type MenuItem, type Snapshot } from './protocol.js';

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

// ---------------------------------------------------------------------------
// Runtime: symbol table, validator and down-level rules.

/** One vocabulary entry: the SF Symbol macOS draws and the Unicode fallback (null where the table has a dash). */
export interface SymbolInfo { sf: string; fallback: string | null }

/** The closed vocabulary from docs/protocol-v2.md § Symbol vocabulary. Marks have no fallback glyph; they fall back to `letters`. */
export const MENU_SYMBOLS: Readonly<Record<MenuSymbol, SymbolInfo>> = {
  'status.ok': { sf: 'checkmark.circle.fill', fallback: '✓' },
  'status.running': { sf: 'circle.fill', fallback: '●' },
  'status.idle': { sf: 'circle', fallback: '○' },
  'status.partial': { sf: 'circle.lefthalf.filled', fallback: '◐' },
  'status.syncing': { sf: 'arrow.triangle.2.circlepath', fallback: '↻' },
  'status.paused': { sf: 'pause.circle', fallback: '⏸︎' },
  'status.attention': { sf: 'exclamationmark.triangle.fill', fallback: '⚠︎' },
  'status.error': { sf: 'xmark.octagon.fill', fallback: '✕' },
  'status.offline': { sf: 'circle.slash', fallback: '⊘' },
  'status.signedOut': { sf: 'person.crop.circle.badge.questionmark', fallback: '?' },
  'status.locked': { sf: 'lock.fill', fallback: '🔒︎' },
  'action.open': { sf: 'arrow.up.forward.app', fallback: '↗' },
  'action.add': { sf: 'plus.circle', fallback: '+' },
  'action.pause': { sf: 'pause.fill', fallback: '⏸︎' },
  'action.resume': { sf: 'play.fill', fallback: '▶︎' },
  'action.refresh': { sf: 'arrow.clockwise', fallback: '↻' },
  'action.folder': { sf: 'folder', fallback: null },
  'action.copy': { sf: 'doc.on.doc', fallback: null },
  'action.settings': { sf: 'gearshape', fallback: '⚙︎' },
  'action.permission': { sf: 'hand.raised', fallback: null },
  'action.signIn': { sf: 'person.crop.circle', fallback: null },
  'action.signOut': { sf: 'rectangle.portrait.and.arrow.right', fallback: null },
  'action.update': { sf: 'arrow.down.circle', fallback: '⤓' },
  'action.help': { sf: 'questionmark.circle', fallback: null },
  'action.support': { sf: 'heart', fallback: '♡' },
  'item.file': { sf: 'doc', fallback: null },
  'item.image': { sf: 'photo', fallback: null },
  'item.chat': { sf: 'bubble.left', fallback: null },
  'item.contact': { sf: 'person', fallback: null },
  'item.room': { sf: 'person.3', fallback: null },
  'item.job': { sf: 'clock.arrow.circlepath', fallback: null },
  'item.camera': { sf: 'camera', fallback: null },
  'item.chart': { sf: 'chart.bar', fallback: null },
  'item.agent': { sf: 'sparkles', fallback: '✦' },
  'item.approval': { sf: 'checkmark.seal', fallback: null },
  'item.key': { sf: 'key', fallback: null },
  'mark.chat': { sf: 'bubble.left.and.bubble.right', fallback: null },
  'mark.masks': { sf: 'theatermasks', fallback: null },
  'mark.drop': { sf: 'drop', fallback: null },
  'mark.dropHalf': { sf: 'drop.halffull', fallback: null },
  'mark.people': { sf: 'person.2', fallback: null },
  'mark.chart': { sf: 'chart.bar.xaxis', fallback: null },
  'mark.camera': { sf: 'camera.aperture', fallback: null },
  'mark.shield': { sf: 'shield.lefthalf.filled', fallback: null },
  'mark.agent': { sf: 'sparkles', fallback: null },
};

const SETTINGS_KINDS: readonly SettingsPermissionKind[] = [
  'full-disk-access', 'automation', 'contacts', 'accessibility', 'screen-recording', 'camera', 'microphone',
  'local-network', 'incoming-connections', 'notifications', 'login-item', 'gatekeeper',
];
/** Every reserved action ID the SDK knows how to handle. */
export const FOUNDATION_ACTION_IDS: ReadonlySet<FoundationActionId> = new Set<FoundationActionId>([
  'foundation.login', ...SETTINGS_KINDS.map(kind => `foundation.settings.${kind}` as const),
]);
export function isFoundationActionId(id: string): id is FoundationActionId { return FOUNDATION_ACTION_IDS.has(id as FoundationActionId); }

/** The vocabulary fallback glyph for a symbol, or null when it shows no glyph there. */
export function symbolFallback(symbol: MenuSymbol | undefined): string | null {
  return symbol ? MENU_SYMBOLS[symbol]?.fallback ?? null : null;
}

const isSymbol = (value: unknown, prefixes: readonly string[]): boolean =>
  typeof value === 'string' && Object.hasOwn(MENU_SYMBOLS, value) && prefixes.some(prefix => value.startsWith(prefix));
const scalars = (text: string): number => [...text].length;
const UNSAFE = /[\u0000-\u001f\u007f-\u009f‪-‮⁦-⁩]/u;
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
const ACTION_ID = /^[A-Za-z0-9._:-]{1,256}$/;
const TONES: readonly MarkTone[] = ['normal', 'attention', 'error', 'paused', 'offline'];
const STATES: readonly ItemState[] = ['on', 'off', 'mixed'];
const ROLES: readonly ActionRole[] = ['primary', 'destructive'];
const OPENS: readonly ActionOpens[] = ['browser', 'finder', 'settings', 'dialog'];
/** The wire limits: more nodes than this, or deeper nesting, freezes no menu; the runner rejects it. */
export const MENU_NODE_BUDGET = 256;
export const MENU_DEPTH_BUDGET = 8;

function text(value: unknown, max: number, code = 'invalid-label'): asserts value is string {
  if (typeof value !== 'string' || !value || scalars(value) > max || UNSAFE.test(value)) throw new Error(code);
}
function exactFields(value: object, fields: readonly string[]): void {
  if (Object.keys(value).some(key => !fields.includes(key))) throw new Error('unknown-protocol-field');
}
function validateMark(mark: StatusMark): void {
  if (!mark || typeof mark !== 'object' || Array.isArray(mark)) throw new Error('invalid-mark');
  exactFields(mark, ['symbol', 'templateIcon', 'letters', 'tone', 'text', 'accessibilityLabel']);
  if (!isSymbol(mark.symbol, ['mark.'])) throw new Error('invalid-symbol');
  if (typeof mark.letters !== 'string' || !/^[A-Za-z0-9]{1,2}$/.test(mark.letters)) throw new Error('invalid-mark');
  if (mark.tone !== undefined && !TONES.includes(mark.tone)) throw new Error('invalid-mark');
  if (mark.text !== undefined) {
    text(mark.text, 4, 'invalid-mark');
    if (mark.tone !== 'attention' && mark.tone !== 'error') throw new Error('invalid-mark');
  }
  if (mark.accessibilityLabel !== undefined) text(mark.accessibilityLabel, 128, 'invalid-mark');
  if (mark.templateIcon !== undefined) {
    const icon = mark.templateIcon;
    if (!icon || typeof icon !== 'object') throw new Error('invalid-mark');
    exactFields(icon, ['width', 'height', 'alpha']);
    const { width, height, alpha } = icon;
    if (!Number.isInteger(width) || width < 1 || width > 64 || !Number.isInteger(height) || height < 1 || height > 64
      || typeof alpha !== 'string' || !BASE64.test(alpha) || alpha.length % 4 !== 0
      || Buffer.from(alpha, 'base64').length !== width * height) throw new Error('invalid-mark');
  }
}

/** Counts every wire node, the way the runner does, so a budget error can name the real size. */
export function countMenuNodes(items: readonly { kind: string; items?: readonly unknown[] }[]): number {
  let count = 0;
  const visit = (list: readonly { kind: string; items?: readonly unknown[] }[]) => {
    for (const item of list) { count++; if (item && item.kind === 'submenu' && Array.isArray(item.items)) visit(item.items as never); }
  };
  if (Array.isArray(items)) visit(items);
  return count;
}

/**
 * Validates a v2 snapshot the way the v2 runner will and returns every action
 * ID (alternates included) with whether it is enabled. Reserved
 * `foundation.*` IDs are accepted only when the SDK handles them. A menu over
 * the node budget throws `menu-too-large: N items (limit 256)`.
 */
export function validateSnapshotV2(value: SnapshotV2): ReadonlyMap<string, boolean> {
  if (!value || typeof value !== 'object') throw new Error('invalid-snapshot');
  exactFields(value, ['version', 'type', 'appId', 'name', 'revision', 'mark', 'tooltip', 'icon', 'items']);
  assertAppId(value.appId);
  text(value.name, 128);
  if (value.version !== 2 || value.type !== 'snapshot' || !Number.isSafeInteger(value.revision) || value.revision < 0) throw new Error('invalid-revision');
  validateMark(value.mark);
  if (value.tooltip !== undefined) text(value.tooltip, 160);
  if (value.icon !== undefined) assertTrayIcon(value.icon);
  const total = countMenuNodes(value.items as never);
  if (total > MENU_NODE_BUDGET) throw new Error(`menu-too-large: ${total} items (limit ${MENU_NODE_BUDGET})`);
  const actions = new Map<string, boolean>();
  const claim = (id: unknown) => {
    if (typeof id !== 'string' || !ACTION_ID.test(id) || actions.has(id) || (id.startsWith('foundation.') && !isFoundationActionId(id))) throw new Error('invalid-action-id');
  };
  function visit(items: readonly MenuItemV2[], depth: number): void {
    if (!Array.isArray(items) || depth > MENU_DEPTH_BUDGET) throw new Error('invalid-menu');
    for (const item of items) {
      if (!item || typeof item !== 'object') throw new Error('invalid-menu');
      switch (item.kind) {
        case 'header': exactFields(item, ['kind', 'label']); text(item.label, 48); break;
        case 'status':
          exactFields(item, ['kind', 'symbol', 'label', 'detail']);
          if (!isSymbol(item.symbol, ['status.'])) throw new Error('invalid-symbol');
          text(item.label, 48);
          if (item.detail !== undefined) text(item.detail, 80);
          break;
        case 'label':
          exactFields(item, ['kind', 'label', 'subtitle']);
          text(item.label, 256);
          if (item.subtitle !== undefined) text(item.subtitle, 80);
          break;
        case 'action': {
          exactFields(item, ['kind', 'id', 'label', 'enabled', 'state', 'symbol', 'subtitle', 'badge', 'tooltip', 'shortcut', 'alternate', 'role', 'opens']);
          claim(item.id);
          text(item.label, 256);
          if (item.enabled !== undefined && typeof item.enabled !== 'boolean') throw new Error('invalid-enabled');
          if (item.state !== undefined && !STATES.includes(item.state)) throw new Error('invalid-state');
          if (item.symbol !== undefined && !isSymbol(item.symbol, ['action.', 'item.'])) throw new Error('invalid-symbol');
          if (item.subtitle !== undefined) text(item.subtitle, 80);
          if (item.badge !== undefined) text(item.badge, 4, 'invalid-badge');
          if (item.tooltip !== undefined) text(item.tooltip, 160);
          if (item.shortcut !== undefined) text(item.shortcut, 64);
          if (item.role !== undefined && !ROLES.includes(item.role)) throw new Error('invalid-role');
          if (item.opens !== undefined && !OPENS.includes(item.opens)) throw new Error('invalid-opens');
          actions.set(item.id, item.enabled !== false);
          if (item.alternate !== undefined) {
            const alternate = item.alternate;
            if (!alternate || typeof alternate !== 'object') throw new Error('invalid-alternate');
            try {
              exactFields(alternate, ['id', 'label', 'symbol']);
              claim(alternate.id);
              text(alternate.label, 256);
              if (alternate.symbol !== undefined && !isSymbol(alternate.symbol, ['action.', 'item.'])) throw new Error('invalid-symbol');
            } catch { throw new Error('invalid-alternate'); }
            actions.set(alternate.id, item.enabled !== false);
          }
          break;
        }
        case 'submenu':
          exactFields(item, ['kind', 'label', 'items', 'symbol']);
          text(item.label, 256);
          if (item.symbol !== undefined && !isSymbol(item.symbol, ['action.', 'item.'])) throw new Error('invalid-symbol');
          visit(item.items, depth + 1);
          break;
        case 'quit': exactFields(item, ['kind', 'label']); text(item.label, 256); break;
        case 'separator': exactFields(item, ['kind']); break;
        default: throw new Error('invalid-menu');
      }
    }
  }
  visit(value.items, 1);
  if (Buffer.byteLength(JSON.stringify(value)) + 1 > MAX_FRAME_BYTES) throw new Error('oversize-frame');
  return actions;
}

/** Cuts composed text to `max` scalar values, replacing the last one with `…`. */
export function fitText(value: string, max: number): string {
  const chars = [...value];
  return chars.length <= max ? value : chars.slice(0, max - 1).join('') + '…';
}

const OPENS_GLYPH: Readonly<Record<ActionOpens, string>> = { browser: ' ↗', finder: '', settings: '…', dialog: '…' };
/** The glyph the renderer appends for an `opens` hint. */
export function opensGlyph(opens: ActionOpens | undefined): string { return opens ? OPENS_GLYPH[opens] : ''; }

function downlevelItem(item: MenuItemV2): MenuItem {
  const fit = (value: string) => fitText(value, 256);
  switch (item.kind) {
    case 'header': return { kind: 'label', label: fit(item.label) };
    case 'status': return { kind: 'label', label: fit(`${symbolFallback(item.symbol)} ${item.label}${item.detail ? ` · ${item.detail}` : ''}`) };
    case 'label': return { kind: 'label', label: fit(`${item.label}${item.subtitle ? ` · ${item.subtitle}` : ''}`) };
    case 'separator': return { kind: 'separator' };
    case 'quit': return { kind: 'quit', label: fit(item.label) };
    case 'submenu': return { kind: 'submenu', label: fit(item.label), items: item.items.map(downlevelItem) };
    case 'action': {
      const glyph = symbolFallback(item.symbol);
      const label = `${item.state === 'mixed' ? '– ' : ''}${glyph ? `${glyph} ` : ''}${item.label}`
        + `${item.subtitle ? ` · ${item.subtitle}` : ''}${item.badge ? `  ${item.badge}` : ''}${opensGlyph(item.opens)}`;
      return {
        kind: 'action', id: item.id, label: fit(label),
        ...(item.enabled !== undefined ? { enabled: item.enabled } : {}),
        ...(item.state !== undefined ? { checked: item.state === 'on' } : {}),
        ...(item.shortcut !== undefined ? { shortcut: item.shortcut } : {}),
      };
    }
  }
}

/** Down-levels a v2 snapshot for a runner that reports no `protocol/…2` (docs/protocol-v2.md § Down-level rules). */
export function downlevelSnapshot(value: SnapshotV2): Snapshot {
  return {
    version: 1, type: 'snapshot', appId: value.appId, name: value.name, title: value.mark.letters,
    ...(value.tooltip !== undefined ? { tooltip: fitText(value.tooltip, 256) } : {}),
    ...(value.icon !== undefined ? { icon: value.icon } : {}),
    revision: value.revision, items: value.items.map(downlevelItem),
  };
}

/** Protocol versions from `hraness-companion --version` output such as `hraness-companion 0.8.0 protocol/1,2`. */
export function parseRunnerProtocols(versionOutput: string): number[] {
  const match = /\bprotocol\/([0-9]+(?:,[0-9]+)*)\b/.exec(versionOutput);
  return match ? match[1]!.split(',').map(Number) : [1];
}
