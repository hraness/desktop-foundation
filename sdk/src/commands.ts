export { openBrowser } from './browser.js';
import { basename } from 'node:path';
import { detectAudience, type Audience } from './audience.js';
import { assertAppBuilt, autostartState, planAutostart, removeAutostart, setAutostart, type AutostartApp, type AutostartPlan } from './autostart.js';
import { packagedManifest, type CompanionOptions } from './client.js';
import { createCliOutput, formatBytes, type CliOutput } from './cli-style.js';
import { CompanionError } from './errors.js';
import { ensureBinary, inspectBinary } from './install.js';
import { loadLoginEnvironment, removeLoginEnvironment, saveLoginEnvironment, type LoginEnvResult } from './login-env.js';
import { LOGIN_ITEM, prePrompt, type PermissionIO } from './permissions.js';
import { diagnosePlatform, type PlatformDiagnostic } from './platform.js';
import { companionStatus, startCompanion, stopCompanion, serveCompanion } from './service.js';

export interface CompanionInvocation {
  args: readonly string[];
  /** Exact product-owned command for the foreground branch, without shell interpolation. */
  foreground: { executable: string; args: readonly string[] };
  /**
   * On macOS, the product's local app (docs/identity.md). When set, the login
   * item starts the product through it, so Login Items shows the product's
   * name, and an older login entry for the product is replaced.
   */
  app?: AutostartApp;
  /**
   * Receives the result object when the output is JSON (`--json`, or an agent
   * audience). People at a terminal get text instead.
   */
  write?: (result: unknown) => void;
  /** How the command reads in next steps and help, such as "textbutler menubar". Default "menubar". */
  command?: string;
  /** Test hooks. */
  audience?: Audience;
  output?: CliOutput;
  permissionIO?: PermissionIO;
  home?: string;
  env?: NodeJS.ProcessEnv;
  fetch?: typeof globalThis.fetch;
}

const COMMANDS = ['start', 'stop', 'status', 'doctor', 'install', 'uninstall'] as const;

/** Grouped help for a product's `menubar` command. */
export function companionHelp(name: string, command = 'menubar'): string {
  return `Usage: ${command} [command] [--json]

Show ${name} in the menu bar.

Commands
  start       Show ${name} in the menu bar (the default)
  stop        Remove it from the menu bar
  status      Show whether it's running and opens at login
  install     Open it at login
  uninstall   Stop opening it at login
  doctor      Check that this computer can show it

Options
  -h, --help  Show this help
  --json      Print machine-readable output
`;
}

/** Plain words and one next step for a failed lifecycle command. The code stays in JSON. */
export function describeCompanionError(error: unknown, name: string, command = 'menubar'): { code: string; message: string; next: string } {
  const doctor = `${command} doctor`;
  if (error instanceof CompanionError) {
    const messages: Record<string, [string, string]> = {
      unsupported_target: [`The menu bar isn't available on this computer.`, doctor],
      invalid_manifest: [`This copy of ${name} has a damaged menu bar helper list. Reinstall ${name}.`, doctor],
      download_failed: [`Couldn't download the menu bar helper. Check your internet connection.`, command],
      integrity_failed: [`The downloaded menu bar helper didn't match its published checksum, so it wasn't used.`, command],
      unsafe_path: [`The menu bar can't use its folder safely: ${error.message}`, doctor],
      cache_conflict: [`Another install of the menu bar helper got in the way.`, command],
      autostart_conflict: [`A login item with ${name}'s name already exists that ${name} didn't create, so it was left alone.`, doctor],
      os_approval_required: [`Your computer blocked the menu bar helper.`, doctor],
      app_missing: [`${name}.app isn't built yet, so it can't open at login.`, doctor],
    };
    const [message, next] = messages[error.code] ?? [error.message, doctor];
    return { code: error.code, message, next };
  }
  const raw = error instanceof Error ? error.message : '';
  const code = /^[a-z0-9-]+/.exec(raw)?.[0] ?? 'companion-failed';
  const known: Record<string, [string, string]> = {
    'companion-start-timeout': [`${name} didn't appear in the menu bar in time.`, doctor],
    'companion-start-failed': [`${name} couldn't start its menu bar.`, doctor],
    'stop-indeterminate': [`Couldn't confirm that ${name} left the menu bar.`, `${command} status`],
    'stop-timeout': [`${name} didn't leave the menu bar in time.`, `${command} status`],
    'release-manifest-unavailable': [`This build of ${name} has no menu bar helper. Install a released version.`, doctor],
    'multiple-service-owners': [`More than one ${name} menu bar is running.`, `${command} stop`],
  };
  const [message, next] = known[code] ?? [`Something went wrong with ${name}'s menu bar.`, doctor];
  return { code, message, next };
}

