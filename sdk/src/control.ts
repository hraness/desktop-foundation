// One owner process per product, reached over two Unix sockets. The Rust
// twin is `hraness_control_kit::control`; docs/control.md has the wire format.
//
// <stateHome>/control/          0700
//   agent.sock                  0600  agents: named protocols plus control.hello
//   admin.sock                  0600  the product's own CLI, with admin.cap
//   admin.cap                   0600  a fresh random capability per owner
//   owner.json                  0600  {schema, pid, bootId, processStartId, generation}
//   owner.lock.<n>              0600  the TS owner's numbered claims (Node has no flock)
//
// Node has no peer-credential API, so a TS owner relies on the 0700
// directory, the 0600 sockets and the capability.

import { execFile, type ChildProcess } from 'node:child_process';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { constants } from 'node:fs';
import { chmod, link, lstat, mkdir, open, readdir, readFile, rename, unlink } from 'node:fs/promises';
import { createConnection, createServer, type Server, type Socket } from 'node:net';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { HranessError, isErrorCode, validProductCode, validProductName, type ErrorCode } from './registry.js';

export const CONTROL_PROTOCOL = 'hraness.control/1';
export const WIRE_VERSION = 1;
export const DEFAULT_MAX_BYTES = 1 << 20;
export const DEFAULT_MAX_CLIENTS = 32;
export const DEFAULT_IDLE_MS = 30_000;

export interface OwnerPaths { stateHome: string; dir: string; agentSock: string; adminSock: string; cap: string; ownerJson: string; lock: string; claim: string }
export interface OwnerFile { schema: 1; pid: number; bootId: string; processStartId: string; generation: string }
export interface OwnerInfo { product: string; pid: number; generation: string; protocols: string[] }
export interface OwnerStatus { running: boolean; stale: boolean; owner?: OwnerInfo }
export interface Peer { socket: 'agent' | 'admin' }
type Handler = (request: unknown, peer: Peer) => Promise<unknown>;

export function ownerPathsIn(stateHome: string): OwnerPaths {
  const dir = join(stateHome, 'control');
  return {
    stateHome, dir,
    agentSock: join(dir, 'agent.sock'), adminSock: join(dir, 'admin.sock'), cap: join(dir, 'admin.cap'),
    ownerJson: join(dir, 'owner.json'), lock: join(dir, 'supervisor.lock'), claim: join(dir, 'owner.lock'),
  };
}
/**
 * `$<PRODUCT>_STATE_HOME` when set, else `~/Library/Application Support/<product>`
 * on macOS and `$XDG_STATE_HOME/<product>` (default `~/.local/state/<product>`).
 */
export function ownerPaths(product: string, env: NodeJS.ProcessEnv = process.env, platform: NodeJS.Platform = process.platform): OwnerPaths {
  if (!validProductName(product)) throw new HranessError('usage', 'Invalid product name.');
  const override = env[`${product.toUpperCase().replaceAll('-', '_')}_STATE_HOME`];
  if (override && isAbsolute(override)) return ownerPathsIn(override);
  const home = env.HOME && isAbsolute(env.HOME) ? env.HOME : homedir();
  const base = platform === 'darwin' ? join(home, 'Library', 'Application Support')
    : env.XDG_STATE_HOME && isAbsolute(env.XDG_STATE_HOME) ? env.XDG_STATE_HOME : join(home, '.local', 'state');
  return ownerPathsIn(join(base, product));
}

const sha256 = (text: string) => createHash('sha256').update(text).digest('hex');
// TZ=UTC: `ps -o lstart=` prints local time, so an owner started under one
// time zone would otherwise look dead to a checker running under another.
function run(program: string, args: string[]): Promise<string | undefined> {
  return new Promise(resolve => execFile(program, args, { env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' }, timeout: 5000 }, (error, stdout) => {
    resolve(error || !stdout.trim() ? undefined : stdout);
  }));
}
/**
 * The part of `sysctl -n kern.boottime` that names the boot: `sec=…,usec=…`.
 * The date text after it is local time and changes with TZ, so it is dropped.
 */
