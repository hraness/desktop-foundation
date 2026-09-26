import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test, { type TestContext } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { MenuActionError, runCompanion, runnerProtocols, type CompanionOptions } from '../src/client.js';
import { layout, openAtLoginItem } from '../src/menu-kit.js';
import type { PermissionIO } from '../src/permissions.js';
import type { MenuItemV2, SnapshotV2 } from '../src/protocol-v2.js';
import type { Snapshot } from '../src/protocol.js';

interface Script { protocols: string; actions?: Record<number, string> }

/**
 * A runner that answers `--version` with the given protocols, records every
 * frame it reads, and sends scripted action events after the Nth snapshot.
 */
function fakeRunner(script: Script, tracePath: string): string {
  return `
    import { createInterface } from 'node:readline';
    import { appendFileSync } from 'node:fs';
    const script = ${JSON.stringify(script)};
    const trace = value => appendFileSync(${JSON.stringify(tracePath)}, JSON.stringify(value) + '\\n');
    if (process.argv.includes('--version')) { trace({ probe: true }); process.stdout.write('hraness-companion 0.8.0 protocol/' + script.protocols + '\\n'); process.exit(0); }
    let seen = 0;
    const alive = setInterval(() => {}, 1000);
    const input = createInterface({ input: process.stdin });
    input.on('line', line => {
      const value = JSON.parse(line);
      trace(value);
      if (value.type === 'quit') process.exit(0);
      if (value.type !== 'snapshot') return;
      seen++;
      const send = event => process.stdout.write(JSON.stringify({ version: value.version, ...event }) + '\\n');
      if (seen === 1) send({ type: 'ready', pid: process.pid, platform: process.platform });
      const id = script.actions?.[seen];
      if (id) send({ type: 'action', id, revision: value.revision });
    });
    input.on('close', () => { clearInterval(alive); process.exit(0); });
  `;
}

const status: MenuItemV2 = { kind: 'status', symbol: 'status.running', label: 'Running' };
const menu = (extra: Partial<Parameters<typeof layout>[0]> = {}): MenuItemV2[] => layout({
  name: 'Client test', status,
  primary: { kind: 'action', id: 'open', label: 'Open dashboard', symbol: 'action.open', opens: 'browser', alternate: { id: 'open.copy', label: 'Copy link', symbol: 'action.copy' } },
  controls: [{ kind: 'action', id: 'pause', label: 'Pause', symbol: 'action.pause' }],
  ...extra,
});

async function start(t: TestContext, script: Script, overrides: Partial<CompanionOptions> = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'companion-v2-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const diagnostics: string[] = [];
  const tracePath = join(dir, 'trace.jsonl');
  const session = await runCompanion({
    appId: 'test.client', name: 'Client test', mark: { symbol: 'mark.agent', letters: 'Ct' }, stateDir: dir,
    binary: process.execPath, binaryArgs: ['--input-type=module', '-e', fakeRunner(script, tracePath), '--'],
    timeoutMs: 2000, refreshMs: 60_000,
    snapshot: () => menu(), onAction: () => {},
    onDiagnostic: code => diagnostics.push(code), ...overrides,
  });
  t.after(() => session.quit());
  const frames = async () => (await readFile(tracePath, 'utf8').catch(() => '')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line) as Record<string, unknown>);
  const snapshots = async () => (await frames()).filter(frame => frame.type === 'snapshot') as unknown as Array<SnapshotV2 | Snapshot>;
  return { session, diagnostics, dir, frames, snapshots };
}

async function eventually(predicate: () => boolean | Promise<boolean>, timeout = 3000): Promise<void> {
  const deadline = Date.now() + timeout;
  while (!await predicate()) {
    if (Date.now() >= deadline) throw new Error('Condition did not become true');
    await delay(20);
  }
}

const labels = (snapshot: SnapshotV2 | Snapshot | undefined) => (snapshot?.items ?? []).map(item => 'label' in item ? item.label : item.kind);

test('runnerProtocols reads --version and falls back to v1', async () => {
  assert.deepEqual(await runnerProtocols(process.execPath, ['-e', 'console.log("hraness-companion 0.8.0 protocol/1,2")', '--']), [1, 2]);
  assert.deepEqual(await runnerProtocols(process.execPath, ['-e', 'console.log("hraness-companion 0.7.0 protocol/1")', '--']), [1]);
  assert.deepEqual(await runnerProtocols(join(tmpdir(), 'missing-runner-binary')), [1]);
  assert.deepEqual(await runnerProtocols(process.execPath, ['-e', 'setTimeout(() => {}, 5000)', '--'], 150), [1]);
});

