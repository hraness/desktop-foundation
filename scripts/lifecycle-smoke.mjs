import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmod, mkdtemp, readdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

const sdkUrl = pathToFileURL(await realpath(resolve('dist/src/index.js'))).href;
const { companionStatus, stopCompanion } = await import(sdkUrl);
const binary = await realpath(resolve(process.argv[2] ?? `target/release/hraness-companion${process.platform === 'win32' ? '.exe' : ''}`));
const root = await mkdtemp(join(await realpath(tmpdir()), 'companion-lifecycle-smoke-'));
await chmod(root, 0o700);
const stateDir = join(root, 'state');
const entrypoint = join(root, 'foreground.mjs');
const appId = 'org.hraness.companion.lifecycle-smoke';
const launcherChildren = new Set();
let cleaned = false;

// The launcher is a separate short-lived CLI process. Its detached foreground
// owner must remain alive after the launcher's process exits. Each foreground
// process has a finite watchdog and records completion only after SDK cleanup.
await writeFile(entrypoint, `
import { randomUUID } from 'node:crypto';
import { appendFile, rename, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { startCompanion, serveCompanion } from ${JSON.stringify(sdkUrl)};
const root = ${JSON.stringify(root)};
const stateDir = ${JSON.stringify(stateDir)};
const appId = ${JSON.stringify(appId)};
if (process.argv[2] === '--launch') {
  try {
    const result = await startCompanion({ appId, stateDir, executable: process.execPath,
      args: [${JSON.stringify(entrypoint)}, '--foreground'], timeoutMs: 15_000 });
    process.stdout.write(JSON.stringify(result) + '\\n');
  } catch (error) {
    process.stderr.write(String(error.message) + '\\n'); process.exitCode = 1;
  }
} else if (process.argv[2] === '--foreground') {
  const id = randomUUID();
  await writeFile(join(root, id + '.started'), '', { flag: 'wx', mode: 0o600 });
  const watchdog = setTimeout(() => {
    // This process and its stdin-owned renderer are only smoke-test resources.
    // Closing this owner is the final fallback; no stored PID is ever signaled.
    process.emit('SIGTERM');
    setTimeout(() => process.exit(124), 6_000).unref();
  }, 35_000);
  let snapshots = 0;
  try {
    process.exitCode = await serveCompanion({ appId, name: 'Companion Lifecycle Smoke', title: 'Te',
      stateDir, binary: ${JSON.stringify(binary)}, refreshMs: 250, timeoutMs: 8_000,
      snapshot: async () => {
        await writeFile(join(root, id + '.snapshots.tmp'), String(++snapshots), { mode: 0o600 });
        await rename(join(root, id + '.snapshots.tmp'), join(root, id + '.snapshots'));
        return [{ kind: 'label', label: 'Synthetic lifecycle smoke ' + snapshots }, { kind: 'quit', label: 'Quit smoke companion' }];
      },
      onAction: () => {},
      onDiagnostic: code => { void appendFile(join(root, 'diagnostics.log'), code + '\\n', { mode: 0o600 }).catch(() => {}); },
    });
  } catch (error) {
    await appendFile(join(root, 'diagnostics.log'), String(error.message) + '\\n', { mode: 0o600 });
    process.exitCode = 1;
  } finally {
    clearTimeout(watchdog);
    await writeFile(join(root, id + '.finished'), '', { flag: 'wx', mode: 0o600 });
  }
} else { throw new Error('Unsupported smoke-test mode'); }
`, { flag: 'wx', mode: 0o600 });

function launch() {
  return new Promise((resolveLaunch, reject) => {
    const child = spawn(process.execPath, [entrypoint, '--launch'], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    launcherChildren.add(child);
    let stdout = '', stderr = '', overflow = false;
    const timer = setTimeout(() => child.kill('SIGKILL'), 22_000);
    child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 8192) { overflow = true; child.kill(); } });
    child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-8192); });
    child.once('error', error => { clearTimeout(timer); launcherChildren.delete(child); reject(error); });
    child.once('close', code => {
      clearTimeout(timer); launcherChildren.delete(child);
      if (code !== 0 || overflow) { reject(new Error(`Detached CLI launch failed (${code}): ${stderr}`)); return; }
      try { resolveLaunch(JSON.parse(stdout)); } catch { reject(new Error('Detached CLI returned invalid status')); }
    });
  });
}

async function receipts() {
  try { return (await readdir(stateDir)).filter(name => /^\.companion-service-[a-f0-9]{32}\.json$/.test(name)); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
}
async function ownersFinished() {
  const names = await readdir(root);
  return names.filter(name => name.endsWith('.started')).every(name => names.includes(name.replace(/\.started$/, '.finished')));
}
async function waitForOwners(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await ownersFinished() && (await receipts()).length === 0) return true;
    await delay(100);
  }
  return false;
}

try {
  const launches = await Promise.allSettled([launch(), launch(), launch()]);
  for (const result of launches) {
    if (result.status === 'rejected') throw result.reason;
    assert.equal(result.value.running, true);
    assert.equal(result.value.state, 'running');
  }
  // All three launch CLI processes have exited. The one detached owner must
  // persist, continue refreshing, and retain the same receipt across reuse.
  assert.equal((await companionStatus(stateDir, appId)).state, 'running');
  const initialReceipts = await receipts();
  assert.equal(initialReceipts.length, 1, 'exactly one native owner must publish a receipt');
  await delay(800);
  assert.equal((await companionStatus(stateDir, appId)).state, 'running');
  const snapshotCounts = await Promise.all((await readdir(root)).filter(name => name.endsWith('.snapshots')).map(async name => Number(await readFile(join(root, name), 'utf8'))));
  assert.ok(snapshotCounts.some(count => count >= 2), 'detached owner must keep refreshing after launchers exit');
  assert.equal((await launch()).state, 'running');
  assert.deepEqual(await receipts(), initialReceipts, 'repeat launch must reuse the original owner');
  assert.equal((await stopCompanion(stateDir, appId)).state, 'stopped');
  assert.equal((await companionStatus(stateDir, appId)).state, 'stopped');
  assert.equal((await stopCompanion(stateDir, appId)).state, 'stopped');
  assert.ok(await waitForOwners(5_000), 'foreground owners must finish after stop');
  console.log(`passed: concurrent CLI start, detached persistence, singleton reuse, refresh, status and repeated stop (${process.platform}/${process.arch}); this is not visual tray qualification`);
} catch (error) {
  const diagnostics = await readFile(join(root, 'diagnostics.log'), 'utf8').catch(() => '');
  if (diagnostics) console.error(`Synthetic native diagnostics: ${diagnostics.slice(-8192).trim()}`);
  throw error;
} finally {
  // No product process, login registration or existing state is touched.
  for (const child of launcherChildren) child.kill('SIGTERM');
  await stopCompanion(stateDir, appId).catch(() => undefined);
  if (await waitForOwners(8_000)) {
    await rm(root, { recursive: true, force: true });
    cleaned = true;
  }
  if (!cleaned) {
    console.error(`Smoke-test cleanup is incomplete; preserving its private diagnostics at ${root}. Owned foreground watchdogs expire within 41 seconds of startup.`);
    process.exitCode = 1;
  }
}
