import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  commandsJson, defineRegistry, envelopeExitCode, ERROR_CODES, errorEnvelope, EXIT, exitCodeFor, HranessError,
  isErrorCode, lookupVerb, okEnvelope, parseArgs, runCli, type CliIO, type Envelope, type Verb,
} from '../src/registry.js';
import { AGENT_ENV_MARKERS, AGENT_PROCESS_NAMES } from '../src/human-gate.js';
import { readContract, validate } from './contract-helpers.js';

const AT = new Date('2026-09-28T00:00:00.000Z');
const schema = readContract('envelope.schema.json');

function example(ran: string[] = []) {
  const verbs: Verb<any, any>[] = [
    { path: ['status'], opClass: 'read', schema: 'example.status/1', summary: 'One-screen health', input: () => ({}), run: async () => { ran.push('status'); return { owner: 'running' }; }, text: o => `owner ${o.owner}` },
    {
      path: ['approvals', 'decide'], opClass: 'decide', schema: 'example.approval/1', summary: 'Allow or deny a waiting request',
      input: a => { if (a.positionals.length !== 2) throw new HranessError('usage', 'Name a request and a decision.'); return { id: a.positionals[0], decision: a.positionals[1] }; },
      gate: { tier: 'T1T2', describe: i => ({ title: `Allow ${i.id}?`, digest: '3f2a' }) },
      run: async i => { ran.push(`decide ${i.id}`); return { id: i.id, decision: i.decision }; },
    },
    { path: ['control', 'stop'], opClass: 'operate', schema: 'hraness.control/1', summary: 'Stop the owner', input: () => ({}), run: async () => { throw new HranessError('owner-unavailable', 'No owner.'); } },
    { path: ['locked'], opClass: 'operate', schema: 'example.locked/1', summary: 'Product code', input: () => ({}), run: async () => { throw new HranessError('example.policy-locked', 'Locked.'); } },
    { path: ['foreign'], opClass: 'operate', schema: 'example.foreign/1', summary: 'Foreign code', input: () => ({}), run: async () => { throw new HranessError('other.thing' as any, 'Not ours.'); } },
    { path: ['legacy'], opClass: 'decide-legacy', schema: 'example.legacy/1', summary: 'Kept as is', input: () => ({}), run: async () => ({}) },
  ];
  return defineRegistry('example', verbs);
}
function io(extra: Partial<CliIO> = {}) {
  let out = '', err = '';
  const value: CliIO = { stdout: { write: (t: string) => { out += t; } }, stderr: { write: (t: string) => { err += t; } }, env: {}, ...extra };
  return { value, out: () => out, err: () => err, json: () => JSON.parse(out) as Envelope<any> };
}

test('error codes and exit codes match contract/error-codes.json', () => {
  const contract = readContract('error-codes.json');
  assert.deepEqual(Object.fromEntries(contract.codes.map((c: any) => [c.code, c.exit])), ERROR_CODES);
  assert.deepEqual(Object.values(EXIT).sort(), Object.keys(contract.exitCodes).map(Number).sort());
  assert.equal(exitCodeFor('example.policy-locked'), 1);
  assert.equal(isErrorCode('example.policy-locked', 'example'), true);
  assert.equal(isErrorCode('other.policy-locked', 'example'), false);
  assert.equal(isErrorCode('nope'), false);
});

test('agent markers match contract/agent-markers.json', () => {
  const contract = readContract('agent-markers.json');
  assert.deepEqual([...AGENT_ENV_MARKERS], contract.env);
  assert.deepEqual([...AGENT_PROCESS_NAMES], contract.processNames);
});

test('every golden envelope passes the schema and a wrong one does not', () => {
  for (const name of ['envelope-ok.json', 'envelope-error.json', 'envelope-human-required.json', 'envelope-product-code.json', 'commands.json']) {
    assert.deepEqual(validate(schema, readContract(`golden/${name}`)), [], name);
  }
  assert.notDeepEqual(validate(schema, { ok: false, schema: 'hraness.error/2', generatedAt: AT.toISOString(), error: { code: 'usage', message: 'x' } }), []);
  assert.notDeepEqual(validate(schema, { ok: true, schema: 'example.status/1', generatedAt: '2026-09-28T00:00:00Z', data: {} }), []);
  assert.notDeepEqual(validate(schema, { ok: true, schema: 'example.status/1', generatedAt: AT.toISOString(), data: {}, extra: 1 }), []);
});

test('the TS registry prints the commands golden byte for byte', () => {
  assert.deepEqual(commandsJson(defineRegistry('example', example().verbs.slice(0, 3)), AT), readContract('golden/commands.json'));
  assert.deepEqual(okEnvelope('example.status/1', { a: 1 }, undefined, AT).generatedAt, AT.toISOString());
  assert.equal(envelopeExitCode(errorEnvelope({ code: 'digest-mismatch', message: 'x' })), 5);
});