test('a v2 product speaks v2 to a runner that reports protocol/2', { timeout: 8000 }, async t => {
  const fixture = await start(t, { protocols: '1,2' });
  assert.equal(await fixture.session.ready, 'running');
  assert.equal(fixture.session.protocol, 2);
  await fixture.session.quit();
  const frames = await fixture.frames();
  assert.deepEqual(frames[0], { probe: true });
  const first = frames[1] as unknown as SnapshotV2;
  assert.equal(first.version, 2);
  assert.deepEqual(first.mark, { symbol: 'mark.agent', letters: 'Ct' });
  assert.equal(first.items[0]!.kind, 'header');
  assert.deepEqual(frames.at(-1), { version: 2, type: 'quit' });
});

test('a v2 product down-levels for a v1 runner', { timeout: 8000 }, async t => {
  const fixture = await start(t, { protocols: '1' });
  await fixture.session.ready;
  assert.equal(fixture.session.protocol, 1);
  const [first] = await fixture.snapshots();
  assert.equal(first!.version, 1);
  assert.equal((first as Snapshot).title, 'Ct');
  assert.deepEqual(labels(first).slice(0, 3), ['Client test', '● Running', 'separator']);
});

test('a v1 product never probes the runner', { timeout: 8000 }, async t => {
  const fixture = await start(t, { protocols: '1,2' }, {
    mark: undefined, title: 'Ct',
    snapshot: () => [{ kind: 'action', id: 'a', label: 'Do it' }, { kind: 'quit', label: 'Quit' }],
  });
  await fixture.session.ready;
  assert.equal(fixture.session.protocol, 1);
  assert.ok((await fixture.frames()).every(frame => !frame.probe));
});

test('subscribe pushes a refresh without waiting for the timer', { timeout: 8000 }, async t => {
  let push: (() => void) | undefined;
  let aborted: AbortSignal | undefined;
  let count = 0;
  const fixture = await start(t, { protocols: '1,2' }, {
    subscribe: (refresh, signal) => { push = refresh; aborted = signal; },
    snapshot: () => menu({ status: { kind: 'status', symbol: 'status.running', label: `Running ${++count}` } }),
  });
  await fixture.session.ready;
  await eventually(() => push !== undefined);
  push!(); push!(); push!();
  await eventually(async () => (await fixture.snapshots()).length >= 2);
  await delay(100);
  // Three pushes during one refresh coalesce into at most two reads.
  assert.ok((await fixture.snapshots()).length <= 3);
  await fixture.session.quit();
  assert.equal(aborted?.aborted, true);
});

test('a failed action shows a ⚠︎ row, generic unless the product throws MenuActionError', { timeout: 8000 }, async t => {
  const fixture = await start(t, { protocols: '1,2', actions: { 1: 'pause', 2: 'open.copy' } }, {
    onAction: id => { if (id === 'pause') throw new MenuActionError("Couldn't pause replies", 'Textbutler is offline'); throw new Error('private detail'); },
  });
  await fixture.session.ready;
  await eventually(async () => (await fixture.snapshots()).length >= 3);
  const [, second, third] = await fixture.snapshots() as SnapshotV2[];
  assert.deepEqual(second!.items.slice(0, 3), [
    { kind: 'header', label: 'Client test' }, status,
    { kind: 'status', symbol: 'status.attention', label: "Couldn't pause replies", detail: 'Textbutler is offline' },
  ]);
  assert.deepEqual(third!.items[2], { kind: 'status', symbol: 'status.attention', label: 'Couldn\'t finish "Copy link"', detail: 'Try again in a moment' });
  assert.ok(!JSON.stringify(third).includes('private'));
  assert.deepEqual(fixture.diagnostics, ['action-indeterminate', 'action-indeterminate']);
});

