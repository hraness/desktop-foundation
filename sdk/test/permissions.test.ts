import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import {
  AUTOMATION, CHROME_SAFE_STORAGE, CONTACTS, INCOMING_CONNECTIONS, LOCAL_NETWORK, LOCAL_SIGNING, LOGIN_ITEM,
  MESSAGES_FDA, PERMISSION_KINDS, SCREEN_RECORDING, XCODE_TOOLS,
  behaviorOf, classifyKeychainStatus, formatNotice, hasSettingsPane, isAllowedSettingsUrl, openPermissionSettings,
  paneName, permissionError, permissionErrorJson, permissionMenuItems, permissionNoticeRequest, permissionStatus,
  prePrompt, renderPrePrompt, renderRecovery, reportPermissionFailure, responsibleApp, settingsPath, settingsUrl,
  type PermissionIO, type PermissionNeed, type ProductRef, type RecoveryState,
} from '../src/permissions.js';

// Compiled to dist/test/permissions.test.js; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
const GOLDEN = new URL('sdk/test/golden/permissions/', root);
const permissionsDoc = readFileSync(new URL('docs/permissions.md', root), 'utf8').replaceAll('\r\n', '\n');
const UTF8 = { LANG: 'en_US.UTF-8' };

const textbutler: ProductRef = { product: 'Textbutler', command: 'textbutler', requester: 'Textbutler' };
const cli = (need: PermissionNeed, interactive = true) =>
  formatNotice(renderPrePrompt(need, 'cli', UTF8), { kind: 'pre-prompt', interactive });

function fakeIO(overrides: Partial<PermissionIO> & { keys?: Array<'enter' | 's' | 'o' | 'timeout'> } = {}) {
  const written: string[] = [];
  const opened: string[] = [];
  const keys = overrides.keys ?? [];
  const io: PermissionIO = {
    env: { ...UTF8 }, stdinIsTTY: true, stderrIsTTY: true,
    write: text => { written.push(text); },
    readKey: async () => keys.shift() ?? 'timeout',
    openUrl: async url => { opened.push(url); return true; },
    fileAccess: async () => 'missing',
    run: async () => ({ status: 0 }),
    ...overrides,
  };
  return { io, written, opened };
}

test('the documented kind table matches the kit', () => {
  const rows = permissionsDoc.split('\n## Permission kinds\n')[1]!.split('\n## ')[0]!.split('\n').filter(row => /^\| `[a-z-]+` \|/.test(row));
  assert.equal(rows.length, PERMISSION_KINDS.length);
  for (const row of rows) {
    const cells = row.split(' | ').map(cell => cell.replace(/^\| |\s*\|$/g, '').replaceAll('`', ''));
    const kind = cells[0] as typeof PERMISSION_KINDS[number];
    assert.ok(PERMISSION_KINDS.includes(kind), kind);
    const behavior = behaviorOf(kind);
    assert.ok(cells[1]!.startsWith(behavior === 'asks' ? 'asks' : behavior === 'notifies' ? 'notifies' : 'settings-only') || (kind === 'gatekeeper' && behavior === 'asks'), `${kind} behavior`);
    assert.equal(paneName(kind) ?? 'none', cells[2], `${kind} pane`);
    assert.equal(settingsPath(kind) ?? 'none', cells[3], `${kind} path`);
    assert.equal(settingsUrl(kind) ?? 'none', cells[4], `${kind} url`);
    assert.equal(hasSettingsPane(kind), settingsUrl(kind) !== null);
  }
  assert.equal(isAllowedSettingsUrl('x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles'), true);
  assert.equal(isAllowedSettingsUrl('https://example.com'), false);
  assert.equal(isAllowedSettingsUrl('x-apple.systempreferences:com.apple.preference.security?Privacy_Anything'), false);
});

