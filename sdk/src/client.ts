import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { ensureBinary, parseReleaseManifest, type ReleaseManifest } from './install.js';
import { validateSnapshot, parseRunnerEvent, MAX_FRAME_BYTES, type CompanionIdentity, type MenuItem, type Snapshot } from './protocol.js';

export interface CompanionOptions extends CompanionIdentity {
  stateDir: string;
  /** Maintainer/test override; production products use the packaged pinned manifest. */
  binary?: string;
  /** Arguments for an explicit maintainer binary, placed before runner arguments. */
  binaryArgs?: readonly string[];
  manifest?: ReleaseManifest;
  cacheDir?: string;
  snapshot: (signal: AbortSignal) => readonly MenuItem[] | Promise<readonly MenuItem[]>;
  onAction: (id: string, signal: AbortSignal) => void | Promise<void>;
  onDiagnostic?: (code: string) => void;
  refreshMs?: number;
  timeoutMs?: number;
}
export interface CompanionSession {
  ready: Promise<'running' | 'already-running'>;
  closed: Promise<number>;
  refresh(): Promise<void>;
  quit(): Promise<void>;
}

export async function packagedManifest(): Promise<ReleaseManifest> {
  try { return parseReleaseManifest(await readFile(new URL('../../release-manifest.json', import.meta.url))); }
  catch { throw new Error('release-manifest-unavailable: use a published package or an explicit maintainer binary'); }
}

