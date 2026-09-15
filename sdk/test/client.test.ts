import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test, { type TestContext } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { runCompanion, type CompanionOptions } from '../src/client.js';
import type { MenuItem } from '../src/protocol.js';

const items: readonly MenuItem[] = [
  { kind: 'action', id: 'enabled', label: 'Enabled' },
  { kind: 'action', id: 'disabled', label: 'Disabled', enabled: false },
  { kind: 'quit', label: 'Quit' },
];

// A real child process exercises pipes, exit races and signal escalation.
// Node's -- separator preserves the native runner arguments on every OS.
function fakeRunner(mode: string): string {
  return `
    import { createInterface } from 'node:readline';
    const mode = ${JSON.stringify(mode)};
    let seen = 0;
    const alive = setInterval(() => {}, 1000);
    const send = value => process.stdout.write(JSON.stringify({version:1,...value})+'\\n');
    if (mode === 'ignore-quit' || mode === 'eof' || mode === 'stalled-input') process.on('SIGTERM', () => {});
    const input = createInterface({input:process.stdin});
    input.on('line', line => {
      const value = JSON.parse(line);
      if (value.type === 'quit') {
        if (mode !== 'ignore-quit' && mode !== 'eof' && mode !== 'stalled-input') process.exit(0);
        return;
      }
      if (value.type !== 'snapshot') return;
      seen++;
      if (seen === 1) {
        if (mode === 'actions') send({type:'action',id:'enabled',revision:value.revision});
        send({type:'ready',pid:process.pid,platform:process.platform});
        if (mode === 'duplicate-ready') {
          send({type:'ready',pid:process.pid,platform:process.platform});
          send({type:'ready',pid:process.pid,platform:process.platform});
        }
        if (mode === 'actions') {
          send({type:'action',id:'disabled',revision:value.revision});
          send({type:'action',id:'enabled',revision:value.revision-1});
          send({type:'action',id:'unknown',revision:value.revision});
          send({type:'action',id:'enabled',revision:value.revision});
          send({type:'action',id:'enabled',revision:value.revision});
        }
        if (mode === 'action-then-stopped') {
          process.stdout.write([
            {version:1,type:'action',id:'enabled',revision:value.revision},
            {version:1,type:'stopped'},
          ].map(JSON.stringify).join('\\n')+'\\n');
        }
        if (mode === 'oversize') process.stdout.write('x'.repeat(256*1024+1));
        if (mode === 'partial') process.stdout.end('{');
        if (mode === 'eof') process.stdout.end();
        if (mode === 'stalled-input') input.pause();
      }
      if (mode === 'repeat-actions') send({type:'action',id:'enabled',revision:value.revision});
    });
    input.on('close', () => {
      if (mode !== 'ignore-quit' && mode !== 'eof' && mode !== 'stalled-input') { clearInterval(alive); process.exit(0); }
    });
  `;
}

async function start(t: TestContext, mode = 'normal', overrides: Partial<CompanionOptions> = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'companion-client-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const diagnostics: string[] = [];
  const session = await runCompanion({
    appId: 'test.client', name: 'Client test', title: 'Te', stateDir: dir,
    binary: process.execPath, binaryArgs: ['--input-type=module', '-e', fakeRunner(mode), '--'],
    timeoutMs: 150, refreshMs: 100,
    snapshot: () => items, onAction: () => {},
    onDiagnostic: code => diagnostics.push(code), ...overrides,
  });
  t.after(() => session.quit());
  return { session, diagnostics };
}

async function eventually(predicate: () => boolean, timeout = 1500): Promise<void> {
  const deadline = Date.now() + timeout;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error('Condition did not become true');
    await delay(10);
  }
}

test('initial snapshot has a deadline even when its callback ignores cancellation', { timeout: 3000 }, async () => {
  let calls = 0;
  let signal: AbortSignal | undefined;
  await assert.rejects(runCompanion({
    appId: 'test.initial', name: 'Initial test', title: 'Te', stateDir: tmpdir(),
    binary: process.execPath, timeoutMs: 100,
    snapshot: current => { calls++; signal = current; return new Promise(() => {}); },
    onAction: () => {},
  }), /callback-aborted/);
  assert.equal(calls, 1);
  assert.equal(signal?.aborted, true);
});