test('responsibleApp names the terminal, then the product inside its local app', () => {
  assert.equal(responsibleApp({ TERM_PROGRAM: 'Apple_Terminal' }), 'Terminal');
  assert.equal(responsibleApp({ __CFBundleIdentifier: 'com.googlecode.iterm2', TERM_PROGRAM: 'vscode' }), 'iTerm');
  assert.equal(responsibleApp({ TERM_PROGRAM: 'ghostty' }), 'Ghostty');
  assert.equal(responsibleApp({ __CFBundleIdentifier: 'com.microsoft.VSCode' }), 'Visual Studio Code');
  assert.equal(responsibleApp({ ZED_TERM: 'true' }), 'Zed');
  assert.equal(responsibleApp({ TERM_PROGRAM: 'WarpTerminal' }), 'Warp');
  assert.equal(responsibleApp({ TERM_PROGRAM: 'WezTerm' }), 'WezTerm');
  assert.equal(responsibleApp({}), 'your terminal app');
  assert.equal(responsibleApp({ HRANESS_APP_BUNDLE_ID: 'app.hraness.textbutler', TERM_PROGRAM: 'Apple_Terminal' }, 'Textbutler'), 'Textbutler');
  // Without a product name the bundle ID alone cannot name the app.
  assert.equal(responsibleApp({ HRANESS_APP_BUNDLE_ID: 'app.hraness.textbutler', TERM_PROGRAM: 'Apple_Terminal' }), 'Terminal');
  const need = MESSAGES_FDA({ product: 'Textbutler', command: 'textbutler' });
  assert.match(renderPrePrompt(need, 'cli', { TERM_PROGRAM: 'iTerm.app' }).lines[0]!, /Turn on iTerm in/);
});

// The exact strings from docs/permissions.md § Presets. Any change here is a
// contract change and updates the doc and the Rust twin in the same PR.
test('presets render the documented copy', () => {
  assert.equal(cli(LOGIN_ITEM(textbutler)),
    '🔐 macOS will show a notice that Textbutler can open at login.\n'
    + '   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.\n');
  // Login Items lists the program the login item runs, never the terminal.
  assert.match(cli(LOGIN_ITEM({ product: 'Textbutler', command: 'textbutler' })), /^🔐 macOS will show a notice that Textbutler can open at login\.\n/);
  assert.equal(cli(LOGIN_ITEM({ ...textbutler, requester: 'bun' })),
    "🔐 macOS will show a notice that bun can open at login. That's Textbutler's menu bar.\n"
    + '   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.\n');
  assert.equal(cli(CHROME_SAFE_STORAGE({ product: 'Ghostget', command: 'ghostget' }, { caller: 'security' })),
    '🔐 macOS will ask to let security use "Chrome Safe Storage" from your keychain for Ghostget.\n'
    + "   Ghostget uses it to read the Chrome sign-in you already have and never stores it. Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.\n"
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(MESSAGES_FDA(textbutler)),
    '🔐 Textbutler needs Full Disk Access to read your Messages.\n'
    + "   macOS doesn't ask for this. Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access. Only the chats you pick are read.\n"
    + '   Press Enter to open Settings · s to skip\n');
  assert.equal(cli(AUTOMATION(textbutler, 'Messages', 'Textbutler only sends replies in chats you turn on.')),
    '🔐 macOS will ask to let Textbutler control Messages.\n'
    + '   Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.\n'
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(CONTACTS({ product: 'PeopleBlade', command: 'peopleblade', requester: 'PeopleBlade' })),
    '🔐 macOS will ask to let PeopleBlade see your contacts.\n'
    + '   PeopleBlade reads names and numbers on this Mac. Change this any time in System Settings › Privacy & Security › Contacts.\n'
    + '   Press Enter to continue · s to skip\n');
  const valhalla = { product: 'Valhalla', command: 'vhalla', requester: 'Valhalla' };
  assert.equal(cli(LOCAL_NETWORK(valhalla, 'Room members on your network connect to this Mac.')),
    '🔐 macOS will ask to let Valhalla find and connect to devices on your local network.\n'
    + '   Room members on your network connect to this Mac. Change this any time in System Settings › Privacy & Security › Local Network.\n'
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(INCOMING_CONNECTIONS(valhalla, { listener: 'vhalla', why: 'Choose Allow so room members can reach this Mac.' })),
    '🔐 macOS will ask to let vhalla accept incoming network connections for Valhalla.\n'
    + '   Choose Allow so room members can reach this Mac. Change this any time in System Settings › Network › Firewall.\n'
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(XCODE_TOOLS({ product: 'algal', command: 'algal' }, { skipEffect: 'algal answers without Apple Intelligence' })),
    "🔐 algal needs Apple's command line tools to build a small helper. macOS will offer to install them (about 1 GB).\n"
    + '   Nothing is installed unless you agree in that window. Or skip: algal answers without Apple Intelligence.\n'
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(SCREEN_RECORDING({ product: 'Slopcamera', command: 'slopcamera', requester: 'Slopcamera' }, 'Slopcamera only captures the window you pick.')),
    '🔐 macOS will ask to let Slopcamera record your screen.\n'
    + '   Slopcamera only captures the window you pick. Change this any time in System Settings › Privacy & Security › Screen & System Audio Recording.\n'
    + '   Press Enter to continue · s to skip\n');
  assert.equal(cli(LOCAL_SIGNING(textbutler)),
    '🔐 macOS will ask to let codesign use your "Hraness Local Signing" key for Textbutler.\n'
    + "   Hraness signs its apps on this Mac with it so they keep their permissions after updates. Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.\n"
    + '   Press Enter to continue · s to skip\n');
});

