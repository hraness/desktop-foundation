// Permission notices and recovery (docs/permissions.md). The kit never triggers
// a macOS prompt: it says what macOS is about to ask and why, probes state only
// where a probe cannot cause a prompt, explains a denial in plain words, and
// opens the right System Settings pane when the person asks. It has no native
// dependencies, so any CLI can import `@hraness/desktop-foundation/permissions`.
import { spawn } from 'node:child_process';
import { open } from 'node:fs/promises';
import { homedir } from 'node:os';
import { isAbsolute, join, normalize } from 'node:path';
import { detectAudience, type Audience } from './audience.js';
import { cliStyle, cliSymbol, type CliStyle } from './cli-style.js';
import type { MenuItemV2, NoticeRequest, NoticeResult, PermissionKind, SettingsPermissionKind } from './protocol-v2.js';

export type { Audience } from './audience.js';
export { detectAudience } from './audience.js';
export type { PermissionKind, SettingsPermissionKind } from './protocol-v2.js';
export type PromptBehavior = 'asks' | 'settings-only' | 'notifies';
export type PermissionState = 'granted' | 'denied' | 'not-determined' | 'unknown';
export type RecoveryState = 'denied' | 'unknown' | 'missing';
export type PrePromptOutcome = 'continue' | 'skip' | 'unattended-proceed' | 'unattended-stop';
export type Surface = 'cli' | 'menu' | 'dialog';

export interface ProductRef {
  /** Display name from the portfolio registry: "Textbutler". */
  product: string;
  /** CLI name used in next steps: "textbutler". */
  command: string;
  /** The name macOS shows in its dialog. Defaults depend on the kind. */
  requester?: string;
}

export interface PermissionNeed extends ProductRef {
  kind: PermissionKind;
  /** "Messages", "Chrome Safe Storage". */
  target?: string;
  /** Verb phrase after "let X", no final period: "control Messages". */
  ask: string;
  /** One sentence, at most 110 characters, ending with a period. */
  why: string;
  /** Recovery command; defaults to `${command} doctor`. */
  next?: string;
  /** Default: 'proceed' for kinds that only notify, 'stop' otherwise. */
  whenUnattended?: 'proceed' | 'stop';
}

export interface RenderedNotice {
  /** Dialog title; for the CLI, the first line without its symbol. */
  title: string;
  /** Following CLI lines without indentation, or the dialog message parts. */
  lines: string[];
  /** Pre-prompt: the confirm line. Recovery: the "press o" hint for the `→` line. Only shown when stdin and stderr are terminals. */
  confirm?: string;
  /** Recovery only: the one next step, printed after `→`. */
  next?: string;
}

/** Everything the kit touches, injectable for tests. */
export interface PermissionIO {
  env: NodeJS.ProcessEnv;
  stdinIsTTY: boolean;
  stderrIsTTY: boolean;
  /** Writes to stderr. */
  write(text: string): void;
  readKey(timeoutSeconds: number): Promise<'enter' | 's' | 'o' | 'timeout'>;
  /** Opens an allowlisted `x-apple.systempreferences:` URL. */
  openUrl(url: string): Promise<boolean>;
  /** Shows the runner `--notice` dialog. */
  notice?(request: NoticeRequest): Promise<NoticeResult>;
  fileAccess?(path: string): Promise<'ok' | 'denied' | 'missing'>;
  run?(argv: readonly string[]): Promise<{ status: number }>;
}

