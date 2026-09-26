import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { autostartState, planAutostart, removeAutostart, setAutostart, type AutostartPlan } from './autostart.js';
import { ensureBinary, parseReleaseManifest, type ReleaseManifest } from './install.js';
import { removeLoginEnvironment, saveLoginEnvironment } from './login-env.js';
import { actionErrorItem, degradedMenu, OPEN_AT_LOGIN_SUBTITLE, type DegradedMenuOptions } from './menu-kit.js';
import { defaultPermissionIO, openPermissionSettings, type PermissionIO } from './permissions.js';
import { validateSnapshot, parseRunnerEvent, MAX_FRAME_BYTES, type CompanionIdentity, type MenuItem, type Snapshot } from './protocol.js';
import {
  downlevelItems, downlevelSnapshot, FOUNDATION_ACTION_IDS, parseRunnerProtocols, validateSnapshotV2,
  type ActionItemV2, type MenuItemV2, type PermissionKind, type SnapshotV2, type StatusMark,
} from './protocol-v2.js';

/** What `snapshot` may return when the mark or tooltip changes with product state. */
export interface MenuModel<Item = MenuItem | MenuItemV2> {
  items: readonly Item[];
  /** v2 products only: the menu-bar mark for this snapshot (a dot or count when something needs the person). */
  mark?: StatusMark;
  tooltip?: string;
}
type SnapshotResult = readonly MenuItem[] | readonly MenuItemV2[] | MenuModel;

/**
 * Throw this from `onAction` to show `message` as a ⚠︎ row at the top of the
 * menu until a later refresh. Other errors show a generic row that never
 * includes their text.
 */
export class MenuActionError extends Error {
  constructor(message: string, readonly detail?: string) { super(message); this.name = 'MenuActionError'; }
}

/** How long a failed action's ⚠︎ row stays, at least. It goes at the first refresh after this. */
export const ACTION_ERROR_MS = 30_000;

export interface CompanionOptions extends Omit<CompanionIdentity, 'title'> {
  /** v1 menu-bar title: one or two letters or one emoji. Required unless `mark` is set. */
  title?: string;
  /**
   * Opts into menu kit v2: `snapshot` returns v2 items and the menu bar shows
   * this template mark. The SDK sends v2 only to a runner that reports
   * `protocol/…2` and down-levels otherwise.
   */
  mark?: StatusMark;
  stateDir: string;
  /** Maintainer/test override; production products use the packaged pinned manifest. */
  binary?: string;
  /** Arguments for an explicit maintainer binary, placed before runner arguments. */
  binaryArgs?: readonly string[];
  manifest?: ReleaseManifest;
  cacheDir?: string;
  snapshot: (signal: AbortSignal) => SnapshotResult | Promise<SnapshotResult>;
  onAction: (id: string, signal: AbortSignal) => void | Promise<void>;
  onDiagnostic?: (code: string) => void;
  /**
   * Push refresh: called once the menu is running. Call `refresh()` whenever
   * product state changes instead of waiting for the next `refreshMs` tick.
   * `signal` aborts when the session ends; remove listeners then.
   */
  subscribe?: (refresh: () => void, signal: AbortSignal) => void;
  /** What the menu offers while product state can't be read (default: a status row and Quit). */
  degraded?: (code: string) => DegradedMenuOptions;
  /**
   * The command a login item runs (the product's `--foreground` command).
   * With it, the SDK shows and handles the `foundation.login` row; without
   * it, that row is disabled. `handleCompanionCommand` sets it for you.
   */
  loginItem?: {
    executable: string;
    args: readonly string[];
    /** Test hooks: the home directory and environment the login entry is planned for. */
    home?: string;
    env?: NodeJS.ProcessEnv;
  };
  /**
   * Environment variables a login-started menu bar needs, such as an API
   * token. `menubar install` (and turning on "Open at login") saves their
   * current values to a private file in `stateDir`; the login-started
   * process fills in any that are unset. See `saveLoginEnvironment`.
   */
  loginEnv?: readonly string[];
  /** Test hook for `foundation.settings.<kind>` actions. */
  permissionIO?: PermissionIO;
  refreshMs?: number;
  timeoutMs?: number;
}
export interface CompanionSession {
  ready: Promise<'running' | 'already-running'>;
  closed: Promise<number>;
  /** The protocol version this session speaks (2 only for a v2 product on a v2 runner). */
  readonly protocol: 1 | 2;
  refresh(): Promise<void>;
  quit(): Promise<void>;
}

