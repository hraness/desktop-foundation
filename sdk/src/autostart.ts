import { createHash, randomUUID } from 'node:crypto';
import { link, lstat, mkdir, readFile, rename, unlink, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, join, posix, win32 } from 'node:path';
import { CompanionError } from './errors.js';
import { assertPhysicalPath } from './install.js';
import type { PlatformOptions } from './platform.js';

/**
 * A product's local app, built by `hraness-companion --assemble-app` at
 * `~/Applications/Hraness/<name>.app` (see docs/identity.md). With it, the
 * macOS login item starts the product through the app, so Login Items and
 * privacy prompts show the product's name instead of `node`, `bun` or `env`.
 * Ignored on Windows and Linux.
 */
export interface AutostartApp {
  /** Registry display name, such as "Textbutler". The app is `<name>.app`. */
  name: string;
  /**
   * Owner-only file in the product's state directory that `--launch` reads
   * the product command from, so no values show in `ps` or the plist.
   */
  argvFile: string;
}
export interface AutostartOptions extends PlatformOptions { id: string; label: string; executable: string; args?: string[]; app?: AutostartApp }
export interface AutostartPlan {
  id: string;
  platform: 'darwin' | 'linux' | 'win32';
  path: string;
  contents: string;
  activation: 'next-login';
  requirements: string[];
  /** Older login entries for this product. Turning login on removes them when they are ours. */
  legacy?: string[];
  /** The argv file `--launch` reads, when the login item starts the product's app. */
  launch?: { path: string; contents: string; program: string };
}
const digest = (text: string) => createHash('sha256').update(text).digest('hex');
function prefix(platform: string) { return platform === 'darwin' ? '<!-- ' : platform === 'win32' ? "' " : '# '; }
/** The largest argv file the app's `--launch` reads; it ignores a bigger one and starts nothing. */
export const MAX_LAUNCH_BYTES = 64 * 1024;
function header(id: string, platform: string, body: string) { return `${prefix(platform)}hraness-companion autostart ${id} sha256:${digest(body)}${platform === 'darwin' ? ' -->' : ''}\n`; }
function xml(text: string) { return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;'); }
/** Windows CommandLineToArgvW quoting, then separately escaped as a VBScript literal. */
function windowsArg(text: string) { return `"${text.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/g, '$1$1')}"`; }
// Keep the .vbs file ASCII so Windows Script Host does not misread a UTF-8
// username or executable path as the legacy system code page.
function vbsLiteral(text: string) { return `"${text.replace(/"/g, '""').replace(/[^\x20-\x7e]/g, unit => `" & ChrW(${unit.charCodeAt(0)}) & "`)}"`; }
function desktopArg(text: string) {
  // Exec quoting occurs after desktop-string unescaping. Escape both layers;
  // double every percent to prevent desktop Exec field-code substitution.
  const quoted = `"${text.replace(/(["`$\\])/g, '\\$1').replace(/%/g, '%%')}"`;
  return quoted.replace(/\\/g, '\\\\');
}
function desktopValue(text: string) { return text.replace(/\\/g, '\\\\').replace(/\t/g, '\\t').replace(/^ +| +$/g, spaces => '\\s'.repeat(spaces.length)); }

