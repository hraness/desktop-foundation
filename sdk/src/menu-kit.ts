// Menu kit v2 helpers: the default layout, the standard rows the foundation
// owns, `lintMenu`, `renderMenuTree` and a fixture helper for product tests.
// The rules are in docs/protocol-v2.md § Default layout and § Menu lint.
import type { MenuItem, Snapshot } from './protocol.js';
import {
  fitText, opensGlyph, symbolFallback, validateSnapshotV2,
  type ActionItemV2, type ItemState, type LabelItemV2, type MenuItemV2, type SnapshotV2, type StatusItemV2,
} from './protocol-v2.js';

// ---------------------------------------------------------------------------
// Standard rows

/** Subtitle on the "Open at login" row while it is off (docs/permissions.md § Dialog). */
export const OPEN_AT_LOGIN_SUBTITLE = 'macOS shows a notice when you turn this on';

/**
 * The foundation-owned "Open at login" toggle. The SDK fills in `state` from
 * the login item it manages and handles the click itself; the product only
 * places the row (or lets `layout` place it).
 */
export function openAtLoginItem(state: ItemState = 'off'): ActionItemV2 {
  return { kind: 'action', id: 'foundation.login', label: 'Open at login', state, ...(state === 'on' ? {} : { subtitle: OPEN_AT_LOGIN_SUBTITLE }) };
}

/** The status row the SDK shows after a menu action fails, until a later refresh. */
/** Collapses control characters and runs of whitespace so any error text is one valid menu line. */
function menuLine(value: string | undefined): string {
  return (value ?? '').replace(/[\p{Cc}\p{Cf}\s]+/gu, ' ').trim();
}

/**
 * A ⚠︎ status row for a failed action. Text is flattened to one line; an
 * empty message falls back to "Something went wrong".
 */
export function actionErrorItem(message: string, detail?: string): StatusItemV2 {
  const label = menuLine(message) || 'Something went wrong';
  const line = menuLine(detail);
  return { kind: 'status', symbol: 'status.attention', label: fitText(label, 48), ...(line ? { detail: fitText(line, 80) } : {}) };
}

export interface DegradedMenuOptions {
  /** Status detail; default "Retrying…". */
  detail?: string;
  /** The one action that still works without product state, such as "Open dashboard" or "Help & support". */
  primary?: ActionItemV2;
  /** Keep the foundation "Open at login" row. */
  openAtLogin?: ItemState;
}

/** The menu shown when the product's state can't be read: "⚠︎ Can't reach {name} · Retrying…". */
export function degradedMenu(name: string, options: DegradedMenuOptions = {}): MenuItemV2[] {
  return layout({
    name,
    status: { kind: 'status', symbol: 'status.attention', label: fitText(`Can't reach ${name}`, 48), detail: options.detail ?? 'Retrying…' },
    ...(options.primary ? { primary: options.primary } : {}),
    ...(options.openAtLogin ? { openAtLogin: options.openAtLogin } : {}),
  });
}

// ---------------------------------------------------------------------------
// Layout

export interface MenuLayout {
  /** Product name: the header and "Quit {name}". */
  name: string;
  /** One or two status rows (permission rows from `permissionMenuItems` may follow). */
  status: StatusItemV2 | readonly MenuItemV2[];
  /** Exactly one primary action; `role: 'primary'` is set for you. Omit only while starting. */
  primary?: ActionItemV2;
  /** Recent or context rows, at most `recentLimit` (default 5) are kept. */
  recent?: readonly (ActionItemV2 | LabelItemV2)[];
  recentLimit?: number;
  /** "Show all (23)" row that opens the browser or Finder. */
  overflow?: ActionItemV2;
  /** Toggles and controls, three at most. */
  controls?: readonly ActionItemV2[];
  /** Adds the foundation "Open at login" row after the controls. */
  openAtLogin?: boolean | ItemState;
  /** "Help & support" and similar, just above Quit. */
  help?: ActionItemV2;
  /** Quit label; default "Quit {name}". */
  quitLabel?: string;
}

/**
 * Builds the default menu order: header, status rows, the primary action,
 * recent rows and overflow, controls and "Open at login", help, and Quit last.
 * Separators go only between groups that have rows.
 */
