import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import type {
  ActionSymbol, FoundationActionId, ItemSymbol, MarkSymbol, NoticeRequest, NoticeResult,
  PermissionKind, SettingsPermissionKind, SnapshotV2, StatusSymbol,
} from '../src/index.js';

// Compiled to dist/test/protocol-v2.test.js; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
// Windows checkouts may convert docs to CRLF; the table parsing below expects LF.
const read = (path: string): string => readFileSync(new URL(path, root), 'utf8').replaceAll('\r\n', '\n');
const protocolDoc = read('docs/protocol-v2.md');
const permissionsDoc = read('docs/permissions.md');

// Each list must name every member of its union exactly once: `Complete` is
// `false` (and the assignment fails to compile) when a member is missing.
type Complete<T, U extends readonly T[]> = [Exclude<T, U[number]>] extends [never] ? true : false;

const STATUS = ['status.ok', 'status.running', 'status.idle', 'status.partial', 'status.syncing', 'status.paused',
  'status.attention', 'status.error', 'status.offline', 'status.signedOut', 'status.locked'] as const satisfies readonly StatusSymbol[];
const ACTION = ['action.open', 'action.add', 'action.pause', 'action.resume', 'action.refresh', 'action.folder', 'action.copy',
  'action.settings', 'action.permission', 'action.signIn', 'action.signOut', 'action.update', 'action.help',
  'action.support'] as const satisfies readonly ActionSymbol[];
const ITEM = ['item.file', 'item.image', 'item.chat', 'item.contact', 'item.room', 'item.job', 'item.camera', 'item.chart',
  'item.agent', 'item.approval', 'item.key'] as const satisfies readonly ItemSymbol[];
const MARK = ['mark.chat', 'mark.masks', 'mark.drop', 'mark.dropHalf', 'mark.people', 'mark.chart', 'mark.camera',
  'mark.shield', 'mark.agent'] as const satisfies readonly MarkSymbol[];
const KINDS = ['full-disk-access', 'automation', 'contacts', 'accessibility', 'screen-recording', 'camera', 'microphone',
  'local-network', 'incoming-connections', 'notifications', 'login-item', 'keychain', 'developer-tools',
  'gatekeeper'] as const satisfies readonly PermissionKind[];

const complete: [
  Complete<StatusSymbol, typeof STATUS>, Complete<ActionSymbol, typeof ACTION>, Complete<ItemSymbol, typeof ITEM>,
  Complete<MarkSymbol, typeof MARK>, Complete<PermissionKind, typeof KINDS>,
] = [true, true, true, true, true];

/** Names in the first column of the doc's table rows, e.g. "| `status.ok` |". */
function tableNames(doc: string, pattern: RegExp): string[] {
  return [...doc.matchAll(/^\| `([^`]+)` \|/gm)].map(match => match[1]!).filter(name => pattern.test(name));
}

/** The body of one `## ` section. */
function section(doc: string, heading: string): string {
  return doc.split(`\n## ${heading}\n`)[1]?.split('\n## ')[0] ?? '';
}

test('the symbol vocabulary tables match the v2 types', () => {
  assert.deepEqual(complete, [true, true, true, true, true]);
  const vocabulary = section(protocolDoc, 'Symbol vocabulary');
  assert.deepEqual(tableNames(vocabulary, /^status\./), [...STATUS]);
  assert.deepEqual(tableNames(vocabulary, /^action\./), [...ACTION]);
  assert.deepEqual(tableNames(vocabulary, /^item\./), [...ITEM]);
  assert.deepEqual(tableNames(vocabulary, /^mark\./), [...MARK]);
});

test('the permission kinds table matches the v2 types', () => {
  assert.deepEqual(tableNames(section(permissionsDoc, 'Permission kinds'), /^[a-z]+(-[a-z]+)*$/), [...KINDS]);
});

test('the documented snapshot example is a valid SnapshotV2', () => {
  const example = {
    version: 2, type: 'snapshot', appId: 'textbutler', name: 'Textbutler', revision: 7,
    mark: { symbol: 'mark.chat', letters: 'Tb', tone: 'attention', text: '3' },
    tooltip: 'Textbutler · 3 replies waiting',
    items: [
      { kind: 'header', label: 'Textbutler' },
      { kind: 'status', symbol: 'status.running', label: 'Running', detail: '3 chats on' },
      { kind: 'separator' },
      { kind: 'action', id: 'open', label: 'Open dashboard', symbol: 'action.open', role: 'primary', opens: 'browser', shortcut: 'CmdOrCtrl+O' },
      { kind: 'separator' },
      { kind: 'action', id: 'chat.1', label: 'Mom', symbol: 'item.chat', subtitle: 'Reply waiting for approval', badge: '2',
        alternate: { id: 'chat.1.copy', label: 'Copy chat ID', symbol: 'action.copy' } },
      { kind: 'action', id: 'chats.all', label: 'Show all chats (23)', opens: 'browser' },
      { kind: 'separator' },
      { kind: 'action', id: 'pause', label: 'Pause automatic replies', symbol: 'action.pause', state: 'off', shortcut: 'CmdOrCtrl+P' },
      { kind: 'action', id: 'foundation.login', label: 'Open at login', state: 'on' },
      { kind: 'separator' },
      { kind: 'action', id: 'help', label: 'Help & support', symbol: 'action.support', opens: 'browser',
        alternate: { id: 'help.diagnostics', label: 'Copy diagnostics', symbol: 'action.copy' } },
      { kind: 'quit', label: 'Quit Textbutler' },
    ],
  } as const satisfies SnapshotV2;
  assert.ok(protocolDoc.includes(JSON.stringify(example)), 'docs/protocol-v2.md snapshot example drifted from SnapshotV2');
});

test('the documented notice frames are valid', () => {
  const request = {
    type: 'notice-request', version: 1, title: 'Textbutler needs access to Messages',
    message: 'macOS will ask to let Textbutler control Messages. Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.',
    primary: 'Continue', secondary: 'Not now', timeoutSeconds: 120,
  } as const satisfies NoticeRequest;
  const result = { type: 'notice-result', version: 1, status: 'primary' } as const satisfies NoticeResult;
  assert.ok(protocolDoc.includes(JSON.stringify(request)));
  assert.ok(protocolDoc.includes(JSON.stringify(result)));
});

test('reserved foundation action IDs cover every kind with a Settings pane', () => {
  const panes = KINDS.filter((kind): kind is SettingsPermissionKind => kind !== 'keychain' && kind !== 'developer-tools');
  const ids: FoundationActionId[] = ['foundation.login', ...panes.map(kind => `foundation.settings.${kind}` as const)];
  assert.equal(new Set(ids).size, KINDS.length - 1);
  // Kinds without a pane only have a `none` Settings URL in the doc table.
  const rows = section(permissionsDoc, 'Permission kinds').split('\n').filter(row => /^\| `(keychain|developer-tools)` \|/.test(row));
  assert.equal(rows.length, 2);
  for (const row of rows) assert.match(row, /\| none \|$/);
});
