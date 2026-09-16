import { spawn } from 'node:child_process';
import { resolve } from 'node:path';

const binary = resolve(process.argv.slice(2).find(arg => !arg.startsWith('--')) ?? `target/release/hraness-companion${process.platform === 'win32' ? '.exe' : ''}`);

// One bounded one-shot invocation: JSON request on stdin, one JSON frame on
// stdout, exit code collected. Output is capped so a misbehaving runner cannot
// stall the job on an unbounded pipe.
const invoke = (args, input, timeoutMs) => new Promise((resolveInvoke, reject) => {
  const child = spawn(binary, args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  let stdout = '', stderr = '';
  const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error(`${args[0]} timed out`)); }, timeoutMs);
  child.once('error', reject);
  child.stdin.on('error', () => {});
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-2000); });
  child.stdout.on('data', chunk => {
    stdout += chunk;
    if (stdout.length > 65536) { child.kill('SIGKILL'); reject(new Error(`${args[0]} exceeded output bound`)); }
  });
  child.once('close', code => {
    clearTimeout(timer);
    const line = stdout.split('\n').find(Boolean);
    let frame;
    try { frame = line ? JSON.parse(line) : undefined; } catch { frame = undefined; }
    resolveInvoke({ code, frame, stderr });
  });
  if (input !== undefined) child.stdin.end(input); else child.stdin.end();
});

const probe = await invoke(['--prompt-probe'], undefined, 10_000);
if (probe.code !== 0 || probe.frame?.type !== 'prompt-capability' || typeof probe.frame?.capable !== 'boolean') {
  throw new Error(`prompt-probe contract failed (code=${probe.code}, frame=${JSON.stringify(probe.frame)}): ${probe.stderr}`);
}
const capable = probe.frame.capable;

// A bounded request proves the real dialog renders on capable hosts and that
// the one-shot mode still answers correctly where no desktop exists. Secret
// mode plus auto-timeout keeps CI click-free and leaves nothing behind.
const request = JSON.stringify({ type: 'prompt-request', version: 1, title: 'Companion Smoke', message: 'Auto-dismissed qualification prompt.', secret: true, timeoutSeconds: 3 });
const result = await invoke(['--prompt'], request + '\n', 30_000);
if (result.code !== 0 || result.frame?.type !== 'prompt-result') {
  throw new Error(`prompt contract failed (code=${result.code}, frame=${JSON.stringify(result.frame)}): ${result.stderr}`);
}
const status = result.frame.status;
const expected = capable ? 'timeout' : 'unavailable';
if (status !== expected) {
  throw new Error(`prompt inconsistent with capability probe: capable=${capable} detail=${probe.frame.detail} status=${status}`);
}
if ('value' in result.frame) {
  throw new Error('timeout/unavailable result must not carry a value');
}
console.log(`prompt: capability=${capable} (${probe.frame.detail}); dialog run returned "${status}" as expected (${process.platform}/${process.arch})`);