export async function runCompanion(options: CompanionOptions): Promise<CompanionSession> {
  const timeout = options.timeoutMs ?? 10_000;
  const refreshMs = options.refreshMs ?? 10_000;
  if (!Number.isFinite(timeout) || timeout < 100 || timeout > 120_000 || !Number.isFinite(refreshMs) || refreshMs < 100 || refreshMs > 3_600_000) throw new Error('invalid-companion-deadline');
  if (options.binaryArgs && !options.binary) throw new Error('binary-arguments-require-explicit-binary');
  const binary = options.binary ?? (await ensureBinary({ manifest: options.manifest ?? await packagedManifest(), cacheDir: options.cacheDir })).path;
  let revision = 0, snapshotBusy = false, refreshBusy = false, dispatchBusy = false, stopping = false, running = false;
  let actions: ReadonlyMap<string, boolean> = new Map();
  const controllers = new Set<AbortController>();
  const diagnostic = (code: string) => { try { options.onDiagnostic?.(code); } catch { /* diagnostic callbacks cannot break process custody */ } };
  function callback<T>(invoke: (signal: AbortSignal) => T | Promise<T>, settled: (aborted: boolean) => void): Promise<T> {
    const controller = new AbortController(); controllers.add(controller);
    const timer = setTimeout(() => controller.abort(), timeout);
    return new Promise<T>((resolve, reject) => {
      const aborted = () => reject(new Error('callback-aborted'));
      controller.signal.addEventListener('abort', aborted, { once: true });
      // The deadline bounds our wait even if the callback ignores its signal.
      // Keep its busy lock until the underlying work settles, so a timeout
      // never starts a second hanging read or overlapping product mutation.
      Promise.resolve().then(() => {
        if (controller.signal.aborted) throw new Error('callback-aborted');
        return invoke(controller.signal);
      }).then(resolve, reject).finally(() => {
        clearTimeout(timer); controller.signal.removeEventListener('abort', aborted);
        controllers.delete(controller); settled(controller.signal.aborted);
      });
    });
  }
  async function model(): Promise<Snapshot> {
    if (snapshotBusy) throw new Error('snapshot-busy');
    snapshotBusy = true;
    const items = await callback(options.snapshot, () => { snapshotBusy = false; });
    const value: Snapshot = { version: 1, type: 'snapshot', appId: options.appId, name: options.name, title: options.title, ...(options.tooltip ? { tooltip: options.tooltip } : {}), revision: revision + 1, items };
    validateSnapshot(value);
    // Retain the exact values sent, not mutable arrays owned by the callback.
    return JSON.parse(JSON.stringify(value)) as Snapshot;
  }
  // Obtain and validate product state before creating any UI or helper process.
  const initial = await model();
  const child = spawn(binary, [...(options.binaryArgs ?? []), '--state-dir', options.stateDir], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  let readyResolve!: (state: 'running' | 'already-running') => void;
  let readyReject!: (error: Error) => void;
  const ready = new Promise<'running' | 'already-running'>((resolve, reject) => { readyResolve = resolve; readyReject = reject; });
  // A failure remains observable to callers without a transient unhandled rejection.
  void ready.catch(() => {});
  let closeResolve!: (status: number) => void;
  const closed = new Promise<number>(resolve => { closeResolve = resolve; });
  const startup = setTimeout(() => fail('startup-timeout'), timeout);
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let killTimer: ReturnType<typeof setTimeout> | undefined;
  let forceKillTimer: ReturnType<typeof setTimeout> | undefined;
  function fail(code: string): void {
    if (stopping) return;
    diagnostic(code); readyReject(new Error(code)); void quit();
  }
  async function send(value: Snapshot | { version: 1; type: 'quit' }): Promise<void> {
    if (child.stdin.destroyed) throw new Error('runner-closed');
    if (value.type === 'snapshot') actions = new Map();
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('runner-write-timeout')), timeout);
      child.stdin.write(JSON.stringify(value) + '\n', error => {
        clearTimeout(timer); error ? reject(error) : resolve();
      });
    });
    if (value.type === 'snapshot') { revision = value.revision; actions = validateSnapshot(value); }
  }
  async function refresh(): Promise<void> {
    if (stopping || snapshotBusy || refreshBusy) return;
    refreshBusy = true;
    try {
      let value: Snapshot;
      try { value = await model(); }
      catch { actions = new Map(); if (!stopping) diagnostic('snapshot-unavailable'); return; }
      if (!stopping) {
        try { await send(value); }
        catch { fail('runner-pipe-closed'); }
      }
    }
    finally { refreshBusy = false; }
  }
  async function dispatch(id: string, eventRevision: number): Promise<void> {
    if (stopping || !running || dispatchBusy || eventRevision !== revision || actions.get(id) !== true) return;
    dispatchBusy = true;
    actions = new Map();
    try {
      await callback(signal => options.onAction(id, signal), aborted => {
        dispatchBusy = false;
        if (aborted) {
          actions = new Map();
          if (!stopping) void refresh();
        }
      });
    }
    catch { if (!stopping) diagnostic('action-indeterminate'); }
    finally {
      if (!stopping) await refresh(); // observe confirmed product state; never retry the action
    }
  }
  async function quit(): Promise<void> {
    if (!stopping) {
      stopping = true; clearTimeout(startup); clearInterval(refreshTimer);
      readyReject(new Error('runner-stopped-before-ready'));
      for (const controller of controllers) controller.abort();
      // Queue a graceful quit, but do not wait for a blocked pipe before
      // scheduling termination. The child may never read from stdin again.
      if (!child.stdin.destroyed) child.stdin.write('{"version":1,"type":"quit"}\n', () => {});
      child.stdin.end();
      if (child.exitCode === null && child.signalCode === null) {
        const grace = Math.min(timeout, 2000);
        killTimer = setTimeout(() => { child.kill('SIGTERM'); }, grace);
        forceKillTimer = setTimeout(() => { child.kill('SIGKILL'); }, grace * 2);
      }
    }
    await closed;
  }
  let pending: Buffer = Buffer.alloc(0);
  child.stdout.on('data', (chunk: Buffer) => {
    if (stopping) return;
    let offset = 0;
    while (offset < chunk.length) {
      const end = chunk.indexOf(10, offset);
      const part = chunk.subarray(offset, end < 0 ? chunk.length : end);
      if (pending.length + part.length > MAX_FRAME_BYTES) { fail('oversize-runner-frame'); return; }
      pending = pending.length ? Buffer.concat([pending, part]) : part;
      if (end < 0) break;
      const line = pending; pending = Buffer.alloc(0); offset = end + 1;
      try {
        const event = parseRunnerEvent(line.toString('utf8'));
        switch (event.type) {
          case 'ready':
            running = true; clearTimeout(startup); readyResolve('running');
            if (!refreshTimer) refreshTimer = setInterval(() => { void refresh(); }, refreshMs);
            break;
          case 'already-running': clearTimeout(startup); readyResolve('already-running'); void quit(); break;
          case 'action': void dispatch(event.id, event.revision); break;
          case 'error': fail(event.code); break;
          case 'validated': fail('unexpected-headless-runner'); break;
          case 'stopped': void quit(); break;
        }
      } catch { fail('invalid-runner-frame'); return; }
      if (stopping) return;
    }
  });
  child.stdout.once('end', () => { if (!stopping) fail(pending.length ? 'invalid-runner-frame' : 'runner-output-closed'); });
  // Drain diagnostics but never expose native paths, environment or daemon data.
  child.stderr.on('data', () => {});
  child.stdin.on('error', () => { if (!stopping) fail('runner-pipe-closed'); });
  child.once('error', () => fail('runner-launch-failed'));
  child.once('close', code => {
    clearTimeout(startup); clearInterval(refreshTimer); clearTimeout(killTimer); clearTimeout(forceKillTimer);
    stopping = true; for (const controller of controllers) controller.abort();
    readyReject(new Error('runner-exited-before-ready'));
    closeResolve(code ?? 1);
  });
  try { await send(initial); } catch { fail('runner-pipe-closed'); }
  return { ready, closed, refresh, quit };
}