test('defineRegistry refuses a decide verb without a gate, a gated read, repeats and bad names', () => {
  const base = { schema: 'x.y/1', summary: 's', input: () => ({}), run: async () => ({}) };
  assert.throws(() => defineRegistry('example', [{ ...base, path: ['allow'], opClass: 'decide' }]), /needs a gate/);
  assert.throws(() => defineRegistry('example', [{ ...base, path: ['look'], opClass: 'read', gate: { tier: 'T1T2', describe: () => ({ title: '', digest: '' }) } }]), /cannot have a gate/);
  assert.throws(() => defineRegistry('example', [{ ...base, path: ['a'], opClass: 'read' }, { ...base, path: ['a'], opClass: 'read' }]), /twice/);
  assert.throws(() => defineRegistry('example', [{ ...base, path: ['commands'], opClass: 'read' }]), /Invalid verb path/);
  assert.throws(() => defineRegistry('Example', []), /Invalid product/);
  assert.throws(() => defineRegistry('example', [{ ...base, path: ['a'], opClass: 'bogus' as any }]), /operation class/);
});

test('parseArgs and lookupVerb', () => {
  assert.deepEqual(parseArgs(['a', '--json', '--width=40', '--', '--x']), { positionals: ['a', '--x'], flags: { json: true, width: '40' } });
  assert.throws(() => parseArgs(['--Bad']), /Unknown option/);
  assert.equal(lookupVerb(example(), ['approvals', 'decide', 'a1'])?.path.join(' '), 'approvals decide');
  assert.equal(lookupVerb(example(), ['approvals']), undefined);
});

test('runCli: read verbs, text, commands and usage', async () => {
  let r = io();
  assert.equal(await runCli(example(), ['status', '--json'], r.value), 0);
  assert.deepEqual(validate(schema, r.json()), []);
  assert.deepEqual((r.json() as any).data, { owner: 'running' });
  r = io();
  assert.equal(await runCli(example(), ['status'], r.value), 0);
  assert.equal(r.out(), 'owner running\n');
  r = io();
  assert.equal(await runCli(example(), ['commands', '--json'], r.value), 0);
  assert.equal((r.json() as any).schema, 'hraness.commands/1');
  r = io();
  assert.equal(await runCli(example(), ['nope', '--json'], r.value), 2);
  assert.equal((r.json() as any).error.next[0].command, 'example commands --json');
  r = io();
  assert.equal(await runCli(example(), ['nope'], r.value), 2);
  assert.match(r.err(), /^usage: Unknown command: nope\./);
});

test('runCli: error codes map to exit codes, and undeclared codes become internal', async () => {
  let r = io();
  assert.equal(await runCli(example(), ['control', 'stop', '--json'], r.value), 4);
  r = io();
  assert.equal(await runCli(example(), ['locked', '--json'], r.value), 1);
  assert.equal((r.json() as any).error.code, 'example.policy-locked');
  r = io();
  assert.equal(await runCli(example(), ['foreign', '--json'], r.value), 1);
  assert.equal((r.json() as any).error.code, 'internal');
});

test('runCli: a gated verb with --json from an agent answers human-required (exit 3) and never prompts', async () => {
  const ran: string[] = [];
  let prompted = 0;
  const gate = async () => { prompted++; return { ok: true as const, proof: { tier: 'T1T2' as const, digest: '3f2a', confirmedAt: AT.toISOString() } }; };
  for (const audience of ['agent', 'quiet'] as const) {
    const r = io({ audience, gate, env: { HRANESS_AUDIENCE: 'human' } });
    assert.equal(await runCli(example(ran), ['approvals', 'decide', 'a1', 'allow-once', '--json', '--confirm'], r.value), 3);
    const envelope = r.json() as any;
    assert.deepEqual(validate(schema, envelope), []);
    assert.equal(envelope.error.code, 'human-required');
    assert.equal(envelope.error.next[0].command, 'example approvals decide a1 allow-once');
    assert.equal(envelope.error.next[0].audience, 'human');
  }
  assert.equal(prompted, 0);
  assert.deepEqual(ran, []);
});

test('runCli: a person passes the gate; a failed or absent gate changes nothing', async () => {
  const ran: string[] = [];
  const pass = async () => ({ ok: true as const, proof: { tier: 'T1T2' as const, digest: '3f2a', confirmedAt: AT.toISOString() } });
  let r = io({ audience: 'human', gate: pass });
  assert.equal(await runCli(example(ran), ['approvals', 'decide', 'a1', 'allow-once', '--json'], r.value), 0);
  assert.deepEqual(ran, ['decide a1']);
  for (const code of ['gate-failed', 'gate-expired', 'human-required'] as const) {
    r = io({ audience: 'human', gate: async () => ({ ok: false as const, code, message: 'no' }) });
    assert.equal(await runCli(example(ran), ['approvals', 'decide', 'a1', 'allow-once', '--json'], r.value), 3);
    assert.equal((r.json() as any).error.code, code);
  }
  // Without --json a person still has to pass the gate.
  r = io({ audience: 'human', gate: async () => ({ ok: false as const, code: 'gate-failed', message: 'The code did not match.' }) });
  assert.equal(await runCli(example(ran), ['approvals', 'decide', 'a1', 'allow-once'], r.value), 3);
  assert.deepEqual(ran, ['decide a1']);
});

test('runCli: T3 answers unsupported-platform', async () => {
  const reg = defineRegistry('example', [{ path: ['allow'], opClass: 'decide', schema: 'example.allow/1', summary: 's', input: () => ({}), gate: { tier: 'T3', describe: () => ({ title: 't', digest: 'd' }) }, run: async () => ({}) }]);
  const r = io({ audience: 'human' });
  assert.equal(await runCli(reg, ['allow', '--json'], r.value), 1);
  assert.equal((r.json() as any).error.code, 'unsupported-platform');
});