test('a failing snapshot sends one degraded menu, then recovers', { timeout: 8000 }, async t => {
  let failing = false;
  const fixture = await start(t, { protocols: '1,2' }, {
    snapshot: () => { if (failing) throw new Error('daemon down'); return menu(); },
    degraded: () => ({ primary: { kind: 'action', id: 'help', label: 'Help & support', symbol: 'action.support', opens: 'browser' } }),
  });
  await fixture.session.ready;
  failing = true;
  await fixture.session.refresh();
  await fixture.session.refresh();
  failing = false;
  await fixture.session.refresh();
  const [, degraded, recovered, extra] = await fixture.snapshots() as SnapshotV2[];
  assert.deepEqual(degraded!.items.slice(0, 2), [
    { kind: 'header', label: 'Client test' },
    { kind: 'status', symbol: 'status.attention', label: "Can't reach Client test", detail: 'Retrying…' },
  ]);
  assert.ok(labels(degraded).includes('Help & support'));
  assert.deepEqual(labels(recovered), labels({ ...degraded!, items: menu() }));
  assert.equal(extra, undefined);
  assert.deepEqual(fixture.diagnostics, ['snapshot-unavailable', 'snapshot-unavailable']);
});

test('a menu over the node budget reports menu-too-large', { timeout: 8000 }, async t => {
  let big = false;
  const fixture = await start(t, { protocols: '1,2' }, {
    snapshot: () => big ? Array.from({ length: 300 }, (_, index): MenuItemV2 => ({ kind: 'action', id: `a${index}`, label: 'Row' })) : menu(),
  });
  await fixture.session.ready;
  big = true;
  await fixture.session.refresh();
  assert.deepEqual(fixture.diagnostics, ['menu-too-large']);
});

test('foundation.login toggles the login item the SDK owns', { timeout: 8000, skip: process.platform === 'win32' ? 'Windows login items need a native .exe' : false }, async t => {
  const home = await mkdtemp(join(tmpdir(), 'companion-home-'));
  t.after(() => rm(home, { recursive: true, force: true }));
  const fixture = await start(t, { protocols: '1,2', actions: { 1: 'foundation.login', 2: 'foundation.login' } }, {
    snapshot: () => menu({ openAtLogin: true }),
    loginItem: { executable: process.execPath, args: ['product.js', '--foreground'], home, env: {} },
  });
  await fixture.session.ready;
  await eventually(async () => (await fixture.snapshots()).length >= 3);
  const rows = (await fixture.snapshots() as SnapshotV2[]).map(snapshot => snapshot.items.find(item => item.kind === 'action' && item.id === 'foundation.login'));
  assert.deepEqual(rows.slice(0, 3), [openAtLoginItem('off'), openAtLoginItem('on'), openAtLoginItem('off')]);
  const file = process.platform === 'darwin' ? join(home, 'Library', 'LaunchAgents', 'app.hraness.companion.test.client.plist') : join(home, '.config', 'autostart', 'hraness-companion-test.client.desktop');
  assert.equal(existsSync(file), false);
});

test('without a login command the Open at login row is disabled', { timeout: 8000 }, async t => {
  const fixture = await start(t, { protocols: '1,2' }, { snapshot: () => menu({ openAtLogin: true }) });
  await fixture.session.ready;
  const [first] = await fixture.snapshots() as SnapshotV2[];
  assert.equal((first!.items.find(item => item.kind === 'action' && item.id === 'foundation.login') as { enabled?: boolean }).enabled, false);
});

test('foundation.settings actions open the allowlisted pane without reaching the product', { timeout: 8000 }, async t => {
  const opened: string[] = [];
  const product: string[] = [];
  const io = { env: {}, stdinIsTTY: false, stderrIsTTY: false, write: () => {}, readKey: async () => 'timeout' as const, openUrl: async (url: string) => { opened.push(url); return true; } } satisfies PermissionIO;
  const fixture = await start(t, { protocols: '1,2', actions: { 1: 'foundation.settings.full-disk-access' } }, {
    permissionIO: io, onAction: id => { product.push(id); },
    snapshot: () => layout({ name: 'Client test', status: { kind: 'status', symbol: 'status.locked', label: 'Needs Full Disk Access' }, primary: { kind: 'action', id: 'foundation.settings.full-disk-access', label: 'Open Full Disk Access settings', symbol: 'action.permission', opens: 'settings' } }),
  });
  await fixture.session.ready;
  await eventually(() => opened.length === 1);
  assert.deepEqual(opened, ['x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles']);
  assert.deepEqual(product, []);
});

test('a v1 product needs a title', async () => {
  await assert.rejects(runCompanion({ appId: 'test.client', name: 'x', stateDir: tmpdir(), binary: process.execPath, snapshot: () => [], onAction: () => {} }), /invalid-title/);
});