interface KindInfo { behavior: PromptBehavior; pane: string | null; path: string | null; url: string | null }
const PRIVACY = 'System Settings › Privacy & Security';
const SECURITY_URL = 'x-apple.systempreferences:com.apple.preference.security?';
const KINDS: Readonly<Record<PermissionKind, KindInfo>> = {
  'full-disk-access': { behavior: 'settings-only', pane: 'Full Disk Access', path: `${PRIVACY} › Full Disk Access`, url: `${SECURITY_URL}Privacy_AllFiles` },
  automation: { behavior: 'asks', pane: 'Automation', path: `${PRIVACY} › Automation`, url: `${SECURITY_URL}Privacy_Automation` },
  contacts: { behavior: 'asks', pane: 'Contacts', path: `${PRIVACY} › Contacts`, url: `${SECURITY_URL}Privacy_Contacts` },
  accessibility: { behavior: 'asks', pane: 'Accessibility', path: `${PRIVACY} › Accessibility`, url: `${SECURITY_URL}Privacy_Accessibility` },
  'screen-recording': { behavior: 'asks', pane: 'Screen & System Audio Recording', path: `${PRIVACY} › Screen & System Audio Recording`, url: `${SECURITY_URL}Privacy_ScreenCapture` },
  camera: { behavior: 'asks', pane: 'Camera', path: `${PRIVACY} › Camera`, url: `${SECURITY_URL}Privacy_Camera` },
  microphone: { behavior: 'asks', pane: 'Microphone', path: `${PRIVACY} › Microphone`, url: `${SECURITY_URL}Privacy_Microphone` },
  'local-network': { behavior: 'asks', pane: 'Local Network', path: `${PRIVACY} › Local Network`, url: `${SECURITY_URL}Privacy_LocalNetwork` },
  'incoming-connections': { behavior: 'asks', pane: 'Firewall', path: 'System Settings › Network › Firewall', url: 'x-apple.systempreferences:com.apple.Network-Settings.extension' },
  notifications: { behavior: 'asks', pane: 'Notifications', path: 'System Settings › Notifications', url: 'x-apple.systempreferences:com.apple.Notifications-Settings.extension' },
  'login-item': { behavior: 'notifies', pane: 'Login Items & Extensions', path: 'System Settings › General › Login Items & Extensions', url: 'x-apple.systempreferences:com.apple.LoginItems-Settings.extension' },
  keychain: { behavior: 'asks', pane: 'Keychain Access', path: 'Keychain Access', url: null },
  'developer-tools': { behavior: 'asks', pane: null, path: null, url: null },
  gatekeeper: { behavior: 'asks', pane: 'Privacy & Security', path: PRIVACY, url: `${SECURITY_URL}General` },
};

/** Every permission kind, in documentation order. */
export const PERMISSION_KINDS = Object.keys(KINDS) as readonly PermissionKind[];

function info(kind: PermissionKind): KindInfo {
  const value = KINDS[kind];
  if (!value) throw new Error('invalid-permission-kind');
  return value;
}
export function behaviorOf(kind: PermissionKind): PromptBehavior { return info(kind).behavior; }
export function paneName(kind: PermissionKind): string | null { return info(kind).pane; }
export function settingsPath(kind: PermissionKind): string | null { return info(kind).path; }
export function settingsUrl(kind: PermissionKind): string | null { return info(kind).url; }
/** True for kinds that have a System Settings pane (and so a `foundation.settings.<kind>` action). */
export function hasSettingsPane(kind: PermissionKind): kind is SettingsPermissionKind { return info(kind).url !== null; }
/** The allowlist: every URL `openPermissionSettings` and `foundation.settings.<kind>` may open. */
export function isAllowedSettingsUrl(url: string): boolean { return Object.values(KINDS).some(value => value.url === url); }

const TERMINALS: ReadonlyArray<readonly [bundle: string, termProgram: string, name: string]> = [
  ['com.apple.Terminal', 'Apple_Terminal', 'Terminal'],
  ['com.googlecode.iterm2', 'iTerm.app', 'iTerm'],
  ['com.mitchellh.ghostty', 'ghostty', 'Ghostty'],
  ['com.microsoft.VSCode', 'vscode', 'Visual Studio Code'],
  ['dev.zed.Zed', 'zed', 'Zed'],
  ['dev.warp.Warp-Stable', 'WarpTerminal', 'Warp'],
  ['com.github.wez.wezterm', 'WezTerm', 'WezTerm'],
];

/**
 * The app macOS names in a privacy prompt for this process. Inside a product's
 * local app (the launcher sets `HRANESS_APP_BUNDLE_ID`) that is the product;
 * otherwise the terminal app that started the command.
 */
export function responsibleApp(env: NodeJS.ProcessEnv = process.env, product?: string): string {
  if ((env.HRANESS_APP_BUNDLE_ID ?? '') !== '' && product) return product;
  const bundle = env.__CFBundleIdentifier ?? '';
  const byBundle = TERMINALS.find(([id]) => id === bundle);
  if (byBundle) return byBundle[2];
  const program = env.TERM_PROGRAM ?? '';
  const byProgram = TERMINALS.find(([, name]) => name === program);
  if (byProgram) return byProgram[2];
  if ((env.ZED_TERM ?? '') !== '') return 'Zed';
  return 'your terminal app';
}

