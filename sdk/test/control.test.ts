import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { lstat, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import {
  adminRequest, agentRequest, bootId, CONTROL_PROTOCOL, controlStatus, ensureOwner, ownerPaths, ownerPathsIn,
  processStartId, readOwnerFile, serveControl, type OwnerInfo, type OwnerPaths,
} from '../src/control.js';
import { HranessError } from '../src/registry.js';
import { readContract } from './contract-helpers.js';

const unixOnly = { skip: process.platform === 'win32' ? 'Unix sockets, modes and launchd are not on Windows' : false };

const controlModule = new URL('../src/control.js', import.meta.url).href;
async function home() { return mkdtemp(join(tmpdir(), 'hc-')); }
function start(paths: OwnerPaths, extra: { admin?: (r: unknown) => Promise<unknown> } = {}) {
  const abort = new AbortController();
  let ready!: (info: OwnerInfo) => void;
  const readyP = new Promise<OwnerInfo>(resolve => { ready = resolve; });
  const done = serveControl({
    product: 'example', paths, signal: abort.signal, onReady: ready,
    agent: { protocols: { 'example.approvals/1': async (req: any) => { if (req?.op === 'boom') throw new HranessError('example.policy-locked', 'Locked.'); if (req?.op === 'crash') throw new Error('secret detail'); return { echo: req }; } }, maxBytes: 256 },
    admin: { handler: extra.admin },
  });
  return { abort, ready: readyP, done };
}
function raw(sock: string, text: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const s = createConnection(sock);
    let out = '';
    s.on('connect', () => s.write(text));
    s.on('data', c => { out += c; if (out.includes('\n')) { s.destroy(); resolve(out); } });
    s.on('end', () => resolve(out));
    s.on('error', reject);
  });
}
const code = (p: Promise<unknown>) => p.then(() => 'ok', (e: HranessError) => e.code);

test('ownerPaths follow the override, macOS and XDG rules', unixOnly, () => {
  assert.equal(ownerPaths('ex-ample', { EX_AMPLE_STATE_HOME: '/s', HOME: '/h' }).agentSock, '/s/control/agent.sock');
  assert.equal(ownerPaths('ex', { HOME: '/h' }, 'darwin').dir, '/h/Library/Application Support/ex/control');
  assert.equal(ownerPaths('ex', { HOME: '/h', XDG_STATE_HOME: '/x' }, 'linux').ownerJson, '/x/ex/control/owner.json');
  assert.equal(ownerPaths('ex', { HOME: '/h', XDG_STATE_HOME: 'rel' }, 'linux').cap, '/h/.local/state/ex/control/admin.cap');
  assert.throws(() => ownerPaths('../x'), /Invalid product/);
});

test('owner.json golden has the shape the owner writes', unixOnly, async () => {
  const golden = readContract('golden/owner.json');
  assert.deepEqual(Object.keys(golden).sort(), ['bootId', 'generation', 'pid', 'processStartId', 'schema']);
  assert.match((await bootId())!, /^[0-9a-f]{64}$/);
  assert.equal(await processStartId(process.pid), await processStartId(process.pid));
  assert.notEqual(await processStartId(process.pid), await processStartId(process.ppid));
});

test('serve: modes, hello on both sockets, agent protocols, admin cap, stop and cleanup', unixOnly, async () => {
  const dir = await home();
  const paths = ownerPathsIn(dir);
  const owner = start(paths);
  try {
    const info = await owner.ready;
    assert.deepEqual(info.protocols, ['example.approvals/1', CONTROL_PROTOCOL]);
    for (const [path, mode] of [[paths.dir, 0o700], [paths.agentSock, 0o600], [paths.adminSock, 0o600], [paths.cap, 0o600], [paths.ownerJson, 0o600]] as const) {
      assert.equal((await lstat(path)).mode & 0o777, mode, path);
    }
    const file = await readOwnerFile(paths);
    assert.equal(file.pid, process.pid);
    assert.equal(file.generation, info.generation);
    assert.deepEqual(await agentRequest(paths, 'example.approvals/1', { op: 'list' }), { echo: { op: 'list' } });
    assert.equal(await code(agentRequest(paths, 'example.approvals/1', { op: 'boom' })), 'example.policy-locked');
    await assert.rejects(agentRequest(paths, 'example.approvals/1', { op: 'crash' }), (e: HranessError) => e.code === 'internal' && !e.message.includes('secret'));
    assert.equal(await code(agentRequest(paths, 'other/1', {})), 'permission-denied');
    assert.equal(await code(agentRequest(paths, CONTROL_PROTOCOL, { op: 'control.stop' })), 'permission-denied');
    assert.deepEqual(JSON.parse(await raw(paths.agentSock, `${JSON.stringify(readContract('golden/control-admin-request.json'))}\n`)), readContract('golden/control-response-error.json'));
    assert.equal(JSON.parse(await raw(paths.agentSock, 'not json\n')).error.code, 'usage');
    assert.equal(JSON.parse(await raw(paths.agentSock, '{"v":2,"protocol":"x","request":{}}\n')).error.code, 'usage');
    assert.equal(JSON.parse(await raw(paths.agentSock, `${'x'.repeat(400)}`)).error.code, 'usage');
    assert.equal(JSON.parse(await raw(paths.adminSock, `{"v":1,"cap":"${'0'.repeat(64)}","request":{"op":"control.hello"}}\n`)).error.code, 'permission-denied');
    assert.equal(JSON.parse(await raw(paths.adminSock, '{"v":1,"request":{"op":"control.hello"}}\n')).error.code, 'permission-denied');
    assert.equal((await adminRequest<OwnerInfo>(paths, { op: 'control.hello' })).generation, info.generation);
    assert.equal(await code(adminRequest(paths, { op: 'unknown' })), 'not-found');
    assert.deepEqual(await controlStatus(paths), { running: true, stale: false, owner: info });
    assert.deepEqual(await adminRequest(paths, { op: 'control.stop' }), readContract('golden/control-response-ok.json').result);
    await owner.done;
    assert.deepEqual((await readdir(paths.dir)).sort(), []);
    assert.deepEqual(await controlStatus(paths), { running: false, stale: false });
    assert.equal(await code(agentRequest(paths, 'example.approvals/1', {})), 'owner-unavailable');
  } finally { owner.abort.abort(); await owner.done.catch(() => {}); await rm(dir, { recursive: true, force: true }); }
});