export async function packagedManifest(): Promise<ReleaseManifest> {
  try { return parseReleaseManifest(await readFile(new URL('../../release-manifest.json', import.meta.url))); }
  catch { throw new Error('release-manifest-unavailable: use a published package or an explicit maintainer binary'); }
}

/** Reads `--version` from the runner; `[1]` when it can't be read in time. */
export async function runnerProtocols(binary: string, binaryArgs: readonly string[] = [], timeoutMs = 5000): Promise<number[]> {
  return await new Promise<number[]>(resolve => {
    let output = '';
    let child: ReturnType<typeof spawn>;
    try { child = spawn(binary, [...binaryArgs, '--version'], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true }); }
    catch { resolve([1]); return; }
    const timer = setTimeout(() => { child.kill('SIGKILL'); resolve([1]); }, timeoutMs);
    child.stdout!.setEncoding('utf8');
    child.stdout!.on('data', (chunk: string) => { if (output.length < 4096) output += chunk; });
    child.once('error', () => { clearTimeout(timer); resolve([1]); });
    child.once('close', () => { clearTimeout(timer); resolve(parseRunnerProtocols(output)); });
  });
}

function findLabel(items: readonly (MenuItem | MenuItemV2)[], id: string): string | undefined {
  for (const item of items) {
    if (item.kind === 'action') {
      if (item.id === id) return item.label;
      const alternate = (item as ActionItemV2).alternate;
      if (alternate?.id === id) return alternate.label;
    }
    if (item.kind === 'submenu') { const found = findLabel(item.items, id); if (found) return found; }
  }
  return undefined;
}

function actionFailure(error: unknown, label: string | undefined, aborted: boolean): { message: string; detail?: string } {
  if (error instanceof MenuActionError) return { message: error.message, ...(error.detail ? { detail: error.detail } : {}) };
  const name = label ? `"${label}"` : 'that action';
  return aborted
    ? { message: `Couldn't confirm ${name}`, detail: 'Check whether it finished before trying again' }
    : { message: `Couldn't finish ${name}`, detail: 'Try again in a moment' };
}

type Frame = Snapshot | SnapshotV2;
interface Built { frame: Frame; actions: ReadonlyMap<string, boolean> }