test('recovery copy follows the templates', () => {
  const recovery = (need: PermissionNeed, state: RecoveryState, interactive = true) =>
    formatNotice(renderRecovery(need, state, 'cli', UTF8), { kind: 'recovery', interactive });
  const fda = MESSAGES_FDA(textbutler);
  assert.equal(recovery(fda, 'denied'),
    "✗ Textbutler can't read your Messages: macOS access is off for Textbutler.\n"
    + '  Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access.\n'
    + '→ textbutler doctor · press o to open Settings\n');
  assert.equal(recovery(fda, 'denied', false),
    "✗ Textbutler can't read your Messages: macOS access is off for Textbutler.\n"
    + '  Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access.\n'
    + '→ textbutler doctor\n');
  assert.equal(recovery(fda, 'unknown'),
    "✗ Textbutler couldn't read your Messages. macOS may be blocking Textbutler.\n"
    + '  Check System Settings › Privacy & Security › Full Disk Access.\n'
    + '→ textbutler doctor\n');
  const chrome = { ...CHROME_SAFE_STORAGE({ product: 'Ghostget', command: 'ghostget' }, { caller: 'security' }), next: 'ghostget auth add' };
  assert.equal(recovery(chrome, 'denied'),
    '✗ Ghostget can\'t use "Chrome Safe Storage" from your keychain: the keychain request was denied.\n'
    + '  Run it again and choose Always Allow when macOS asks.\n'
    + '→ ghostget auth add\n');
  assert.equal(recovery(chrome, 'unknown'),
    '✗ Ghostget couldn\'t use "Chrome Safe Storage" from your keychain. macOS may be blocking security.\n'
    + '  Unlock your login keychain, then retry.\n'
    + '→ ghostget auth add\n');
  assert.equal(recovery(XCODE_TOOLS({ product: 'algal', command: 'algal' }, { skipEffect: 'x' }), 'missing'),
    "✗ algal needs Apple's command line tools. Nothing was installed.\n→ xcode-select --install\n");
  assert.equal(formatNotice(renderRecovery(fda, 'denied', 'cli', UTF8), { kind: 'recovery', interactive: false, style: { color: false, ascii: true } }),
    "FAIL Textbutler can't read your Messages: macOS access is off for Textbutler.\n"
    + '  Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access.\n'
    + '-> textbutler doctor\n');
});