test('a second owner gets control-already-running and the first keeps serving', unixOnly, async () => {
  const dir = await home();
  const paths = ownerPathsIn(dir);
  const first = start(paths);
  try {
    const info = await first.ready;
    const second = start(paths);
    assert.equal(await code(second.done), 'control-already-running');
    assert.equal((await controlStatus(paths)).owner?.generation, info.generation);
  } finally { first.abort.abort(); await first.done; await rm(dir, { recursive: true, force: true }); }
});

const childScript = (dir: string) => `import(${JSON.stringify(controlModule)}).then(m => m.serveControl({ product: 'example', paths: m.ownerPathsIn(${JSON.stringify(dir)}), agent: { protocols: {} }, admin: {}, signal: new AbortController().signal }))`;

test('a killed owner leaves stale files that the next owner reclaims without signalling anyone', unixOnly, async () => {
  const dir = await home();
  const paths = ownerPathsIn(dir);
  const child = spawn(process.execPath, ['--input-type=module', '-e', childScript(dir)], { stdio: 'ignore' });
  try {
    for (let i = 0; i < 200 && !(await controlStatus(paths)).running; i++) await delay(25);
    assert.equal((await controlStatus(paths)).running, true);
    child.kill('SIGKILL');
    await new Promise(resolve => child.once('exit', resolve));
    assert.deepEqual(await controlStatus(paths), { running: false, stale: true });
    const owner = start(paths);
    const info = await owner.ready;
    assert.equal(info.pid, process.pid);
    assert.ok((await readdir(paths.dir)).some(name => name.startsWith('owner.lock.stale-')), 'the stale claim is renamed aside, not deleted');
    owner.abort.abort();
    await owner.done;
  } finally { child.kill('SIGKILL'); await rm(dir, { recursive: true, force: true }); }
});

test('a non-socket at a socket path is refused, never removed', unixOnly, async () => {
  const dir = await home();
  const paths = ownerPathsIn(dir);
  const first = start(paths);
  await first.ready;
  first.abort.abort();
  await first.done;
  await writeFile(paths.agentSock, 'mine');
  try {
    assert.equal(await code(start(paths).done), 'permission-denied');
    assert.equal(await readFile(paths.agentSock, 'utf8'), 'mine');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('ensureOwner starts one owner, reuses it, and kills only its own child when it never answers', unixOnly, async () => {
  const dir = await home();
  const paths = ownerPathsIn(dir);
  let spawned = 0;
  const spawnOwner = () => { spawned++; return spawn(process.execPath, ['--input-type=module', '-e', childScript(dir)], { stdio: 'ignore' }); };
  try {
    const info = await ensureOwner(paths, spawnOwner);
    assert.equal((await ensureOwner(paths, spawnOwner)).generation, info.generation);
    assert.equal(spawned, 1);
    await adminRequest(paths, { op: 'control.stop' });
    for (let i = 0; i < 100 && (await controlStatus(paths)).running; i++) await delay(20);
    const silent = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
    await assert.rejects(ensureOwner(paths, () => silent, 300), (e: HranessError) => e.code === 'owner-unavailable');
    assert.equal(silent.signalCode, 'SIGKILL');
    const exits = spawn(process.execPath, ['-e', 'process.exit(0)'], { stdio: 'ignore' });
    await assert.rejects(ensureOwner(paths, () => exits, 300), (e: HranessError) => e.code === 'owner-unavailable');
  } finally { await adminRequest(paths, { op: 'control.stop' }).catch(() => {}); await rm(dir, { recursive: true, force: true }); }
});
