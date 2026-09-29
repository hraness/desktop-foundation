import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  commandsJson, defineRegistry, envelopeExitCode, HELP_SCHEMA, ERROR_CODES, errorEnvelope, EXIT, exitCodeFor, formatCommand, HranessError,
  isErrorCode, lookupVerb, okEnvelope, parseArgs, runCli, validProductCode, validProductName, validSchemaId, validVerbSegment,
  type CliIO, type Envelope, type Verb,
} from '../src/registry.js';
import { ownerPaths } from '../src/control.js';
import { AGENT_ENV_MARKERS, AGENT_PROCESS_NAMES } from '../src/human-gate.js';
import { readContract, validate } from './contract-helpers.js';

const AT = new Date('2026-09-28T00:00:00.000Z');
const schema = readContract('envelope.schema.json');

function example(ran: string[] = []) {
  const verbs: Verb<any, any>[] = [
    { path: ['status'], opClass: 'read', schema: 'example.status/1', summary: 'One-screen health', input: () => ({}), run: async () => { ran.push('status'); return { owner: 'running' }; }, text: o => `owner ${o.owner}` },
    {
      path: ['approvals', 'decide'], opClass: 'decide', schema: 'example.approval/1', summary: 'Allow or deny a waiting request',
      flags: ['confirm'],
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
  for (const name of ['envelope-ok.json', 'envelope-error.json', 'envelope-human-required.json', 'envelope-product-code.json', 'envelope-permission.json', 'commands.json']) {
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
  assert.equal(r.err(), 'FAIL Unknown command "nope".\n-> example --help\n');
});

const UTF8 = { LANG: 'en_US.UTF-8' };

test('runCli: text errors are one ✗ sentence and one → step, with "Did you mean" for a near miss', async () => {
  const cases: [string[], string][] = [
    [['stats'], '✗ Unknown command "stats". Did you mean "status"?\n→ example --help\n'],
    [['approvals', 'decid'], '✗ Unknown command "approvals decid". Did you mean "approvals decide"?\n→ example approvals --help\n'],
    [['approvals'], '✗ Name a command after "approvals".\n→ example approvals --help\n'],
    [[], '✗ Name a command.\n→ example --help\n'],
    [['comands'], '✗ Unknown command "comands". Did you mean "commands"?\n→ example --help\n'],
    [['zzzzzz'], '✗ Unknown command "zzzzzz".\n→ example --help\n'],
    [['status', '--frce'], '✗ Unknown option "--frce" for "example status", so nothing ran.\n→ example status --help\n'],
    [['control', 'stop'], '✗ No owner.\n→ example control stop --help\n'],
  ];
  for (const [argv, want] of cases) {
    const r = io({ audience: 'human', env: UTF8 });
    const code = await runCli(example(), argv, r.value);
    assert.ok(code === 2 || code === 4, argv.join(' '));
    assert.equal(r.out(), '', argv.join(' '));
    assert.equal(r.err(), want, argv.join(' '));
    const [first, second] = r.err().split('\n');
    assert.equal(first.match(/[.?!](\s|$)/g)?.length ?? 0, first.includes('Did you mean') ? 2 : 1, `one sentence (plus the suggestion): ${first}`);
    assert.match(second, /^→ \S/);
  }
  // HRANESS_DEBUG adds the code, and nothing else changes.
  const r = io({ audience: 'human', env: { ...UTF8, HRANESS_DEBUG: '1' } });
  await runCli(example(), ['stats'], r.value);
  assert.equal(r.err(), '✗ Unknown command "stats". Did you mean "status"?\n→ example --help\n  code: usage\n');
});

test('runCli: an agent gets the JSON envelope without --json', async () => {
  const ran: string[] = [];
  for (const extra of [{ audience: 'agent' as const }, { env: { CLAUDECODE: '1' } }]) {
    let r = io(extra);
    assert.equal(await runCli(example(ran), ['stats'], r.value), 2);
    assert.equal(r.err(), '');
    const envelope = r.json() as any;
    assert.deepEqual(validate(schema, envelope), []);
    assert.equal(envelope.error.message, 'Unknown command "stats". Did you mean "status"?');
    assert.deepEqual(envelope.error.next.map((n: any) => n.audience), ['agent', 'human']);
    r = io(extra);
    assert.equal(await runCli(example(ran), ['status'], r.value), 0);
    assert.deepEqual((r.json() as any).data, { owner: 'running' });
    r = io(extra);
    assert.equal(await runCli(example(ran), ['--help'], r.value), 0);
    assert.equal((r.json() as any).schema, HELP_SCHEMA);
    // A decision still answers human-required and never prompts.
    r = io({ ...extra, gate: async () => { throw new Error('prompted'); } });
    assert.equal(await runCli(example(ran), ['approvals', 'decide', 'a1', 'allow-once'], r.value), 3);
    assert.equal((r.json() as any).error.code, 'human-required');
  }
  assert.deepEqual(ran, ['status', 'status']);
  // A person without --json still reads text.
  const r = io({ audience: 'human', env: UTF8 });
  assert.equal(await runCli(example(), ['status'], r.value), 0);
  assert.equal(r.out(), 'owner running\n');
});

test('help and error text: sentence-case Usage, `help <cmd>`, and no "gate"', async () => {
  const reg = example();
  const texts: string[] = [];
  for (const argv of [['--help'], ['help'], ['help', 'approvals', 'decide'], ['approvals', '--help'], ['help', 'status'], ['commands'], ['help', 'commands'], ['nope'], ['approvals', 'decide', 'a1', 'allow-once']]) {
    const r = io({ audience: 'human', env: UTF8, gate: async () => ({ ok: false as const, code: 'gate-failed' as const, message: 'The code did not match, so nothing changed.' }) });
    await runCli(reg, argv, r.value);
    texts.push(r.out() + r.err());
  }
  for (const text of texts) {
    assert.doesNotMatch(text, /\bgate\b/i, text);
    assert.doesNotMatch(text, /^usage:/m, text);
  }
  assert.match(texts[0], /^Usage: example <command> \[options\]\n/);
  assert.match(texts[0], /Run `example help <command>` for one command\./);
  assert.equal(texts[1], texts[0]);
  assert.match(texts[2], /^Usage: example approvals decide \[options\]\n\nAllow or deny a waiting request\nA person decides this at their own terminal\.\n/);
  assert.match(texts[3], /^Usage: example approvals <command> \[options\]/);
  assert.match(texts[3], /Run `example help approvals <command>`/);
});

test('error.permission is optional, round-trips and matches the schema', () => {
  const golden = readContract('golden/envelope-permission.json');
  assert.deepEqual(validate(schema, golden), []);
  const body = new HranessError('example.full-disk-access', golden.error.message, undefined, golden.error.next, golden.error.permission).toBody();
  assert.deepEqual(body, golden.error);
  assert.deepEqual(validate(schema, errorEnvelope({ code: 'permission-denied', message: 'm', permission: { kind: 'keychain', settingsUrl: null } })), []);
  assert.deepEqual(validate(schema, errorEnvelope({ code: 'permission-denied', message: 'm', permission: { kind: 'keychain' } })), []);
  assert.notDeepEqual(validate(schema, errorEnvelope({ code: 'permission-denied', message: 'm', permission: { settingsUrl: null } as any })), []);
  assert.notDeepEqual(validate(schema, errorEnvelope({ code: 'permission-denied', message: 'm', permission: { kind: 'keychain', settingsUrl: 'https://x' } })), []);
  assert.notDeepEqual(validate(schema, errorEnvelope({ code: 'permission-denied', message: 'm', permission: { kind: 'keychain', extra: 1 } as any })), []);
  assert.equal('permission' in new HranessError('usage', 'm').toBody(), false);
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
    assert.equal(envelope.error.next[0].command, 'example approvals decide a1 allow-once --confirm');
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

test('names follow contract/names.json, the same limits the Rust kit uses', () => {
  const names = readContract('names.json');
  const checks: Record<string, (v: string) => boolean> = { productName: validProductName, verbSegment: validVerbSegment, schemaId: validSchemaId, productCode: validProductCode };
  for (const [kind, check] of Object.entries(checks)) {
    const pattern = new RegExp(names[kind].pattern);
    for (const v of names.cases[kind].valid) { assert.equal(check(v), true, `${kind} ${v}`); assert.equal(pattern.test(v), true, `${kind} pattern ${v}`); }
    for (const v of names.cases[kind].invalid) { assert.equal(check(v), false, `${kind} ${v}`); assert.equal(pattern.test(v), false, `${kind} pattern ${v}`); }
  }
  const envelopeSchema = readContract('envelope.schema.json');
  for (const v of names.cases.schemaId.valid) assert.deepEqual(validate(envelopeSchema, { ok: true, schema: v, generatedAt: AT.toISOString(), data: {} }), [], v);
  for (const v of names.cases.productCode.valid) assert.equal(isErrorCode(v, v.split('.')[0]), true, v);
  const long = 'a'.repeat(33);
  assert.throws(() => defineRegistry(long, []), /Invalid product/);
  assert.throws(() => ownerPaths(long, {}), /Invalid product/);
  assert.throws(() => defineRegistry('example', [{ path: [long], opClass: 'read', schema: 'example.a/1', summary: 's', input: () => ({}), run: async () => ({}) }]), /Invalid verb path/);
  assert.throws(() => defineRegistry('example', [{ path: ['a'], opClass: 'read', schema: 'example', summary: 's', input: () => ({}), run: async () => ({}) }]), /invalid schema/);
  assert.doesNotThrow(() => defineRegistry('example', [{ path: ['a'], opClass: 'read', schema: 'example.2fa/1', summary: 's', input: () => ({}), run: async () => ({}) }]));
});

function decideRegistry(ran: string[]) {
  const decide: Verb<{ id: string; digest: string; decision: string }, unknown> = {
    path: ['approvals', 'decide'], opClass: 'decide', schema: 'example.approval/1', summary: 'Allow or deny a waiting request',
    valueFlags: ['digest'],
    input: a => {
      if (a.positionals.length !== 2 || typeof a.flags.digest !== 'string') throw new HranessError('usage', 'Name a request, --digest and a decision.');
      return { id: a.positionals[0], digest: a.flags.digest, decision: a.positionals[1] };
    },
    gate: { tier: 'T1T2', describe: i => ({ title: `Allow ${i.id}?`, digest: i.digest }) },
    operateWhen: { summary: 'deny', test: i => i.decision === 'deny' },
    run: async i => { ran.push(`${i.decision} ${i.id} ${i.digest}`); return i; },
  };
  return defineRegistry('example', [decide]);
}

test('operateWhen: an agent may deny on the decide path; allowing still needs a person', async () => {
  const ran: string[] = [];
  let r = io({ audience: 'agent' });
  assert.equal(await runCli(decideRegistry(ran), ['approvals', 'decide', 'a1', '--digest', 'abc', 'deny', '--json'], r.value), 0);
  assert.deepEqual(ran, ['deny a1 abc']);
  r = io({ audience: 'agent' });
  assert.equal(await runCli(decideRegistry(ran), ['approvals', 'decide', 'a1', '--digest=abc', 'allow-once', '--json'], r.value), 3);
  assert.equal(r.json().ok, false);
  assert.deepEqual(ran, ['deny a1 abc']);
  const listed = (commandsJson(decideRegistry([]), AT) as any).data.verbs[0];
  assert.equal(listed.opClass, 'decide');
  assert.equal(listed.operateWhen, 'deny');
  const base = { path: ['x'], schema: 'example.x/1', summary: 's', input: () => ({}), run: async () => ({}) };
  assert.throws(() => defineRegistry('example', [{ ...base, opClass: 'decide-legacy', operateWhen: { summary: 'deny', test: () => true } }]), /operateWhen/);
  assert.throws(() => defineRegistry('example', [{ ...base, opClass: 'operate', gate: { tier: 'T1T2', describe: () => ({ title: '', digest: '' }) } }]), /cannot have a gate/);
});

test('value flags: --flag value parses, and the human next command keeps every flag', async () => {
  assert.deepEqual(parseArgs(['approvals', 'decide', 'a1', '--digest', 'abc', 'deny'], ['digest']), { positionals: ['approvals', 'decide', 'a1', 'deny'], flags: { digest: 'abc' } });
  assert.deepEqual(parseArgs(['tui', '--width', '40'], ['width']), { positionals: ['tui'], flags: { width: '40' } });
  assert.throws(() => parseArgs(['a', '--digest'], ['digest']), /needs a value/);
  assert.throws(() => parseArgs(['a', '--digest', '--json'], ['digest']), /needs a value/);
  const r = io({ audience: 'agent' });
  assert.equal(await runCli(decideRegistry([]), ['approvals', 'decide', 'a1', '--digest', 'abc', 'allow-once', '--json'], r.value), 3);
  const command = (r.json() as any).error.next[0].command;
  assert.equal(command, 'example approvals decide a1 allow-once --digest=abc');
  // The command reads back to the same input.
  const words = command.split(' ').slice(1);
  assert.deepEqual(parseArgs(words, ['digest']), { positionals: ['approvals', 'decide', 'a1', 'allow-once'], flags: { digest: 'abc' } });
  assert.equal(formatCommand('example', ['x'], { positionals: ['--odd'], flags: { a: 'b c' } }), "example x '--a=b c' -- --odd");
  // A usage error from the value parse is an envelope, not a throw.
  const bad = io({ audience: 'agent' });
  assert.equal(await runCli(decideRegistry([]), ['approvals', 'decide', 'a1', 'deny', '--json', '--digest'], bad.value), 2);
  assert.equal((bad.json() as any).error.code, 'usage');
});

test('raw verbs own stdout and their exit status', async () => {
  const tui: Verb<unknown, number> = {
    path: ['tui'], opClass: 'read', schema: 'example.status/1', summary: 'Terminal view', output: 'raw', valueFlags: ['width'],
    input: a => a,
    run: async (a: any) => { out.push(`snapshot ${a.flags.width ?? 80}`); return 4; },
  };
  const out: string[] = [];
  const reg = defineRegistry('example', [tui]);
  let r = io();
  assert.equal(await runCli(reg, ['tui', '--width', '40', '--json'], r.value), 4);
  assert.equal(r.out(), '');
  assert.deepEqual(out, ['snapshot 40']);
  const broken = defineRegistry('example', [{ ...tui, run: async () => 'nope' as any }]);
  r = io();
  assert.equal(await runCli(broken, ['tui', '--json'], r.value), 1);
  assert.equal((r.json() as any).error.code, 'internal');
  assert.throws(() => defineRegistry('example', [{ ...tui, output: 'weird' as any }]), /output mode/);
});

test('runCli: --help prints help, exits 0 and never runs the verb', async () => {
  let runs = 0;
  let prompted = 0;
  const stop: Verb<unknown, { stopping: boolean }> = {
    path: ['control', 'stop'], opClass: 'operate', schema: 'example.stop/1', summary: 'Stop the owner',
    input: () => ({}), run: async () => { runs++; return { stopping: true }; },
  };
  const decide: Verb<any, any> = {
    path: ['approvals', 'decide'], opClass: 'decide', schema: 'example.approval/1', summary: 'Allow or deny a waiting request',
    usage: '<id> <allow-once|deny>', valueFlags: ['digest'], flags: ['dry-run'],
    input: a => a, run: async () => { runs++; return {}; },
    gate: { tier: 'T1T2', describe: () => ({ title: 't', digest: 'd' }) },
  };
  const reg = defineRegistry('example', [stop, decide]);
  const gate = async () => { prompted++; return { ok: true as const, proof: { tier: 'T1T2' as const, digest: 'd', confirmedAt: AT.toISOString() } }; };
  for (const argv of [['control', 'stop', '--help'], ['control', 'stop', '-h'], ['--help', 'control', 'stop'], ['control', '--help', 'stop']]) {
    const r = io({ gate });
    assert.equal(await runCli(reg, argv, r.value), 0, argv.join(' '));
    assert.match(r.out(), /^Usage: example control stop \[options\]\n\nStop the owner\n\nOptions\n/);
  }
  let r = io({ gate });
  assert.equal(await runCli(reg, ['approvals', 'decide', 'a1', 'allow-once', '--digest', 'd', '--help'], r.value), 0);
  assert.match(r.out(), /Usage: example approvals decide <id> <allow-once\|deny> \[options\]/);
  assert.match(r.out(), /--digest <value>\n  --dry-run\n/);
  assert.match(r.out(), /A person decides this at their own terminal\./);
  r = io({ gate });
  assert.equal(await runCli(reg, ['control', 'stop', '--help', '--json'], r.value), 0);
  assert.equal((r.json() as any).schema, HELP_SCHEMA);
  assert.deepEqual((r.json() as any).data.verbs.map((v: any) => v.path.join(' ')), ['control stop']);
  // A group and the top level list their verbs.
  r = io({ gate });
  assert.equal(await runCli(reg, ['--help'], r.value), 0);
  assert.match(r.out(), /example control stop {6}Stop the owner/);
  assert.match(r.out(), /example approvals decide {2}Allow or deny a waiting request/);
  r = io({ gate });
  assert.equal(await runCli(reg, ['approvals', '-h'], r.value), 0);
  assert.doesNotMatch(r.out(), /control stop/);
  r = io({ gate });
  assert.equal(await runCli(reg, ['nope', '--help'], r.value), 2);
  assert.equal(runs, 0);
  assert.equal(prompted, 0);
  // After `--`, --help is an ordinary positional.
  r = io({ gate });
  assert.equal(await runCli(reg, ['control', 'stop', '--json', '--', '--help'], r.value), 0);
  assert.equal(runs, 1);
});

test('runCli: undeclared flags are usage errors and never run the verb', async () => {
  let runs = 0;
  const stop: Verb<unknown, { stopping: boolean }> = {
    path: ['control', 'stop'], opClass: 'operate', schema: 'example.stop/1', summary: 'Stop the owner', flags: ['force'], valueFlags: ['wait'],
    input: () => ({}), run: async () => { runs++; return { stopping: true }; },
  };
  const reg = defineRegistry('example', [stop]);
  for (const argv of [['control', 'stop', '--frce', '--json'], ['control', 'stop', '--hlep', '--json'], ['control', 'stop', '--force=yes', '--json']]) {
    const r = io();
    assert.equal(await runCli(reg, argv, r.value), 2, argv.join(' '));
    assert.equal((r.json() as any).error.code, 'usage');
  }
  const r = io();
  assert.equal(await runCli(reg, ['control', 'stop', '--frce', '--json'], r.value), 2);
  assert.equal((r.json() as any).error.next[0].command, 'example control stop --help');
  assert.equal(runs, 0);
  assert.equal(await runCli(reg, ['control', 'stop', '--force', '--wait', '5', '--json'], io().value), 0);
  assert.equal(runs, 1);
  assert.throws(() => defineRegistry('example', [{ ...stop, flags: ['help'] }]), /invalid flag/);
  assert.throws(() => defineRegistry('example', [{ ...stop, flags: ['wait'] }]), /invalid flag/);
  assert.throws(() => defineRegistry('example', [{ ...stop, valueFlags: ['help'] }]), /invalid value flag/);
});
