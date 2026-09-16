export const PROTOCOL_VERSION = 1 as const;
export const MAX_FRAME_BYTES = 256 * 1024;
export type MenuItem =
  | { kind: 'action'; id: string; label: string; enabled?: boolean; checked?: boolean; shortcut?: string }
  | { kind: 'label'; label: string }
  | { kind: 'separator' }
  | { kind: 'submenu'; label: string; items: readonly MenuItem[] }
  | { kind: 'quit'; label: string };
/** Adapter-supplied tray art for icon-only surfaces (Windows/Linux). `rgba` is standard base64 and must decode to exactly `width * height * 4` bytes. macOS renders the status-item title natively and ignores this field. */
export interface TrayIcon { width: number; height: number; rgba: string }
export interface CompanionIdentity { appId: string; name: string; title: string; tooltip?: string; icon?: TrayIcon }
export interface Snapshot extends CompanionIdentity { version: 1; type: 'snapshot'; revision: number; items: readonly MenuItem[] }
export type RunnerEvent =
  | { version: 1; type: 'ready'; pid: number; platform: string }
  | { version: 1; type: 'already-running' | 'stopped' }
  | { version: 1; type: 'validated'; revision: number }
  | { version: 1; type: 'action'; id: string; revision: number }
  | { version: 1; type: 'error'; code: string };
const unsafe = /[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u;
// A status-item title is the classic one-or-two ASCII letter badge or a single
// emoji grapheme: one pictographic scalar plus an optional emoji presentation
// selector. Multi-scalar sequences (ZWJ chains, flag pairs) are rejected so
// every platform can bound the rendered mark. Keep the scalar ranges in sync
// with `is_pictographic` in src/protocol.rs.
const TITLE_BADGE = /^[a-zA-Z0-9]{1,2}$/;
const TITLE_EMOJI = /^[\u{2190}-\u{21ff}\u{2300}-\u{23ff}\u{2600}-\u{27bf}\u{2b00}-\u{2bff}\u{1f000}-\u{1faff}]\u{fe0f}?$/u;
const MAX_ICON_SIDE = 64;
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
export function assertAppId(id: string): void {
  if (!/^[a-z][a-z0-9.-]{0,63}$/.test(id) || id.includes('..')) throw new Error('invalid-app-id');
}
function label(value: unknown, max = 256): asserts value is string {
  if (typeof value !== 'string' || !value || [...value].length > max || unsafe.test(value)) throw new Error('invalid-label');
}
function icon(value: unknown): asserts value is TrayIcon {
  exact(value as object, ['width', 'height', 'rgba']);
  const { width, height, rgba } = value as TrayIcon;
  if (!Number.isInteger(width) || width < 1 || width > MAX_ICON_SIDE
    || !Number.isInteger(height) || height < 1 || height > MAX_ICON_SIDE
    || typeof rgba !== 'string' || !BASE64.test(rgba) || rgba.length % 4 !== 0
    || Buffer.from(rgba, 'base64').length !== width * height * 4) throw new Error('invalid-icon');
}
function exact(value: object, fields: readonly string[]): void {
  if (Object.keys(value).some(key => !fields.includes(key))) throw new Error('unknown-protocol-field');
}
export function validateSnapshot(value: Snapshot): ReadonlyMap<string, boolean> {
  exact(value, ['version','type','appId','name','title','tooltip','icon','revision','items']);
  assertAppId(value.appId);
  label(value.name, 128);
  if (!TITLE_BADGE.test(value.title) && !TITLE_EMOJI.test(value.title)) throw new Error('invalid-title');
  if (value.tooltip !== undefined) label(value.tooltip);
  if (value.icon !== undefined) icon(value.icon);
  if (value.version !== 1 || value.type !== 'snapshot' || !Number.isSafeInteger(value.revision) || value.revision < 0) throw new Error('invalid-revision');
  let count = 0;
  const actions = new Map<string, boolean>();
  function visit(items: readonly MenuItem[], depth: number): void {
    if (!Array.isArray(items) || depth > 8) throw new Error('invalid-menu');
    for (const item of items) {
      if (!item || ++count > 256) throw new Error('invalid-menu');
      if (item.kind !== 'separator') label(item.label);
      switch (item.kind) {
        case 'action':
          exact(item, ['kind','id','label','enabled','checked','shortcut']);
          if (!/^[A-Za-z0-9._:-]{1,256}$/.test(item.id) || item.id.startsWith('foundation.') || actions.has(item.id)) throw new Error('invalid-action-id');
          if (item.enabled !== undefined && typeof item.enabled !== 'boolean') throw new Error('invalid-enabled');
          if (item.checked !== undefined && typeof item.checked !== 'boolean') throw new Error('invalid-checked');
          if (item.shortcut !== undefined) label(item.shortcut, 64);
          actions.set(item.id, item.enabled !== false);
          break;
        case 'submenu': exact(item, ['kind','label','items']); visit(item.items, depth + 1); break;
        case 'label': case 'quit': exact(item, ['kind','label']); break;
        case 'separator': exact(item, ['kind']); break;
        default: throw new Error('invalid-menu');
      }
    }
  }
  visit(value.items, 1);
  if (Buffer.byteLength(JSON.stringify(value)) + 1 > MAX_FRAME_BYTES) throw new Error('oversize-frame');
  return actions;
}
export function parseRunnerEvent(line: string): RunnerEvent {
  if (Buffer.byteLength(line) > MAX_FRAME_BYTES) throw new Error('oversize-frame');
  const value: unknown = JSON.parse(line);
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid-runner-event');
  const v = value as Record<string, unknown>;
  if (v.version !== 1) throw new Error('unsupported-protocol');
  const keys: Record<string, string[]> = { ready: ['version','type','pid','platform'], action: ['version','type','id','revision'], error: ['version','type','code'], stopped: ['version','type'], 'already-running': ['version','type'], validated: ['version','type','revision'] };
  const allowed = typeof v.type === 'string' ? keys[v.type] : undefined;
  if (!allowed || Object.keys(v).some(key => !allowed.includes(key))) throw new Error('invalid-runner-event');
  if (v.type === 'ready' && (!Number.isSafeInteger(v.pid) || Number(v.pid) < 1 || typeof v.platform !== 'string')) throw new Error('invalid-runner-event');
  if (v.type === 'action' && (typeof v.id !== 'string' || !/^[A-Za-z0-9._:-]{1,256}$/.test(v.id) || !Number.isSafeInteger(v.revision) || Number(v.revision) < 0)) throw new Error('invalid-runner-event');
  if (v.type === 'error' && (typeof v.code !== 'string' || !/^[a-z0-9-]{1,80}$/.test(v.code))) throw new Error('invalid-runner-event');
  if (v.type === 'validated' && (!Number.isSafeInteger(v.revision) || Number(v.revision) < 0)) throw new Error('invalid-runner-event');
  return v as unknown as RunnerEvent;
}
