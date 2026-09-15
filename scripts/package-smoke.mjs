import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

const execute = promisify(execFile);
const packageName = '@hraness/desktop-foundation';
const candidates = process.argv.slice(2);
if (!candidates.length) {
  const artifacts = resolve('artifacts');
  for (const name of await readdir(artifacts)) {
    if (/^hraness-desktop-foundation-.*\.tgz$/.test(name)) candidates.push(join(artifacts, name));
  }
}
assert.equal(candidates.length, 1, 'Provide exactly one SDK tarball, or one matching tarball in artifacts/');
const tarball = resolve(candidates[0]);
const scratch = await mkdtemp(join(tmpdir(), 'companion-package-smoke-'));
const home = join(scratch, 'home');
const audit = join(scratch, 'forbidden-operations.log');
const guard = join(scratch, 'guard.mjs');
const probe = join(scratch, 'probe.mjs');
const npmConfig = join(scratch, 'empty.npmrc');
const privatePaths = {
  HOME: home, USERPROFILE: home,
  XDG_CACHE_HOME: join(home, 'cache'), XDG_CONFIG_HOME: join(home, 'config'), XDG_DATA_HOME: join(home, 'data'),
  APPDATA: join(home, 'roaming'), LOCALAPPDATA: join(home, 'local'),
};
const env = { ...process.env, ...privatePaths };
for (const key of Object.keys(env)) if (/^npm_config_/i.test(key)) delete env[key];
env.npm_config_cache = join(scratch, 'npm-cache');
env.npm_config_userconfig = npmConfig;
delete env.HRANESS_COMPANION_BINARY;
delete env.NODE_OPTIONS;
delete env.NODE_PATH;
// Headless packaging workers are expected to report missing display/session.
// Avoid inheriting a developer's actual display or desktop bus in local runs.
delete env.DISPLAY;
delete env.WAYLAND_DISPLAY;
delete env.DBUS_SESSION_BUS_ADDRESS;

