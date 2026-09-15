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
