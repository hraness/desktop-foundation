import { createServer, request } from 'node:http';
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { open, mkdir, lstat, readFile, rename, unlink } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { runCompanion, type CompanionOptions } from './client.js';
import { assertAppId } from './protocol.js';
import { ensurePrivateDirectory } from './install.js';

interface Receipt { version: 1; appId: string; port: number; token: string; instance: string }
export interface CompanionStatus { running: boolean; appId: string }
const receiptPath = (directory: string) => join(directory, 'companion-service.json');
async function privateDirectory(directory: string): Promise<void> {
  if (!isAbsolute(directory)) throw new Error('absolute-state-directory-required');
  await ensurePrivateDirectory(directory);
}
async function readReceipt(directory: string, appId: string): Promise<Receipt | undefined> {
  const path = receiptPath(directory);
  let handle;
  try {
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 2048 || (process.platform !== 'win32' && ((stat.mode & 0o077) !== 0 || stat.uid !== process.getuid?.()))) throw new Error('unsafe-service-receipt');
    handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const actual = await handle.stat();
    if (actual.ino !== stat.ino || actual.dev !== stat.dev || actual.size > 2048) throw new Error('changed-service-receipt');
    const value = JSON.parse(await handle.readFile('utf8')) as Receipt;
    if (value.version !== 1 || value.appId !== appId || !Number.isInteger(value.port) || value.port < 1 || value.port > 65535 || !/^[a-f0-9]{64}$/.test(value.token) || !/^[a-f0-9]{32}$/.test(value.instance)) throw new Error('invalid-service-receipt');
    return value;
  } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined; throw error; }
  finally { await handle?.close(); }
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
  const receipt = await readReceipt(stateDir, appId);
  return { appId, running: receipt ? await exchange(receipt, 'status') : false };
}
export async function stopCompanion(stateDir: string, appId: string): Promise<CompanionStatus> {
  assertAppId(appId);
  const receipt = await readReceipt(stateDir, appId);
  if (!receipt || !await exchange(receipt, 'status')) return { appId, running: false };
  if (!await exchange(receipt, 'stop')) throw new Error('stop-indeterminate');
  for (let attempt = 0; attempt < 60; attempt++) {
    if (!await exchange(receipt, 'status')) return { appId, running: false };
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
  const state = await session.ready;
  if (state === 'already-running') return await session.closed;
  const receipt: Receipt = { version: 1, appId: options.appId, port: 0, token: randomBytes(32).toString('hex'), instance: randomBytes(16).toString('hex') };
  const server = createServer((req, res) => {
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
    // Native lock is held before publishing a receipt; another owner cannot replace it.
    const handle = await open(temp, 'wx', 0o600);
    try { await handle.writeFile(JSON.stringify(receipt)); await handle.sync(); } finally { await handle.close(); }
    await rename(temp, receiptPath(options.stateDir));
    process.once('SIGINT', signal); process.once('SIGTERM', signal);
    return await session.closed;
  } finally {
    process.removeListener('SIGINT', signal); process.removeListener('SIGTERM', signal);
    await session.quit();
    server.closeAllConnections(); await new Promise<void>(resolveClose => server.close(() => resolveClose()));
    const current = await readReceipt(options.stateDir, options.appId).catch(() => undefined);
    if (current?.instance === receipt.instance) await unlink(receiptPath(options.stateDir)).catch(() => {});
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
  let failed = false, exited = false;
  child.once('error', () => { failed = true; }); child.once('exit', () => { exited = true; }); child.unref();
  const deadline = Date.now() + timeout;
  try {
    while (Date.now() < deadline) {
      const status = await companionStatus(options.stateDir, options.appId);
      if (status.running) return status;
      if (failed || exited) throw new Error('companion-start-failed: run the foreground command and doctor for OS approval or dependency guidance');
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