/** The requester for a need: explicit, else the kind's default (docs/permissions.md § Requester defaults). */
export function requesterOf(need: PermissionNeed, env: NodeJS.ProcessEnv = process.env): string {
  if (need.requester) return need.requester;
  if (need.kind === 'keychain') return 'security';
  if (need.kind === 'incoming-connections') return need.command;
  // Login Items lists the program the login item runs, never the terminal.
  if (need.kind === 'login-item') return need.product;
  return responsibleApp(env, need.product);
}

const nextOf = (need: PermissionNeed) => need.next ?? `${need.command} doctor`;
const forProduct = (requester: string, need: PermissionNeed) => requester === need.product ? '' : ` for ${need.product}`;

/** The pre-prompt copy for one surface. Pure; `env` only resolves the default requester. */
export function renderPrePrompt(need: PermissionNeed, surface: Surface, env: NodeJS.ProcessEnv = process.env): RenderedNotice {
  const { behavior, pane, path } = info(need.kind);
  const requester = requesterOf(need, env);
  let title: string, detail: string, confirm: string | undefined;
  if (need.kind === 'developer-tools') {
    title = `${need.product} needs Apple's command line tools to ${need.ask}. macOS will offer to install them (about 1 GB).`;
    detail = need.why;
    confirm = 'Press Enter to continue · s to skip';
  } else if (behavior === 'notifies') {
    const thatsProduct = requester === need.product ? '' : `. That's ${need.product}'s menu bar`;
    title = `macOS will show a notice that ${requester} can open at login${thatsProduct}.`;
    detail = `${need.why} Turn it off any time in ${path}.`;
  } else if (behavior === 'settings-only') {
    title = `${need.product} needs ${pane} to ${need.ask}.`;
    detail = `macOS doesn't ask for this. Turn on ${requester} in ${path}. ${need.why}`;
    confirm = 'Press Enter to open Settings · s to skip';
  } else {
    title = `macOS will ask to let ${requester} ${need.ask}${forProduct(requester, need)}.`;
    detail = need.kind === 'keychain'
      ? `${need.why} Enter your Mac password if asked, then choose Always Allow so macOS doesn't ask again.`
      : `${need.why} Change this any time in ${path}.`;
    confirm = 'Press Enter to continue · s to skip';
  }
  if (surface === 'cli') return { title, lines: [detail], ...(confirm ? { confirm } : {}) };
  const heading = behavior === 'settings-only'
    ? `${need.product} needs ${pane}`
    : need.kind === 'developer-tools' ? `${need.product} needs Apple's command line tools` : `${need.product} needs access to ${need.target ?? pane}`;
  return { title: heading, lines: [`${title} ${detail}`] };
}

/** The recovery copy after a denial or an unexplained failure. */
export function renderRecovery(need: PermissionNeed, state: RecoveryState, surface: Surface, env: NodeJS.ProcessEnv = process.env): RenderedNotice {
  const { pane, path, url } = info(need.kind);
  const requester = requesterOf(need, env);
  const next = nextOf(need);
  let title: string, lines: string[], confirm: string | undefined, step = next;
  if (need.kind === 'developer-tools') {
    title = `${need.product} needs Apple's command line tools. Nothing was installed.`;
    lines = [];
    step = 'xcode-select --install';
  } else if (need.kind === 'keychain' && state === 'denied') {
    title = `${need.product} can't ${need.ask}: the keychain request was denied.`;
    lines = ['Run it again and choose Always Allow when macOS asks.'];
  } else if (need.kind === 'keychain' && state === 'missing') {
    title = need.target ? `${need.product} can't find "${need.target}" in your keychain.` : `${need.product} can't ${need.ask}: the item isn't in your keychain.`;
    lines = [];
  } else if (need.kind === 'keychain') {
    title = `${need.product} couldn't ${need.ask}. macOS may be blocking ${requester}.`;
    lines = ['Unlock your login keychain, then retry.'];
  } else if (state === 'denied') {
    title = `${need.product} can't ${need.ask}: macOS access is off for ${requester}.`;
    lines = [`Turn on ${requester} in ${path}.`];
    if (url) confirm = 'press o to open Settings';
  } else {
    title = `${need.product} couldn't ${need.ask}. macOS may be blocking ${requester}.`;
    lines = path ? [`Check ${path}.`] : [];
  }
  if (surface === 'cli') return { title, lines, ...(confirm ? { confirm } : {}), next: step };
  const heading = need.kind === 'developer-tools' ? `${need.product} needs Apple's command line tools`
    : state === 'denied' && pane && need.kind !== 'keychain' ? `${pane} is off for ${need.product}` : `${need.product} can't ${need.ask}`;
  return { title: heading, lines: [[title, ...lines].join(' ')], next: step };
}

