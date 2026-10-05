import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { access, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
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
      'release-manifest.json', 'dist/src/index.js', 'dist/src/index.d.ts',
      'README.md', 'LICENSE', 'docs/installation.md', 'docs/platforms.md', 'skills/companion/SKILL.md',
      'docs/architecture.md', 'docs/adoption.md', 'docs/protocol.md', 'docs/migration-1.0.md',
      'docs/permissions.md', 'docs/identity.md', 'docs/control.md', 'docs/human-gate.md',
      'src/lib.rs', 'src/bin/hraness-companion.rs', 'src/bin/hraness-helper.rs',
      'crates/hraness-local-app/src/identity.rs', 'crates/hraness-local-app/src/service.rs',
      'crates/hraness-local-app/src/helper.rs',
      'crates/hraness-control-kit/src/lib.rs', 'contract/helper-argv.v0.8.1.json', 'contract/companion-alias.v1.json',
      'contract/error-codes.json',
      'dist/src/registry.js', 'dist/src/control.js', 'dist/src/human-gate.js',
      'dist/src/login.js', 'dist/src/retire.js', 'dist/src/helper.js',
      'dist/src/permissions.js', 'dist/src/audience.js', 'dist/src/cli-style.js',
    ]) await access(join(root, file));
    // 1.0 removed the menu bar; 2.0 removed the TUI and the notice/prompt dialogs.
    for (const file of [
      'dist/src/menu-kit.js', 'dist/src/cli.js', 'dist/src/client.js', 'dist/src/commands.js', 'dist/src/protocol.js',
      'dist/src/protocol-v2.js', 'dist/src/service.js', 'dist/src/browser.js',
      'src/protocol.rs', 'src/protocol_v2.rs', 'src/outputs.rs', 'src/symbols.rs',
      'dist/src/tui.js', 'dist/src/notice.js', 'dist/src/prompt.js',
      'sdk/src/tui.ts', 'sdk/src/notice.ts', 'sdk/src/prompt.ts',
      'crates/hraness-local-app/src/notice.rs', 'crates/hraness-local-app/src/prompt.rs',
      'crates/hraness-control-kit/src/tui.rs',
    ]) await assert.rejects(access(join(root, file)), undefined, file + ' must not ship');
    assert.equal(pkg.bin, undefined, 'the companion CLI was removed in 1.0');
    assert.equal(pkg.exports['./menu-kit'], undefined, 'menu-kit was removed in 1.0');
    await assert.rejects(import(${JSON.stringify(packageName + '/menu-kit')}), /ERR_PACKAGE_PATH_NOT_EXPORTED|not exported/);
    const sdk = await import(${JSON.stringify(packageName)});
    assert.equal(import.meta.resolve(${JSON.stringify(packageName)}), pathToFileURL(join(root, 'dist/src/index.js')).href);
    for (const name of ['packagedManifest', 'parseReleaseManifest', 'inspectBinary', 'ensureBinary',
      'diagnosePlatform', 'planAutostart', 'userPaths', 'loadLoginEnvironment', 'saveLoginEnvironment'])
      assert.equal(typeof sdk[name], 'function', name);
    for (const name of ['promptNative', 'promptCapability', 'promptTui', 'validatePromptRequest'])
      assert.equal(typeof sdk[name], 'undefined', name + ' was removed in 2.0');
    for (const name of ['runCompanion', 'startCompanion', 'stopCompanion', 'companionStatus', 'handleCompanionCommand',
      'layout', 'lintMenu', 'validateSnapshotV2', 'downlevelSnapshot', 'parseRunnerProtocols', 'openBrowser', 'permissionMenuItems'])
      assert.equal(name in sdk, false, name + ' was removed in 1.0');
    const manifest = await sdk.packagedManifest();
    assert.deepEqual(manifest, sdk.parseReleaseManifest(await readFile(join(root, 'release-manifest.json'))));
    assert.equal(manifest.version, pkg.version);
    assert.equal(manifest.tag, 'v'+pkg.version);
    assert.equal(manifest.repository, 'hraness/desktop-foundation');
    const targets = [
      'aarch64-apple-darwin', 'x86_64-apple-darwin',
      'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
      'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    ].sort();
    assert.deepEqual(manifest.assets.map(asset => asset.target).sort(), targets);
    assert.deepEqual((manifest.helperAssets ?? []).map(asset => asset.target).sort(), targets, 'every target ships hraness-helper');
    const skill = await readFile(join(root, 'skills/companion/SKILL.md'), 'utf8');
    assert.match(skill, /^---\\nname: companion\\n/);

    const permissions = await import(${JSON.stringify(packageName + '/permissions')});
    const audience = await import(${JSON.stringify(packageName + '/audience')});
    const style = await import(${JSON.stringify(packageName + '/cli-style')});
    const sub = async name => import(${JSON.stringify(packageName + '/')} + name);
    const [registry, control, gate, login, retire, helper] = await Promise.all(
      ['registry', 'control', 'human-gate', 'login', 'retire', 'helper'].map(sub));
    for (const [module, names] of [
      [permissions, ['prePrompt', 'renderPrePrompt', 'renderRecovery', 'permissionErrorJson', 'LOCAL_SIGNING', 'MESSAGES_FDA']],
      [audience, ['detectAudience']],
      [style, ['createCliOutput', 'cliStyle', 'exitQuietlyOnBrokenPipe']],
      [registry, ['defineRegistry', 'runCli', 'commandsJson', 'okEnvelope', 'errorEnvelope', 'isErrorCode']],
      [control, ['serveControl', 'agentRequest', 'adminRequest', 'controlStatus', 'ensureOwner', 'ownerPaths']],
      [gate, ['requireHuman', 'detectAgent', 'oneTimeCode', 'ownerAuthorize']],
      [login, ['planLoginItem', 'installLoginItem', 'uninstallLoginItem']],
      [retire, ['retireLegacyLoginItem', 'launches']],
      [helper, ['resolveHelper']],
    ]) for (const name of names) assert.equal(typeof module[name], 'function', name);
    assert.equal('permissionMenuItems' in permissions, false);
    assert.equal(audience.detectAudience({ env: { HRANESS_AUDIENCE: 'agent' } }), 'agent');
    process.stdout.write(JSON.stringify({version: pkg.version, assets: manifest.assets.length})+'\\n');
  `);
  const result = await execute(process.execPath, ['--import', guard, probe], { cwd: scratch, env, timeout: 10_000, maxBuffer: 1024 * 1024 });
  const installed = JSON.parse(result.stdout);
  assert.equal(result.stderr, '', 'Installed SDK import must not emit errors');
  assert.equal(await readFile(audit, 'utf8'), '', 'SDK import must not contact services or spawn helpers');

  // The packaged helper and its companion alias for this host report the
  // package version, and the alias refuses the removed tray mode. Required in
  // CI, where the package job has every native artifact; a local run without
  // artifacts reports it as skipped.
  const hostTarget = { 'darwin-arm64': 'aarch64-apple-darwin', 'darwin-x64': 'x86_64-apple-darwin', 'linux-x64': 'x86_64-unknown-linux-gnu', 'linux-arm64': 'aarch64-unknown-linux-gnu', 'win32-x64': 'x86_64-pc-windows-msvc', 'win32-arm64': 'aarch64-pc-windows-msvc' }[`${process.platform}-${process.arch}`];
  const exe = process.platform === 'win32' ? '.exe' : '';
  const companion = hostTarget && resolve('artifacts', `hraness-companion-${hostTarget}${exe}`);
  const helperBinary = hostTarget && resolve('artifacts', `hraness-helper-${hostTarget}${exe}`);
  const present = async path => path && await access(path).then(() => true, () => false);
  let runnerResult = 'skipped';
  if (await present(companion) && await present(helperBinary)) {
    for (const binary of [companion, helperBinary]) if (process.platform !== 'win32') await execute('chmod', ['+x', binary]);
    assert.equal((await execute(companion, ['--version'], { env, timeout: 10_000 })).stdout.trim(), `hraness-companion ${installed.version} protocol/1,2`);
    assert.equal((await execute(helperBinary, ['--version'], { env, timeout: 10_000 })).stdout.trim(), `hraness-helper ${installed.version} protocol/1,2`);
    const refusal = await execute(companion, [], { env, timeout: 10_000 }).then(() => ({ code: 0 }), error => error);
    assert.equal(refusal.code, 2, 'the alias refuses the removed tray mode with exit 2');
    assert.equal(refusal.stdout.trim(), '{"type":"error","version":1,"code":"tray-removed"}');
    assert.match(refusal.stderr, /status --json/);
    runnerResult = `${hostTarget} helper and alias report ${installed.version}; tray mode refused`;
  } else if (process.env.CI) {
    throw new Error(`Package smoke needs the ${hostTarget} helper and companion in artifacts/`);
  }
  console.log(JSON.stringify({ package: packageName, version: installed.version, assets: installed.assets, packageInstall: 'passed', network: 'unused', menuKit: 'absent', runner: runnerResult }));
} finally {
  await rm(scratch, { recursive: true, force: true });
}
