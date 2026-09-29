import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { actionFor, box, chooseMode, clean, fit, renderSnapshot, runTui, table, type View } from '../src/tui.js';
import { errorEnvelope, okEnvelope } from '../src/registry.js';
import { goldenPath } from './contract-helpers.js';

interface Status { owner: string; pending: { id: string; what: string }[] }
const state: Status = { owner: 'running', pending: [{ id: 'a1', what: 'codex wants network access to api.example.com' }, { id: 'b2', what: 'claude wants to read ~/Documents/notes' }] };
const statusView: View<Status> = {
  id: 'status', title: 'Status',
  render: (s, width) => box(`Owner ${s.owner}`, table(['ID', 'Waiting request'], s.pending.map(p => [p.id, p.what]), width - 2), width),
};
const helpView: View<Status> = { id: 'help', title: 'Help', render: () => ['Tab next view, r reload, q quit'] };
const AT = new Date('2026-09-28T00:00:00.000Z');

function sink() { let out = ''; return { stdout: { write: (t: string) => { out += t; } }, out: () => out }; }

test('mode: --json wins, then --snapshot, and a pipe gets a snapshot', () => {
  assert.equal(chooseMode(true, true, true), 'json');
  assert.equal(chooseMode(false, true, true), 'snapshot');
  assert.equal(chooseMode(false, false, false), 'snapshot');
  assert.equal(chooseMode(false, false, true), 'interactive');
});

test('snapshot goldens at widths 40, 80 and 120', async () => {
  for (const width of [40, 80, 120]) {
    const text = renderSnapshot([statusView], state, width);
    for (const line of text.split('\n')) assert.ok([...line].length <= width, `line wider than ${width}: ${line}`);
    const path = goldenPath(`tui-status-${width}.txt`);
    if (process.env.UPDATE_GOLDEN === '1') await writeFile(path, text);
    assert.equal(text, await readFile(path, 'utf8'), `tui-status-${width}.txt`);
  }
});

test('runTui: --json prints the same envelope status --json prints, snapshots print every view', async () => {
  const envelope = okEnvelope('example.status/1', state, undefined, AT);
  let io = sink();
  assert.equal(await runTui({ load: async () => envelope, views: [statusView, helpView], mode: 'json', io }), 0);
  assert.deepEqual(JSON.parse(io.out()), envelope);
  // Byte for byte what `status --json` prints: one compact line.
  assert.equal(io.out(), `${JSON.stringify(envelope)}\n`);
  assert.equal(io.out().split('\n').length, 2);
  io = sink();
  assert.equal(await runTui({ load: async () => envelope, views: [statusView, helpView], mode: 'snapshot', width: 40, io }), 0);
  assert.equal(io.out(), renderSnapshot([statusView, helpView], state, 40));
  assert.match(io.out(), /== Help ==\nTab next view/);
  io = sink();
  assert.equal(await runTui({ load: async () => errorEnvelope({ code: 'owner-unavailable', message: 'No owner.' }), views: [statusView], mode: 'snapshot', io }), 4);
  assert.equal(io.out(), 'owner-unavailable: No owner.\n');
  // Interactive without a terminal on stdin falls back to a snapshot.
  io = sink();
  assert.equal(await runTui({ load: async () => envelope, views: [helpView], mode: 'interactive', io }), 0);
  assert.equal(io.out(), '== Help ==\nTab next view, r reload, q quit\n');
});

test('product text cannot drive the terminal', () => {
  assert.equal(clean('a\x1b[2Jb'), 'ab');
  assert.equal(clean('b\x07c\td'), 'b c d');
  assert.equal(fit('héllo', 3), 'hél');
  assert.equal(fit('ab', 4), 'ab  ');
  const lines = box('T\x1b]0;x\x07', ['\x1b[31mred'], 12);
  assert.ok(lines.every(line => !/\x1b/.test(line)));
  assert.ok(lines.every(line => [...line].length === 12));
});

test('keys', () => {
  assert.equal(actionFor('\t'), 'next');
  assert.equal(actionFor('\x1b[Z'), 'previous');
  assert.equal(actionFor('r'), 'reload');
  for (const key of ['q', '\x1b', '\x03']) assert.equal(actionFor(key), 'quit');
  assert.equal(actionFor('x'), 'none');
});
