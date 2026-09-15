import { createServer, request } from 'node:http';
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { open, lstat, readdir, rename, unlink } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { runCompanion, type CompanionOptions } from './client.js';
import { assertAppId } from './protocol.js';
import { ensurePrivateDirectory } from './install.js';

interface Receipt { version: 1; appId: string; port: number; token: string; instance: string }
export interface CompanionStatus { running: boolean | null; appId: string; state: 'running' | 'stopped' | 'unreachable' }
const receiptPath = (directory: string, instance: string) => join(directory, `.companion-service-${instance}.json`);
async function privateDirectory(directory: string): Promise<void> {
  if (!isAbsolute(directory)) throw new Error('absolute-state-directory-required');
  await ensurePrivateDirectory(directory);
}
async function readReceipt(path: string, appId: string): Promise<Receipt | undefined> {
  let handle;
  try {
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 2048 || (process.platform !== 'win32' && ((stat.mode & 0o077) !== 0 || stat.uid !== process.getuid?.()))) throw new Error('unsafe-service-receipt');
    handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const actual = await handle.stat();
    if (actual.ino !== stat.ino || actual.dev !== stat.dev || actual.size > 2048) throw new Error('changed-service-receipt');
    const value = JSON.parse(await handle.readFile('utf8')) as Receipt;
    if (value.version !== 1 || value.appId !== appId || !Number.isInteger(value.port) || value.port < 1 || value.port > 65535 || !/^[a-f0-9]{64}$/.test(value.token) || !/^[a-f0-9]{32}$/.test(value.instance)) throw new Error('invalid-service-receipt');
    if (!path.endsWith(`.companion-service-${value.instance}.json`)) throw new Error('invalid-service-instance');
    return value;
  } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined; throw error; }
  finally { await handle?.close(); }
}
async function receipts(directory: string, appId: string): Promise<Receipt[]> {
  let names: string[];
  try { names = (await readdir(directory)).filter(name => /^\.companion-service-[a-f0-9]{32}\.json$/.test(name)); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []; throw error; }
  if (names.length > 128) throw new Error('too-many-service-receipts: inspect stale companion owners');
  return (await Promise.all(names.map(name => readReceipt(join(directory, name), appId)))).filter((value): value is Receipt => !!value);
}
async function discover(directory: string, appId: string): Promise<{ owner?: Receipt; uncertain: boolean }> {
  const found = await receipts(directory, appId);
  const live = (await Promise.all(found.map(async value => await exchange(value, 'status') ? value : undefined))).filter((value): value is Receipt => !!value);
  if (live.length > 1) throw new Error('multiple-service-owners');
  return { owner: live[0], uncertain: found.length > 0 };
}
async function exchange(receipt: Receipt, operation: 'status' | 'stop'): Promise<boolean> {
  return await new Promise(resolveResult => {
    let settled = false;
    const finish = (value: boolean) => { if (!settled) { settled = true; clearTimeout(deadline); resolveResult(value); } };
    const deadline = setTimeout(() => { req.destroy(); finish(false); }, 1000);
    const req = request({ hostname: '127.0.0.1', port: receipt.port, path: `/${operation}`, method: operation === 'stop' ? 'POST' : 'GET', headers: { Authorization: `Bearer ${receipt.token}` }, timeout: 750 }, response => {
      let bytes = 0, text = '';
      response.on('data', chunk => { bytes += chunk.length; if (bytes > 2048) { req.destroy(); finish(false); } else text += chunk; });
      response.on('end', () => {
        try { const body = JSON.parse(text); finish(response.statusCode === 200 && body.instance === receipt.instance && body.appId === receipt.appId); }
        catch { finish(false); }
      });
      response.on('error', () => finish(false));
    });
    req.on('timeout', () => req.destroy()); req.on('error', () => finish(false)); req.end();
  });
}
export async function companionStatus(stateDir: string, appId: string): Promise<CompanionStatus> {
  assertAppId(appId);
  const { owner, uncertain } = await discover(stateDir, appId);
  return owner ? { appId, running: true, state: 'running' } : uncertain ? { appId, running: null, state: 'unreachable' } : { appId, running: false, state: 'stopped' };
}
export async function stopCompanion(stateDir: string, appId: string): Promise<CompanionStatus> {
  assertAppId(appId);
  const { owner, uncertain } = await discover(stateDir, appId);
  if (!owner) {
    if (uncertain) throw new Error('stop-indeterminate: service receipt exists but the owner is unreachable');
    return { appId, running: false, state: 'stopped' };
  }
  if (!await exchange(owner, 'stop')) throw new Error('stop-indeterminate');
  for (let attempt = 0; attempt < 60; attempt++) {
    if (!await readReceipt(receiptPath(stateDir, owner.instance), appId)) return await companionStatus(stateDir, appId);
    await delay(100);
  }
  throw new Error('stop-timeout');
}

