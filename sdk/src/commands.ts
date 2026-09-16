export { openBrowser } from './browser.js';
import { packagedManifest, type CompanionOptions } from './client.js';
import { ensureBinary, inspectBinary } from './install.js';
import { diagnosePlatform } from './platform.js';
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
    const status = await companionStatus(options.stateDir, options.appId).catch(() => ({ running: null, appId: options.appId, state: 'invalid-receipt' }));
    let artifact: unknown;
    let artifactFailed = false;
    try {
      artifact = options.binary ? { path: options.binary, source: 'maintainer-override', integrity: 'not-release-verified' } : await inspectBinary({ manifest: options.manifest ?? await packagedManifest(), cacheDir: options.cacheDir });
    } catch (error) { artifactFailed = true; artifact = { state: 'unavailable', code: (error as {code?:string}).code ?? 'release-manifest-unavailable' }; }
    write({ status, artifact, diagnostics, signing: 'unsigned', notarization: 'none', documentation: 'https://github.com/hraness/desktop-foundation/blob/main/docs/installation.md' });
    return artifactFailed || diagnostics.some(d => d.severity === 'error') ? 1 : 0;
  }
  if (command === 'status') { write(await companionStatus(options.stateDir, options.appId)); return 0; }
  if (command === 'stop') { write(await stopCompanion(options.stateDir, options.appId)); return 0; }
  if (command === 'install' || command === 'uninstall') {
    const plan = planAutostart({ id: options.appId, label: options.name, executable: invocation.foreground.executable, args: [...invocation.foreground.args] });
    if (command === 'install') { await setAutostart(plan); write({ loginStartup: 'enabled', takesEffect: 'next-login', requirements: plan.requirements }); }
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
