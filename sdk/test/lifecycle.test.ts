import { afterEach, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chmod, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { ensureBinary, parseReleaseManifest, type ReleaseManifest } from '../src/install.js';
import { diagnosePlatform, resolveTarget, userPaths, TARGETS } from '../src/platform.js';
import { planAutostart, removeAutostart, setAutostart } from '../src/autostart.js';

const scratch: string[] = [];
async function temp() { const dir = await mkdtemp(join(tmpdir(), 'companion-test-')); scratch.push(dir); return dir; }
afterEach(async () => { await Promise.all(scratch.splice(0).map(p => rm(p, { recursive: true, force: true }))); });
const bytes = new TextEncoder().encode('synthetic-test-binary-never-executed');
const target = 'aarch64-apple-darwin' as const;
function manifest(): ReleaseManifest { return { schemaVersion: 1, version: '0.5.0', tag: 'v0.5.0', repository: 'hraness/desktop-foundation', assets: [{ target, name: `hraness-companion-${target}`, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }] }; }
function responseFetch(response: () => Response): typeof fetch { return (async () => response()) as typeof fetch; }

describe('pinned companion installation', () => {
  test('installs verified bytes and revalidates before offline reuse', async () => {
    const cacheDir = await temp();
    let calls = 0;
    const fetcher = responseFetch(() => { calls++; return new Response(bytes); });
    const first = await ensureBinary({ manifest: manifest(), target, cacheDir, fetch: fetcher });
    assert.equal(first.reused, false);
    assert.deepEqual(await readFile(first.path), Buffer.from(bytes));
    assert.equal((await ensureBinary({ manifest: manifest(), target, cacheDir, fetch: fetcher })).reused, true);
    assert.equal(calls, 1);
    await writeFile(first.path, 'corrupted');
    await assert.rejects(ensureBinary({ manifest: manifest(), target, cacheDir, fetch: fetcher }), { code: 'integrity_failed' });
    assert.equal(calls, 1);
  });
  test('concurrent installers publish one valid file and clean partial files', async () => {
    const options = { manifest: manifest(), target, cacheDir: await temp(), fetch: responseFetch(() => new Response(bytes)) };
    const results = await Promise.all(Array.from({ length: 6 }, () => ensureBinary(options)));
    assert.equal(new Set(results.map(r => r.path)).size, 1);
    assert.equal(results.filter(r => !r.reused).length, 1);
    assert.deepEqual(await readdir(dirname(results[0]!.path)), [manifest().assets[0]!.name]);
  });
  test('rejects corrupt and oversized streams without publishing', async () => {
    for (const payload of [new Uint8Array(bytes.length).fill(1), new Uint8Array(bytes.length + 1)]) {
      const cacheDir = await temp();
      await assert.rejects(ensureBinary({ manifest: manifest(), target, cacheDir, fetch: responseFetch(() => new Response(payload)) }), { code: 'integrity_failed' });
      assert.deepEqual(await readdir(join(cacheDir, 'hraness', 'desktop-foundation', '0.5.0', target)), []);
    }
  });
  test('follows approved GitHub asset redirect but rejects other hosts', async () => {
    let count = 0;
    const accepted = responseFetch(() => ++count === 1 ? new Response(null, { status: 302, headers: { location: 'https://release-assets.githubusercontent.com/test' } }) : new Response(bytes));
    await ensureBinary({ manifest: manifest(), target, cacheDir: await temp(), fetch: accepted });
    assert.equal(count, 2);
    await assert.rejects(ensureBinary({ manifest: manifest(), target, cacheDir: await temp(), fetch: responseFetch(() => new Response(null, { status: 302, headers: { location: 'https://example.com/binary' } })) }), { code: 'download_failed' });
  });
  test('rejects relative cache and symbolic link or junction cache/ancestors', async () => {
    const options = { manifest: manifest(), target, fetch: responseFetch(() => new Response(bytes)) };
    await assert.rejects(ensureBinary({ ...options, cacheDir: './cache' }), { code: 'unsafe_path' });
    const root = await temp(); const actual = await temp();
    await symlink(actual, join(root, 'cache'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(ensureBinary({ ...options, cacheDir: join(root, 'cache') }), { code: 'unsafe_path' });
    await assert.rejects(ensureBinary({ ...options, cacheDir: join(root, 'cache', 'nested') }), { code: 'unsafe_path' });
    await symlink(actual, join(root, 'hraness'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(ensureBinary({ ...options, cacheDir: root }), { code: 'unsafe_path' });
  });
  test('rejects nonprivate cache and group-writable executable', { skip: process.platform === 'win32' }, async () => {
    const cacheDir = await temp();
    const options = { manifest: manifest(), target, cacheDir, fetch: responseFetch(() => new Response(bytes)) };
    await chmod(cacheDir, 0o755);
    await assert.rejects(ensureBinary(options), { code: 'unsafe_path' });
    await chmod(cacheDir, 0o700);
    const result = await ensureBinary(options);
    await chmod(result.path, 0o775);
    await assert.rejects(ensureBinary(options), { code: 'unsafe_path' });
  });
  test('requires fixed tag, strict asset names and fields, bounded manifest and explicit target', async () => {
    assert.deepEqual(parseReleaseManifest(JSON.stringify(manifest())), manifest());
    for (const m of [ { ...manifest(), tag: 'latest' }, { ...manifest(), repository: 'owner/..' }, { ...manifest(), surprise: 1 }, { ...manifest(), assets: [{ ...manifest().assets[0], name: '../other' }] } ]) {
      assert.throws(() => parseReleaseManifest(JSON.stringify(m)));
    }
    assert.throws(() => parseReleaseManifest(' '.repeat(65537)));
    await assert.rejects(ensureBinary({ manifest: manifest(), target: 'x86_64-pc-windows-msvc', cacheDir: await temp() }), { code: 'unsupported_target' });
  });
});

describe('platform diagnostics and opt-in autostart', () => {
  test('maps every supported architecture without claiming unsupported platforms', () => {
    const targets = ['darwin', 'win32', 'linux'].flatMap(platform => ['arm64', 'x64'].map(arch => resolveTarget({ platform: platform as NodeJS.Platform, arch })));
    assert.deepEqual(new Set(targets), new Set(TARGETS));
    assert.throws(() => resolveTarget({ platform: 'freebsd', arch: 'x64' }));
    assert.throws(() => resolveTarget({ platform: 'linux', arch: 'ia32' }));
    assert.ok(diagnosePlatform({ platform: 'linux', arch: 'x64', env: {} }).some(d => d.code === 'graphical_session_missing'));
    assert.ok(!diagnosePlatform({ platform: 'linux', arch: 'x64', env: { DISPLAY: ':0', DBUS_SESSION_BUS_ADDRESS: 'unix:fake' } }).some(d => d.code === 'graphical_session_missing'));
  });
  test('uses platform user paths and rejects relative XDG overrides', () => {
    assert.equal(userPaths({ platform: 'linux', home: '/home/u', env: { XDG_CACHE_HOME: 'relative' } }).cacheDir, '/home/u/.cache/hraness-companion');
    assert.equal(userPaths({ platform: 'darwin', home: '/Users/u', env: {} }).cacheDir, '/Users/u/Library/Caches/hraness-companion');
    assert.equal(userPaths({ platform: 'win32', home: 'C:\\Users\\u', env: {} }).cacheDir, 'C:\\Users\\u\\AppData\\Local\\hraness-companion\\Cache');
  });
  test('renders macOS program argv separately with XML escaping', () => {
    const plan = planAutostart({ id: 'example', label: 'Example', platform: 'darwin', home: '/Users/u', executable: '/opt/Example & tools/cli', args: ['tray', '<value>'] });
    assert.ok(plan.contents.includes('<string>/opt/Example &amp; tools/cli</string><string>tray</string><string>&lt;value&gt;</string>'));
    assert.ok(!plan.contents.includes('KeepAlive'));
    assert.equal(plan.activation, 'next-login');
  });
  test('renders Windows startup without cmd and refuses environment expansion', () => {
    const plan = planAutostart({ id: 'example', label: 'Example', platform: 'win32', home: 'C:\\Users\\u', env: {}, executable: 'C:\\Program Files\\Example\\cli.exe', args: ['tray', 'x & calc.exe'] });
    assert.ok(plan.path.endsWith('Startup\\hraness-companion-example.vbs'));
    assert.ok(plan.contents.includes('""x & calc.exe""'));
    assert.ok(!plan.contents.includes('cmd.exe'));
    assert.throws(() => planAutostart({ id: 'example', label: 'Example', platform: 'win32', home: 'C:\\Users\\u', executable: 'C:\\%USERNAME%\\cli.exe' }));
    assert.throws(() => planAutostart({ id: 'example', label: 'Example', platform: 'win32', home: 'C:\\Users\\u', executable: 'C:\\tools\\cli.cmd' }));
    const unicode = planAutostart({ id: 'example', label: 'Example', platform: 'win32', home: 'C:\\Users\\Zoë', executable: 'C:\\Users\\Zoë\\cli.exe', args: ['你好'] });
    assert.ok(/^[\x00-\x7f]*$/.test(unicode.contents));
    assert.ok(unicode.contents.includes('ChrW(235)'));
  });
  test('escapes desktop field codes and rejects executable/control injection', () => {
    const plan = planAutostart({ id: 'example', label: 'Example', platform: 'linux', home: '/home/u', env: {}, executable: '/usr/local/bin/cli', args: ['%u', '$HOME', 'a"b'] });
    assert.ok(plan.contents.includes('"%%u"'));
    assert.ok(plan.contents.includes('"\\\\$HOME"'));
    assert.throws(() => planAutostart({ id: '../escape', label: 'X', platform: 'linux', home: '/home/u', executable: '/bin/true' }));
    assert.throws(() => planAutostart({ id: 'example', label: 'X\nExec=bad', platform: 'linux', home: '/home/u', executable: '/bin/true' }));
    assert.throws(() => planAutostart({ id: 'example', label: 'X', platform: 'linux', home: '/home/u', executable: 'relative' }));
  });
  test('installs only on request, updates owned files, and preserves manually edited files', async () => {
    const home = await temp();
    const executable = process.execPath;
    const plan = planAutostart({ id: 'example', label: 'Example', home, env: {}, executable, args: ['tray'] });
    assert.equal((await setAutostart(plan)).changed, true);
    assert.equal((await setAutostart(plan)).changed, false);
    const updated = planAutostart({ id: 'example', label: 'Example', home, env: {}, executable, args: ['tray', 'new'] });
    assert.equal((await setAutostart(updated)).changed, true);
    await writeFile(plan.path, plan.contents + '\nUser edit\n');
    await assert.rejects(removeAutostart(plan), { code: 'autostart_conflict' });
    await assert.rejects(setAutostart(updated), { code: 'autostart_conflict' });
    await writeFile(plan.path, plan.contents);
    assert.equal((await removeAutostart(plan)).removed, true);
    assert.equal((await removeAutostart(plan)).removed, false);
  });
});