/** Renders an opt-in, per-user file. Calling this function alone has no side effects. */
export function planAutostart(options: AutostartOptions): AutostartPlan {
  const platform = options.platform ?? process.platform;
  const home = options.home ?? homedir();
  const env = options.env ?? process.env;
  if (!['darwin', 'linux', 'win32'].includes(platform)) throw new CompanionError('unsupported_target', `Autostart is unsupported on ${platform}.`);
  if (!/^[a-z][a-z0-9.-]{0,63}$/.test(options.id) || options.id.includes('..')) throw new CompanionError('unsafe_path', 'Autostart ID must be a short lowercase application identifier.');
  const pathApi = platform === 'win32' ? win32 : posix;
  if (!pathApi.isAbsolute(home) || !pathApi.isAbsolute(options.executable)) throw new CompanionError('unsafe_path', 'Autostart requires absolute home and executable paths.');
  const args = [...(options.args ?? [])];
  if ([home, options.executable, options.label, ...args].some(s => typeof s !== 'string' || /[\x00-\x1f\x7f]/.test(s))) throw new CompanionError('unsafe_path', 'Autostart values cannot contain control characters.');
  if (args.length > 64 || args.some(s => s.length > 8192) || options.label.length > 256) throw new CompanionError('unsafe_path', 'Autostart configuration is too large.');
  let path: string;
  let body: string;
  const requirements: string[] = [];
  if (platform === 'darwin' && options.app) {
    // Same file the Rust service helper writes (src/service.rs `plan`), so
    // either side recognizes and migrates the other's login item.
    const { name, argvFile } = options.app;
    if (typeof name !== 'string' || !name || name.length > 128 || name.startsWith('.') || /[/:\x00-\x1f\x7f]/.test(name)) throw new CompanionError('unsafe_path', 'The app name must be a short display name without slashes.');
    if (typeof argvFile !== 'string' || !posix.isAbsolute(argvFile) || /[\x00-\x1f\x7f]/.test(argvFile) || argvFile.length > 1024) throw new CompanionError('unsafe_path', 'The app launch file must be an absolute path.');
    // `--launch` reads at most 64 entries, the executable included, from a
    // file of at most 64 KiB (src/identity.rs MAX_ARGV_FILE_BYTES).
    if (args.length > 63 || Buffer.byteLength(JSON.stringify([options.executable, ...args])) > MAX_LAUNCH_BYTES) throw new CompanionError('unsafe_path', 'Autostart configuration is too large.');
    const label = `app.hraness.${options.id}`;
    const agents = posix.join(home, 'Library', 'LaunchAgents');
    const program = posix.join(home, 'Applications', 'Hraness', `${name}.app`, 'Contents', 'MacOS', name);
    path = posix.join(agents, `${label}.plist`);
    body = `<plist version="1.0"><dict>\n<key>Label</key><string>${xml(label)}</string>\n<key>ProgramArguments</key><array>${[program, '--launch', argvFile].map(arg => `<string>${xml(arg)}</string>`).join('')}</array>\n<key>AssociatedBundleIdentifiers</key><array><string>${xml(label)}</string></array>\n<key>RunAtLoad</key><true/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n<key>ProcessType</key><string>Interactive</string>\n</dict></plist>\n`;
    return {
      id: options.id, platform: 'darwin', path, contents: header(options.id, platform, body) + body, activation: 'next-login', requirements,
      legacy: [posix.join(agents, `app.hraness.companion.${options.id}.plist`)],
      launch: { path: argvFile, contents: JSON.stringify([options.executable, ...args]), program },
    };
  } else if (platform === 'darwin') {
    const label = `app.hraness.companion.${options.id}`;
    path = posix.join(home, 'Library', 'LaunchAgents', `${label}.plist`);
    body = `<plist version="1.0"><dict>\n<key>Label</key><string>${label}</string>\n<key>ProgramArguments</key><array>${[options.executable, ...args].map(arg => `<string>${xml(arg)}</string>`).join('')}</array>\n<key>RunAtLoad</key><true/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n</dict></plist>\n`;
  } else if (platform === 'linux') {
    const config = env.XDG_CONFIG_HOME && posix.isAbsolute(env.XDG_CONFIG_HOME) ? env.XDG_CONFIG_HOME : posix.join(home, '.config');
    path = posix.join(config, 'autostart', `hraness-companion-${options.id}.desktop`);
    body = `[Desktop Entry]\nType=Application\nVersion=1.0\nName=${desktopValue(options.label)}\nExec=${[options.executable, ...args].map(desktopArg).join(' ')}\nTerminal=false\nStartupNotify=false\nX-GNOME-Autostart-enabled=true\n`;
    requirements.push('An XDG-compatible desktop session that runs autostart entries.');
  } else {
    // WScript.Shell.Run expands %ENV% before spawning. Reject % rather than
    // silently changing paths/arguments; never interpolate into cmd.exe.
    if ([options.executable, ...args].some(s => s.includes('%'))) throw new CompanionError('unsafe_path', 'Windows autostart paths and arguments cannot contain percent signs.');
    if (!/\.exe$/i.test(options.executable)) throw new CompanionError('unsafe_path', 'Windows autostart requires a native .exe; shell and batch scripts are unsupported.');
    const appData = env.APPDATA && win32.isAbsolute(env.APPDATA) ? env.APPDATA : win32.join(home, 'AppData', 'Roaming');
    path = win32.join(appData, 'Microsoft', 'Windows', 'Start Menu', 'Programs', 'Startup', `hraness-companion-${options.id}.vbs`);
    const command = [options.executable, ...args].map(windowsArg).join(' ');
    body = `Option Explicit\nDim shell\nSet shell = CreateObject("WScript.Shell")\nshell.Run ${vbsLiteral(command)}, 0, False\n`;
    requirements.push('Windows Script Host must be enabled by user/organization policy. VBScript may be an optional OS feature; if unavailable, launch the CLI manually.');
  }
  return { id: options.id, platform: platform as AutostartPlan['platform'], path, contents: header(options.id, platform, body) + body, activation: 'next-login', requirements };
}
async function ownedAt(path: string, id: string, platform: string): Promise<string | undefined> {
  let info;
  try { info = await lstat(path); } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; throw error; }
  if (!info.isFile() || info.isSymbolicLink() || info.size > 1024 * 1024) throw new CompanionError('autostart_conflict', 'Refusing to change an autostart file that is not an owned regular file.');
  const existing = await readFile(path, 'utf8');
  const newline = existing.indexOf('\n');
  if (newline < 0 || existing.slice(0, newline + 1) !== header(id, platform, existing.slice(newline + 1))) throw new CompanionError('autostart_conflict', 'Refusing to change an unrelated or manually edited autostart file.');
  return existing;
}
const ownedContents = (plan: AutostartPlan) => ownedAt(plan.path, plan.id, plan.platform);
/** Our legacy files that still exist. Anything that is not ours is left alone. */
async function ownedLegacy(plan: AutostartPlan): Promise<string[]> {
  const found: string[] = [];
  for (const path of plan.legacy ?? []) {
    try { if (await ownedAt(path, plan.id, plan.platform) !== undefined) found.push(path); }
    catch (error) { if (!(error instanceof CompanionError && error.code === 'autostart_conflict')) throw error; }
  }
  return found;
}
/** The launch file's contents when it is an owner-only regular file, otherwise undefined. */
async function launchContents(path: string): Promise<string | undefined> {
  let info;
  try { info = await lstat(path); } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; throw error; }
  if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_LAUNCH_BYTES || (info.mode & 0o077) !== 0) return;
  if (typeof process.getuid === 'function' && info.uid !== process.getuid()) return;
  return readFile(path, 'utf8');
}
function validatePlan(plan: AutostartPlan) {
  if (plan.platform !== process.platform) throw new CompanionError('unsupported_target', 'Cannot modify autostart files for another operating system.');
  const app = plan.launch !== undefined;
  const filename = plan.platform === 'darwin' ? `app.hraness.${app ? '' : 'companion.'}${plan.id}.plist` : `hraness-companion-${plan.id}.${plan.platform === 'win32' ? 'vbs' : 'desktop'}`;
  const newline = plan.contents.indexOf('\n');
  const legacyName = `app.hraness.companion.${plan.id}.plist`;
  let argv: unknown;
  try { argv = app ? JSON.parse(plan.launch!.contents) : undefined; } catch { argv = undefined; }
  if (!/^[a-z][a-z0-9.-]{0,63}$/.test(plan.id) || plan.id.includes('..') || basename(plan.path) !== filename
      || newline < 0 || plan.contents.slice(0, newline + 1) !== header(plan.id, plan.platform, plan.contents.slice(newline + 1))
      || (plan.legacy ?? []).some(path => dirname(path) !== dirname(plan.path) || basename(path) !== legacyName)
      || (app && (plan.platform !== 'darwin' || !posix.isAbsolute(plan.launch!.path) || Buffer.byteLength(plan.launch!.contents) > MAX_LAUNCH_BYTES
        || !Array.isArray(argv) || argv.length === 0 || argv.length > 64
        || !argv.every(arg => typeof arg === 'string') || !posix.isAbsolute(argv[0] as string) || !posix.isAbsolute(plan.launch!.program)
        || !plan.contents.includes(`<array><string>${xml(plan.launch!.program)}</string><string>--launch</string><string>${xml(plan.launch!.path)}</string></array>`)))) throw new CompanionError('unsafe_path', 'Autostart plan is not a valid framework-owned file.');
}
/** Writes the launch file atomically and owner-only. Returns whether it changed. */
async function writeLaunch(launch: NonNullable<AutostartPlan['launch']>): Promise<boolean> {
  const parent = dirname(launch.path);
  await assertPhysicalPath(parent);
  await mkdir(parent, { recursive: true, mode: 0o700 });
  await assertPhysicalPath(parent);
  const info = await lstat(parent);
  if (!info.isDirectory() || info.isSymbolicLink()) throw new CompanionError('unsafe_path', 'The app launch file must be in a real directory.');
  try {
    const current = await lstat(launch.path);
    if (!current.isFile() || current.isSymbolicLink()) throw new CompanionError('autostart_conflict', 'Refusing to replace an app launch file that is not a regular file.');
  } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
  if (await launchContents(launch.path) === launch.contents) return false;
  const temp = join(parent, `.launch-argv-${randomUUID()}.tmp`);
  try {
    await writeFile(temp, launch.contents, { flag: 'wx', mode: 0o600 });
    await rename(temp, launch.path);
  } finally { await unlink(temp).catch(error => { if (error.code !== 'ENOENT') throw error; }); }
  return true;
}
/**
 * Whether this framework's login entry exists: `on` (exactly this plan),
 * `outdated` (ours, but for an older command), `off`, or `conflict` (a file we
 * did not write). Read-only; never changes anything.
 */
