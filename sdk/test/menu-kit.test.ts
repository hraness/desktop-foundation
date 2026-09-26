import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import {
  MENU_SYMBOLS, downlevelSnapshot, fitText, parseRunnerProtocols, validateSnapshotV2,
  type MenuItemV2, type MenuSymbol, type SnapshotV2,
} from '../src/protocol-v2.js';
import { validateSnapshot } from '../src/protocol.js';
import {
  assertMenuFixture, degradedMenu, layout, lintMenu, menuKit, openAtLoginItem, renderMenuTree,
  type MenuLintRule,
} from '../src/menu-kit.js';
import { permissionMenuItems, MESSAGES_FDA } from '../src/permissions.js';

// Compiled to dist/test/menu-kit.test.js; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), 'utf8').replaceAll('\r\n', '\n');
const protocolDoc = read('docs/protocol-v2.md');
const GOLDEN = new URL('sdk/test/golden/menu/', root);

/** Compares with sdk/test/golden/menu/<name>.txt; regenerate with UPDATE_GOLDEN=1 npm run check:sdk. */
function golden(name: string, actual: string): void {
  const file = new URL(`${name}.txt`, GOLDEN);
  if (process.env.UPDATE_GOLDEN === '1') { mkdirSync(GOLDEN, { recursive: true }); writeFileSync(file, actual); }
  assert.ok(existsSync(file), `missing golden ${name}.txt; run with UPDATE_GOLDEN=1`);
  assert.equal(actual, readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), `golden ${name}.txt`);
}

const docSnapshot = (): SnapshotV2 => JSON.parse(protocolDoc.split('## Snapshot\n\n```jsonl\n')[1]!.split('\n')[0]!) as SnapshotV2;

const snapshot = (items: MenuItemV2[], extra: Partial<SnapshotV2> = {}): SnapshotV2 => ({
  version: 2, type: 'snapshot', appId: 'textbutler', name: 'Textbutler', revision: 1,
  mark: { symbol: 'mark.chat', letters: 'Tb' }, items, ...extra,
});

const running = (): MenuItemV2[] => layout({
  name: 'Textbutler',
  status: { kind: 'status', symbol: 'status.running', label: 'Running', detail: '3 chats on' },
  primary: { kind: 'action', id: 'open', label: 'Open dashboard', symbol: 'action.open', opens: 'browser', shortcut: 'CmdOrCtrl+O' },
  recent: [
    { kind: 'action', id: 'chat.1', label: 'Mom', symbol: 'item.chat', subtitle: 'Reply waiting for approval', badge: '2', alternate: { id: 'chat.1.copy', label: 'Copy chat ID', symbol: 'action.copy' } },
    { kind: 'action', id: 'chat.2', label: 'Sam Rivera', symbol: 'item.chat', subtitle: 'Replied 2 minutes ago' },
  ],
  overflow: { kind: 'action', id: 'chats.all', label: 'Show all chats (23)', opens: 'browser' },
  controls: [{ kind: 'action', id: 'pause', label: 'Pause automatic replies', symbol: 'action.pause', state: 'off', shortcut: 'CmdOrCtrl+P' }],
  openAtLogin: 'on',
  help: { kind: 'action', id: 'help', label: 'Help & support', symbol: 'action.support', opens: 'browser', alternate: { id: 'help.diagnostics', label: 'Copy diagnostics', symbol: 'action.copy' } },
});