/** Plain text for a rendered CLI notice or recovery. `interactive` shows the confirm or "press o" hint. */
export function formatNotice(notice: RenderedNotice, options: { kind: 'pre-prompt' | 'recovery'; interactive: boolean; style?: CliStyle }): string {
  const style = options.style ?? { color: false, ascii: false };
  if (options.kind === 'pre-prompt') {
    const lines = [`${cliSymbol('notice', style)} ${notice.title}`, ...notice.lines.map(line => `   ${line}`)];
    if (options.interactive && notice.confirm) lines.push(`   ${notice.confirm}`);
    return lines.join('\n') + '\n';
  }
  const lines = [`${cliSymbol('fail', style)} ${notice.title}`, ...notice.lines.map(line => `  ${line}`)];
  if (notice.next) lines.push(`${cliSymbol('next', style)} ${notice.next}${options.interactive && notice.confirm ? ` · ${notice.confirm}` : ''}`);
  return lines.join('\n') + '\n';
}

/** The `--notice` dialog request for a need, or `null` when no dialog applies (login items: the toggle is the consent). */
export function permissionNoticeRequest(need: PermissionNeed, env: NodeJS.ProcessEnv = process.env): NoticeRequest | null {
  const behavior = behaviorOf(need.kind);
  if (behavior === 'notifies') return null;
  const rendered = renderPrePrompt(need, 'dialog', env);
  const settings = behavior === 'settings-only' && hasSettingsPane(need.kind) ? need.kind : undefined;
  return {
    type: 'notice-request', version: 1, title: clip(rendered.title, 128), message: clip(rendered.lines.join(' '), 512),
    primary: settings ? 'Open System Settings' : 'Continue', secondary: 'Not now', ...(settings ? { settings } : {}),
  };
}

function clip(text: string, max: number): string {
  const chars = [...text];
  return chars.length <= max ? text : chars.slice(0, max - 1).join('') + '…';
}

let interactiveKeyBusy = false;
/** Reads one key from a raw-mode TTY stdin: Enter, s (or Escape) and o. Ctrl-C restores the terminal and raises SIGINT. */
async function readTerminalKey(timeoutSeconds: number): Promise<'enter' | 's' | 'o' | 'timeout'> {
  const input = process.stdin;
  if (!input.isTTY || interactiveKeyBusy) return 'timeout';
  interactiveKeyBusy = true;
  return await new Promise(resolve => {
    const restore = () => {
      clearTimeout(timer); input.removeListener('data', onData);
      input.setRawMode?.(false); input.pause(); interactiveKeyBusy = false;
      process.stderr.write('\n');
    };
    const finish = (value: 'enter' | 's' | 'o' | 'timeout') => { restore(); resolve(value); };
    // Raw mode swallows Ctrl-C. Restore the terminal, then behave like an
    // ordinary interrupt: exit 130, or hand it to the product's own handler.
    // The promise stays pending so the interrupt is never read as a skip.
    const interrupt = () => {
      restore();
      if (process.listenerCount('SIGINT') === 0) process.exit(130);
      process.kill(process.pid, 'SIGINT');
    };
    const onData = (chunk: Buffer) => {
      for (const byte of chunk) {
        if (byte === 3) return interrupt();
        if (byte === 13 || byte === 10) return finish('enter');
        if (byte === 27 || byte === 115 || byte === 83) return finish('s');
        if (byte === 111 || byte === 79) return finish('o');
      }
    };
    const timer = setTimeout(() => finish('timeout'), timeoutSeconds * 1000);
    input.setRawMode?.(true); input.on('data', onData); input.resume();
  });
}

function runQuietly(argv: readonly string[], timeoutMs = 5000): Promise<{ status: number }> {
  return new Promise(resolve => {
    let child;
    try { child = spawn(argv[0]!, argv.slice(1), { stdio: 'ignore', shell: false, windowsHide: true }); }
    catch { resolve({ status: 127 }); return; }
    const timer = setTimeout(() => { child.kill('SIGKILL'); }, timeoutMs);
    child.once('error', () => { clearTimeout(timer); resolve({ status: 127 }); });
    child.once('close', code => { clearTimeout(timer); resolve({ status: code ?? 1 }); });
  });
}