test('dialogs, menu rows and JSON errors', () => {
  const automation = AUTOMATION(textbutler, 'Messages', 'Textbutler only sends replies in chats you turn on.');
  assert.deepEqual(permissionNoticeRequest(automation), {
    type: 'notice-request', version: 1, title: 'Textbutler needs access to Messages',
    message: 'macOS will ask to let Textbutler control Messages. Textbutler only sends replies in chats you turn on. Change this any time in System Settings › Privacy & Security › Automation.',
    primary: 'Continue', secondary: 'Not now',
  });
  const fda = MESSAGES_FDA(textbutler);
  assert.deepEqual(permissionNoticeRequest(fda), {
    type: 'notice-request', version: 1, title: 'Textbutler needs Full Disk Access',
    message: "Textbutler needs Full Disk Access to read your Messages. macOS doesn't ask for this. Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access. Only the chats you pick are read.",
    primary: 'Open System Settings', secondary: 'Not now', settings: 'full-disk-access',
  });
  assert.equal(permissionNoticeRequest(LOGIN_ITEM(textbutler)), null);
  assert.deepEqual(permissionMenuItems(fda, 'granted'), []);
  assert.deepEqual(permissionMenuItems(fda, 'not-determined'), [
    { kind: 'status', symbol: 'status.locked', label: 'Needs Full Disk Access', detail: 'To read your Messages' },
    { kind: 'action', id: 'foundation.settings.full-disk-access', label: 'Open Full Disk Access settings', symbol: 'action.permission', opens: 'settings' },
  ]);
  assert.deepEqual(permissionMenuItems(fda, 'denied')[0], { kind: 'status', symbol: 'status.locked', label: 'Full Disk Access is off', detail: 'Turn on Textbutler to read your Messages' });
  // Keychain has no pane, so no Settings action.
  assert.equal(permissionMenuItems(CHROME_SAFE_STORAGE({ product: 'Ghostget', command: 'ghostget' }, { caller: 'security' }), 'unknown').length, 1);
  assert.deepEqual(permissionErrorJson(fda, 'denied'), JSON.parse(
    '{"ok":false,"error":{"code":"permission-denied","message":"Textbutler can\'t read your Messages: macOS access is off for Textbutler.","next":"textbutler doctor","permission":{"kind":"full-disk-access","settingsUrl":"x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"}}}'));
  assert.equal(permissionError(fda, 'unknown').code, 'permission-unknown');
});

