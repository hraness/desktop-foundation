import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launches, retireLegacyLoginItem, type RetireOptions } from '../src/retire.js';

const unixOnly = { skip: process.platform === 'win32' ? 'Unix sockets, modes and launchd are not on Windows' : false };

// Ported from textbutler's install-textbutler.test.ts (retireMenuLoginItem).
const LABEL = 'app.hraness.companion.textbutler';
const accepts: RetireOptions['accepts'] = ({ text }) =>
  (text.includes('textbutler.mjs</string>') && text.includes('<string>menubar</string>')) || text.includes('/TextButler.app/Contents/MacOS/TextButler<');
const plist = (args: string[]) => `<plist version="1.0"><dict><key>Label</key><string>${LABEL}</string><key>ProgramArguments</key><array>${args.map(a => `<string>${a}</string>`).join('')}</array></dict></plist>\n`;

async function fixture() {
  const home = await mkdtemp(join(tmpdir(), 'retire-'));
  const agents = join(home, 'Library', 'LaunchAgents');
  await mkdir(agents, { recursive: true });
  const booted: string[] = [];
  const options = (extra: Partial<RetireOptions> = {}): RetireOptions => ({ home, labels: [LABEL], accepts, bootout: async label => { booted.push(label); }, now: () => new Date(1_700_000_000_000), ...extra });
  return { home, agents, path: join(agents, `${LABEL}.plist`), booted, options };
}

test('nothing to retire when the item is absent', unixOnly, async () => {
  const f = await fixture();
  try {
    assert.equal(await retireLegacyLoginItem(f.options()), null);
    assert.deepEqual(f.booted, []);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('a foreign plist at the label is left alone', unixOnly, async () => {
  const f = await fixture();
  try {
    const text = plist(['/usr/local/bin/something-else', '--serve']);
    await writeFile(f.path, text);
    assert.equal(await retireLegacyLoginItem(f.options()), null);
    assert.equal(await readFile(f.path, 'utf8'), text);
    assert.deepEqual(f.booted, []);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('the menubar role is booted out and renamed aside, never deleted', unixOnly, async () => {
  const f = await fixture();
  try {
    const text = plist(['/opt/bun', '/x/textbutler.mjs', 'menubar']);
    await writeFile(f.path, text);
    assert.deepEqual(await retireLegacyLoginItem(f.options()), { label: LABEL, from: f.path, to: `${f.path}.retired-1700000000000` });
    assert.deepEqual(f.booted, [LABEL]);
    assert.deepEqual(await readdir(f.agents), [`${LABEL}.plist.retired-1700000000000`]);
    assert.equal(await readFile(`${f.path}.retired-1700000000000`, 'utf8'), text);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('the app variant is retired too', unixOnly, async () => {
  const f = await fixture();
  try {
    await writeFile(f.path, plist(['/Users/x/Applications/TextButler.app/Contents/MacOS/TextButler']));
    assert.equal((await retireLegacyLoginItem(f.options()))?.label, LABEL);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('a symlink at the label is not followed or touched', unixOnly, async () => {
  const f = await fixture();
  try {
    const target = join(f.home, 'real.plist');
    await writeFile(target, plist(['/opt/bun', '/x/textbutler.mjs', 'menubar']));
    await symlink(target, f.path);
    assert.equal(await retireLegacyLoginItem(f.options()), null);
    assert.deepEqual((await readdir(f.agents)).sort(), [`${LABEL}.plist`]);
    assert.deepEqual(f.booted, []);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test("a file owned by someone else is not touched", unixOnly, async () => {
  const f = await fixture();
  try {
    await writeFile(f.path, plist(['/opt/bun', '/x/textbutler.mjs', 'menubar']));
    assert.equal(await retireLegacyLoginItem(f.options({ uid: (process.getuid?.() ?? 0) + 1 })), null);
    assert.deepEqual(await readdir(f.agents), [`${LABEL}.plist`]);
    assert.deepEqual(f.booted, []);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('a directory or an oversized file is not touched, and labels are validated', unixOnly, async () => {
  const f = await fixture();
  try {
    await mkdir(f.path);
    assert.equal(await retireLegacyLoginItem(f.options()), null);
    await rm(f.path, { recursive: true });
    await writeFile(f.path, `${plist(['/opt/bun', '/x/textbutler.mjs', 'menubar'])}${' '.repeat(70_000)}`);
    assert.equal(await retireLegacyLoginItem(f.options()), null);
    await assert.rejects(retireLegacyLoginItem(f.options({ labels: ['../escape'] })), /Invalid login item label/);
  } finally { await rm(f.home, { recursive: true, force: true }); }
});

test('launches() matches the program and argument', unixOnly, () => {
  const text = plist(['/Users/x/.local/bin/hraness-companion', '--serve', 'hraness-companion-sponge-watch']);
  assert.equal(launches('/hraness-companion', 'hraness-companion-sponge-watch')({ label: LABEL, path: '', text }), true);
  assert.equal(launches('/hraness-companion', 'other')({ label: LABEL, path: '', text }), false);
  assert.equal(launches(/TextButler$/)({ label: LABEL, path: '', text }), false);
});
