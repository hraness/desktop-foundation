import { spawn } from 'node:child_process';
import { runCompanion, packagedManifest, type CompanionOptions } from './client.js';
import { ensureBinary } from './install.js';
import { diagnosePlatform, resolveTarget } from './platform.js';
import { companionStatus, startCompanion, stopCompanion, serveCompanion } from './service.js';
import { planAutostart, setAutostart, removeAutostart } from './autostart.js';

export interface CompanionInvocation {
  args: readonly string[];
  /** Exact product-owned command for the foreground branch, without shell interpolation. */
  foreground: { executable: string; args: readonly string[] };
  write?: (result: unknown) => void;
}
export async function handleCompanionCommand(options: CompanionOptions, invocation: CompanionInvocation): Promise<number> {
  const write = invocation.write ?? (result => process.stdout.write(JSON.stringify(result) + '\n'));
  const args = invocation.args.filter(value => value !== '--json');
  const command = args.length ? args.join(' ') : 'start';
  if (command === '--foreground') return await serveCompanion(options);
  if (command === 'doctor') {
    const diagnostics = diagnosePlatform();
    const status = await companionStatus(options.stateDir, options.appId).catch(() => ({ running: false, appId: options.appId, receipt: 'invalid' }));
    write({ status, diagnostics, signing: 'unsigned', notarization: 'none', documentation: 'https://github.com/hraness/desktop-foundation/blob/main/docs/installation.md' });
    return diagnostics.some(d => d.severity === 'error') ? 1 : 0;
  }
  if (command === 'status') { write(await companionStatus(options.stateDir, options.appId)); return 0; }
  if (command === 'stop') { write(await stopCompanion(options.stateDir, options.appId)); return 0; }
  if (command === 'install' || command === 'uninstall') {
    const plan = planAutostart({ id: options.appId, label: options.name, executable: invocation.foreground.executable, args: [...invocation.foreground.args] });
    if (command === 'install') { await setAutostart(plan); write({ loginStartup: 'enabled', takesEffect: 'next-login' }); }
    else { await removeAutostart(plan); write({ loginStartup: 'disabled', runningCompanion: 'unchanged' }); }
    return 0;
  }
  if (command !== 'start') throw new Error('Usage: menubar [start|stop|status|doctor|install|uninstall] [--json]');
  const diagnostics = diagnosePlatform();
  if (diagnostics.some(d => d.severity === 'error')) { write({ running: false, diagnostics }); return 1; }
  if ((await companionStatus(options.stateDir, options.appId)).running) { write({ running: true, alreadyRunning: true }); return 0; }
  // Download/verify in the visible CLI before detaching, so failures remain actionable.
  if (!options.binary) await ensureBinary({ manifest: options.manifest ?? await packagedManifest(), cacheDir: options.cacheDir });
  write(await startCompanion({ appId: options.appId, stateDir: options.stateDir, ...invocation.foreground }));
  return 0;
}

/** Open browser UI only; never accept arbitrary URI schemes or shell commands. */
export async function openBrowser(address: string): Promise<void> {
  const url = new URL(address);
  if (url.username || url.password || !(url.protocol === 'https:' || (url.protocol === 'http:' && ['localhost','127.0.0.1','[::1]'].includes(url.hostname)))) throw new Error('unsupported-browser-url');
  const program = process.platform === 'darwin' ? '/usr/bin/open' : process.platform === 'win32' ? 'rundll32.exe' : 'xdg-open';
  const argv = process.platform === 'win32' ? ['url.dll,FileProtocolHandler', url.href] : [url.href];
  await new Promise<void>((resolve, reject) => {
    const child = spawn(program, argv, { stdio: 'ignore', windowsHide: true, shell: false });
    const timeout = setTimeout(() => { child.kill(); reject(new Error('browser-open-timeout')); }, 10_000);
    child.once('error', () => { clearTimeout(timeout); reject(new Error('browser-open-failed')); });
    child.once('exit', code => { clearTimeout(timeout); code === 0 ? resolve() : reject(new Error('browser-open-failed')); });
  });
}
