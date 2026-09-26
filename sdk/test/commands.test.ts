import { test, type TestContext } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { chmod, mkdtemp, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Audience } from '../src/audience.js';
import { createCliOutput } from '../src/cli-style.js';
import { companionHelp, describeCompanionError, handleCompanionCommand, type CompanionInvocation } from '../src/commands.js';
import type { CompanionOptions } from '../src/client.js';
import { CompanionError } from '../src/errors.js';
import { loadLoginEnvironment, loginEnvironmentPath, saveLoginEnvironment } from '../src/login-env.js';
import type { PermissionIO } from '../src/permissions.js';
import { resolveTarget } from '../src/platform.js';

const UTF8 = { LANG: 'en_US.UTF-8' };
const posixOnly = process.platform === 'win32' ? 'login items on Windows need a native .exe' : false;

async function setup(t: TestContext, overrides: Partial<CompanionOptions> = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'companion-commands-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const options: CompanionOptions = {
    appId: 'sponge', name: 'Sponge', mark: { symbol: 'mark.drop', letters: 'Sp' },
    stateDir: join(dir, 'state'), cacheDir: join(dir, 'cache'), binary: join(dir, 'runner'),
    snapshot: () => [], onAction: () => {}, ...overrides,
  };
  const run = async (args: string[], extra: { audience?: Audience; tty?: boolean; env?: NodeJS.ProcessEnv } & Partial<CompanionInvocation> = {}) => {
    let stdout = '', stderr = '';
    const results: unknown[] = [];
    const notices: string[] = [];
    // Color is checked on its own below; everything else compares plain text.
    const env = { ...UTF8, NO_COLOR: '1', ...extra.env };
    const tty = extra.tty ?? true;
    const io: PermissionIO = { env, stdinIsTTY: tty, stderrIsTTY: tty, write: text => { notices.push(text); }, readKey: async () => 'timeout', openUrl: async () => false };
    const output = createCliOutput({
      audience: extra.audience ?? 'human', env,
      stdout: { write: (text: string) => { stdout += text; }, isTTY: tty },
      stderr: { write: (text: string) => { stderr += text; }, isTTY: tty },
    });
    const code = await handleCompanionCommand(options, {
      args, command: 'sponge menubar', foreground: { executable: '/opt/tools/bun', args: ['/opt/sponge/cli.js', 'menubar', '--foreground'] },
      write: result => { results.push(result); }, audience: extra.audience ?? 'human', output, permissionIO: io, home: dir, env,
      ...(extra.fetch ? { fetch: extra.fetch } : {}),
      ...(extra.foreground ? { foreground: extra.foreground } : {}),
    });
    return { code, stdout, stderr, results, notices: notices.join('') };
  };
  return { dir, options, run };
}

test('help, per-command help and bare help exit 0 and fit the terminal', async t => {
  const { run } = await setup(t);
  for (const args of [['help'], ['--help'], ['-h'], ['install', '--help'], ['status', '-h']]) {
    const result = await run(args);
    assert.equal(result.code, 0, args.join(' '));
    assert.equal(result.stdout, companionHelp('Sponge', 'sponge menubar'));
    assert.equal(result.stderr, '');
  }
  const lines = companionHelp('Sponge', 'sponge menubar').trimEnd().split('\n');
  assert.ok(lines.length <= 25 && lines.every(line => line.length <= 80));
  assert.equal(lines[0], 'Usage: sponge menubar [command] [--json]');
});

test('an unknown command is one line and one next step, exit 2', async t => {
  const { run } = await setup(t);
  const human = await run(['frobnicate']);
  assert.equal(human.code, 2);
  assert.equal(human.stderr, '✗ Unknown menu bar command "frobnicate".\n→ sponge menubar --help\n');
  const json = await run(['frobnicate', '--json']);
  assert.equal(json.code, 2);
  assert.deepEqual(json.results, [{ ok: false, error: { code: 'usage', message: 'Unknown menu bar command "frobnicate".', next: 'sponge menubar --help' } }]);
  assert.equal(json.stdout + json.stderr, '');
});