export function bootTimeKey(raw: string): string {
  const match = /sec\s*=\s*(\d+)\s*,\s*usec\s*=\s*(\d+)/.exec(raw);
  return match ? `sec=${match[1]},usec=${match[2]}` : raw.trim();
}
/** A digest of this boot. Same value the Rust kit writes. */
export async function bootId(): Promise<string | undefined> {
  const raw = process.platform === 'linux'
    ? (await readFile('/proc/sys/kernel/random/boot_id', 'utf8').catch(() => undefined))?.trim()
    : await run('sysctl', ['-n', 'kern.boottime']).then(text => text === undefined ? undefined : bootTimeKey(text));
  return raw === undefined ? undefined : sha256(`boot:${raw}`);
}
/** A digest of when `pid` started, so a reused pid does not match. Independent of the caller's TZ. */
export async function processStartId(pid: number): Promise<string | undefined> {
  let raw: string | undefined;
  if (process.platform === 'linux') {
    const stat = await readFile(`/proc/${pid}/stat`, 'utf8').catch(() => undefined);
    raw = stat?.slice(stat.lastIndexOf(')') + 1).trim().split(/\s+/)[19];
  } else raw = await run('ps', ['-o', 'lstart=', '-p', String(pid)]);
  return raw === undefined ? undefined : sha256(`start:${pid}:${raw.trim()}`);
}
async function identityMatches(pid: number, boot: string, start: string): Promise<boolean> {
  return (await bootId()) === boot && (await processStartId(pid)) === start;
}

const errno = (error: unknown, code: string) => (error as NodeJS.ErrnoException)?.code === code;
/** Creates `dir` 0700, tightens a wider one we own, and refuses a symlink or someone else's directory. */
async function privateDir(dir: string): Promise<void> {
  await mkdir(dir, { recursive: true, mode: 0o700 });
  const info = await lstat(dir);
  if (!info.isDirectory() || info.isSymbolicLink()) throw new HranessError('permission-denied', 'The control directory is not a directory.');
  if (typeof process.getuid === 'function' && info.uid !== process.getuid()) throw new HranessError('permission-denied', 'The control directory belongs to someone else.');
  if ((info.mode & 0o777) !== 0o700) await chmod(dir, 0o700);
}
async function writePrivate(path: string, text: string, exclusive = false): Promise<void> {
  if (!exclusive) await unlink(path).catch(error => { if (!errno(error, 'ENOENT')) throw error; });
  const file = await open(path, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW, 0o600);
  try { await file.writeFile(text); await file.sync(); } finally { await file.close(); }
}
/** Reads `owner.json`. */
export async function readOwnerFile(paths: OwnerPaths): Promise<OwnerFile> {
  let value: OwnerFile;
  try { value = JSON.parse(await readFile(paths.ownerJson, 'utf8')); }
  catch { throw new HranessError('owner-unavailable', 'No readable owner.json.'); }
  if (value?.schema !== 1 || !Number.isSafeInteger(value.pid) || typeof value.bootId !== 'string' || typeof value.processStartId !== 'string' || typeof value.generation !== 'string') {
    throw new HranessError('owner-unavailable', 'owner.json is malformed.');
  }
  return value;
}

interface Claim { pid: number; bootId: string; processStartId: string }
/** How many numbered claims below its own a new owner keeps. */
const KEPT_CLAIMS = 16;
const claimPath = (paths: OwnerPaths, n: number) => `${paths.claim}.${n}`;
/** The numbers of every `owner.lock.<n>`, highest first. */
async function claimNumbers(paths: OwnerPaths): Promise<number[]> {
  const prefix = `${paths.claim.slice(paths.dir.length + 1)}.`;
  return (await readdir(paths.dir))
    .filter(name => name.startsWith(prefix) && /^[1-9][0-9]{0,14}$/.test(name.slice(prefix.length)))
    .map(name => Number(name.slice(prefix.length)))
    .sort((a, b) => b - a);
}
/**
 * Creates `owner.lock.<n>` with its whole content in one step: the claim is
 * written to a private temporary file and hard-linked into place, so no one
 * ever reads an empty or partial claim. False when `n` is taken.
 */