export function layout(input: MenuLayout): MenuItemV2[] {
  const groups: MenuItemV2[][] = [];
  groups.push([{ kind: 'header', label: fitText(input.name, 48) }, ...(Array.isArray(input.status) ? input.status : [input.status as StatusItemV2])]);
  if (input.primary) groups.push([{ ...input.primary, role: 'primary' }]);
  const recent = (input.recent ?? []).slice(0, input.recentLimit ?? 5);
  groups.push([...recent, ...(input.overflow ? [input.overflow] : [])]);
  const login = input.openAtLogin === undefined || input.openAtLogin === false ? [] : [openAtLoginItem(input.openAtLogin === true ? 'off' : input.openAtLogin)];
  groups.push([...(input.controls ?? []).slice(0, 3), ...login]);
  groups.push([...(input.help ? [input.help] : [])]);
  const items: MenuItemV2[] = [];
  for (const group of groups) {
    if (!group.length) continue;
    if (items.length) items.push({ kind: 'separator' });
    items.push(...group);
  }
  // The header group always leads; Quit shares the help group or stands alone after a separator.
  if (!input.help) items.push({ kind: 'separator' });
  items.push({ kind: 'quit', label: input.quitLabel ?? `Quit ${input.name}` });
  return items;
}

// ---------------------------------------------------------------------------
// Tree rendering

type AnyItem = MenuItemV2 | MenuItem;
type AnySnapshot = SnapshotV2 | Snapshot;

function shortcutText(shortcut: string): string {
  return shortcut.split('+').map(part => ({ cmdorctrl: '⌘', cmd: '⌘', command: '⌘', super: '⌘', shift: '⇧', alt: '⌥', option: '⌥', ctrl: '⌃', control: '⌃' } as Record<string, string>)[part.toLowerCase()] ?? part).join('');
}

/**
 * The stable text form for fixtures and pull requests: two spaces per depth,
 * the fallback glyph, the label, ` · subtitle`, badge, the `opens` glyph, the
 * shortcut, and `⌥ label` on its own line under its item.
 */
export function renderMenuTree(snapshot: AnySnapshot): string {
  const lines: string[] = [];
  if (snapshot.version === 2) {
    const mark = snapshot.mark;
    const tone = mark.tone && mark.tone !== 'normal' ? `, ${mark.tone}` : '';
    lines.push(`[${mark.symbol} ${mark.letters}${tone}${mark.text ? `, ${mark.text}` : ''}]${snapshot.tooltip ? ` ${snapshot.tooltip}` : ''}`);
  } else lines.push(`[${snapshot.title}]${snapshot.tooltip ? ` ${snapshot.tooltip}` : ''}`);
  const visit = (items: readonly AnyItem[], depth: number) => {
    const pad = '  '.repeat(depth);
    for (const item of items) {
      switch (item.kind) {
        case 'separator': lines.push(`${pad}─────────`); break;
        case 'header': lines.push(`${pad}${item.label}`); break;
        case 'status': lines.push(`${pad}${symbolFallback(item.symbol)} ${item.label}${item.detail ? ` · ${item.detail}` : ''}`); break;
        case 'label': lines.push(`${pad}${item.label}${'subtitle' in item && item.subtitle ? ` · ${item.subtitle}` : ''} (label)`); break;
        case 'quit': lines.push(`${pad}${item.label}`); break;
        case 'submenu': {
          const glyph = 'symbol' in item ? symbolFallback(item.symbol) : null;
          lines.push(`${pad}${glyph ? `${glyph} ` : ''}${item.label} ▸`);
          visit(item.items as readonly AnyItem[], depth + 1);
          break;
        }
        case 'action': {
          const v2 = item as ActionItemV2;
          const v1 = item as Extract<MenuItem, { kind: 'action' }>;
          const state = v2.state ?? (v1.checked === undefined ? undefined : v1.checked ? 'on' : 'off');
          const mark = state === 'on' ? '✓ ' : state === 'mixed' ? '– ' : '';
          const glyph = symbolFallback(v2.symbol);
          let line = `${pad}${mark}${glyph ? `${glyph} ` : ''}${item.label}${v2.subtitle ? ` · ${v2.subtitle}` : ''}${v2.badge ? `  ${v2.badge}` : ''}${opensGlyph(v2.opens)}`;
          if (v2.role) line += ` (${v2.role})`;
          if (item.enabled === false) line += ' (disabled)';
          if (item.shortcut) line += `  ${shortcutText(item.shortcut)}`;
          lines.push(line);
          if (v2.alternate) lines.push(`${pad}  ⌥ ${v2.alternate.label}`);
          break;
        }
      }
    }
  };
  visit(snapshot.items as readonly AnyItem[], 0);
  return lines.join('\n') + '\n';
}