test('status is text for people, JSON with --json or for agents', async t => {
  const { run } = await setup(t);
  const human = await run(['status']);
  assert.equal(human.code, 0);
  assert.equal(human.stdout, "○ Sponge isn't in the menu bar\n○ Doesn't open at login\n");
  assert.equal(human.stderr, 'Next: sponge menubar\n');
  const quiet = await run(['status'], { audience: 'quiet', tty: false });
  assert.equal(quiet.stdout, "○ Sponge isn't in the menu bar\n○ Doesn't open at login\n");
  assert.equal(quiet.stderr, '');
  const dumb = await run(['status'], { env: { TERM: 'dumb' } });
  assert.equal(dumb.stdout, "o Sponge isn't in the menu bar\no Doesn't open at login\n");
  const colored = await run(['frobnicate'], { env: { NO_COLOR: '' } });
  assert.equal(colored.stderr, '\u001b[31m✗\u001b[0m Unknown menu bar command "frobnicate".\n\u001b[2m→\u001b[0m sponge menubar --help\n');
  const noColor = await run(['status'], { env: { NO_COLOR: '1', FORCE_COLOR: '1' } });
  assert.ok(!noColor.stdout.includes('\u001b['));
  for (const extra of [{ args: ['status', '--json'] }, { args: ['status'], audience: 'agent' as const }]) {
    const json = await run(extra.args, extra);
    assert.deepEqual(json.results, [{ running: false, appId: 'sponge', state: 'stopped' }]);
    assert.equal(json.stdout + json.stderr, '');
  }
});

