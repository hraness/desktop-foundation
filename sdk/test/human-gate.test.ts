import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  ChallengeIssuer, detectAgent, normalizeCode, oneTimeCode, ownerAuthorize, ownerChallenge, redeemChallenge, requireHuman, type GateTerminal,
} from '../src/human-gate.js';

function fakeTerminal(opts: { foreground?: boolean; answer?: (prompt: string) => string | null }) {
  let written = '';
  let closed = false;
  const term: GateTerminal = {
    isForeground: async () => opts.foreground ?? true,
    write: text => { written += text; },
    readLine: async () => opts.answer?.(written) ?? null,
    close: () => { closed = true; },
  };
  return { term, written: () => written, closed: () => closed };
}
const codeIn = (prompt: string) => /Type ([2-9A-Z]{3}-[2-9A-Z]{3}) to confirm/.exec(prompt)![1];

test('the right code typed at the foreground terminal passes', async () => {
  const f = fakeTerminal({ answer: p => `${codeIn(p).toLowerCase().replace('-', ' ')}\n` });
  const result = await requireHuman({ title: 'Allow a1?', digest: '3f2a', tier: 'T1T2', terminal: f.term });
  assert.equal(result.ok, true);
  if (result.ok) assert.equal(result.proof.digest, '3f2a');
  assert.equal(f.closed(), true);
});

test('a background terminal, a wrong code and silence all fail', async () => {
  let f = fakeTerminal({ foreground: false, answer: p => codeIn(p) });
  assert.deepEqual((await requireHuman({ title: 't', digest: 'd', tier: 'T1T2', terminal: f.term }) as any).code, 'human-required');
  assert.equal(f.written(), '');
  f = fakeTerminal({ answer: () => 'AAA-AAA\n' });
  assert.equal((await requireHuman({ title: 't', digest: 'd', tier: 'T1T2', terminal: f.term }) as any).code, 'gate-failed');
  f = fakeTerminal({ answer: () => null });
  assert.equal((await requireHuman({ title: 't', digest: 'd', tier: 'T1T2', terminal: f.term, ttlMs: 10 }) as any).code, 'gate-expired');
});

test('the prompt strips control characters from the title and digest', async () => {
  const f = fakeTerminal({ answer: () => null });
  await requireHuman({ title: 'Allow\x1b[2J\x07 me', digest: 'ab\rcd', tier: 'T1T2', terminal: f.term, ttlMs: 1 });
  assert.doesNotMatch(f.written().split('Type')[0].replace(/\n/g, ''), /[\x00-\x1f]/);
  assert.match(f.written(), /Allow\[2J me/);
});

test('T3 is reserved', async () => {
  assert.equal((await requireHuman({ title: 't', digest: 'd', tier: 'T3' }) as any).code, 'unsupported-platform');
  await assert.rejects(ownerAuthorize('/bin/false', { reason: 'r', digest: 'd' }), (e: any) => e.code === 'unsupported-platform');
});

test('without a controlling terminal the gate answers human-required and never reads stdin', { skip: process.platform === 'win32' ? 'no /dev/tty on Windows' : false }, () => {
  // setsid is not portable; a detached child has no controlling terminal.
  const script = `import(${JSON.stringify(new URL('../src/human-gate.js', import.meta.url).href)}).then(async m => { const r = await m.requireHuman({ title: 't', digest: 'd', tier: 'T1T2' }); process.stdout.write(JSON.stringify(r)); })`;
  const run = spawnSync(process.execPath, ['--input-type=module', '-e', script], { input: 'AAA-AAA\n', encoding: 'utf8', detached: true, timeout: 20_000 } as any);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(JSON.parse(run.stdout).code, 'human-required');
});

test('codes use the unambiguous alphabet', () => {
  for (let i = 0; i < 200; i++) assert.match(oneTimeCode(), /^[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{3}-[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{3}$/);
  assert.equal(normalizeCode(' ab3-x7k\n'), 'AB3X7K');
});

test('challenges are single-use, bound to the verb and digest, unforgeable and expiring', () => {
  const issuer = new ChallengeIssuer();
  const c = issuer.issue({ verb: 'approvals decide', digest: '3f2a', ttlMs: 1000 });
  assert.equal((issuer.redeem(c, 'approvals decide', 'ffff') as any).code, 'digest-mismatch');
  assert.equal((issuer.redeem(c, 'other', '3f2a') as any).code, 'digest-mismatch');
  assert.equal((issuer.redeem({ ...c, digest: 'ffff' }, 'approvals decide', 'ffff') as any).code, 'gate-failed');
  assert.equal((new ChallengeIssuer().redeem(c, 'approvals decide', '3f2a') as any).code, 'gate-failed');
  assert.equal((issuer.redeem(c, 'approvals decide', '3f2a', c.expiresAtMs + 1) as any).code, 'gate-expired');
  assert.deepEqual(issuer.redeem(c, 'approvals decide', '3f2a'), { ok: true });
  assert.equal((issuer.redeem(c, 'approvals decide', '3f2a') as any).code, 'gate-failed');
  // A redeemed challenge stays spent when its fields come back in another JSON shape.
  const wire = JSON.parse(JSON.stringify(c));
  for (const shaped of [{ ...wire, id: [c.id] }, { ...wire, id: [[c.id]] }, { ...wire, verb: [c.verb] }, { ...wire, digest: [c.digest] },
    { ...wire, mac: [c.mac] }, { ...wire, expiresAtMs: String(c.expiresAtMs) }, { ...wire, expiresAtMs: [c.expiresAtMs] }, null, 'x']) {
    assert.equal((issuer.redeem(shaped as any, 'approvals decide', '3f2a') as any).code, 'gate-failed', JSON.stringify(shaped));
  }
  // Wrong types are rejected before the single-use check, so a fresh challenge sent with an array id is not spent by it.
  const fresh = issuer.issue({ verb: 'approvals decide', digest: '3f2a' });
  assert.equal((issuer.redeem({ ...fresh, id: [fresh.id] } as any, 'approvals decide', '3f2a') as any).code, 'gate-failed');
  assert.deepEqual(issuer.redeem(JSON.parse(JSON.stringify(fresh)), 'approvals decide', '3f2a'), { ok: true });
  assert.equal((issuer.redeem(JSON.parse(JSON.stringify(fresh)), 'approvals decide', '3f2a') as any).code, 'gate-failed');
  const d = ownerChallenge({ verb: 'v', digest: 'd' });
  assert.deepEqual(redeemChallenge(d, 'v', 'd'), { ok: true });
  assert.equal(redeemChallenge(d, 'v', 'd').ok, false);
});

test('agent markers are advisory detection only', () => {
  assert.deepEqual(detectAgent({}, ['zsh', 'login']), { agent: false, markers: [] });
  assert.equal(detectAgent({ CLAUDECODE: '1' }).agent, true);
  assert.equal(detectAgent({}, ['node', '/usr/local/bin/codex']).agent, true);
});