test('the symbol table matches the vocabulary tables in docs/protocol-v2.md', () => {
  const vocabulary = protocolDoc.split('\n## Symbol vocabulary\n')[1]!.split('\n## ')[0]!;
  const rows = [...vocabulary.matchAll(/^\| `([a-z]+\.[A-Za-z]+)` \| `([^`]+)` \| ([^|]+?) \|/gm)];
  assert.equal(rows.length, Object.keys(MENU_SYMBOLS).length);
  for (const [, name, sf, third] of rows) {
    const entry = MENU_SYMBOLS[name as MenuSymbol];
    assert.ok(entry, name);
    assert.equal(entry.sf, sf, name);
    // Marks have a product column instead of a fallback glyph.
    if (!name!.startsWith('mark.')) assert.equal(entry.fallback, third === '-' ? null : third, name);
  }
});

test('validateSnapshotV2 accepts the documented snapshot and returns alternates as actions', () => {
  const actions = validateSnapshotV2(docSnapshot());
  assert.deepEqual([...actions.keys()], ['open', 'chat.1', 'chat.1.copy', 'chats.all', 'pause', 'foundation.login', 'help', 'help.diagnostics']);
});

test('validateSnapshotV2 rejects what the v2 runner rejects', () => {
  const cases: Array<[string, (value: any) => void]> = [
    ['unknown-protocol-field', value => { value.title = 'Tb'; }],
    ['unknown-protocol-field', value => { value.items[5].checked = true; }],
    ['invalid-symbol', value => { value.mark.symbol = 'mark.sponge'; }],
    ['invalid-symbol', value => { value.items[1].symbol = 'status.happy'; }],
    ['invalid-symbol', value => { value.items[5].symbol = 'status.ok'; }],
    ['invalid-mark', value => { value.mark.tone = 'normal'; }],
    ['invalid-mark', value => { value.mark.letters = 'Tbx'; }],
    ['invalid-mark', value => { value.mark.templateIcon = { width: 2, height: 2, alpha: 'AAAA' }; }],
    ['invalid-state', value => { value.items[8].state = 'yes'; }],
    ['invalid-badge', value => { value.items[5].badge = '12345'; }],
    ['invalid-role', value => { value.items[3].role = 'secondary'; }],
    ['invalid-opens', value => { value.items[3].opens = 'terminal'; }],
    ['invalid-alternate', value => { value.items[5].alternate.id = 'open'; }],
    ['invalid-alternate', value => { value.items[5].alternate.id = 'foundation.login'; value.items[9].id = 'login.row'; }],
    ['invalid-action-id', value => { value.items[3].id = 'foundation.bogus'; }],
    ['invalid-label', value => { value.items[0].label = 'x'.repeat(49); }],
    ['invalid-label', value => { value.tooltip = 'x'.repeat(161); }],
    ['invalid-menu', value => { value.items.push({ kind: 'banner', label: 'Hi' }); }],
    ['invalid-revision', value => { value.version = 1; }],
  ];
  for (const [code, mutate] of cases) {
    const value = docSnapshot();
    mutate(value);
    assert.throws(() => validateSnapshotV2(value), new RegExp(`^Error: ${code}$`), `${code}: ${mutate}`);
  }
  const huge = snapshot(Array.from({ length: 300 }, (_, index) => ({ kind: 'action', id: `a${index}`, label: 'Row' })));
  assert.throws(() => validateSnapshotV2(huge), /^Error: menu-too-large: 300 items \(limit 256\)$/);
  const icon = docSnapshot();
  icon.mark.templateIcon = { width: 2, height: 2, alpha: Buffer.alloc(4, 255).toString('base64') };
  validateSnapshotV2(icon);
});

test('down-level turns the documented snapshot into a valid v1 snapshot', () => {
  const v1 = downlevelSnapshot(docSnapshot());
  validateSnapshot(v1, new Set(['foundation.login']));
  assert.throws(() => validateSnapshot(v1), /invalid-action-id/);
  assert.equal(v1.title, 'Tb');
  golden('downlevel-doc', JSON.stringify(v1, null, 2) + '\n');
  const long = downlevelSnapshot(snapshot([{ kind: 'action', id: 'x', label: 'y'.repeat(200), subtitle: 'z'.repeat(80), state: 'mixed', symbol: 'action.pause' }]));
  const label = (long.items[0] as { label: string }).label;
  assert.equal([...label].length, 256);
  assert.ok(label.startsWith('– ⏸︎ y') && label.endsWith('z…'));
  assert.equal((long.items[0] as { checked?: boolean }).checked, false);
  assert.equal(fitText('abc', 3), 'abc');
  assert.equal(fitText('abcd', 3), 'ab…');
});

test('parseRunnerProtocols reads the --version line', () => {
  assert.deepEqual(parseRunnerProtocols('hraness-companion 0.7.0 protocol/1\n'), [1]);
  assert.deepEqual(parseRunnerProtocols('hraness-companion 0.8.0 protocol/1,2\n'), [1, 2]);
  assert.deepEqual(parseRunnerProtocols('garbage'), [1]);
});

test('layout builds the default order and passes strict lint', () => {
  const value = snapshot(running());
  const report = assertMenuFixture(value);
  assert.deepEqual(report.findings, []);
  golden('layout-running', report.tree);
  golden('doc-snapshot', renderMenuTree(docSnapshot()));
  assert.deepEqual(lintMenu(docSnapshot(), { strict: true }), []);
  // Recent rows are capped, and an empty group adds no separator.
  const capped = layout({
    name: 'Textbutler', status: { kind: 'status', symbol: 'status.idle', label: 'Idle' },
    primary: { kind: 'action', id: 'open', label: 'Open dashboard' },
    recent: Array.from({ length: 8 }, (_, index) => ({ kind: 'action' as const, id: `r${index}`, label: `Row ${index}` })),
  });
  assert.equal(capped.filter(item => item.kind === 'action' && item.id.startsWith('r')).length, 5);
  assert.deepEqual(capped.slice(-2), [{ kind: 'separator' }, { kind: 'quit', label: 'Quit Textbutler' }]);
  assert.equal(menuKit.layout, layout);
});

test('standard rows: open at login, degraded and permission rows', () => {
  assert.deepEqual(openAtLoginItem('on'), { kind: 'action', id: 'foundation.login', label: 'Open at login', state: 'on' });
  assert.equal(openAtLoginItem().subtitle, 'macOS shows a notice when you turn this on');
  const degraded = snapshot(degradedMenu('Textbutler', { primary: { kind: 'action', id: 'help', label: 'Help & support', symbol: 'action.support', opens: 'browser' }, openAtLogin: 'off' }));
  golden('degraded', assertMenuFixture(degraded).tree);
  const locked = permissionMenuItems(MESSAGES_FDA({ product: 'Textbutler', command: 'textbutler', requester: 'Textbutler' }), 'denied');
  const [status, action] = locked as [MenuItemV2 & { kind: 'status' }, MenuItemV2 & { kind: 'action' }];
  const permission = snapshot(layout({ name: 'Textbutler', status, primary: action }));
  golden('needs-permission', assertMenuFixture(permission).tree);
});

test('lintMenu flags each rule, as warnings unless strict', () => {
  const findings = (items: MenuItemV2[], extra: Partial<SnapshotV2> = {}) => lintMenu(snapshot(items, extra));
  const rules = (items: MenuItemV2[], extra: Partial<SnapshotV2> = {}) => [...new Set(findings(items, extra).map(f => f.rule))];
  const base = running();
  const header: MenuItemV2 = { kind: 'header', label: 'Textbutler' };
  const quit: MenuItemV2 = { kind: 'quit', label: 'Quit Textbutler' };
  const primary: MenuItemV2 = { kind: 'action', id: 'open', label: 'Open dashboard', role: 'primary' };
  const expect = (rule: MenuLintRule, items: MenuItemV2[], extra: Partial<SnapshotV2> = {}) =>
    assert.ok(rules(items, extra).includes(rule), `${rule}: ${JSON.stringify(rules(items, extra))}`);
  assert.deepEqual(rules(base), []);
  expect('top-level-count', [header, primary, ...Array.from({ length: 10 }, (_, i) => ({ kind: 'action' as const, id: `x${i}`, label: 'Row' })), quit]);
  expect('depth', [header, primary, { kind: 'submenu', label: 'More', items: [{ kind: 'submenu', label: 'Even more', items: [{ kind: 'action', id: 'deep', label: 'Deep' }] }] }, quit]);
  expect('depth', [header, { kind: 'submenu', label: 'More', items: [primary] }, quit]);
  expect('primary-count', [header, { kind: 'action', id: 'open', label: 'Open dashboard' }, quit]);
  expect('primary-count', [header, primary, { ...primary, id: 'open2' } as MenuItemV2, quit]);
  assert.deepEqual(rules([header, { kind: 'status', symbol: 'status.syncing', label: 'Starting Textbutler' }, { kind: 'separator' }, quit]), []);
  const status = (label: string, detail?: string): MenuItemV2 => ({ kind: 'status', symbol: 'status.running', label, ...(detail ? { detail } : {}) });
  expect('status-count', [header, status('One'), status('Two'), status('Three'), primary, quit]);
  expect('status-count', [header, primary, { kind: 'separator' }, status('Late'), quit]);
  expect('status-repeat', [header, status('Running', 'running'), primary, quit]);
  expect('status-repeat', [header, status('Running'), status('Running'), primary, quit]);
  expect('quit', [header, primary]);
  expect('quit', [header, quit, primary]);
  expect('quit', [header, primary, { kind: 'quit', label: 'Quit' }]);
  expect('header', [{ kind: 'header', label: 'Menu' }, primary, quit]);
  expect('header', [header, header, primary, quit]);
  for (const label of ['Open ~/Library/Logs', 'Open https://example.com', 'Copy 3e5161bd4f', 'Run with --debug', 'Run textbutler doctor', 'Job 123e4567-e89b-12d3-a456-426614174000', 'Open logs/today.txt']) {
    expect('raw-text', [header, primary, { kind: 'action', id: 'x', label }, quit]);
  }
  assert.deepEqual(rules([header, primary, { kind: 'action', id: 'x', label: 'Replied 1/2 of chats' }, quit]), []);
  expect('sentence-case', [header, primary, { kind: 'action', id: 'x', label: 'Pause Automatic Replies' }, quit]);
  assert.deepEqual(rules([header, primary, { kind: 'action', id: 'x', label: 'Turn on Full Disk Access in System Settings' }, quit]), []);
  assert.deepEqual(rules([header, primary, { kind: 'action', id: 'x', label: 'Copy ID and open PDF' }, quit]), []);
  assert.deepEqual(rules([header, primary, { kind: 'action', id: 'x', label: 'Sam Rivera', symbol: 'item.contact' }, quit]), []);
  assert.deepEqual(lintMenu(snapshot([header, primary, { kind: 'action', id: 'x', label: 'Ask Sam Rivera' }, quit]), { properNouns: ['Sam Rivera'] }), []);
  expect('length', [header, primary, { kind: 'action', id: 'x', label: 'x'.repeat(49) }, quit]);
  expect('length', [header, primary, { kind: 'action', id: 'x', label: 'Row', subtitle: 'x'.repeat(81) } as MenuItemV2, quit]);
  for (const label of ['🧽 Sponge', '● Running', 'Open dashboard ↗', 'Settings…', 'Settings...']) {
    expect('glyph-in-label', [header, primary, { kind: 'action', id: 'x', label }, quit]);
  }
  expect('mark-text', [header, primary, quit], { mark: { symbol: 'mark.chat', letters: 'Tb', text: '3' } });
  expect('shortcut-repeat', [header, { ...primary, shortcut: 'CmdOrCtrl+O' } as MenuItemV2, { kind: 'action', id: 'x', label: 'Other', shortcut: 'cmdorctrl+o' }, quit]);
  expect('empty-submenu', [header, primary, { kind: 'submenu', label: 'More', items: [] }, quit]);
  const warning = findings([header, primary, { kind: 'action', id: 'x', label: 'Bad Case' }, quit]);
  assert.equal(warning[0]!.severity, 'warning');
  assert.equal(lintMenu(snapshot([header, primary, { kind: 'action', id: 'x', label: 'Bad Case' }, quit]), { strict: true })[0]!.severity, 'error');
  assert.throws(() => assertMenuFixture(snapshot([header, primary, { kind: 'action', id: 'x', label: 'Bad Case' }, quit])), /sentence-case at items\[2\]/);
});

test('renderMenuTree also renders v1 snapshots', () => {
  const tree = renderMenuTree({
    version: 1, type: 'snapshot', appId: 'demo', name: 'Demo', title: 'Hc', revision: 1,
    items: [{ kind: 'label', label: 'Running' }, { kind: 'action', id: 'a', label: 'Toggle', checked: true, enabled: false }, { kind: 'submenu', label: 'More', items: [{ kind: 'action', id: 'b', label: 'Inner', shortcut: 'Shift+Alt+K' }] }, { kind: 'separator' }, { kind: 'quit', label: 'Quit' }],
  });
  assert.equal(tree, '[Hc]\nRunning (label)\n✓ Toggle (disabled)\nMore ▸\n  Inner  ⇧⌥K\n─────────\nQuit\n');
});