async function createClaim(paths: OwnerPaths, n: number, self: Claim): Promise<boolean> {
  const temp = `${paths.claim}.new-${randomBytes(8).toString('hex')}`;
  await writePrivate(temp, JSON.stringify(self), true);
  try { await link(temp, claimPath(paths, n)); return true; }
  catch (error) { if (errno(error, 'EEXIST')) return false; throw error; }
  finally { await unlink(temp).catch(() => {}); }
}
/** Marks this owner's claim released on the way out; the file itself stays. */
async function releaseClaim(paths: OwnerPaths, n: number): Promise<void> {
  const temp = `${paths.claim}.new-${randomBytes(8).toString('hex')}`;
  try { await writePrivate(temp, JSON.stringify({ released: true }), true); await rename(temp, claimPath(paths, n)); }
  catch { await unlink(temp).catch(() => {}); }
}
async function claimIsLive(paths: OwnerPaths, n: number): Promise<boolean> {
  let claim: Claim | undefined;
  try { claim = JSON.parse(await readFile(claimPath(paths, n), 'utf8')); } catch { return false; }
  return !!claim && Number.isSafeInteger(claim.pid) && typeof claim.bootId === 'string' && typeof claim.processStartId === 'string'
    && await identityMatches(claim.pid, claim.bootId, claim.processStartId);
}
/**
 * Claims the owner slot and returns the claim's number. Claims are numbered
 * files, `owner.lock.<n>`, created with a hard link, which fails when the
 * name exists. The highest number is the owner. A starter creates the next
 * number only after it finds the highest claim's process gone, so two
 * starters that both find a crashed owner race for the same name and one
 * loses. A starter that created a number and then sees a higher one backs
 * off. The highest claim is never removed, even after a clean stop (it is
 * marked released in place), so the numbers only grow; a new owner removes claims more than ${KEPT_CLAIMS} below its own.
 * A live owner (it answers, or its claim names a running process) gets
 * `control-already-running`.
 */
async function claimOwner(paths: OwnerPaths, self: Claim): Promise<number> {
  const running = new HranessError('control-already-running', 'Another owner already holds the control lock.');
  for (let attempt = 0; attempt < 8; attempt++) {
    const top = (await claimNumbers(paths))[0] ?? 0;
    if (top) {
      if (await hello(paths).then(() => true, () => false)) throw running;
      if (await claimIsLive(paths, top)) throw running;
    }
    const mine = top + 1;
    if (!await createClaim(paths, mine, self)) continue;
    const numbers = await claimNumbers(paths);
    if (numbers[0] > mine) {
      await unlink(claimPath(paths, mine)).catch(() => {});
      throw running;
    }
    for (const n of numbers) if (n <= mine - KEPT_CLAIMS) await unlink(claimPath(paths, n)).catch(() => {});
    return mine;
  }
  throw new HranessError('control-already-running', 'Another owner claimed the control lock first.');
}
/**
 * Removes a leftover socket file. A socket that still accepts a connection
 * belongs to a live owner and is refused, as is anything that is not a socket.
 */
async function reclaimSocket(path: string): Promise<void> {
  let info;
  try { info = await lstat(path); } catch (error) { if (errno(error, 'ENOENT')) return; throw error; }
  if (!info.isSocket()) throw new HranessError('permission-denied', `${path} exists and is not a socket.`);
  const live = await new Promise<boolean>(resolve => {
    const socket = createConnection(path);
    const done = (value: boolean) => { socket.destroy(); resolve(value); };
    socket.setTimeout(1000, () => done(true));
    socket.once('connect', () => done(true));
    socket.once('error', () => done(false));
  });
  if (live) throw new HranessError('control-already-running', 'Another owner is still listening on the control socket.');
  await unlink(path);
}