// ---------------------------------------------------------------------------
// Lint

export type MenuLintRule =
  | 'top-level-count' | 'depth' | 'primary-count' | 'status-count' | 'status-repeat' | 'quit' | 'header'
  | 'raw-text' | 'sentence-case' | 'length' | 'glyph-in-label' | 'mark-text' | 'shortcut-repeat' | 'empty-submenu';

export interface MenuLintFinding {
  rule: MenuLintRule;
  severity: 'warning' | 'error';
  message: string;
  /** Where it is: "items[3]" or "items[5].items[0].subtitle". */
  path: string;
}

export interface MenuLintOptions {
  /** Every finding becomes an error. */
  strict?: boolean;
  /** Extra proper nouns the product uses (contact names, service names). */
  properNouns?: readonly string[];
  /** CLI names whose invocations must not appear in menu text; the product's lowercase name is always included. */
  commands?: readonly string[];
}

/** Proper nouns every menu may capitalize: macOS names and every permission pane name. */
export const MENU_PROPER_NOUNS: readonly string[] = [
  'macOS', 'Mac', 'Messages', 'Chrome', 'Safari', 'Finder', 'System Settings', 'Keychain Access', 'Privacy & Security',
  'Full Disk Access', 'Automation', 'Contacts', 'Accessibility', 'Screen & System Audio Recording', 'Camera',
  'Microphone', 'Local Network', 'Firewall', 'Notifications', 'Login Items & Extensions', 'Login Items', 'Hraness',
];