test('pre-ready, stale, disabled, unknown and duplicate-in-flight actions do not dispatch', { timeout: 4000 }, async t => {
  const calls: string[] = [];
  const { session } = await start(t, 'actions', { onAction: async id => { calls.push(id); await delay(30); } });
  assert.equal(await session.ready, 'running');
  await eventually(() => calls.length === 1);
  await delay(170);
  assert.deepEqual(calls, ['enabled']);
});

test('action rejection reconciles state without retrying the mutation', { timeout: 4000 }, async t => {
  let calls = 0, reads = 0;
  const { session, diagnostics } = await start(t, 'actions', {
    snapshot: () => { reads++; return items; },
    onAction: () => { calls++; throw new Error('private provider failure'); },
  });
  await session.ready;
  await eventually(() => diagnostics.includes('action-indeterminate') && reads >= 2);
  assert.equal(calls, 1);
  assert.equal(diagnostics.filter(value => value === 'action-indeterminate').length, 1);
  assert.ok(diagnostics.every(value => !value.includes('private')));
});

test('a queued callback is not invoked after the same output batch stops the session', { timeout: 4000 }, async t => {
  let calls = 0;
  const { session } = await start(t, 'action-then-stopped', { onAction: () => { calls++; } });
  await session.ready;
  await session.closed;
  assert.equal(calls, 0);
});

test('timed-out action remains single-flight while an uncooperative callback is unresolved', { timeout: 4000 }, async t => {
  let calls = 0;
  let signal: AbortSignal | undefined;
  const { session, diagnostics } = await start(t, 'repeat-actions', {
    onAction: (_id, current) => { calls++; signal = current; return new Promise(() => {}); },
  });
  await session.ready;
  await eventually(() => diagnostics.includes('action-indeterminate'));
  await delay(250);
  assert.equal(calls, 1);
  assert.equal(signal?.aborted, true);
  assert.equal(diagnostics.filter(value => value === 'action-indeterminate').length, 1);
});

test('timed-out snapshot returns while retaining the read lock until callback settlement', { timeout: 4000 }, async t => {
  let reads = 0;
  const { session, diagnostics } = await start(t, 'normal', {
    snapshot: () => ++reads === 1 ? items : new Promise(() => {}),
  });
  await session.ready;
  await session.refresh();
  assert.ok(diagnostics.includes('snapshot-unavailable'));
  await session.refresh();
  await delay(250);
  assert.equal(reads, 2);
});

test('duplicate ready events do not create duplicate refresh intervals', { timeout: 4000 }, async t => {
  let reads = 0;
  const { session } = await start(t, 'duplicate-ready', { snapshot: () => { reads++; return items; } });
  await session.ready;
  await delay(260);
  assert.ok(reads >= 2 && reads <= 4, `Expected one refresh timer, observed ${reads} reads`);
});

for (const mode of ['oversize', 'partial']) {
  test(`invalid ${mode} output closes the child with a bounded diagnostic`, { timeout: 4000 }, async t => {
    const { session, diagnostics } = await start(t, mode);
    await session.ready;
    await session.closed;
    assert.ok(diagnostics.includes(mode === 'oversize' ? 'oversize-runner-frame' : 'invalid-runner-frame'));
  });
}

test('stdout EOF closes a still-running child rather than waiting forever', { timeout: 4000 }, async t => {
  const { session, diagnostics } = await start(t, 'eof');
  await session.ready;
  await session.closed;
  assert.ok(diagnostics.includes('runner-output-closed'));
});

test('quit aborts active work and escalates when the runner ignores graceful shutdown', { timeout: 4000 }, async t => {
  let reads = 0;
  let signal: AbortSignal | undefined;
  const { session } = await start(t, 'ignore-quit', {
    snapshot: current => { signal = current; return ++reads === 1 ? items : new Promise(() => {}); },
  });
  await session.ready;
  const refresh = session.refresh();
  await delay(10);
  await session.quit();
  await refresh;
  assert.equal(signal?.aborted, true);
});

test('a blocked snapshot pipe ends the session without accumulating queued refreshes', { timeout: 4000 }, async t => {
  let reads = 0;
  const large: MenuItem[] = Array.from({ length: 256 }, (_, i) => ({
    kind: 'action', id: `a${i}${'x'.repeat(240)}`, label: 'L'.repeat(250),
  }));
  const { session, diagnostics } = await start(t, 'stalled-input', {
    snapshot: () => ++reads === 1 ? items : large,
  });
  await session.ready;
  await session.refresh();
  await session.closed;
  assert.ok(diagnostics.includes('runner-pipe-closed'));
  assert.ok(reads <= 3, `Unexpected queued refreshes: ${reads}`);
});
