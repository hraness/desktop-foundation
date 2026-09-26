import { afterEach, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { autostartState, planAutostart, removeAutostart, setAutostart, type AutostartOptions } from '../src/autostart.js';
import { describeCompanionError } from '../src/commands.js';
import { CompanionError } from '../src/errors.js';

const scratch: string[] = [];
async function temp() { const dir = await mkdtemp(join(tmpdir(), 'companion-app-')); scratch.push(dir); return dir; }
afterEach(async () => { await Promise.all(scratch.splice(0).map(p => rm(p, { recursive: true, force: true }))); });
const darwinOnly = process.platform === 'darwin' ? false : 'writes a macOS LaunchAgent';

function appOptions(home: string, overrides: Partial<AutostartOptions> = {}): AutostartOptions {
  return {
    id: 'textbutler', label: 'Textbutler', platform: 'darwin', home, env: {},
    executable: '/opt/tools/bun', args: ['/opt/textbutler/cli.js', 'menubar', '--foreground'],
    app: { name: 'Textbutler', argvFile: join(home, 'state', 'launch.json') },
    ...overrides,
  };
}
async function buildApp(home: string, name = 'Textbutler') {
  const macos = join(home, 'Applications', 'Hraness', `${name}.app`, 'Contents', 'MacOS');
  await mkdir(macos, { recursive: true });
  await writeFile(join(macos, name), 'fake runner, never executed', { mode: 0o755 });
}

describe('login items that start the product app', () => {
  test('match the Rust LaunchAgent byte for byte', () => {
    const plan = planAutostart(appOptions('/Users/u', { app: { name: 'Textbutler', argvFile: '/Users/u/Library/Application Support/textbutler/launch.json' } }));
    const body = '<plist version="1.0"><dict>\n'
      + '<key>Label</key><string>app.hraness.textbutler</string>\n'
      + '<key>ProgramArguments</key><array><string>/Users/u/Applications/Hraness/Textbutler.app/Contents/MacOS/Textbutler</string><string>--launch</string><string>/Users/u/Library/Application Support/textbutler/launch.json</string></array>\n'
      + '<key>AssociatedBundleIdentifiers</key><array><string>app.hraness.textbutler</string></array>\n'
      + '<key>RunAtLoad</key><true/>\n'
      + '<key>LimitLoadToSessionType</key><string>Aqua</string>\n'
      + '<key>ProcessType</key><string>Interactive</string>\n'
      + '</dict></plist>\n';
    const sha = createHash('sha256').update(body).digest('hex');
    assert.equal(plan.path, '/Users/u/Library/LaunchAgents/app.hraness.textbutler.plist');
    assert.equal(plan.contents, `<!-- hraness-companion autostart textbutler sha256:${sha} -->\n${body}`);
    assert.deepEqual(plan.legacy, ['/Users/u/Library/LaunchAgents/app.hraness.companion.textbutler.plist']);
    // The product command lives in the launch file, never in the plist.
    assert.deepEqual(plan.launch, {
      path: '/Users/u/Library/Application Support/textbutler/launch.json',
      contents: '["/opt/tools/bun","/opt/textbutler/cli.js","menubar","--foreground"]',
      program: '/Users/u/Applications/Hraness/Textbutler.app/Contents/MacOS/Textbutler',
    });
    assert.ok(!plan.contents.includes('/opt/tools/bun'));
  });

  test('keeps the old file on Windows and Linux', () => {
    const plan = planAutostart(appOptions('/home/u', { platform: 'linux' }));
    assert.equal(plan.launch, undefined);
    assert.equal(plan.legacy, undefined);
    assert.ok(plan.path.endsWith('/autostart/hraness-companion-textbutler.desktop'));
  });

  test('refuse unsafe app names and launch files', () => {
    for (const name of ['', '.hidden', 'a/b', 'a:b', 'x\ny', 'x'.repeat(129)]) {
      assert.throws(() => planAutostart(appOptions('/Users/u', { app: { name, argvFile: '/Users/u/launch.json' } })), { code: 'unsafe_path' }, name);
    }
    assert.throws(() => planAutostart(appOptions('/Users/u', { app: { name: 'Textbutler', argvFile: 'launch.json' } })), { code: 'unsafe_path' });
    assert.throws(() => planAutostart(appOptions('/Users/u', { args: Array.from({ length: 64 }, () => 'a') })), { code: 'unsafe_path' });
    assert.doesNotThrow(() => planAutostart(appOptions('/Users/u', { args: Array.from({ length: 63 }, () => 'a') })));
  });

  test('replace the old login entry, write an owner-only launch file, and report drift', { skip: darwinOnly }, async () => {
    const home = await temp();
    const legacy = planAutostart({ ...appOptions(home), app: undefined });
    await setAutostart(legacy);
    const plan = planAutostart(appOptions(home));
    // The old entry still opens the menu bar at login, so it reads as "outdated", not "off".
    assert.equal(await autostartState(plan), 'outdated');

    await assert.rejects(setAutostart(plan), (error: unknown) => error instanceof CompanionError && error.code === 'app_missing' && error.message === `Textbutler.app is not built in ${join(home, 'Applications', 'Hraness')}.`);
    assert.ok(existsSync(legacy.path), 'a failed switch keeps the old entry');

    await buildApp(home);
    assert.deepEqual(await setAutostart(plan), { path: plan.path, changed: true, activation: 'next-login' });
    assert.equal(await readFile(plan.path, 'utf8'), plan.contents);
    assert.equal(existsSync(legacy.path), false);
    assert.equal(await readFile(plan.launch!.path, 'utf8'), plan.launch!.contents);
    assert.equal((await stat(plan.launch!.path)).mode & 0o777, 0o600);
    assert.equal(await autostartState(plan), 'on');
    assert.equal((await setAutostart(plan)).changed, false);

    await writeFile(plan.launch!.path, '["/bin/sh"]', { mode: 0o600 });
    assert.equal(await autostartState(plan), 'outdated');
    assert.equal((await setAutostart(plan)).changed, true);
    assert.equal(await autostartState(plan), 'on');

    // --launch refuses a launch file others can read; the state says so too.
    const { chmod } = await import('node:fs/promises');
    await chmod(plan.launch!.path, 0o644);
    assert.equal(await autostartState(plan), 'outdated');
    await setAutostart(plan);
    assert.equal((await stat(plan.launch!.path)).mode & 0o777, 0o600);

    assert.deepEqual(await removeAutostart(plan), { path: plan.path, removed: true });
    assert.equal(existsSync(plan.path), false);
    assert.equal(await autostartState(plan), 'off');
    assert.deepEqual(await removeAutostart(plan), { path: plan.path, removed: false });
  });

  test('leave an old entry someone edited, and remove an old entry on uninstall', { skip: darwinOnly }, async () => {
    const home = await temp();
    await buildApp(home);
    const legacy = planAutostart({ ...appOptions(home), app: undefined });
    await setAutostart(legacy);
    const plan = planAutostart(appOptions(home));
    assert.deepEqual(await removeAutostart(plan), { path: plan.path, removed: true });
    assert.equal(existsSync(legacy.path), false);

    await setAutostart(legacy);
    await writeFile(legacy.path, `${legacy.contents}<!-- edited -->\n`);
    await setAutostart(plan);
    assert.ok(existsSync(legacy.path));
    assert.equal(await autostartState(plan), 'on');
  });

  test('refuse a launch file that is a link or a folder', { skip: darwinOnly }, async () => {
    const home = await temp();
    await buildApp(home);
    const plan = planAutostart(appOptions(home));
    await mkdir(plan.launch!.path, { recursive: true });
    await assert.rejects(setAutostart(plan), { code: 'autostart_conflict' });
    await rm(plan.launch!.path, { recursive: true });
    const { symlink } = await import('node:fs/promises');
    await symlink('/etc/hosts', plan.launch!.path);
    await assert.rejects(setAutostart(plan), { code: 'autostart_conflict' });
    assert.equal(existsSync(plan.path), false);
  });

  test('tampered plans are refused before any write', { skip: darwinOnly }, async () => {
    const home = await temp();
    const plan = planAutostart(appOptions(home));
    await assert.rejects(setAutostart({ ...plan, legacy: [join(home, 'Library', 'LaunchAgents', 'com.apple.other.plist')] }), { code: 'unsafe_path' });
    await assert.rejects(setAutostart({ ...plan, launch: { ...plan.launch!, contents: '["relative"]' } }), { code: 'unsafe_path' });
    await assert.rejects(setAutostart({ ...plan, launch: { ...plan.launch!, program: '/usr/bin/true' } }), { code: 'unsafe_path' });
    await assert.rejects(setAutostart({ ...plan, launch: { ...plan.launch!, path: join(home, 'elsewhere.json') } }), { code: 'unsafe_path' });
  });

  test('a missing app reads as one sentence and one next step', () => {
    assert.deepEqual(describeCompanionError(new CompanionError('app_missing', 'x'), 'Textbutler', 'textbutler menubar'), {
      code: 'app_missing', message: "Textbutler.app isn't built yet, so it can't open at login.", next: 'textbutler menubar doctor',
    });
  });
});