async function probeFile(path: string): Promise<'ok' | 'denied' | 'missing'> {
  try { const handle = await open(path, 'r'); await handle.close(); return 'ok'; }
  catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === 'EPERM' || code === 'EACCES') return 'denied';
    if (code === 'ENOENT' || code === 'ENOTDIR') return 'missing';
    throw error;
  }
}

/** The real process IO. Tests pass their own. */
export function defaultPermissionIO(): PermissionIO {
  return {
    env: process.env,
    stdinIsTTY: process.stdin.isTTY === true,
    stderrIsTTY: process.stderr.isTTY === true,
    write: text => { process.stderr.write(text); },
    readKey: readTerminalKey,
    openUrl: async url => {
      if (process.platform !== 'darwin' || !isAllowedSettingsUrl(url)) return false;
      return (await runQuietly(['/usr/bin/open', url], 10_000)).status === 0;
    },
    fileAccess: probeFile,
    run: argv => runQuietly(argv),
  };
}

/** Opens the pane for an explicit human action only. Returns false when the kind has no pane. */
export async function openPermissionSettings(kind: PermissionKind, io: PermissionIO = defaultPermissionIO()): Promise<boolean> {
  const url = settingsUrl(kind);
  if (!url || !isAllowedSettingsUrl(url)) return false;
  try { return await io.openUrl(url); } catch { return false; }
}

/**
 * Shows the notice for the audience, waits for Enter or s when a confirm
 * applies and both stdin and stderr are terminals, and never triggers the
 * macOS prompt itself.
 */
export async function prePrompt(need: PermissionNeed, options: { audience?: Audience; io?: PermissionIO; timeoutSeconds?: number } = {}): Promise<PrePromptOutcome> {
  const io = options.io ?? defaultPermissionIO();
  const audience = options.audience ?? detectAudience({ env: io.env, stderrIsTTY: io.stderrIsTTY });
  const behavior = behaviorOf(need.kind);
  const unattended: PrePromptOutcome = (need.whenUnattended ?? (behavior === 'notifies' ? 'proceed' : 'stop')) === 'proceed' ? 'unattended-proceed' : 'unattended-stop';
  const notice = renderPrePrompt(need, 'cli', io.env);
  if (audience === 'quiet') return unattended;
  if (audience === 'agent') {
    io.write(JSON.stringify({ type: 'permission-notice', product: need.product, kind: need.kind, message: [notice.title, ...notice.lines].join(' ') }) + '\n');
    return unattended;
  }
  const interactive = io.stdinIsTTY && io.stderrIsTTY;
  const style = cliStyle({ isTTY: io.stderrIsTTY }, io.env);
  io.write(formatNotice(notice, { kind: 'pre-prompt', interactive, style }));
  if (!notice.confirm) return 'continue';
  if (!interactive) return unattended;
  const key = await io.readKey(options.timeoutSeconds ?? 120);
  if (key === 's' || key === 'timeout') return 'skip';
  if (behavior === 'settings-only' || key === 'o') await openPermissionSettings(need.kind, io);
  return 'continue';
}

/**
 * Prints the recovery for a failure on stderr (human and quiet audiences), and
 * opens Settings when the person presses o. Prints nothing for agents: print
 * `permissionErrorJson` on stdout for them instead.
 */
export async function reportPermissionFailure(need: PermissionNeed, state: RecoveryState, options: { audience?: Audience; io?: PermissionIO; timeoutSeconds?: number } = {}): Promise<void> {
  const io = options.io ?? defaultPermissionIO();
  const audience = options.audience ?? detectAudience({ env: io.env, stderrIsTTY: io.stderrIsTTY });
  if (audience === 'agent') return;
  const recovery = renderRecovery(need, state, 'cli', io.env);
  const interactive = audience === 'human' && io.stdinIsTTY && io.stderrIsTTY;
  const style = audience === 'human' ? cliStyle({ isTTY: io.stderrIsTTY }, io.env) : { ...cliStyle({ isTTY: false }, io.env), color: false };
  io.write(formatNotice(recovery, { kind: 'recovery', interactive, style }));
  if (interactive && recovery.confirm && await io.readKey(options.timeoutSeconds ?? 30) === 'o') await openPermissionSettings(need.kind, io);
}