test('install shows the login item notice once, saves login credentials, uninstall removes both', { skip: posixOnly }, async t => {
  const { run, options, dir } = await setup(t, { loginEnv: ['SPONGE_API_TOKEN'] });
  const first = await run(['install'], { env: { SPONGE_API_TOKEN: 'secret-token' } });
  assert.equal(first.code, 0);
  assert.equal(first.notices, "🔐 macOS will show a notice that bun can open at login. That's Sponge's menu bar.\n   Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions.\n");
  assert.match(first.stdout, /^✓ Sponge will open at login\.\n/);
  assert.match(first.stdout, /  Saved SPONGE_API_TOKEN in a private file so it starts signed in\.\n$/);
  assert.ok(!first.stdout.includes('secret-token') && !first.stderr.includes('secret-token'));
  if (process.platform === 'darwin') {
    const plist = join(dir, 'Library', 'LaunchAgents', 'app.hraness.companion.sponge.plist');
    assert.ok(existsSync(plist));
    assert.equal((await stat(loginEnvironmentPath(options.stateDir))).mode & 0o777, 0o600);
  }
  const again = await run(['install'], { env: { SPONGE_API_TOKEN: 'secret-token' } });
  assert.equal(again.notices, '');
  assert.match(again.stdout, /^● Sponge already opens at login\.\n/);
  const missing = await run(['install', '--json'], { env: {} });
  assert.deepEqual(missing.results, [{ loginStartup: 'enabled', takesEffect: 'next-login', requirements: (missing.results[0] as { requirements: string[] }).requirements, loginEnv: { saved: [], missing: ['SPONGE_API_TOKEN'] } }]);
  assert.equal(existsSync(loginEnvironmentPath(options.stateDir)), false);
  const warn = await run(['install'], { env: {} });
  assert.match(warn.stderr, /⚠ SPONGE_API_TOKEN isn't set here, so Sponge will start signed out at login\.\nNext: set SPONGE_API_TOKEN, then run sponge menubar install again\n$/);
  await run(['install'], { env: { SPONGE_API_TOKEN: 'secret-token' } });
  const removed = await run(['uninstall']);
  assert.equal(removed.stdout, "✓ Sponge won't open at login anymore.\n");
  assert.equal(existsSync(loginEnvironmentPath(options.stateDir)), false);
  const none = await run(['uninstall']);
  assert.equal(none.stdout, "○ Sponge wasn't set to open at login.\n");
  const agent = await run(['install'], { audience: 'agent' });
  assert.equal(agent.notices, '{"type":"permission-notice","product":"Sponge","kind":"login-item","message":"macOS will show a notice that bun can open at login. That\'s Sponge\'s menu bar. Its menu bar icon opens when you log in. Nothing else runs in the background. Turn it off any time in System Settings › General › Login Items & Extensions."}\n');
});

test('login credentials load only into unset variables and only from a private file', { skip: process.platform === 'win32' ? 'POSIX file modes' : false }, async t => {
  const dir = await mkdtemp(join(tmpdir(), 'login-env-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  assert.deepEqual(await saveLoginEnvironment(dir, ['A_TOKEN', 'B_TOKEN'], { A_TOKEN: 'a', B_TOKEN: '' }), { saved: ['A_TOKEN'], missing: ['B_TOKEN'] });
  const env: NodeJS.ProcessEnv = {};
  assert.deepEqual(await loadLoginEnvironment(dir, ['A_TOKEN', 'B_TOKEN'], env), ['A_TOKEN']);
  assert.equal(env.A_TOKEN, 'a');
  const set: NodeJS.ProcessEnv = { A_TOKEN: 'from terminal' };
  assert.deepEqual(await loadLoginEnvironment(dir, ['A_TOKEN'], set), []);
  assert.equal(set.A_TOKEN, 'from terminal');
  assert.deepEqual(await loadLoginEnvironment(dir, ['OTHER'], {}), []);
  await chmod(loginEnvironmentPath(dir), 0o644);
  assert.deepEqual(await loadLoginEnvironment(dir, ['A_TOKEN'], {}), []);
  await rm(loginEnvironmentPath(dir));
  await writeFile(join(dir, 'elsewhere.json'), JSON.stringify({ version: 1, env: { A_TOKEN: 'x' } }), { mode: 0o600 });
  await symlink(join(dir, 'elsewhere.json'), loginEnvironmentPath(dir));
  assert.deepEqual(await loadLoginEnvironment(dir, ['A_TOKEN'], {}), []);
  await assert.rejects(saveLoginEnvironment(dir, ['BAD NAME'], {}), /invalid-login-env-name/);
});

test('doctor prints checks, a count and JSON on request', async t => {
  const { run } = await setup(t);
  const human = await run(['doctor']);
  assert.match(human.stdout, /– Using a local test build of the menu bar helper\n○ Sponge isn't in the menu bar\n○ Doesn't open at login\n\n/);
  assert.ok(!human.stdout.includes('sha256') && !human.stdout.includes('{'));
  if (process.platform === 'darwin') {
    assert.equal(human.code, 0);
    assert.match(human.stdout, /^✓ This computer can show menu bar icons\n/);
    assert.match(human.stdout, /\nEverything looks good\.\n$/);
  }
  const json = await run(['doctor', '--json']);
  const [result] = json.results as [{ artifact: { source: string }; signing: string }];
  assert.equal(result.artifact.source, 'maintainer-override');
  assert.equal(result.signing, 'unsigned');
});

test('start shows one download progress line, then a plain error with one next step', { skip: process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY ? 'needs a graphical session' : false }, async t => {
  const bytes = Buffer.from('fake runner bytes');
  const target = resolveTarget();
  const { run } = await setup(t, {
    binary: undefined,
    manifest: { schemaVersion: 1, version: '0.8.0', tag: 'v0.8.0', repository: 'hraness/desktop-foundation', assets: [{ target, name: `hraness-companion-${target}${target.includes('windows') ? '.exe' : ''}`, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }] },
  });
  const fetches: string[] = [];
  const fetch = (async (url: URL) => { fetches.push(String(url)); return new Response(bytes, { headers: { 'content-length': String(bytes.length) } }); }) as unknown as typeof globalThis.fetch;
  const failing = { executable: process.execPath, args: ['-e', 'process.exit(3)'] };
  const first = await run([], { fetch, foreground: failing });
  assert.equal(fetches.length, 1);
  assert.match(first.stderr, /^↻ Downloading the menu bar helper \(17 bytes\)…\r\u001b\[2K/);
  assert.match(first.stderr, /✗ Sponge couldn't start its menu bar\.\n→ sponge menubar doctor\n$/);
  assert.equal(first.code, 1);
  const json = await run(['--json'], { fetch, foreground: failing });
  assert.equal(json.stderr, '');
  assert.deepEqual(json.results, [{ ok: false, error: { code: 'companion-start-failed', message: "Sponge couldn't start its menu bar.", next: 'sponge menubar doctor' } }]);
});

test('lifecycle errors become one sentence and one next step', () => {
  const cases: Array<[unknown, string, string, string]> = [
    [new CompanionError('download_failed', 'HTTP 503'), 'download_failed', "Couldn't download the menu bar helper. Check your internet connection.", 'x menubar'],
    [new CompanionError('autostart_conflict', 'x'), 'autostart_conflict', "A login item with Sponge's name already exists that Sponge didn't create, so it was left alone.", 'x menubar doctor'],
    [new Error('stop-indeterminate: service receipt exists but the owner is unreachable'), 'stop-indeterminate', "Couldn't confirm that Sponge left the menu bar.", 'x menubar status'],
    [new Error('companion-start-timeout: check doctor and the foreground command'), 'companion-start-timeout', "Sponge didn't appear in the menu bar in time.", 'x menubar doctor'],
    [new Error('Weird Internal Thing'), 'companion-failed', "Something went wrong with Sponge's menu bar.", 'x menubar doctor'],
    ['not an error', 'companion-failed', "Something went wrong with Sponge's menu bar.", 'x menubar doctor'],
  ];
  for (const [error, code, message, next] of cases) assert.deepEqual(describeCompanionError(error, 'Sponge', 'x menubar'), { code, message, next });
});