export interface ServeControlOptions {
  product: string;
  paths: OwnerPaths;
  agent: { protocols: Record<string, Handler>; maxClients?: number; maxBytes?: number; idleMs?: number };
  admin: { handler?: Handler };
  signal: AbortSignal;
  /** Called once both sockets listen. */
  onReady?: (info: OwnerInfo) => void;
}

function responseLine(result: { ok: true; value: unknown } | { ok: false; code: ErrorCode; message: string }): string {
  return `${JSON.stringify(result.ok ? { ok: true, result: result.value ?? null } : { ok: false, error: { code: result.code, message: result.message } })}\n`;
}

/**
 * Serves until `signal` aborts or an admin sends `control.stop`. Claims the
 * owner slot, writes owner.json and a new admin.cap, reclaims stale sockets,
 * and binds both sockets 0600 inside the 0700 directory. Cleans up only
 * files it wrote.
 */
export async function serveControl(opts: ServeControlOptions): Promise<void> {
  const { paths, product } = opts;
  const maxBytes = opts.agent.maxBytes ?? DEFAULT_MAX_BYTES;
  const maxClients = opts.agent.maxClients ?? DEFAULT_MAX_CLIENTS;
  const idleMs = opts.agent.idleMs ?? DEFAULT_IDLE_MS;
  if (opts.signal.aborted) return;
  if (process.platform === 'win32') throw new HranessError('unsupported-platform', 'The control owner needs Unix sockets; Windows is not supported yet.');
  await privateDir(paths.dir);
  const [boot, start] = [await bootId(), await processStartId(process.pid)];
  if (!boot || !start) throw new HranessError('internal', "Could not read this process's identity.");
  const claimed = await claimOwner(paths, { pid: process.pid, bootId: boot, processStartId: start });
  const cap = randomBytes(32);
  const owner: OwnerFile = { schema: 1, pid: process.pid, bootId: boot, processStartId: start, generation: randomBytes(16).toString('hex') };
  const info: OwnerInfo = { product, pid: process.pid, generation: owner.generation, protocols: [...Object.keys(opts.agent.protocols), CONTROL_PROTOCOL].sort() };
  const servers: Server[] = [];
  const created: string[] = [];
  const sockets = new Set<Socket>();
  let stop!: () => void;
  const stopped = new Promise<void>(resolve => { stop = resolve; });
  const onAbort = () => stop();
  opts.signal.addEventListener('abort', onAbort, { once: true });
  try {
    // Refuse before writing anything a live owner uses.
    await reclaimSocket(paths.agentSock);
    await reclaimSocket(paths.adminSock);
    await writePrivate(paths.cap, cap.toString('hex'));
    created.push(paths.cap);
    await writePrivate(paths.ownerJson, JSON.stringify(owner));
    const handle = async (line: string, peer: Peer) => {
      let frame: Record<string, unknown>;
      try { frame = JSON.parse(line); } catch { return responseLine({ ok: false, code: 'usage', message: 'The request is not JSON.' }); }
      if (!frame || typeof frame !== 'object' || Array.isArray(frame)) return responseLine({ ok: false, code: 'usage', message: 'The request is not an object.' });
      if (frame.v !== WIRE_VERSION) return responseLine({ ok: false, code: 'usage', message: 'Unsupported wire version.' });
      const request = frame.request ?? null;
      const op = (request as { op?: unknown } | null)?.op;
      const call = async (handler: Handler) => {
        try { return responseLine({ ok: true, value: await handler(request, peer) }); }
        catch (error) {
          if (error instanceof HranessError && isErrorCode(error.code, product)) return responseLine({ ok: false, code: error.code, message: error.message });
          return responseLine({ ok: false, code: 'internal', message: 'The owner failed to answer.' });
        }
      };
      const denied = (message: string) => responseLine({ ok: false, code: 'permission-denied', message });
      if (peer.socket === 'agent') {
        if ('cap' in frame) return denied('The agent socket does not take admin requests.');
        if (typeof frame.protocol !== 'string') return responseLine({ ok: false, code: 'usage', message: 'The request names no protocol.' });
        if (frame.protocol === CONTROL_PROTOCOL) return op === 'control.hello' ? responseLine({ ok: true, value: info }) : denied('The agent socket does not take admin requests.');
        const handler = Object.hasOwn(opts.agent.protocols, frame.protocol) ? opts.agent.protocols[frame.protocol] : undefined;
        return handler ? call(handler) : denied('Unknown protocol.');
      }
      const given = typeof frame.cap === 'string' && /^[0-9a-f]{64}$/.test(frame.cap) ? Buffer.from(frame.cap, 'hex') : Buffer.alloc(32);
      if (!timingSafeEqual(given, cap) || typeof frame.cap !== 'string') return denied('The admin capability does not match.');
      if (op === 'control.hello') return responseLine({ ok: true, value: info });
      if (op === 'control.stop') { setImmediate(stop); return responseLine({ ok: true, value: { stopping: true } }); }
      return opts.admin.handler ? call(opts.admin.handler) : responseLine({ ok: false, code: 'not-found', message: 'Unknown admin operation.' });
    };
    const listen = async (path: string, peer: Peer) => {
      await reclaimSocket(path);
      const server = createServer(socket => {
        if (sockets.size >= maxClients) { socket.end(responseLine({ ok: false, code: 'owner-unavailable', message: 'The owner is busy. Try again.' })); return; }
        sockets.add(socket);
        socket.setTimeout(idleMs, () => socket.destroy());
        socket.on('close', () => sockets.delete(socket));
        socket.on('error', () => {});
        let buffer = '';
        let busy = Promise.resolve();
        socket.on('data', chunk => {
          buffer += chunk.toString('utf8');
          let newline: number;
          while ((newline = buffer.indexOf('\n')) >= 0) {
            const line = buffer.slice(0, newline);
            buffer = buffer.slice(newline + 1);
            if (Buffer.byteLength(line) > maxBytes) { socket.end(responseLine({ ok: false, code: 'usage', message: 'The request is too large or unterminated.' })); return; }
            busy = busy.then(async () => { if (!socket.destroyed) socket.write(await handle(line, peer)); });
          }
          if (Buffer.byteLength(buffer) > maxBytes) socket.end(responseLine({ ok: false, code: 'usage', message: 'The request is too large or unterminated.' }));
        });
      });
      servers.push(server);
      await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(path, () => { server.off('error', reject); resolve(); }); });
      created.push(path);
      await chmod(path, 0o600);
    };
    await listen(paths.agentSock, { socket: 'agent' });
    await listen(paths.adminSock, { socket: 'admin' });
    opts.onReady?.(info);
    await stopped;
  } finally {
    opts.signal.removeEventListener('abort', onAbort);
    for (const server of servers) server.close();
    for (const socket of sockets) socket.destroy();
    for (const path of created) await unlink(path).catch(() => {});
    const current = await readOwnerFile(paths).catch(() => undefined);
    if (current?.generation === owner.generation) await unlink(paths.ownerJson).catch(() => {});
    await releaseClaim(paths, claimed);
  }
}