/**
 * Only `~/Library` data outside other apps' containers and cloud folders is
 * guarded by Full Disk Access alone. Documents, Desktop, Downloads, iCloud
 * Drive, removable volumes and app containers each raise their own prompt.
 */
function isSilentProbePath(path: string, home: string): boolean {
  const library = join(home, 'Library') + '/';
  if (!path.startsWith(library)) return false;
  return !/^(Containers|Group Containers|Mobile Documents|CloudStorage|Daemon Containers)(\/|$)/.test(path.slice(library.length));
}

const HOME_TARGETS: Readonly<Record<string, readonly string[]>> = {
  Messages: ['Library', 'Messages', 'chat.db'],
  Safari: ['Library', 'Safari'],
};

/**
 * Read-only probe that never causes a prompt; 'unknown' when no probe is safe.
 * `full-disk-access` takes a target ("Messages", "Safari" or an absolute path
 * outside app containers), `login-item` takes the product's app ID.
 */
export async function permissionStatus(kind: PermissionKind, target?: string, io: PermissionIO = defaultPermissionIO()): Promise<PermissionState> {
  const home = io.env.HOME && isAbsolute(io.env.HOME) ? io.env.HOME : homedir();
  const access = io.fileAccess ?? probeFile;
  try {
    if (kind === 'full-disk-access') {
      const path = target === undefined ? undefined : HOME_TARGETS[target] ? join(home, ...HOME_TARGETS[target]!) : isAbsolute(target) ? normalize(target) : undefined;
      if (!path || !isSilentProbePath(path, home)) return 'unknown';
      const result = await access(path);
      return result === 'ok' ? 'granted' : result === 'denied' ? 'denied' : 'unknown';
    }
    if (kind === 'developer-tools') {
      // `xcode-select -p` only reads the selected path; the xcrun, swiftc and git shims are what open the install dialog.
      if (!io.run) return 'unknown';
      return (await io.run(['/usr/bin/xcode-select', '-p'])).status === 0 ? 'granted' : 'not-determined';
    }
    if (kind === 'login-item') {
      if (!target || !/^[a-z][a-z0-9.-]{0,63}$/.test(target) || target.includes('..')) return 'unknown';
      for (const label of [`app.hraness.${target}`, `app.hraness.companion.${target}`]) {
        if (await access(join(home, 'Library', 'LaunchAgents', `${label}.plist`)) === 'ok') return 'granted';
      }
      return 'not-determined';
    }
  } catch { return 'unknown'; }
  return 'unknown';
}

const KEYCHAIN_STATUS = new Map<number, RecoveryState>([[-128, 'denied'], [-25293, 'denied'], [-25308, 'unknown'], [-25300, 'missing']]);
/**
 * Classifies a keychain OSStatus, or the exit status of `/usr/bin/security`
 * (the OSStatus's low byte: 44 not found, 36 locked, 51 and 128 denied), for
 * `renderRecovery`. Returns `undefined` for success and unrelated codes.
 */
export function classifyKeychainStatus(status: number): RecoveryState | undefined {
  const direct = KEYCHAIN_STATUS.get(status);
  if (direct) return direct;
  if (Number.isInteger(status) && status > 0 && status < 256) {
    for (const [code, state] of KEYCHAIN_STATUS) if ((code & 0xff) === status) return state;
  }
  return undefined;
}

/** Menu rows: a `status.locked` row plus the `foundation.settings.<kind>` action when a pane exists. */
export function permissionMenuItems(need: PermissionNeed, state: PermissionState, env: NodeJS.ProcessEnv = process.env): MenuItemV2[] {
  if (state === 'granted') return [];
  const { pane } = info(need.kind);
  const name = pane ?? "Apple's command line tools";
  const requester = requesterOf(need, env);
  const rows: MenuItemV2[] = [need.kind === 'keychain'
    ? state === 'denied'
      ? { kind: 'status', symbol: 'status.locked', label: 'Keychain access is off', detail: 'Choose Always Allow when macOS asks again' }
      : { kind: 'status', symbol: 'status.locked', label: 'Needs keychain access', detail: clip(`To ${need.ask}`, 80) }
    : state === 'denied'
    ? { kind: 'status', symbol: 'status.locked', label: clip(`${name} is off`, 48), detail: clip(`Turn on ${requester} to ${need.ask}`, 80) }
    : { kind: 'status', symbol: 'status.locked', label: clip(`Needs ${name}`, 48), detail: clip(`To ${need.ask}`, 80) }];
  if (hasSettingsPane(need.kind)) rows.push({ kind: 'action', id: `foundation.settings.${need.kind}`, label: clip(`Open ${name} settings`, 48), symbol: 'action.permission', opens: 'settings' });
  return rows;
}