export async function autostartState(plan: AutostartPlan): Promise<'on' | 'outdated' | 'off' | 'conflict'> {
  validatePlan(plan);
  try {
    const existing = await ownedContents(plan);
    // A login entry this product wrote before it had an app still starts it.
    if (existing === undefined) return (await ownedLegacy(plan)).length ? 'outdated' : 'off';
    if (existing !== plan.contents) return 'outdated';
    if (plan.launch && await launchContents(plan.launch.path) !== plan.launch.contents) return 'outdated';
    return (await ownedLegacy(plan)).length ? 'outdated' : 'on';
  } catch (error) {
    if (error instanceof CompanionError && error.code === 'autostart_conflict') return 'conflict';
    throw error;
  }
}
/**
 * Fails with `app_missing` when the plan starts the product's app and the
 * app isn't built, so a login item never points at nothing.
 */
export async function assertAppBuilt(plan: AutostartPlan): Promise<void> {
  if (!plan.launch) return;
  const program = await lstat(plan.launch.program).catch(() => undefined);
  if (!program?.isFile()) throw new CompanionError('app_missing', `${basename(plan.launch.program)}.app is not built in ${dirname(dirname(dirname(dirname(plan.launch.program))))}.`);
}
/** Explicit opt-in only. Activates at the next graphical login, not immediately. */
export async function setAutostart(plan: AutostartPlan): Promise<{ path: string; changed: boolean; activation: 'next-login' }> {
  validatePlan(plan);
  const parent = dirname(plan.path);
  await assertPhysicalPath(parent);
  await mkdir(parent, { recursive: true, mode: 0o700 });
  await assertPhysicalPath(parent);
  const info = await lstat(parent);
  if (!info.isDirectory() || info.isSymbolicLink()) throw new CompanionError('unsafe_path', 'Autostart destination must be a real directory.');
  const existing = await ownedContents(plan);
  await assertAppBuilt(plan);
  // The launch file goes first, so the login item never points at a missing one.
  const launched = plan.launch ? await writeLaunch(plan.launch) : false;
  if (existing === plan.contents) return { path: plan.path, changed: (await removeLegacy(plan)) || launched, activation: 'next-login' };
  const temp = join(parent, `.hraness-companion-${randomUUID()}.tmp`);
  try {
    await writeFile(temp, plan.contents, { flag: 'wx', mode: 0o600 });
    if (await ownedContents(plan) !== existing) throw new CompanionError('autostart_conflict', 'Autostart changed concurrently; retry after reviewing it.');
    if (existing === undefined) {
      try { await link(temp, plan.path); }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error;
        if (await ownedContents(plan) !== plan.contents) throw new CompanionError('autostart_conflict', 'Another process created the autostart file; review it before retrying.');
      }
    } else await rename(temp, plan.path);
  } finally { await unlink(temp).catch(error => { if (error.code !== 'ENOENT') throw error; }); }
  // Replace the old login entry in the same command, after the new one exists.
  await removeLegacy(plan);
  return { path: plan.path, changed: true, activation: 'next-login' };
}
async function removeLegacy(plan: AutostartPlan): Promise<boolean> {
  let removed = false;
  for (const path of await ownedLegacy(plan)) {
    await unlink(path).catch(error => { if (error.code !== 'ENOENT') throw error; });
    removed = true;
  }
  return removed;
}
/**
 * Removes only this framework's unedited files, including an older login
 * entry for the same product. Does not stop a running companion. The app's
 * launch file stays, like the app itself.
 */
export async function removeAutostart(plan: AutostartPlan): Promise<{ path: string; removed: boolean }> {
  validatePlan(plan);
  await assertPhysicalPath(dirname(plan.path));
  const current = await ownedContents(plan) !== undefined;
  if (current) await unlink(plan.path);
  const legacy = await removeLegacy(plan);
  return { path: plan.path, removed: current || legacy };
}
