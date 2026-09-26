import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { AGENT_MARKERS, detectAudience } from '../src/audience.js';
import { cliLine, cliStyle, cliSymbol, createCliOutput, exitQuietlyOnBrokenPipe, formatBytes, renderCliError } from '../src/cli-style.js';

test('detectAudience: override, exact agent markers, then the terminal', () => {
  assert.equal(detectAudience({ env: {}, stderrIsTTY: true }), 'human');
  assert.equal(detectAudience({ env: {}, stderrIsTTY: false }), 'quiet');
  for (const marker of AGENT_MARKERS) assert.equal(detectAudience({ env: { [marker]: '1' }, stderrIsTTY: true }), 'agent', marker);
  assert.equal(detectAudience({ env: { CLAUDECODE: '' }, stderrIsTTY: true }), 'human');
  // Prefixes are human configuration, never agent markers.
  assert.equal(detectAudience({ env: { CODEX_HOME: '/x', DEVIN_API_KEY: 'k', CLAUDE_CODE_ENTRYPOINT: 'cli' }, stderrIsTTY: true }), 'human');
  assert.equal(detectAudience({ env: { HRANESS_AUDIENCE: 'human', CLAUDECODE: '1' }, stderrIsTTY: false }), 'human');
  assert.equal(detectAudience({ env: { HRANESS_AUDIENCE: 'agent' }, stderrIsTTY: true }), 'agent');
  assert.equal(detectAudience({ env: { HRANESS_AUDIENCE: 'quiet' }, stderrIsTTY: true }), 'quiet');
  assert.equal(detectAudience({ env: { HRANESS_AUDIENCE: 'off', CLAUDECODE: '1' }, stderrIsTTY: true }), 'quiet');
  assert.equal(detectAudience({ env: { HRANESS_AUDIENCE: 'robots' }, stderrIsTTY: true }), 'human');
});

test('cliStyle: color only at a terminal without NO_COLOR, ASCII without UTF-8', () => {
  const utf8 = { LANG: 'en_US.UTF-8' };
  assert.deepEqual(cliStyle({ isTTY: true }, utf8), { color: true, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: false }, utf8), { color: false, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: true }, { ...utf8, NO_COLOR: '1' }), { color: false, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: true }, { ...utf8, NO_COLOR: '' }), { color: true, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: false }, { ...utf8, FORCE_COLOR: '1' }), { color: true, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: true }, { ...utf8, FORCE_COLOR: '1', NO_COLOR: '1' }), { color: false, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: true }, { ...utf8, TERM: 'dumb' }), { color: false, ascii: true });
  assert.deepEqual(cliStyle({ isTTY: true }, { LANG: 'C' }), { color: true, ascii: true });
  assert.deepEqual(cliStyle({ isTTY: true }, { LC_ALL: 'C.UTF-8' }), { color: true, ascii: false });
  assert.deepEqual(cliStyle({ isTTY: true }, { ...utf8, HRANESS_ASCII: '1' }), { color: true, ascii: true });
});

test('symbols, fallbacks and the error form', () => {
  const plain = { color: false, ascii: false };
  const ascii = { color: false, ascii: true };
  assert.deepEqual(['ok', 'fail', 'warn', 'next', 'on', 'off', 'skip', 'progress', 'notice'].map(name => cliSymbol(name as never, plain)), ['✓', '✗', '⚠', '→', '●', '○', '–', '↻', '🔐']);
  assert.deepEqual(['ok', 'fail', 'warn', 'next', 'on', 'off', 'skip', 'progress', 'notice'].map(name => cliSymbol(name as never, ascii)), ['OK', 'FAIL', 'WARN', '->', '*', 'o', '-', '...', 'NOTE']);
  // The warning sign carries no emoji presentation selector in the CLI.
  assert.equal(cliSymbol('warn', plain).length, 1);
  // Only the symbol is colored, never the sentence.
  assert.equal(cliLine('ok', 'Done.', { color: true, ascii: false }), '\u001b[32m✓\u001b[0m Done.');
  assert.equal(cliLine('off', 'Idle.', { color: true, ascii: false }), '○ Idle.');
  assert.equal(renderCliError({ message: 'No chat matches "Mom".', next: 'textbutler chats list' }, plain), '✗ No chat matches "Mom".\n→ textbutler chats list\n');
  assert.equal(renderCliError({ message: 'Stopped.' }, ascii), 'FAIL Stopped.\n');
});

function sink(isTTY: boolean) { const chunks: string[] = []; return { chunks, stream: { isTTY, write: (text: string) => { chunks.push(text); } } }; }

test('createCliOutput routes results, errors, hints and progress by audience', () => {
  const env = { LANG: 'en_US.UTF-8' };
  {
    const out = sink(true), err = sink(true);
    const cli = createCliOutput({ audience: 'human', stdout: out.stream, stderr: err.stream, env: { ...env, NO_COLOR: '1' } });
    const stop = cli.progress('Downloading the menu bar helper (1.8 MB)…');
    cli.result('Added Mom. Automatic replies are off.', 'ok');
    stop();
    cli.detail('More detail');
    cli.next('textbutler chats on Mom');
    cli.warn('Check this.');
    cli.error({ message: 'Nope.', next: 'textbutler doctor' });
    assert.deepEqual(out.chunks, ['✓ Added Mom. Automatic replies are off.\n', '  More detail\n']);
    assert.deepEqual(err.chunks, ['↻ Downloading the menu bar helper (1.8 MB)…', '\r\u001b[2K', 'Next: textbutler chats on Mom\n', '⚠ Check this.\n', '✗ Nope.\n→ textbutler doctor\n']);
  }
  {
    const out = sink(false), err = sink(false);
    const cli = createCliOutput({ audience: 'quiet', stdout: out.stream, stderr: err.stream, env: { ...env, FORCE_COLOR: '1' } });
    cli.progress('Working…')();
    cli.result('Done.', 'ok');
    cli.next('x');
    assert.deepEqual(out.chunks, ['✓ Done.\n']);
    assert.deepEqual(err.chunks, []);
  }
  {
    // Non-TTY stderr for a human audience gets no redrawn progress.
    const err = sink(false);
    const cli = createCliOutput({ audience: 'human', stdout: sink(false).stream, stderr: err.stream, env });
    cli.progress('Working…')();
    assert.deepEqual(err.chunks, []);
  }
  {
    // Audience defaults from the environment and the stderr stream.
    const cli = createCliOutput({ stdout: sink(false).stream, stderr: sink(true).stream, env: { ...env, CLAUDECODE: '1' } });
    assert.equal(cli.audience, 'agent');
  }
});

test('formatBytes and quiet EPIPE exits', () => {
  assert.equal(formatBytes(1_812_345), '1.8 MB');
  assert.equal(formatBytes(640_000), '640 KB');
  assert.equal(formatBytes(999_499), '999 KB');
  assert.equal(formatBytes(999_500), '1.0 MB');
  assert.equal(formatBytes(12), '12 bytes');
  const stream = new EventEmitter() as unknown as NodeJS.WriteStream;
  const codes: number[] = [];
  exitQuietlyOnBrokenPipe(stream, code => { codes.push(code); });
  stream.emit('error', Object.assign(new Error('write EPIPE'), { code: 'EPIPE' }));
  assert.deepEqual(codes, [0]);
  assert.throws(() => stream.emit('error', Object.assign(new Error('other'), { code: 'EIO' })), /other/);
});
