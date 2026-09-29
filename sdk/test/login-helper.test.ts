import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chmod, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { installLoginItem, planLoginItem, uninstallLoginItem } from '../src/login.js';
import { resolveHelper } from '../src/helper.js';
import { parseReleaseManifest, type ReleaseManifest } from '../src/install.js';
import { resolveTarget } from '../src/platform.js';

test('owner login item: opt-in, idempotent, removed only when ours', async () => {
  if (process.platform !== 'darwin' && process.platform !== 'linux') return;
  const home = await mkdtemp(join(tmpdir(), 'login-'));
  const opts = { product: 'example', program: '/usr/local/bin/example', args: ['control', 'serve'], home, env: { XDG_CONFIG_HOME: join(home, '.config') } };
  try {
    const plan = planLoginItem(opts);
    assert.match(plan.path, process.platform === 'darwin' ? /LaunchAgents\/app\.hraness\.companion\.example-owner\.plist$/ : /autostart\/hraness-companion-example-owner\.desktop$/);
    assert.equal((await installLoginItem(opts)).changed, true);
    assert.equal((await installLoginItem(opts)).changed, false);
    assert.match(await readFile(plan.path, 'utf8'), /control/);
    assert.equal((await uninstallLoginItem(opts)).changed, true);
    assert.equal((await uninstallLoginItem(opts)).changed, false);
    await writeFile(plan.path, 'edited by hand');
    await assert.rejects(uninstallLoginItem(opts), /autostart file/);
    assert.equal(await readFile(plan.path, 'utf8'), 'edited by hand');
  } finally { await rm(home, { recursive: true, force: true }); }
  assert.throws(() => planLoginItem({ product: 'Bad/Name', program: '/x' }), /product/);
});

function manifest(helper: boolean, bytes: Buffer): ReleaseManifest {
  const target = resolveTarget();
  const exe = target.includes('windows') ? '.exe' : '';
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  return parseReleaseManifest(JSON.stringify({
    schemaVersion: 1, version: '0.9.0', repository: 'hraness/desktop-foundation', tag: 'v0.9.0',
    assets: [{ target, name: `hraness-companion-${target}${exe}`, size: bytes.length, sha256 }],
    ...(helper ? { helperAssets: [{ target, name: `hraness-helper-${target}${exe}`, size: bytes.length, sha256 }] } : {}),
  }));
}

test('manifest: helperAssets is optional, validated and named hraness-helper-<target>', () => {
  const bytes = Buffer.from('#!/bin/sh\n');
  assert.equal(manifest(false, bytes).helperAssets, undefined);
  assert.equal(manifest(true, bytes).helperAssets?.length, 1);
  const good = JSON.parse(JSON.stringify(manifest(true, bytes)));
  for (const bad of [
    { ...good, helperAssets: [] },
    { ...good, helperAssets: [{ ...good.helperAssets[0], name: good.assets[0].name }] },
    { ...good, helperAssets: [{ ...good.helperAssets[0], extra: 1 }] },
    { ...good, helperAssets: [good.helperAssets[0], good.helperAssets[0]] },
    { ...good, other: [] },
  ]) assert.throws(() => parseReleaseManifest(JSON.stringify(bad)), /manifest|asset|target/i);
});

test('resolveHelper: override, then helper asset, then the companion alias', { skip: process.platform === 'win32' ? 'symlinks and execute bits differ on Windows' : false }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'helper-'));
  try {
    const exe = join(dir, 'hraness-helper');
    await writeFile(exe, '#!/bin/sh\n');
    await chmod(exe, 0o755);
    assert.deepEqual(await resolveHelper({ override: exe }), { path: exe, source: 'override' });
    await assert.rejects(resolveHelper({ override: 'relative/helper' }), /absolute/);
    await symlink(exe, join(dir, 'link'));
    await assert.rejects(resolveHelper({ override: join(dir, 'link') }), /regular file/);
    await chmod(exe, 0o644);
    await assert.rejects(resolveHelper({ override: exe }), /not executable/);
    await assert.rejects(resolveHelper({}), /manifest/);

    const bytes = Buffer.from('#!/bin/sh\necho helper\n');
    const urls: string[] = [];
    const fetch = (async (url: URL) => { urls.push(String(url)); return new Response(bytes, { status: 200, headers: { 'content-length': String(bytes.length) } }); }) as unknown as typeof globalThis.fetch;
    const cacheDir = join(dir, 'cache');
    const helper = await resolveHelper({ manifest: manifest(true, bytes), cacheDir, fetch });
    assert.equal(helper.source, 'helper');
    assert.ok(helper.path.endsWith(`hraness-helper-${resolveTarget()}${process.platform === 'win32' ? '.exe' : ''}`), helper.path);
    const companion = await resolveHelper({ manifest: manifest(false, bytes), cacheDir, fetch });
    assert.equal(companion.source, 'companion');
    assert.match(companion.path, /hraness-companion-/);
    assert.deepEqual(urls.map(u => u.split('/').pop()!.split('-').slice(0, 2).join('-')), ['hraness-helper', 'hraness-companion']);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
