import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { chmod, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as root from '../src/index.js';
import { loadLoginEnvironment, loginEnvironmentPath, saveLoginEnvironment } from '../src/login-env.js';
import { MAX_FRAME_BYTES, type NoticeRequest } from '../src/notice.js';
import type { PromptRequest } from '../src/prompt.js';

// Compiled to dist/test/package.test.js; the repository root is two levels up.
const repo = new URL('../../', import.meta.url);
const pkg = JSON.parse(readFileSync(new URL('package.json', repo), 'utf8')) as {
  exports: Record<string, unknown>; files: string[]; bin?: unknown;
};

test('1.0 exports no menu kit and no tray API', () => {
  assert.deepEqual(Object.keys(pkg.exports).sort(), [
    '.', './audience', './cli-style', './control', './helper', './human-gate', './login', './permissions', './registry', './retire', './tui',
  ]);
  assert.equal(pkg.bin, undefined, 'the companion CLI was removed');
  for (const name of [
    'menuKit', 'layout', 'lintMenu', 'assertMenuFixture', 'renderMenuTree', 'openAtLoginItem', 'degradedMenu', 'actionErrorItem', 'MenuActionError',
    'runCompanion', 'startCompanion', 'stopCompanion', 'companionStatus', 'serveCompanion', 'handleCompanionCommand', 'describeCompanionError', 'companionHelp',
    'validateSnapshot', 'validateSnapshotV2', 'downlevelSnapshot', 'parseRunnerProtocols', 'runnerProtocols', 'openBrowser', 'permissionMenuItems',
  ]) assert.equal(name in root, false, `${name} is still exported`);
  const dist = new URL('dist/src/', repo);
  assert.equal(existsSync(new URL('menu-kit.js', dist)), false, 'a stale menu-kit build would ship through files: dist/src');
  assert.deepEqual(readdirSync(new URL('sdk/src/', repo)).filter(name => /menu|protocol|client|service|commands|browser|cli\.ts/.test(name)), []);
});

test('the root keeps the installer, manifest and prompt API', () => {
  for (const name of ['packagedManifest', 'parseReleaseManifest', 'ensureBinary', 'inspectBinary', 'userPaths', 'diagnosePlatform', 'promptNative', 'promptTui', 'formatNotice', 'loadLoginEnvironment', 'saveLoginEnvironment']) {
    assert.equal(typeof (root as Record<string, unknown>)[name], 'function', `${name} missing`);
  }
  assert.equal(MAX_FRAME_BYTES, 256 * 1024);
  const notice: NoticeRequest = { type: 'notice-request', version: 1, title: 't', message: 'm', primary: 'OK' };
  const prompt: PromptRequest = { title: 't', message: 'm', secret: true };
  assert.equal(notice.type, 'notice-request');
  assert.equal(prompt.secret, true);
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