export async function runCompanion(options: CompanionOptions): Promise<CompanionSession> {
  const timeout = options.timeoutMs ?? 10_000;
  const refreshMs = options.refreshMs ?? 10_000;
  if (!Number.isFinite(timeout) || timeout < 100 || timeout > 120_000 || !Number.isFinite(refreshMs) || refreshMs < 100 || refreshMs > 3_600_000) throw new Error('invalid-companion-deadline');
  if (options.binaryArgs && !options.binary) throw new Error('binary-arguments-require-explicit-binary');
  if (options.mark === undefined && typeof options.title !== 'string') throw new Error('invalid-title');
  const binary = options.binary ?? (await ensureBinary({ manifest: options.manifest ?? await packagedManifest(), cacheDir: options.cacheDir })).path;
  const v2Product = options.mark !== undefined;
  let revision = 0, snapshotBusy = false, refreshBusy = false, refreshAgain = false, dispatchBusy = false, stopping = false, running = false;
  // JSON of the degraded menu on screen, so a repeat failure keeps its actions live without resending.
  let degradedShown: string | undefined;
  let actions: ReadonlyMap<string, boolean> = new Map();
  let lastItems: readonly (MenuItem | MenuItemV2)[] = [];
  let actionError: { message: string; detail?: string; until: number } | undefined;
  let actionErrorTimer: ReturnType<typeof setTimeout> | undefined;
  const controllers = new Set<AbortController>();
  const lifetime = new AbortController();
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

  // Protocol negotiation: only a v2 product asks the runner what it speaks.
  const protocol: 1 | 2 = v2Product && (await runnerProtocols(binary, options.binaryArgs ?? [], timeout)).includes(2) ? 2 : 1;

  // The foundation "Open at login" row.
  const loginPlan: AutostartPlan | undefined = (() => {
    if (!options.loginItem) return undefined;
    const { executable, args, home, env } = options.loginItem;
    try { return planAutostart({ id: options.appId, label: options.name, executable, args: [...args], ...(home ? { home } : {}), ...(env ? { env } : {}) }); }
    catch { return undefined; }
  })();
  async function loginState(): Promise<'on' | 'off' | undefined> {
    if (!loginPlan) return undefined;
    try { const state = await autostartState(loginPlan); return state === 'conflict' ? undefined : state === 'off' ? 'off' : 'on'; }
    catch { return undefined; }
  }
  function withLogin<T extends MenuItem | MenuItemV2>(items: readonly T[], state: 'on' | 'off' | undefined): T[] {
    return items.map(item => {
      if (item.kind === 'submenu') return { ...item, items: withLogin(item.items as readonly T[], state) } as T;
      if (item.kind !== 'action' || item.id !== 'foundation.login') return item;
      if (state === undefined) return { ...item, enabled: false } as T;
      if ('checked' in item || !v2Product) return { ...item, checked: state === 'on' } as T;
      const { subtitle: _subtitle, ...rest } = item as ActionItemV2;
      return { ...rest, state, ...(state === 'off' ? { subtitle: OPEN_AT_LOGIN_SUBTITLE } : {}) } as T;
    });
  }
  function withActionError(items: readonly MenuItemV2[]): MenuItemV2[] {
    if (!actionError || Date.now() >= actionError.until) { actionError = undefined; return [...items]; }
    let index = 0;
    while (index < items.length && (items[index]!.kind === 'header' || items[index]!.kind === 'status')) index++;
    return [...items.slice(0, index), actionErrorItem(actionError.message, actionError.detail), ...items.slice(index)];
  }
  function withActionErrorV1(items: readonly MenuItem[]): MenuItem[] {
    if (!actionError || Date.now() >= actionError.until) { actionError = undefined; return [...items]; }
    return [...downlevelItems([actionErrorItem(actionError.message, actionError.detail)]), ...items];
  }

  /** Builds and validates the frame for this session's protocol. */
  function build(items: readonly (MenuItem | MenuItemV2)[], model: Omit<MenuModel, 'items'>, login: 'on' | 'off' | undefined, next: number): Built {
    const tooltip = model.tooltip ?? options.tooltip;
    if (v2Product) {
      const v2: SnapshotV2 = {
        version: 2, type: 'snapshot', appId: options.appId, name: options.name, revision: next,
        mark: model.mark ?? options.mark!, ...(tooltip ? { tooltip } : {}), ...(options.icon ? { icon: options.icon } : {}),
        items: withActionError(withLogin(items as readonly MenuItemV2[], login)),
      };
      const v2Actions = validateSnapshotV2(v2);
      if (protocol === 2) return { frame: v2, actions: v2Actions };
      const v1 = downlevelSnapshot(v2);
      return { frame: v1, actions: validateSnapshot(v1, FOUNDATION_ACTION_IDS) };
    }
    const v1: Snapshot = {
      version: 1, type: 'snapshot', appId: options.appId, name: options.name, title: options.title!,
      ...(tooltip ? { tooltip } : {}), ...(options.icon ? { icon: options.icon } : {}), revision: next,
      items: withActionErrorV1(withLogin(items as readonly MenuItem[], login)),
    };
    return { frame: v1, actions: validateSnapshot(v1, FOUNDATION_ACTION_IDS) };
  }
  async function model(): Promise<Built> {
    if (snapshotBusy) throw new Error('snapshot-busy');
    snapshotBusy = true;
    const result = await callback(options.snapshot, () => { snapshotBusy = false; });
    const value: MenuModel = Array.isArray(result) ? { items: result as readonly MenuItem[] } : result as MenuModel;
    if (!v2Product && value.mark !== undefined) throw new Error('invalid-mark');
    // Retain the exact values sent, not mutable arrays owned by the callback.
    const copy = JSON.parse(JSON.stringify(value)) as MenuModel;
    const built = build(copy.items, copy, await loginState(), revision + 1);
    lastItems = copy.items;
    return built;
  }
  async function degradedFrame(code: string): Promise<Built> {
    let extra: DegradedMenuOptions = {};
    try { extra = options.degraded?.(code) ?? {}; } catch { /* the degraded menu cannot depend on a failing product */ }
    const login = await loginState();
    const items = degradedMenu(options.name, { ...extra, ...(login && !extra.openAtLogin ? { openAtLogin: login } : {}) });
    lastItems = items;
    if (v2Product) return build(items, {}, login, revision + 1);
    const v1: Snapshot = {
      version: 1, type: 'snapshot', appId: options.appId, name: options.name, title: options.title!,
      ...(options.tooltip ? { tooltip: options.tooltip } : {}), ...(options.icon ? { icon: options.icon } : {}), revision: revision + 1,
      items: downlevelItems(items),
    };
    return { frame: v1, actions: validateSnapshot(v1, FOUNDATION_ACTION_IDS) };
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
  async function send(built: Built | { frame: { version: 1 | 2; type: 'quit' } }): Promise<void> {
    if (child.stdin.destroyed) throw new Error('runner-closed');
    const snapshot = built.frame.type === 'snapshot';
    if (snapshot) actions = new Map();
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('runner-write-timeout')), timeout);
      child.stdin.write(JSON.stringify(built.frame) + '\n', error => {
        clearTimeout(timer); error ? reject(error) : resolve();
      });
    });
    if (snapshot) { revision = (built.frame as Frame).revision; actions = (built as Built).actions; }
  }
  async function refreshOnce(): Promise<void> {
    let built: Built;
    try { built = await model(); degradedShown = undefined; }
    catch (error) {
      if (stopping) return;
      const message = error instanceof Error ? error.message : '';
      // A read that is still running keeps its lock and will send its own menu.
      if (message === 'snapshot-busy') return;
      actions = new Map();
      const code = message.startsWith('menu-too-large') ? 'menu-too-large' : 'snapshot-unavailable';
      diagnostic(code);
      // Show that state is stale instead of leaving the last menu up. The
      // degraded menu reads nothing from the product.
      try { built = await degradedFrame(code); }
      catch { return; }
      const key = JSON.stringify(built.frame.items);
      if (key === degradedShown) { actions = built.actions; return; }
      degradedShown = key;
    }
    if (!stopping) {
      try { await send(built); }
      catch { degradedShown = undefined; fail('runner-pipe-closed'); }
    }
  }
  /** Coalesces pushes: a refresh requested during another one runs once more after it. */
  async function refresh(): Promise<void> {
    if (stopping) return;
    if (refreshBusy || snapshotBusy) { refreshAgain = true; return; }
    refreshBusy = true;
    try {
      do { refreshAgain = false; await refreshOnce(); }
      while (refreshAgain && !stopping && !snapshotBusy);
    }
    finally { refreshBusy = false; }
  }
  function showActionError(failure: { message: string; detail?: string }): void {
    actionError = { ...failure, until: Date.now() + ACTION_ERROR_MS };
    clearTimeout(actionErrorTimer);
    actionErrorTimer = setTimeout(() => { void refresh(); }, ACTION_ERROR_MS + 50);
    actionErrorTimer.unref?.();
  }
  async function foundationAction(id: string): Promise<void> {
    if (id === 'foundation.login') {
      if (!loginPlan) throw new MenuActionError("Couldn't change Open at login");
      const on = (await loginState()) === 'on';
      try {
        if (on) { await removeAutostart(loginPlan); await removeLoginEnvironment(options.stateDir); }
        else { await setAutostart(loginPlan); if (options.loginEnv?.length) await saveLoginEnvironment(options.stateDir, options.loginEnv); }
      }
      catch { throw new MenuActionError(on ? "Couldn't turn off Open at login" : "Couldn't turn on Open at login"); }
      return;
    }
    const kind = id.slice('foundation.settings.'.length) as PermissionKind;
    if (!await openPermissionSettings(kind, options.permissionIO ?? defaultPermissionIO())) throw new MenuActionError("Couldn't open System Settings");
  }
  async function dispatch(id: string, eventRevision: number): Promise<void> {
    if (stopping || !running || dispatchBusy || eventRevision !== revision || actions.get(id) !== true) return;
    dispatchBusy = true;
    actions = new Map();
    const label = findLabel(lastItems, id);
    const handler = id.startsWith('foundation.') ? () => foundationAction(id) : (signal: AbortSignal) => options.onAction(id, signal);
    let aborted = false;
    try {
      await callback(handler, wasAborted => {
        dispatchBusy = false;
        if (wasAborted) {
          actions = new Map();
          if (!stopping) void refresh();
        }
      });
    }
    catch (error) {
      aborted = error instanceof Error && error.message === 'callback-aborted';
      if (!stopping) { diagnostic('action-indeterminate'); showActionError(actionFailure(error, label, aborted)); }
    }
    finally {
      if (!stopping) await refresh(); // observe confirmed product state; never retry the action
    }
  }
  async function quit(): Promise<void> {
    if (!stopping) {
      stopping = true; clearTimeout(startup); clearInterval(refreshTimer); clearTimeout(actionErrorTimer);
      lifetime.abort();
      readyReject(new Error('runner-stopped-before-ready'));
      for (const controller of controllers) controller.abort();
      // Queue a graceful quit, but do not wait for a blocked pipe before
      // scheduling termination. The child may never read from stdin again.
      if (!child.stdin.destroyed) child.stdin.write(`{"version":${protocol},"type":"quit"}\n`, () => {});
      child.stdin.end();
      if (child.exitCode === null && child.signalCode === null) {
        const grace = Math.min(timeout, 2000);
        killTimer = setTimeout(() => { child.kill('SIGTERM'); }, grace);
        forceKillTimer = setTimeout(() => { child.kill('SIGKILL'); }, grace * 2);
      }
    }
    await closed;
  }
  function subscribe(): void {
    if (!options.subscribe) return;
    try { options.subscribe(() => { void refresh(); }, lifetime.signal); }
    catch { diagnostic('subscribe-failed'); }
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
        const event = parseRunnerEvent(line.toString('utf8'), protocol);
        switch (event.type) {
          case 'ready':
            running = true; clearTimeout(startup); readyResolve('running');
            if (!refreshTimer) { refreshTimer = setInterval(() => { void refresh(); }, refreshMs); subscribe(); }
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
    clearTimeout(startup); clearInterval(refreshTimer); clearTimeout(killTimer); clearTimeout(forceKillTimer); clearTimeout(actionErrorTimer);
    stopping = true; lifetime.abort(); for (const controller of controllers) controller.abort();
    readyReject(new Error('runner-exited-before-ready'));
    closeResolve(code ?? 1);
  });
  try { await send(initial); } catch { fail('runner-pipe-closed'); }
  return { ready, closed, protocol, refresh, quit };
}