export interface PermissionErrorInfo {
  code: 'permission-denied' | 'permission-unknown' | 'permission-missing';
  kind: PermissionKind;
  message: string;
  next: string;
  settingsUrl: string | null;
}
/** The `--json` error fields for a permission failure. */
export function permissionError(need: PermissionNeed, state: RecoveryState, env: NodeJS.ProcessEnv = process.env): PermissionErrorInfo {
  const recovery = renderRecovery(need, state, 'cli', env);
  return { code: `permission-${state}`, kind: need.kind, message: recovery.title, next: recovery.next ?? nextOf(need), settingsUrl: settingsUrl(need.kind) };
}
/** The whole `--json` error document: `{"ok":false,"error":{…,"permission":{"kind","settingsUrl"}}}`. */
export function permissionErrorJson(need: PermissionNeed, state: RecoveryState, env: NodeJS.ProcessEnv = process.env) {
  const error = permissionError(need, state, env);
  return { ok: false as const, error: { code: error.code, message: error.message, next: error.next, permission: { kind: error.kind, settingsUrl: error.settingsUrl } } };
}

// Presets. Products pass their ProductRef and the parameters shown.

export function LOGIN_ITEM(ref: ProductRef): PermissionNeed {
  return { ...ref, kind: 'login-item', ask: 'open at login', why: 'Its menu bar icon opens when you log in. Nothing else runs in the background.', whenUnattended: 'proceed' };
}
export function CHROME_SAFE_STORAGE(ref: ProductRef, options: { browser?: string; caller: string; why?: string }): PermissionNeed {
  const browser = options.browser ?? 'Chrome';
  return {
    ...ref, kind: 'keychain', requester: options.caller, target: `${browser} Safe Storage`,
    ask: `use "${browser} Safe Storage" from your keychain`,
    why: options.why ?? `${ref.product} uses it to read the ${browser} sign-in you already have and never stores it.`,
  };
}
export function MESSAGES_FDA(ref: ProductRef, options: { why?: string } = {}): PermissionNeed {
  return { ...ref, kind: 'full-disk-access', target: 'Messages', ask: 'read your Messages', why: options.why ?? 'Only the chats you pick are read.' };
}
export function AUTOMATION(ref: ProductRef, app: string, why: string): PermissionNeed {
  return { ...ref, kind: 'automation', target: app, ask: `control ${app}`, why };
}
export function CONTACTS(ref: ProductRef, options: { why?: string } = {}): PermissionNeed {
  return { ...ref, kind: 'contacts', ask: 'see your contacts', why: options.why ?? `${ref.product} reads names and numbers on this Mac.` };
}
export function LOCAL_NETWORK(ref: ProductRef, why: string): PermissionNeed {
  return { ...ref, kind: 'local-network', ask: 'find and connect to devices on your local network', why };
}
export function INCOMING_CONNECTIONS(ref: ProductRef, options: { listener: string; why: string }): PermissionNeed {
  return { ...ref, kind: 'incoming-connections', requester: options.listener, ask: 'accept incoming network connections', why: options.why };
}
export function XCODE_TOOLS(ref: ProductRef, options: { skipEffect: string }): PermissionNeed {
  const effect = options.skipEffect.replace(/\.+$/, '');
  return { ...ref, kind: 'developer-tools', ask: 'build a small helper', why: `Nothing is installed unless you agree in that window. Or skip: ${effect}.`, whenUnattended: 'stop' };
}
export function SCREEN_RECORDING(ref: ProductRef, why: string): PermissionNeed {
  return { ...ref, kind: 'screen-recording', ask: 'record your screen', why };
}
export function LOCAL_SIGNING(ref: ProductRef): PermissionNeed {
  return {
    ...ref, kind: 'keychain', requester: 'codesign', target: 'Hraness Local Signing', ask: 'use your "Hraness Local Signing" key',
    why: 'Hraness signs its apps on this Mac with it so they keep their permissions after updates.',
  };
}