function exchange(sock: string, frame: unknown, timeoutMs = DEFAULT_IDLE_MS): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const unavailable = (detail?: string) => new HranessError('owner-unavailable', 'The owner is not answering.', detail);
    const socket = createConnection(sock);
    let reply = '';
    let settled = false;
    const finish = (fn: () => void) => { if (!settled) { settled = true; socket.destroy(); fn(); } };
    socket.setTimeout(timeoutMs, () => finish(() => reject(unavailable('timeout'))));
    socket.on('error', error => finish(() => reject(unavailable((error as NodeJS.ErrnoException).code))));
    socket.on('connect', () => socket.write(`${JSON.stringify(frame)}\n`));
    const settle = () => {
      let value: { ok?: unknown; result?: unknown; error?: { code?: unknown; message?: unknown } };
      try { value = JSON.parse(reply); } catch { return finish(() => reject(new HranessError('owner-unavailable', 'The owner answered something unreadable.'))); }
      if (value.ok === true) return finish(() => resolve(value.result ?? null));
      const raw = value.error?.code;
      const code: ErrorCode = typeof raw === 'string' && (isErrorCode(raw) || validProductCode(raw)) ? raw as ErrorCode : 'internal';
      const message = typeof value.error?.message === 'string' ? value.error.message : 'The owner refused the request.';
      finish(() => reject(new HranessError(code, message)));
    };
    socket.on('data', chunk => {
      reply += chunk.toString('utf8');
      if (reply.includes('\n')) { reply = reply.slice(0, reply.indexOf('\n')); settle(); }
      else if (reply.length > DEFAULT_MAX_BYTES) finish(() => reject(unavailable('reply too large')));
    });
    socket.on('end', () => { if (!settled) settle(); });
  });
}