test('prePrompt shows notices by audience and never waits without two terminals', async () => {
  const fda = MESSAGES_FDA(textbutler);
  {
    const { io, written, opened } = fakeIO({ keys: ['enter'] });
    assert.equal(await prePrompt(fda, { io, audience: 'human' }), 'continue');
    assert.match(written.join(''), /Press Enter to open Settings/);
    assert.deepEqual(opened, ['x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles']);
  }
  {
    const { io, opened } = fakeIO({ keys: ['s'] });
    assert.equal(await prePrompt(fda, { io, audience: 'human' }), 'skip');
    assert.deepEqual(opened, []);
  }
  {
    const { io } = fakeIO({ keys: ['timeout'] });
    assert.equal(await prePrompt(AUTOMATION(textbutler, 'Messages', 'x.'), { io, audience: 'human' }), 'skip');
  }
  {
    // Asks-kind Enter continues without opening Settings.
    const { io, opened } = fakeIO({ keys: ['enter'] });
    assert.equal(await prePrompt(AUTOMATION(textbutler, 'Messages', 'x.'), { io, audience: 'human' }), 'continue');
    assert.deepEqual(opened, []);
  }
  {
    let asked = false;
    const { io, written } = fakeIO({ stdinIsTTY: false, readKey: async () => { asked = true; return 'enter'; } });
    assert.equal(await prePrompt(fda, { io, audience: 'human' }), 'unattended-stop');
    assert.equal(asked, false);
    assert.doesNotMatch(written.join(''), /Press Enter/);
  }
  {
    const { io, written } = fakeIO();
    assert.equal(await prePrompt(LOGIN_ITEM(textbutler), { io, audience: 'human' }), 'continue');
    assert.doesNotMatch(written.join(''), /Press Enter/);
  }
  {
    const { io, written } = fakeIO();
    assert.equal(await prePrompt(fda, { io, audience: 'quiet' }), 'unattended-stop');
    assert.equal(await prePrompt(LOGIN_ITEM(textbutler), { io, audience: 'quiet' }), 'unattended-proceed');
    assert.deepEqual(written, []);
  }
  {
    const { io, written } = fakeIO();
    assert.equal(await prePrompt({ ...fda, whenUnattended: 'proceed' }, { io, audience: 'agent' }), 'unattended-proceed');
    const line = JSON.parse(written.join(''));
    assert.deepEqual(Object.keys(line), ['type', 'product', 'kind', 'message']);
    assert.equal(line.type, 'permission-notice');
    assert.equal(line.kind, 'full-disk-access');
  }
  {
    // The audience comes from the environment when not given.
    const { io, written } = fakeIO({ env: { ...UTF8, CLAUDECODE: '1' } });
    await prePrompt(fda, { io });
    assert.match(written.join(''), /^\{"type":"permission-notice"/);
  }
  {
    const { io, written } = fakeIO({ env: { ...UTF8, NO_COLOR: '1', TERM: 'dumb' }, stdinIsTTY: false });
    await prePrompt(fda, { io, audience: 'human' });
    assert.match(written.join(''), /^NOTE Textbutler needs Full Disk Access/);
  }
});

test('reportPermissionFailure offers Settings only at a terminal', async () => {
  const fda = MESSAGES_FDA(textbutler);
  const { io, written, opened } = fakeIO({ keys: ['o'] });
  await reportPermissionFailure(fda, 'denied', { io, audience: 'human' });
  assert.match(written.join(''), /press o to open Settings/);
  assert.equal(opened.length, 1);
  const agent = fakeIO({ keys: ['o'] });
  await reportPermissionFailure(fda, 'denied', { io: agent.io, audience: 'agent' });
  assert.deepEqual(agent.written, []);
  const quiet = fakeIO({ keys: ['o'] });
  await reportPermissionFailure(fda, 'denied', { io: quiet.io, audience: 'quiet' });
  assert.doesNotMatch(quiet.written.join(''), /press o/);
  assert.equal(quiet.opened.length, 0);
});

test('permissionStatus probes only where no prompt can appear', async () => {
  const seen: string[] = [];
  const { io } = fakeIO({ env: { HOME: '/Users/test' }, fileAccess: async path => { seen.push(path); return path.endsWith('chat.db') ? 'denied' : 'ok'; } });
  assert.equal(await permissionStatus('full-disk-access', 'Messages', io), 'denied');
  assert.equal(await permissionStatus('full-disk-access', 'Safari', io), 'granted');
  assert.equal(await permissionStatus('full-disk-access', '/Users/test/Library/Containers/com.apple.Safari/Data/x', io), 'unknown');
  assert.equal(await permissionStatus('full-disk-access', 'relative/path', io), 'unknown');
  for (const path of ['/Users/test/Documents/a', '/Users/test/Desktop', '/Volumes/USB/x', '/Users/test/Library/Mobile Documents/x', '/Users/test/Library/Group Containers/x', '/Users/test/Library/CloudStorage/x']) {
    assert.equal(await permissionStatus('full-disk-access', path, io), 'unknown', path);
  }
  assert.equal(await permissionStatus('full-disk-access', '/Users/test/Library/Mail', io), 'granted');
  assert.equal(await permissionStatus('full-disk-access', undefined, io), 'unknown');
  assert.deepEqual(seen, ['/Users/test/Library/Messages/chat.db', '/Users/test/Library/Safari', '/Users/test/Library/Mail']);
  for (const kind of ['automation', 'contacts', 'keychain', 'camera', 'notifications'] as const) assert.equal(await permissionStatus(kind, undefined, io), 'unknown');
  const argv: string[][] = [];
  const tools = fakeIO({ run: async command => { argv.push([...command]); return { status: 2 }; } });
  assert.equal(await permissionStatus('developer-tools', undefined, tools.io), 'not-determined');
  assert.deepEqual(argv, [['/usr/bin/xcode-select', '-p']]);
  const login = fakeIO({ env: { HOME: '/Users/test' }, fileAccess: async path => path.endsWith('app.hraness.companion.sponge.plist') ? 'ok' : 'missing' });
  assert.equal(await permissionStatus('login-item', 'sponge', login.io), 'granted');
  assert.equal(await permissionStatus('login-item', 'textbutler', login.io), 'not-determined');
  assert.equal(await permissionStatus('login-item', '../evil', login.io), 'unknown');
  const broken = fakeIO({ fileAccess: async () => { throw new Error('boom'); } });
  assert.equal(await permissionStatus('full-disk-access', '/tmp/x', broken.io), 'unknown');
});

test('openPermissionSettings opens only allowlisted panes', async () => {
  const { io, opened } = fakeIO();
  assert.equal(await openPermissionSettings('keychain', io), false);
  assert.equal(await openPermissionSettings('developer-tools', io), false);
  assert.equal(await openPermissionSettings('login-item', io), true);
  assert.deepEqual(opened, ['x-apple.systempreferences:com.apple.LoginItems-Settings.extension']);
});

test('classifyKeychainStatus maps OSStatus and security exit codes', () => {
  assert.equal(classifyKeychainStatus(-128), 'denied');
  assert.equal(classifyKeychainStatus(-25293), 'denied');
  assert.equal(classifyKeychainStatus(51), 'denied');
  assert.equal(classifyKeychainStatus(-25308), 'unknown');
  assert.equal(classifyKeychainStatus(36), 'unknown');
  assert.equal(classifyKeychainStatus(-25300), 'missing');
  assert.equal(classifyKeychainStatus(44), 'missing');
  assert.equal(classifyKeychainStatus(0), undefined);
  assert.equal(classifyKeychainStatus(1), undefined);
});

// Golden files hold every preset for every surface and state. The Rust twin
// (crates/hraness-cli-kit) checks the same files. Regenerate with
// UPDATE_GOLDEN=1 npm run check:sdk and review the diff.
const PRESETS: Record<string, (ref: ProductRef) => PermissionNeed> = {
  'login-item': ref => LOGIN_ITEM(ref),
  'chrome-safe-storage': ref => CHROME_SAFE_STORAGE(ref, { caller: 'security' }),
  'messages-fda': ref => MESSAGES_FDA(ref),
  automation: ref => AUTOMATION(ref, 'Messages', `${ref.product} only sends replies in chats you turn on.`),
  contacts: ref => CONTACTS(ref),
  'local-network': ref => LOCAL_NETWORK(ref, 'Room members on your network connect to this Mac.'),
  'incoming-connections': ref => INCOMING_CONNECTIONS(ref, { listener: 'vhalla', why: 'Choose Allow so room members can reach this Mac.' }),
  'xcode-tools': ref => XCODE_TOOLS(ref, { skipEffect: `${ref.product} runs without its native helper` }),
  'screen-recording': ref => SCREEN_RECORDING(ref, `${ref.product} only captures the window you pick.`),
  'local-signing': ref => LOCAL_SIGNING(ref),
};
function golden(name: string, make: (ref: ProductRef) => PermissionNeed): string {
  const out: string[] = [];
  const env = { LANG: 'en_US.UTF-8', TERM_PROGRAM: 'Apple_Terminal' };
  for (const [variant, ref] of [['local app', { product: 'Example', command: 'example', requester: 'Example' }], ['terminal', { product: 'Example', command: 'example' }]] as const) {
    const need = make(ref);
    out.push(`# ${name} · requester: ${variant}`);
    out.push('## cli pre-prompt, interactive', formatNotice(renderPrePrompt(need, 'cli', env), { kind: 'pre-prompt', interactive: true }).trimEnd());
    out.push('## cli pre-prompt, ascii', formatNotice(renderPrePrompt(need, 'cli', env), { kind: 'pre-prompt', interactive: false, style: { color: false, ascii: true } }).trimEnd());
    out.push('## dialog', JSON.stringify(permissionNoticeRequest(need, env)));
    for (const state of ['denied', 'unknown', 'missing'] as const) {
      out.push(`## cli recovery ${state}`, formatNotice(renderRecovery(need, state, 'cli', env), { kind: 'recovery', interactive: true }).trimEnd());
      out.push(`## dialog recovery ${state}`, JSON.stringify(renderRecovery(need, state, 'dialog', env)));
      out.push(`## json ${state}`, JSON.stringify(permissionErrorJson(need, state, env)));
    }
    for (const state of ['not-determined', 'denied'] as const) out.push(`## menu ${state}`, ...permissionMenuItems(need, state, env).map(item => JSON.stringify(item)));
    out.push('');
  }
  return out.join('\n');
}
test('golden copy for every preset, surface and state', () => {
  if (process.env.UPDATE_GOLDEN === '1') mkdirSync(GOLDEN, { recursive: true });
  for (const [name, make] of Object.entries(PRESETS)) {
    const file = new URL(`${name}.txt`, GOLDEN);
    const actual = golden(name, make);
    if (process.env.UPDATE_GOLDEN === '1') writeFileSync(file, actual);
    assert.ok(existsSync(file), `missing golden ${name}.txt; run with UPDATE_GOLDEN=1`);
    assert.equal(actual, readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), `${name}.txt drifted`);
  }
});