/** Foreground owner, invoked by the product CLI's private --foreground branch. */
export async function serveCompanion(options: CompanionOptions): Promise<number> {
  assertAppId(options.appId);
  await privateDirectory(options.stateDir);
  if ((await companionStatus(options.stateDir, options.appId)).running) return 0;
  const session = await runCompanion(options);
  let rendererClosed = false;
  void session.closed.then(() => { rendererClosed = true; });
  const state = await session.ready;
  if (state === 'already-running') return await session.closed;
  const receipt: Receipt = { version: 1, appId: options.appId, port: 0, token: randomBytes(32).toString('hex'), instance: randomBytes(16).toString('hex') };
  const server = createServer((req, res) => {
    if (rendererClosed) { res.writeHead(503).end(); return; }
    const auth = req.headers.authorization ?? '';
    const expected = `Bearer ${receipt.token}`;
    const supplied = Buffer.from(auth), wanted = Buffer.from(expected);
    if (req.headers.origin || supplied.length !== wanted.length || !timingSafeEqual(supplied, wanted)) { res.writeHead(403).end(); return; }
    if (!((req.method === 'GET' && req.url === '/status') || (req.method === 'POST' && req.url === '/stop'))) { res.writeHead(404).end(); return; }
    if (req.url === '/stop') res.once('finish', () => { void session.quit(); });
    res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }).end(JSON.stringify({ appId: receipt.appId, instance: receipt.instance }));
  });
  server.requestTimeout = 1000; server.headersTimeout = 1000; server.maxHeadersCount = 16;
  const signal = () => { void session.quit(); };
  const temp = join(options.stateDir, `.service-${receipt.instance}.tmp`);
  try {
    await new Promise<void>((resolveListen, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolveListen(); }); });
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('service-listen-failed');
    receipt.port = address.port;
    // Instance-specific receipts cannot replace a newer owner's discovery data,
    // even if the renderer dies while this asynchronous setup is in progress.
    if (rendererClosed) return await session.closed;
    const handle = await open(temp, 'wx', 0o600);
    try { await handle.writeFile(JSON.stringify(receipt)); await handle.sync(); } finally { await handle.close(); }
    if (rendererClosed) return await session.closed;
    await rename(temp, receiptPath(options.stateDir, receipt.instance));
    process.once('SIGINT', signal); process.once('SIGTERM', signal);
    return await session.closed;
  } finally {
    process.removeListener('SIGINT', signal); process.removeListener('SIGTERM', signal);
    await session.quit();
    server.closeAllConnections(); await new Promise<void>(resolveClose => server.close(() => resolveClose()));
    await unlink(receiptPath(options.stateDir, receipt.instance)).catch(() => {});
    await unlink(temp).catch(() => {});
  }
}

/** Spawn the existing product entrypoint without a shell, then confirm its service. */
export async function startCompanion(options: { appId: string; stateDir: string; executable: string; args: readonly string[]; timeoutMs?: number }): Promise<CompanionStatus> {
  assertAppId(options.appId); await privateDirectory(options.stateDir);
  if (!isAbsolute(options.executable) || options.args.some(arg => arg.includes('\0'))) throw new Error('invalid-launch-command');
  const existing = await companionStatus(options.stateDir, options.appId);
  if (existing.running) return existing;
  const timeout = options.timeoutMs ?? 30_000;
  if (!Number.isFinite(timeout) || timeout < 100 || timeout > 300_000) throw new Error('invalid-launch-deadline');
  const child = spawn(options.executable, [...options.args], { detached: true, stdio: 'ignore', windowsHide: true });
  let failed = false, exited = false, exitCode: number | null = null;
  child.once('error', () => { failed = true; }); child.once('exit', code => { exited = true; exitCode = code; }); child.unref();
  const deadline = Date.now() + timeout;
  try {
    while (Date.now() < deadline) {
      const status = await companionStatus(options.stateDir, options.appId);
      if (status.running) return status;
      // A successful contender may exit on the native singleton lock just
      // before the winning owner publishes its receipt. Keep waiting for it.
      if (failed || (exited && exitCode !== 0)) throw new Error('companion-start-failed: run the foreground command and doctor for OS approval or dependency guidance');
      await delay(100);
    }
    throw new Error('companion-start-timeout: check doctor and the foreground command');
  } catch (error) {
    // Only this directly spawned child is ours; never signal a receipt PID.
    if (!failed && !exited) {
      child.kill('SIGTERM');
      for (let i=0; i<20 && !exited; i++) await delay(50);
      if (!exited) child.kill('SIGKILL');
    }
    throw error;
  }
}