try {
  await mkdir(home, { mode: 0o700 });
  await writeFile(npmConfig, '');
  await writeFile(join(scratch, 'package.json'), JSON.stringify({ name: 'companion-package-smoke', version: '1.0.0', private: true, type: 'module' }));
  await writeFile(audit, '');
  // The dependency is the tarball, never a source checkout or workspace link.
  // Offline mode also proves this dependency-free SDK needs no registry fetch.
  await execute('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund', '--offline', '--package-lock=false', '--save-exact', tarball], {
    cwd: scratch, env, timeout: 60_000, maxBuffer: 1024 * 1024,
  });

  // Record even caught network/spawn attempts, so doctor cannot silently try a
  // download and still pass by converting its failure into a diagnostic.
  await writeFile(guard, `
    import { appendFileSync } from 'node:fs';
    import { syncBuiltinESMExports } from 'node:module';
    import http from 'node:http';
    import https from 'node:https';
    import net from 'node:net';
    import tls from 'node:tls';
    import dns from 'node:dns';
    import children from 'node:child_process';
    const audit = ${JSON.stringify(audit)};
    const deny = name => function () {
      appendFileSync(audit, name+'\\n');
      throw new Error('package-smoke-forbidden-'+name);
    };
    globalThis.fetch = deny('fetch');
    if (globalThis.WebSocket) globalThis.WebSocket = deny('websocket');
    for (const [object, names] of [
      [http, ['request', 'get']], [https, ['request', 'get']],
      [net, ['connect', 'createConnection', 'createServer']],
      [net.Socket.prototype, ['connect']], [tls, ['connect', 'createServer']],
      [dns, ['lookup', 'resolve', 'resolve4', 'resolve6']],
      [dns.promises, ['lookup', 'resolve', 'resolve4', 'resolve6']],
      [children, ['spawn', 'spawnSync', 'exec', 'execSync', 'execFile', 'execFileSync', 'fork']],
    ]) for (const name of names) object[name] = deny(name);
    syncBuiltinESMExports();
  `);
  await writeFile(probe, `
    import assert from 'node:assert/strict';
    import { access, lstat, readFile } from 'node:fs/promises';
    import { join } from 'node:path';
    import { pathToFileURL } from 'node:url';
    const root = join(process.cwd(), 'node_modules', '@hraness', 'desktop-foundation');
    assert.equal((await lstat(root)).isSymbolicLink(), false, 'Tarball must install real package bytes');
    const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
    assert.equal(pkg.name, ${JSON.stringify(packageName)});
    for (const file of [
      'release-manifest.json', 'dist/src/index.js', 'dist/src/index.d.ts', 'dist/src/cli.js',
      'README.md', 'LICENSE', 'docs/installation.md', 'docs/platforms.md', 'skills/companion/SKILL.md',
      'docs/architecture.md', 'docs/adoption.md', 'docs/protocol.md',
      'src/lib.rs', 'src/protocol.rs', 'src/bin/hraness-companion.rs', 'src/outputs.rs',
      'sdk/src/protocol.ts', 'sdk/src/client.ts', 'sdk/src/commands.ts',
    ]) await access(join(root, file));
    const sdk = await import(${JSON.stringify(packageName)});
    assert.equal(import.meta.resolve(${JSON.stringify(packageName)}), pathToFileURL(join(root, 'dist/src/index.js')).href);
    for (const name of ['runCompanion', 'startCompanion', 'stopCompanion', 'companionStatus',
      'handleCompanionCommand', 'packagedManifest', 'parseReleaseManifest', 'inspectBinary',
      'diagnosePlatform', 'planAutostart', 'userPaths']) assert.equal(typeof sdk[name], 'function', name);
    const manifest = await sdk.packagedManifest();
    assert.deepEqual(manifest, sdk.parseReleaseManifest(await readFile(join(root, 'release-manifest.json'))));
    assert.equal(manifest.version, pkg.version);
    assert.equal(manifest.tag, 'v'+pkg.version);
    assert.equal(manifest.repository, 'hraness/desktop-foundation');
    assert.deepEqual(manifest.assets.map(asset => asset.target).sort(), [
      'aarch64-apple-darwin', 'x86_64-apple-darwin',
      'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
      'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    ].sort());
    assert.equal(pkg.bin.companion, './dist/src/cli.js');
    const skill = await readFile(join(root, 'skills/companion/SKILL.md'), 'utf8');
    assert.match(skill, /^---\\nname: companion\\n/);
    process.stdout.write(JSON.stringify({version: pkg.version, assets: manifest.assets.length})+'\\n');
  `);
  const result = await execute(process.execPath, ['--import', guard, probe], { cwd: scratch, env, timeout: 10_000, maxBuffer: 1024 * 1024 });
  const installed = JSON.parse(result.stdout);
  assert.equal(result.stderr, '', 'Installed SDK import must not emit errors');
  assert.equal(await readFile(audit, 'utf8'), '', 'SDK import must not contact services or spawn helpers');
  const before = await readdir(home, { recursive: true });

  const bin = join(scratch, 'node_modules', '@hraness', 'desktop-foundation', 'dist', 'src', 'cli.js');
  let stdout, stderr, exitCode;
  try {
    const doctor = await execute(process.execPath, ['--import', guard, bin, 'doctor', '--json'], { cwd: scratch, env, timeout: 10_000, maxBuffer: 1024 * 1024 });
    ({ stdout, stderr } = doctor); exitCode = 0;
  } catch (error) {
    if (error.code !== 1) throw error;
    ({ stdout, stderr } = error); exitCode = 1;
  }
  assert.equal(stderr, '', 'Packaged doctor must emit its result as JSON');
  const doctor = JSON.parse(stdout);
  assert.equal(doctor.status.running, false);
  assert.equal(doctor.artifact.installed, false);
  assert.equal(doctor.artifact.integrity, 'missing');
  assert.equal(doctor.artifact.version, installed.version);
  assert.equal(doctor.artifact.repository, 'hraness/desktop-foundation');
  assert.match(doctor.artifact.sha256, /^[a-f0-9]{64}$/);
  assert.equal(doctor.signing, 'unsigned');
  assert.equal(doctor.notarization, 'none');
  assert.ok(Array.isArray(doctor.diagnostics));
  assert.equal(exitCode, doctor.diagnostics.some(item => item.severity === 'error') ? 1 : 0);
  assert.equal(await readFile(audit, 'utf8'), '', 'Doctor must not contact services, download artifacts or spawn helpers');
  assert.deepEqual(await readdir(home, { recursive: true }), before, 'Doctor must not create state, cache or login registration');
  console.log(JSON.stringify({ package: packageName, version: installed.version, assets: installed.assets, packageInstall: 'passed', doctor: 'passed', artifact: 'missing', network: 'unused' }));
} finally {
  await rm(scratch, { recursive: true, force: true });
}
