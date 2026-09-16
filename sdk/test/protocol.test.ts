import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseRunnerEvent, validateSnapshot, type Snapshot, type MenuItem } from '../src/protocol.js';
const snapshot = (items: readonly MenuItem[] = []): Snapshot => ({ version: 1, type: 'snapshot', appId: 'org.example.companion', name: 'Example', title: 'Ex', revision: 1, items });
test('menus preserve enabled action authority and reject duplicate IDs', () => {
  assert.deepEqual([...validateSnapshot(snapshot([{ kind: 'action', id: 'pause', label: 'Pause', checked: false }, { kind: 'action', id: 'open', label: 'Open', enabled: false }]))], [['pause', true], ['open', false]]);
  assert.throws(() => validateSnapshot(snapshot([{kind:'action',id:'same',label:'First'},{kind:'submenu',label:'More',items:[{kind:'action',id:'same',label:'Second'}]}])), /action-id/);
});
test('labels cannot introduce control characters, unsafe revisions or executable content', () => {
  for (const text of ['status\nsecret', '\u202eDanger', 'x'.repeat(257)]) assert.throws(() => validateSnapshot(snapshot([{kind:'label',label:text}])));
  assert.throws(() => validateSnapshot({...snapshot(), revision: Number.MAX_SAFE_INTEGER + 1}));
  assert.throws(() => validateSnapshot(snapshot([{ kind: 'action', id: 'foundation.quit', label: 'Run' }])));
  assert.throws(() => parseRunnerEvent('{"version":1,"type":"action","id":"x","revision":1,"command":"rm"}'));
});
test('frame and menu limits apply recursively', () => {
  assert.throws(() => validateSnapshot(snapshot(Array.from({length:257}, () => ({kind:'separator'})))));
  let nodes: readonly MenuItem[] = [{kind:'label',label:'Leaf'}];
  for (let i=0;i<8;i++) nodes=[{kind:'submenu',label:'Level',items:nodes}];
  assert.throws(() => validateSnapshot(snapshot(nodes)));
});
test('titles accept a badge or one emoji grapheme', () => {
  for (const title of ['Gg', 'A', '7x', '👻', '🧽', '🟠', '🤖', '📷', '⚔️', '📸'])
    validateSnapshot({ ...snapshot(), title });
  for (const title of ['', 'abc', '👻👻', '👻️x', '👨‍👩‍👧', '🇺🇸', '*', '📷️️', 'é'])
    assert.throws(() => validateSnapshot({ ...snapshot(), title }), /invalid-title/);
});
test('icons carry bounded base64 RGBA pixel data', () => {
  const rgba = Buffer.alloc(16, 7).toString('base64');
  validateSnapshot({ ...snapshot(), icon: { width: 2, height: 2, rgba } });
  for (const icon of [
    { width: 0, height: 2, rgba },
    { width: 2, height: 2, rgba: Buffer.alloc(12, 7).toString('base64') },
    { width: 65, height: 1, rgba: Buffer.alloc(65 * 4, 7).toString('base64') },
    { width: 2, height: 2, rgba: '!!!!' },
    { width: 2, height: 2, rgba: 'abc' },
    { width: 2, height: 2, rgba: '' },
    { width: 2, height: 2, rgba, opacity: 0.5 },
  ]) assert.throws(() => validateSnapshot({ ...snapshot(), icon: icon as Snapshot['icon'] }), /invalid-icon|unknown-protocol-field/);
});
