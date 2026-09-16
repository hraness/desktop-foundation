import assert from 'node:assert/strict';
import { Readable, Writable } from 'node:stream';
import test from 'node:test';
import {
  promptCapability, promptNative, promptSecret, promptTui, validatePromptRequest,
  PROMPT_LIMITS, type PromptRequest,
} from '../src/prompt.js';

const request: PromptRequest = { title: 'Textbutler', message: 'Enter the app password' };

function fakePrompt(script: string) {
  return { binary: process.execPath, binaryArgs: ['--input-type=module', '-e', script, '--'] };
}

function tty(keys: Buffer) {
  const input = new Readable({ read() {} }) as Readable & { isTTY?: boolean };
  input.isTTY = true;
  const output = new Writable({ write(_c, _e, done) { done(); } });
  queueMicrotask(() => { input.push(keys); input.push(null); });
  return { input, output };
}

test('prompt requests enforce the shared bounds and safe display text', () => {
  const spec = validatePromptRequest(request);
  assert.equal(spec.secret, true);
  assert.equal(spec.timeoutSeconds, PROMPT_LIMITS.defaultTimeoutSeconds);
  assert.equal(validatePromptRequest({ ...request, timeoutSeconds: 600 }).timeoutSeconds, 600);
  for (const bad of [
    { ...request, title: '' }, { ...request, title: 'x'.repeat(129) },
    { ...request, message: 'x'.repeat(513) }, { ...request, title: 'bad‮title' },
    { ...request, prefill: 'x'.repeat(4097) }, { ...request, timeoutSeconds: 0 },
    { ...request, timeoutSeconds: 601 }, { ...request, timeoutSeconds: 1.5 },
    { ...request, secret: 'yes' }, { ...request, extra: true },
  ] as unknown as PromptRequest[]) {
    assert.throws(() => validatePromptRequest(bad), /invalid-prompt/);
  }
});

test('native prompt pipes the spec on stdin and returns the result frame', async () => {
  const script = `
    const chunks = [];
    process.stdin.on('data', c => chunks.push(c));
    process.stdin.on('end', () => {
      const spec = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      if (spec.prefill !== undefined && process.argv.includes(spec.prefill)) process.exit(9);
      if (spec.prefill !== 'seed-secret') process.exit(8);
      if (spec.timeoutSeconds !== 5) process.exit(7);
      process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'submitted',value:'typed-secret'})+'\\n');
    });
  `;
  const result = await promptNative(
    { ...request, prefill: 'seed-secret', timeoutSeconds: 5 },
    fakePrompt(script),
  );
  assert.deepEqual(result, { status: 'submitted', value: 'typed-secret' });
});

test('native prompt maps runner statuses and rejects malformed frames', async () => {
  for (const [emit, expected] of [
    ['cancelled', { status: 'cancelled' }],
    ['timeout', { status: 'timeout' }],
    ['unavailable', { status: 'unavailable' }],
  ] as const) {
    const script = `process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'${emit}'})+'\\n');`;
    assert.deepEqual(await promptNative(request, fakePrompt(script)), expected);
  }
  const bad = `process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'submitted'})+'\\n');`;
  await assert.rejects(promptNative(request, fakePrompt(bad)), /invalid-prompt-frame/);
  const smuggle = `process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'cancelled',value:'x'})+'\\n');`;
  await assert.rejects(promptNative(request, fakePrompt(smuggle)), /invalid-prompt-frame/);
  const error = `process.stdout.write(JSON.stringify({type:'error',version:1,code:'invalid-prompt'})+'\\n');`;
  await assert.rejects(promptNative(request, fakePrompt(error)), /prompt-runner-invalid-prompt/);
  await assert.rejects(promptNative(request, fakePrompt('process.exit(3)')), /prompt-runner-exit-3/);
});

test('capability probe reports the bounded frame', async () => {
  const script = `process.stdout.write(JSON.stringify({type:'prompt-capability',version:1,capable:false,detail:'no-gui-session'})+'\\n');`;
  assert.deepEqual(await promptCapability(fakePrompt(script)), { capable: false, detail: 'no-gui-session' });
  const bad = `process.stdout.write('{}\\n');`;
  await assert.rejects(promptCapability(fakePrompt(bad)), /invalid-prompt-frame/);
});

test('tty prompt masks input, honours editing keys and bounds', async () => {
  const submitted = await promptTui(request, tty(Buffer.from('hunter2\r')));
  assert.deepEqual(submitted, { status: 'submitted', value: 'hunter2' });
  const edited = await promptTui(request, tty(Buffer.from([0x61, 0x7f, 0x62, 0x0d])));
  assert.deepEqual(edited, { status: 'submitted', value: 'b' });
  const cancelled = await promptTui(request, tty(Buffer.from([0x03])));
  assert.deepEqual(cancelled, { status: 'cancelled' });
  const utf8 = await promptTui(request, tty(Buffer.concat([Buffer.from('🔑', 'utf8'), Buffer.from('\r')])));
  assert.deepEqual(utf8, { status: 'submitted', value: '🔑' });
  const utf8Edited = await promptTui(request, tty(Buffer.concat([Buffer.from('a🔑', 'utf8'), Buffer.from([0x7f]), Buffer.from('b\r')])));
  assert.deepEqual(utf8Edited, { status: 'submitted', value: 'ab' });
  const timed = await promptTui({ ...request, timeoutSeconds: 1 }, {
    input: Object.assign(new Readable({ read() {} }), { isTTY: true }),
    output: new Writable({ write(_c, _e, done) { done(); } }),
  });
  assert.deepEqual(timed, { status: 'timeout' });
  const piped = await promptTui(request, { input: new Readable({ read() {} }), output: new Writable({ write(_c, _e, d) { d(); } }) });
  assert.deepEqual(piped, { status: 'unavailable' });
});

test('secret prompt falls back to the tty only when native is unavailable', async () => {
  const unavailable = `process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'unavailable'})+'\\n');`;
  const viaTui = await promptSecret(request, { ...fakePrompt(unavailable), io: tty(Buffer.from('typed\r')) });
  assert.deepEqual(viaTui, { status: 'submitted', value: 'typed' });
  const submitted = `process.stdout.write(JSON.stringify({type:'prompt-result',version:1,status:'submitted',value:'native'})+'\\n');`;
  const viaNative = await promptSecret(request, { ...fakePrompt(submitted), io: tty(Buffer.from('ignored\r')) });
  assert.deepEqual(viaNative, { status: 'submitted', value: 'native' });
});