const FALLBACK_GLYPHS = ['✓', '●', '○', '◐', '↻', '⏸', '⚠', '✕', '⊘', '🔒', '↗', '▶', '⚙', '⤓', '♡', '✦'];
const EMOJI = /\p{Extended_Pictographic}/u;
const UUID = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/i;
const HEX = /\b(?=[0-9a-f]*[a-f])(?=[0-9a-f]*[0-9])[0-9a-f]{8,}\b/i;
const URL_TEXT = /\b[a-z][a-z0-9+.-]*:\/\/|\bwww\./i;
const PATH_TEXT = /(^|[\s("'])(~\/|\.{1,2}\/|\/[^\s/])|\S\/\S*\/\S|\b[\w.-]+\/[\w.-]+\.[a-z0-9]{1,5}\b/i;
const FLAG = /(^|\s)--?[a-z][a-z0-9-]*/i;

function escape(text: string): string { return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

/**
 * The first word that breaks sentence case: a capitalized word that does not
 * start a sentence, is not all capitals, has no digits and is not part of a
 * proper noun.
 */
function titleCaseWord(value: string, nouns: readonly string[]): string | undefined {
  const exempt: [number, number][] = [];
  for (const noun of nouns) {
    for (const match of value.matchAll(new RegExp(`(?<![\\p{L}\\p{N}])${escape(noun)}(?![\\p{L}\\p{N}])`, 'gu'))) exempt.push([match.index, match.index + noun.length]);
  }
  for (const match of value.matchAll(/\S+/g)) {
    if (match.index === 0 || /[.!?:]\s+$/.test(value.slice(0, match.index))) continue;
    const start = match.index + Math.max(0, match[0].search(/[\p{L}\p{N}]/u));
    if (exempt.some(([from, to]) => start >= from && start < to)) continue;
    const bare = match[0].replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '');
    if (!bare || /\d/.test(bare) || bare === bare.toUpperCase() || !/^\p{Lu}/u.test(bare)) continue;
    return bare;
  }
  return undefined;
}

/** Checks a menu snapshot (v2, or v1 for the rules that apply) against the menu rules. Warnings unless `strict`. */
export function lintMenu(snapshot: AnySnapshot, options: MenuLintOptions = {}): MenuLintFinding[] {
  const findings: MenuLintFinding[] = [];
  const severity = options.strict ? 'error' : 'warning';
  const add = (rule: MenuLintRule, path: string, message: string) => { findings.push({ rule, severity, message, path }); };
  const name = snapshot.name;
  const nouns = [...MENU_PROPER_NOUNS, name, ...(options.properNouns ?? [])].sort((a, b) => b.length - a.length);
  const commands = [...new Set([name.toLowerCase(), ...(options.commands ?? [])])].filter(Boolean);
  const cli = commands.length ? new RegExp(`\\b(?:${commands.map(escape).join('|')})\\s+[a-z][a-z-]*\\b`) : undefined;
  const items = snapshot.items as readonly AnyItem[];

  const checkText = (value: string | undefined, path: string, max: number) => {
    if (!value) return;
    if (URL_TEXT.test(value)) add('raw-text', path, `"${value}" contains a URL. Use \`opens: 'browser'\` and plain words.`);
    else if (PATH_TEXT.test(value)) add('raw-text', path, `"${value}" contains a file path.`);
    if (UUID.test(value) || HEX.test(value)) add('raw-text', path, `"${value}" contains an ID. Put IDs behind a ⌥ "Copy ID" alternate.`);
    if (FLAG.test(value)) add('raw-text', path, `"${value}" contains a command-line flag.`);
    if (cli?.test(value)) add('raw-text', path, `"${value}" contains a command. Menus say what happens, not what to type.`);
    if ([...value].length > max) add('length', path, `"${value}" is ${[...value].length} characters; the limit is ${max}. Put detail in the subtitle.`);
  };
  const checkLabel = (value: string, path: string, content: boolean) => {
    checkText(value, path, 48);
    if (EMOJI.test(value) || FALLBACK_GLYPHS.some(glyph => value.includes(glyph))) {
      add('glyph-in-label', path, `"${value}" carries a glyph. Use \`symbol\` instead.`);
    } else if (/(↗|…|\.\.\.)$/.test(value)) add('glyph-in-label', path, `"${value}" ends with an opens glyph. Use \`opens\` instead.`);
    if (!content) {
      const bad = titleCaseWord(value, nouns);
      if (bad) add('sentence-case', path, `"${value}" capitalizes "${bad}". Use sentence case, or pass it in properNouns.`);
    }
  };

  // Top-level structure.
  const top = items.filter(item => item.kind !== 'separator' && item.kind !== 'header');
  if (top.length > 10) add('top-level-count', 'items', `${top.length} top-level rows; the limit is 10. Move overflow to "Show all" in the browser.`);
  const headers = items.map((item, index) => [item, index] as const).filter(([item]) => item.kind === 'header') as unknown as [{ label: string }, number][];
  if (headers.length > 1) add('header', `items[${headers[1]![1]}]`, 'More than one header. Use the header only for the product name.');
  for (const [item, index] of headers) if (item.label !== name) add('header', `items[${index}]`, `The header "${item.label}" should be the product name "${name}".`);
  const firstSeparator = items.findIndex(item => item.kind === 'separator');
  const statuses = items.map((item, index) => [item, index] as const).filter(([item]) => item.kind === 'status') as [StatusItemV2, number][];
  if (statuses.length > 2) add('status-count', `items[${statuses[2]![1]}]`, `${statuses.length} status rows; the limit is 2.`);
  for (const [, index] of statuses) if (firstSeparator >= 0 && index > firstSeparator) add('status-count', `items[${index}]`, 'A status row sits below the first separator. Keep status at the top.');
  const seen = new Set<string>();
  for (const [status, index] of statuses) {
    const key = status.label.trim().toLowerCase();
    if (seen.has(key) || (status.detail && status.detail.trim().toLowerCase() === key)) add('status-repeat', `items[${index}]`, `"${status.label}" repeats status text.`);
    seen.add(key);
  }
  const last = items[items.length - 1];
  const quitIndex = items.findIndex(item => item.kind === 'quit');
  if (quitIndex < 0) add('quit', 'items', `No Quit item. End the menu with "Quit ${name}".`);
  else {
    if (last?.kind !== 'quit') add('quit', `items[${quitIndex}]`, 'Quit must be the last item.');
    const quit = items[quitIndex] as { label: string };
    if (quit.label !== `Quit ${name}`) add('quit', `items[${quitIndex}]`, `The quit label "${quit.label}" should be "Quit ${name}".`);
  }

  // Every item.
  let primaries = 0;
  const shortcuts = new Map<string, string>();
  const visit = (list: readonly AnyItem[], depth: number, base: string, inSubmenu: boolean) => {
    list.forEach((item, index) => {
      const path = `${base}[${index}]`;
      switch (item.kind) {
        case 'header': checkLabel(item.label, path, false); break;
        case 'status':
          if (depth > 1) add('status-count', path, 'A status row sits inside a submenu. Keep status at the top.');
          checkLabel(item.label, path, false); checkText(item.detail, `${path}.detail`, 80); break;
        case 'label':
          checkLabel(item.label, path, false);
          if ('subtitle' in item) checkText(item.subtitle, `${path}.subtitle`, 80);
          break;
        case 'quit': break;
        case 'separator': break;
        case 'submenu':
          checkLabel(item.label, path, false);
          if (!item.items.length) add('empty-submenu', path, `The submenu "${item.label}" is empty.`);
          visit(item.items as readonly AnyItem[], depth + 1, `${path}.items`, true);
          break;
        case 'action': {
          const v2 = item as ActionItemV2;
          const content = typeof v2.symbol === 'string' && v2.symbol.startsWith('item.');
          checkLabel(item.label, path, content);
          checkText(v2.subtitle, `${path}.subtitle`, 80);
          checkText(v2.tooltip, `${path}.tooltip`, 160);
          if (v2.alternate) checkLabel(v2.alternate.label, `${path}.alternate`, content);
          if (depth > 2) add('depth', path, `"${item.label}" is ${depth} levels deep. Keep actions at most one submenu down.`);
          if (v2.role === 'primary') {
            primaries++;
            if (inSubmenu) add('depth', path, `The primary action "${item.label}" is inside a submenu.`);
          }
          if (item.shortcut) {
            const key = item.shortcut.toLowerCase().replace(/\s+/g, '');
            const other = shortcuts.get(key);
            if (other !== undefined) add('shortcut-repeat', path, `"${item.label}" and "${other}" share the shortcut ${item.shortcut}.`);
            else shortcuts.set(key, item.label);
          }
          break;
        }
      }
    });
  };
  visit(items, 1, 'items', false);
  const starting = statuses.length > 0 && statuses.every(([status]) => status.symbol === 'status.syncing');
  if (snapshot.version === 2 && primaries !== 1 && !(primaries === 0 && starting)) {
    add('primary-count', 'items', `${primaries} primary actions; a menu has exactly one (role: 'primary').`);
  }
  if (snapshot.version === 2) {
    const mark = snapshot.mark;
    if (mark?.text !== undefined && mark.tone !== 'attention' && mark.tone !== 'error') add('mark-text', 'mark.text', 'A count shows only when the tone is attention or error.');
    checkText(snapshot.tooltip, 'tooltip', 160);
  }
  return findings;
}

// ---------------------------------------------------------------------------
// Fixture helper

export interface MenuFixtureReport { tree: string; findings: MenuLintFinding[] }

/**
 * Validates a snapshot fixture, lints it (strict by default) and renders its
 * tree. Throws with the tree and every finding when anything fails, so a
 * product test is one line per state fixture:
 *
 *     assertMenuFixture(await snapshotFor('signed-out'), { properNouns: ['Mom'] });
 */
export function assertMenuFixture(snapshot: AnySnapshot, options: MenuLintOptions = {}): MenuFixtureReport {
  const strict = options.strict ?? true;
  if (snapshot.version === 2) validateSnapshotV2(snapshot);
  const findings = lintMenu(snapshot, { ...options, strict });
  const tree = renderMenuTree(snapshot);
  const failing = findings.filter(finding => finding.severity === 'error');
  if (failing.length) {
    throw new Error(`Menu lint failed for ${snapshot.appId}:\n${failing.map(f => `  ${f.rule} at ${f.path}: ${f.message}`).join('\n')}\n\n${tree}`);
  }
  return { tree, findings };
}

/** Everything above under one name: `menuKit.layout(…)`, `menuKit.lint(…)`. */
export const menuKit = {
  layout, lint: lintMenu, renderTree: renderMenuTree, assertFixture: assertMenuFixture,
  openAtLogin: openAtLoginItem, actionError: actionErrorItem, degraded: degradedMenu,
} as const;
