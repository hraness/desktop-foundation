// Checks the shipped binaries for one target: `hraness-companion` answers
// every one-shot argv with the same bytes and exit status as
// `hraness-helper`, and refuses the removed menu-bar argv with exit 2
// (contract/companion-alias.v1.json). Nothing here opens a dialog: the argv
// below either probes, or fails validation before any window is created.
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const dir = process.argv[2] ?? 'target/release';
const exe = process.platform === 'win32' ? '.exe' : '';
const helper = resolve(dir, `hraness-helper${exe}`);
const companion = resolve(dir, `hraness-companion${exe}`);
const alias = JSON.parse(readFileSync(new URL('../contract/companion-alias.v1.json', import.meta.url), 'utf8'));

const run = (binary, args, input = '') => new Promise((done, reject) => {
  const child = spawn(binary, args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  const out = [], err = [];
  let size = 0;
  const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error(`${args.join(' ') || '(no argv)'} timed out`)); }, 15_000);
  const collect = into => chunk => {
    size += chunk.length;
    if (size > 65536) { child.kill('SIGKILL'); reject(new Error('output bound exceeded')); }
    into.push(chunk);
  };
  child.once('error', reject);
  child.stdin.on('error', () => {});
  child.stdout.on('data', collect(out));
  child.stderr.on('data', collect(err));
  child.once('close', code => { clearTimeout(timer); done({ code, stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8') }); });
  child.stdin.end(input);
});

const cases = [
  ['--prompt-probe'],
  ...alias.parityExtraArgv.filter(argv => argv[0] !== '--prompt' || argv.length > 1),
];
const invalidNotice = JSON.stringify({ type: 'notice-request', version: 1, title: 'T', message: 'M', primary: 'OK', settings: 'keychain' }) + '\n';
let checked = 0;
for (const [argv, input] of [...cases.map(argv => [argv, '']), [['--notice'], invalidNotice]]) {
  const [a, b] = await Promise.all([run(helper, argv, input), run(companion, argv, input)]);
  const name = JSON.stringify(argv);
  if (a.code !== b.code || a.stdout !== b.stdout || a.stderr !== b.stderr) {
    throw new Error(`alias parity failed for ${name}: helper=${JSON.stringify(a)} companion=${JSON.stringify(b)}`);
  }
  checked += 1;
}
const [hv, cv] = await Promise.all([run(helper, ['--version']), run(companion, ['--version'])]);
if (hv.code !== 0 || cv.code !== 0 || hv.stdout.replace(/^hraness-helper /, '') !== cv.stdout.replace(/^hraness-companion /, '')) {
  throw new Error(`--version differs beyond the binary name: ${JSON.stringify(hv)} ${JSON.stringify(cv)}`);
}
for (const argv of alias.trayModes) {
  const r = await run(companion, argv);
  if (r.code !== alias.refusalExit || r.stdout.trim() !== alias.refusalStdout || !r.stderr.startsWith(alias.refusalStderrPrefix)) {
    throw new Error(`tray mode ${JSON.stringify(argv)} was not refused: ${JSON.stringify(r)}`);
  }
}
console.log(`alias: ${checked + 1} argv match hraness-helper byte for byte, ${alias.trayModes.length} tray argv refused with exit ${alias.refusalExit} (${process.platform}/${process.arch}; ${cv.stdout.trim()})`);