export async function handleCompanionCommand(options: CompanionOptions, invocation: CompanionInvocation): Promise<number> {
  const env = invocation.env ?? process.env;
  const command = invocation.command ?? 'menubar';
  const json = invocation.args.includes('--json') || (invocation.audience ?? detectAudience({ env })) === 'agent';
  const output = invocation.output ?? createCliOutput({ env, ...(invocation.audience ? { audience: invocation.audience } : {}) });
  const write = invocation.write ?? (result => process.stdout.write(JSON.stringify(result) + '\n'));
  const args = invocation.args.filter(value => value !== '--json');
  const verb = args.length ? args.join(' ') : 'start';
  const name = options.name;

  if (verb === '--foreground') {
    if (options.loginEnv?.length) await loadLoginEnvironment(options.stateDir, options.loginEnv);
    return await serveCompanion({ ...options, loginItem: options.loginItem ?? { ...invocation.foreground, ...(invocation.app ? { app: invocation.app } : {}) } });
  }
  if (verb === 'help' || verb === '--help' || verb === '-h' || args.includes('--help') || args.includes('-h')) {
    output.result(companionHelp(name, command).trimEnd());
    return 0;
  }
  if (!(COMMANDS as readonly string[]).includes(verb)) {
    const message = `Unknown menu bar command "${verb}".`;
    if (json) write({ ok: false, error: { code: 'usage', message, next: `${command} --help` } });
    else output.error({ message, next: `${command} --help` });
    return 2;
  }

  const plan = (): AutostartPlan => planAutostart({
    id: options.appId, label: name, executable: invocation.foreground.executable, args: [...invocation.foreground.args],
    ...(invocation.app ? { app: invocation.app } : {}),
    ...(invocation.home ? { home: invocation.home } : {}), ...(invocation.env ? { env: invocation.env } : {}),
  });
  const loginState = async () => { try { return await autostartState(plan()); } catch { return 'unknown' as const; } };

  try {
    switch (verb as typeof COMMANDS[number]) {
      case 'doctor': return await doctor();
      case 'status': return await status();
      case 'stop': return await stop();
      case 'install': return await install();
      case 'uninstall': return await uninstall();
      case 'start': return await start();
    }
  } catch (error) {
    const described = describeCompanionError(error, name, command);
    if (json) write({ ok: false, error: described });
    else output.error(described);
    return 1;
  }

  async function doctor(): Promise<number> {
    const diagnostics = diagnosePlatform();
    const running = await companionStatus(options.stateDir, options.appId).catch(() => ({ running: null, appId: options.appId, state: 'invalid-receipt' }));
    let artifact: unknown;
    let artifactFailed = false;
    try {
      artifact = options.binary ? { path: options.binary, source: 'maintainer-override', integrity: 'not-release-verified' } : await inspectBinary({ manifest: options.manifest ?? await packagedManifest(), cacheDir: options.cacheDir });
    } catch (error) { artifactFailed = true; artifact = { state: 'unavailable', code: (error as { code?: string }).code ?? 'release-manifest-unavailable' }; }
    const platformFailed = diagnostics.some(d => d.severity === 'error');
    if (json) {
      write({ status: running, artifact, diagnostics, signing: 'unsigned', notarization: 'none', documentation: 'https://github.com/hraness/desktop-foundation/blob/main/docs/installation.md' });
      return artifactFailed || platformFailed ? 1 : 0;
    }
    let problems = 0, warnings = 0;
    const notable = diagnostics.filter(d => d.severity !== 'info');
    for (const diagnostic of notable) {
      if (diagnostic.severity === 'error') problems++; else warnings++;
      output.result(diagnostic.message, diagnostic.severity === 'error' ? 'fail' : 'warn');
      if (diagnostic.guidance) output.detail(diagnostic.guidance);
    }
    if (!notable.length) output.result('This computer can show menu bar icons', 'ok');
    const helper = artifact as { source?: string; installed?: boolean; version?: string };
    if (artifactFailed) { problems++; output.result(`The menu bar helper isn't available in this build of ${name}`, 'fail'); }
    else if (helper.source === 'maintainer-override') output.result('Using a local test build of the menu bar helper', 'skip');
    else if (helper.installed) output.result(`The menu bar helper is downloaded and checked (version ${helper.version})`, 'ok');
    else output.result(`The menu bar helper downloads the first time you run ${command}`, 'skip');
    const state = (running as { running: boolean | null }).running;
    output.result(state ? `${name} is in the menu bar` : state === false ? `${name} isn't in the menu bar` : `${name}'s menu bar isn't responding`, state ? 'on' : state === false ? 'off' : 'warn');
    if (state === null) warnings++;
    const login = await loginState();
    output.result(login === 'on' || login === 'outdated' ? 'Opens at login' : login === 'conflict' ? `A login item with ${name}'s name wasn't made by ${name}` : "Doesn't open at login", login === 'on' || login === 'outdated' ? 'on' : login === 'conflict' ? 'warn' : 'off');
    if (login === 'conflict') warnings++;
    output.result('');
    const summary = [problems ? `${problems} problem${problems === 1 ? '' : 's'}` : '', warnings ? `${warnings} warning${warnings === 1 ? '' : 's'}` : ''].filter(Boolean).join(', ');
    output.result(summary ? `${summary}.` : 'Everything looks good.');
    if (problems) output.result(`${command} doctor --json`, 'next');
    return artifactFailed || platformFailed ? 1 : 0;
  }

  async function status(): Promise<number> {
    const current = await companionStatus(options.stateDir, options.appId);
    if (json) { write(current); return 0; }
    const login = await loginState();
    if (current.running) output.result(`${name} is in the menu bar`, 'on');
    else if (current.running === false) output.result(`${name} isn't in the menu bar`, 'off');
    else output.result(`${name}'s menu bar isn't responding`, 'warn');
    const opens = login === 'on' || login === 'outdated';
    output.result(opens ? 'Opens at login' : "Doesn't open at login", opens ? 'on' : 'off');
    if (current.running === null) output.next(`${command} stop`);
    else if (!current.running) output.next(command);
    else if (!opens) output.next(`${command} install`);
    return 0;
  }

  async function stop(): Promise<number> {
    const before = await companionStatus(options.stateDir, options.appId).catch(() => undefined);
    const after = await stopCompanion(options.stateDir, options.appId);
    if (json) { write(after); return 0; }
    if (before?.running === false) output.result(`${name} wasn't in the menu bar.`, 'off');
    else output.result(`${name} is no longer in the menu bar.`, 'ok');
    return 0;
  }

  async function install(): Promise<number> {
    const current = plan();
    const state = await autostartState(current);
    await assertAppBuilt(current);
    if (state !== 'on' && state !== 'conflict') {
      // Login items only notify, so the notice needs no confirmation.
      // macOS names whatever the login item starts: the app when there is one.
      const requester = current.launch ? invocation.app!.name : basename(invocation.foreground.executable).replace(/\.exe$/i, '');
      const need = LOGIN_ITEM({ product: name, command, requester });
      await prePrompt(need, { audience: json ? 'agent' : output.audience, ...(invocation.permissionIO ? { io: invocation.permissionIO } : {}) });
    }
    await setAutostart(current);
    const saved: LoginEnvResult | undefined = options.loginEnv?.length ? await saveLoginEnvironment(options.stateDir, options.loginEnv, env) : undefined;
    if (json) {
      write({ loginStartup: 'enabled', takesEffect: 'next-login', requirements: current.requirements, ...(saved ? { loginEnv: saved } : {}) });
      return 0;
    }
    if (state === 'on') output.result(`${name} already opens at login.`, 'on');
    else output.result(`${name} will open at login.`, 'ok');
    for (const requirement of current.requirements) output.detail(requirement);
    if (saved?.saved.length) output.detail(`Saved ${saved.saved.join(', ')} in a private file so it starts signed in.`);
    if (saved?.missing.length) {
      output.warn(`${saved.missing.join(', ')} ${saved.missing.length === 1 ? "isn't" : "aren't"} set here, so ${name} will start signed out at login.`);
      output.next(`set ${saved.missing[0]}, then run ${command} install again`);
    }
    return 0;
  }

  async function uninstall(): Promise<number> {
    const removed = await removeAutostart(plan());
    await removeLoginEnvironment(options.stateDir).catch(() => false);
    if (json) { write({ loginStartup: 'disabled', runningCompanion: 'unchanged' }); return 0; }
    if (!removed.removed) { output.result(`${name} wasn't set to open at login.`, 'off'); return 0; }
    output.result(`${name} won't open at login anymore.`, 'ok');
    if ((await companionStatus(options.stateDir, options.appId).catch(() => undefined))?.running) output.detail('It stays in the menu bar until you quit it.');
    return 0;
  }

  async function start(): Promise<number> {
    const diagnostics = diagnosePlatform();
    const blocking = diagnostics.filter((d: PlatformDiagnostic) => d.severity === 'error');
    if (blocking.length) {
      if (json) { write({ running: false, diagnostics }); return 1; }
      output.error({ message: `${name} can't show a menu bar here: ${blocking[0]!.message}`, next: `${command} doctor` });
      return 1;
    }
    if ((await companionStatus(options.stateDir, options.appId)).running) {
      if (json) write({ running: true, alreadyRunning: true });
      else output.result(`${name} is already in the menu bar.`, 'on');
      return 0;
    }
    // Download/verify in the visible CLI before detaching, so failures remain actionable.
    if (!options.binary) {
      const manifest = options.manifest ?? await packagedManifest();
      let done = () => {};
      try {
        const current = await inspectBinary({ manifest, cacheDir: options.cacheDir });
        if (!current.installed) done = output.progress(`Downloading the menu bar helper (${formatBytes(current.size)})…`);
      } catch { /* ensureBinary reports the real problem */ }
      try { await ensureBinary({ manifest, cacheDir: options.cacheDir, ...(invocation.fetch ? { fetch: invocation.fetch } : {}) }); }
      finally { done(); }
    }
    const result = await startCompanion({ appId: options.appId, stateDir: options.stateDir, ...invocation.foreground });
    if (json) { write(result); return 0; }
    output.result(`${name} is in your menu bar.`, 'ok');
    const login = await loginState();
    if (login === 'off') output.next(`${command} install`);
    return 0;
  }
  return 1;
}