/** Sends one admin request with the capability from admin.cap. */
export async function adminRequest<T>(paths: OwnerPaths, request: unknown): Promise<T> {
  let cap: string;
  try { cap = (await readFile(paths.cap, 'utf8')).trim(); }
  catch { throw new HranessError('owner-unavailable', 'No admin capability.'); }
  return await exchange(paths.adminSock, { v: WIRE_VERSION, cap, request }) as T;
}
/** Sends one agent request under a named protocol. */
export async function agentRequest<T>(paths: OwnerPaths, protocol: string, request: unknown): Promise<T> {
  return await exchange(paths.agentSock, { v: WIRE_VERSION, protocol, request }) as T;
}
function hello(paths: OwnerPaths): Promise<OwnerInfo> {
  return exchange(paths.agentSock, { v: WIRE_VERSION, protocol: CONTROL_PROTOCOL, request: { op: 'control.hello' } }, 2000) as Promise<OwnerInfo>;
}

/** Whether an owner answers. Never signals any process. */
export async function controlStatus(paths: OwnerPaths): Promise<OwnerStatus> {
  try { return { running: true, stale: false, owner: await hello(paths) }; } catch { /* not answering */ }
  const file = await readOwnerFile(paths).catch(() => undefined);
  return { running: false, stale: file ? !await identityMatches(file.pid, file.bootId, file.processStartId) : false };
}

/**
 * The running owner, or spawns one and waits until it answers. A child
 * that exits or does not answer within `timeoutMs` is killed; nothing else
 * is ever signalled. If two callers race, the loser's child exits with
 * `control-already-running` and this still returns the winner.
 */
export async function ensureOwner(paths: OwnerPaths, spawn: () => ChildProcess, timeoutMs = 10_000): Promise<OwnerInfo> {
  try { return await hello(paths); } catch { /* start one */ }
  const child = spawn();
  let exited = child.exitCode !== null || child.signalCode !== null;
  child.once('exit', () => { exited = true; });
  child.once('error', () => { exited = true; });
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try { return await hello(paths); } catch { /* not yet */ }
    if (Date.now() >= deadline) break;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  if (!exited) {
    child.kill('SIGKILL');
    await new Promise(resolve => { if (child.exitCode !== null || child.signalCode !== null) resolve(undefined); else child.once('exit', resolve); });
  }
  throw new HranessError('owner-unavailable', 'The owner did not start in time.', exited ? 'The owner exited.' : 'The owner never answered; it was stopped.');
}
