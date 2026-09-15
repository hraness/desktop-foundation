import { createHash, randomUUID } from 'node:crypto';
import { link, lstat, mkdir, readFile, rename, unlink, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, join, posix, win32 } from 'node:path';
import { CompanionError } from './errors.js';
import { assertPhysicalPath } from './install.js';
import type { PlatformOptions } from './platform.js';

export interface AutostartOptions extends PlatformOptions { id: string; label: string; executable: string; args?: string[] }
export interface AutostartPlan {
  id: string;
  platform: 'darwin' | 'linux' | 'win32';
  path: string;
  contents: string;
  activation: 'next-login';
  requirements: string[];
}
const digest = (text: string) => createHash('sha256').update(text).digest('hex');
function prefix(platform: string) { return platform === 'darwin' ? '<!-- ' : platform === 'win32' ? "' " : '# '; }
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
  if (platform === 'darwin') {
    const label = `app.hraness.companion.${options.id}`;
    path = posix.join(home, 'Library', 'LaunchAgents', `${label}.plist`);
    body = `<plist version="1.0"><dict>\n<key>Label</key><string>${label}</string>\n<key>ProgramArguments</key><array>${[options.executable, ...args].map(arg => `<string>${xml(arg)}</string>`).join('')}</array>\n<key>RunAtLoad</key><true/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n</dict></plist>\n`;
  } else if (platform === 'linux') {
    const config = env.XDG_CONFIG_HOME && posix.isAbsolute(env.XDG_CONFIG_HOME) ? env.XDG_CONFIG_HOME : posix.join(home, '.config');
    path = posix.join(config, 'autostart', `hraness-companion-${options.id}.desktop`);
    body = `[Desktop Entry]\nType=Application\nVersion=1.0\nName=${desktopValue(options.label)}\nExec=${[options.executable, ...args].map(desktopArg).join(' ')}\nTerminal=false\nStartupNotify=false\nX-GNOME-Autostart-enabled=true\n`;
    requirements.push('An XDG-compatible graphical desktop with a tray host.');
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
async function ownedContents(plan: AutostartPlan): Promise<string | undefined> {
  let info;
  try { info = await lstat(plan.path); } catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; throw error; }
  if (!info.isFile() || info.isSymbolicLink() || info.size > 1024 * 1024) throw new CompanionError('autostart_conflict', 'Refusing to change an autostart file that is not an owned regular file.');
  const existing = await readFile(plan.path, 'utf8');
  const newline = existing.indexOf('\n');
  if (newline < 0 || existing.slice(0, newline + 1) !== header(plan.id, plan.platform, existing.slice(newline + 1))) throw new CompanionError('autostart_conflict', 'Refusing to change an unrelated or manually edited autostart file.');
  return existing;
}
function validatePlan(plan: AutostartPlan) {
  if (plan.platform !== process.platform) throw new CompanionError('unsupported_target', 'Cannot modify autostart files for another operating system.');
  const filename = plan.platform === 'darwin' ? `app.hraness.companion.${plan.id}.plist` : `hraness-companion-${plan.id}.${plan.platform === 'win32' ? 'vbs' : 'desktop'}`;
  const newline = plan.contents.indexOf('\n');
  if (!/^[a-z][a-z0-9.-]{0,63}$/.test(plan.id) || plan.id.includes('..') || basename(plan.path) !== filename
      || newline < 0 || plan.contents.slice(0, newline + 1) !== header(plan.id, plan.platform, plan.contents.slice(newline + 1))) throw new CompanionError('unsafe_path', 'Autostart plan is not a valid framework-owned file.');
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
  if (existing === plan.contents) return { path: plan.path, changed: false, activation: 'next-login' };
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
  return { path: plan.path, changed: true, activation: 'next-login' };
}
/** Removes only this framework's unedited file. Does not stop a running companion. */
export async function removeAutostart(plan: AutostartPlan): Promise<{ path: string; removed: boolean }> {
  validatePlan(plan);
  await assertPhysicalPath(dirname(plan.path));
  if (await ownedContents(plan) === undefined) return { path: plan.path, removed: false };
  await unlink(plan.path);
  return { path: plan.path, removed: true };
}
