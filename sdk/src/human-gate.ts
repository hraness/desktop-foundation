// The human gate for `decide` verbs. The Rust twin is
// `hraness_control_kit::gate`; docs/human-gate.md has the threat model.
//
// T0 detectAgent  advisory agent markers; they pick wording, never satisfy a gate
// T1              a controlling terminal, with this process in its foreground group
// T2              a one-time code written to /dev/tty and typed back at /dev/tty
// T3              reserved for OS owner authentication: `unsupported-platform`
//
// T1+T2 stops an agent approving by accident or by piping text. It does not
// stop a determined process running as the same user, which can drive a
// pseudo-terminal.

import { execFile } from 'node:child_process';
import { createHmac, randomBytes, randomInt, timingSafeEqual } from 'node:crypto';
import { closeSync, openSync, writeSync } from 'node:fs';
import { ReadStream } from 'node:tty';

export type GateTierName = 'T0' | 'T1' | 'T2' | 'T3';
export interface GateProof { tier: 'T1T2'; digest: string; confirmedAt: string }
export type GateResult =
  | { ok: true; proof: GateProof }
  | { ok: false; code: 'human-required' | 'gate-failed' | 'gate-expired' | 'unsupported-platform'; message: string };

/** contract/agent-markers.json; a test keeps the two equal. */
export const AGENT_ENV_MARKERS = ['AI_AGENT', 'CLAUDECODE', 'CODEX_SANDBOX', 'CODEX_SANDBOX_NETWORK_DISABLED', 'CURSOR_AGENT', 'GEMINI_CLI'] as const;
export const AGENT_PROCESS_NAMES = ['claude', 'codex', 'cursor-agent', 'gemini', 'aider', 'devin'] as const;
export const DEFAULT_CODE_TTL_MS = 120_000;
export const DEFAULT_CHALLENGE_TTL_MS = 300_000;

/** T0: agent markers in `env` and in ancestor process names. Advisory only. */
export function detectAgent(env: NodeJS.ProcessEnv = process.env, ancestry: readonly string[] = []): { agent: boolean; markers: string[] } {
  const markers: string[] = [];
  for (const key of AGENT_ENV_MARKERS) {
    const value = env[key];
    if (value !== undefined && value !== '' && value !== '0') markers.push(`env:${key}`);
  }
  for (const name of ancestry) {
    const base = name.split('/').pop() ?? name;
    const marker = `process:${base}`;
    if ((AGENT_PROCESS_NAMES as readonly string[]).includes(base) && !markers.includes(marker)) markers.push(marker);
  }
  return { agent: markers.length > 0, markers };
}

function ps(args: string[]): Promise<string | undefined> {
  return new Promise(resolve => {
    execFile('ps', args, { env: { ...process.env, LC_ALL: 'C' }, timeout: 5000 }, (error, stdout) => resolve(error ? undefined : stdout.trim()));
  });
}
/** Names of the ancestors of `pid`, nearest first. Reads `ps`; never signals. */
export async function ancestorNames(pid: number = process.pid, limit = 16): Promise<string[]> {
  const names: string[] = [];
  let current = pid;
  for (let i = 0; i < limit; i++) {
    const parent = Number(await ps(['-o', 'ppid=', '-p', String(current)]));
    if (!Number.isSafeInteger(parent) || parent <= 1) break;
    const name = await ps(['-o', 'comm=', '-p', String(parent)]);
    if (!name) break;
    names.push(name);
    current = parent;
  }
  return names;
}

/** The terminal the gate talks to. The default is /dev/tty; tests inject their own. */
export interface GateTerminal {
  isForeground(): Promise<boolean>;
  write(text: string): void;
  /** One line, or `null` when `timeoutMs` passes first. */
  readLine(timeoutMs: number): Promise<string | null>;
  close(): void;
}

/** Opens the controlling terminal. Throws (ENXIO) without one. */
export function openDevTty(path = '/dev/tty'): GateTerminal {
  const fd = openSync(path, 'r+');
  let input: ReadStream | undefined;
  return {
    async isForeground() {
      // tpgid is the terminal's foreground group; pgid is ours.
      const out = await ps(['-o', 'tpgid=,pgid=', '-p', String(process.pid)]);
      const [tpgid, pgid] = (out ?? '').split(/\s+/).map(Number);
      return Number.isSafeInteger(tpgid) && tpgid > 0 && tpgid === pgid;
    },
    write(text) { writeSync(fd, text); },
    readLine(timeoutMs) {
      input ??= new ReadStream(fd);
      const stream = input;
      return new Promise(resolve => {
        let line = '';
        const done = (value: string | null) => { clearTimeout(timer); stream.off('data', onData); stream.off('end', onEnd); stream.pause(); resolve(value); };
        const onData = (chunk: Buffer) => { line += chunk.toString('utf8'); if (line.includes('\n') || line.length > 256) done(line); };
        const onEnd = () => done(line);
        const timer = setTimeout(() => done(null), timeoutMs);
        stream.on('data', onData); stream.once('end', onEnd); stream.resume();
      });
    },
    close() {
      if (input) { input.destroy(); input = undefined; } // closes fd
      else closeSync(fd);
    },
  };
}

// No 0/O/1/I/L, so a code survives being read aloud.
const ALPHABET = '23456789ABCDEFGHJKMNPQRSTUVWXYZ';
export function oneTimeCode(): string {
  const symbols = Array.from({ length: 6 }, () => ALPHABET[randomInt(ALPHABET.length)]).join('');
  return `${symbols.slice(0, 3)}-${symbols.slice(3)}`;
}
export const normalizeCode = (code: string) => code.trim().replace(/[\s-]/g, '').toUpperCase();
const sameText = (a: string, b: string) => {
  const x = Buffer.from(a), y = Buffer.from(b);
  return x.length === y.length && timingSafeEqual(x, y);
};
// eslint-disable-next-line no-control-regex
const printable = (text: string) => text.replace(/[\x00-\x1f\x7f-\x9f]/g, '');

export interface RequireHumanOptions {
  title: string;
  digest: string;
  tier: 'T1T2' | 'T3';
  /** The terminal device; defaults to /dev/tty. */
  tty?: string;
  ttlMs?: number;
  /** Replaces the terminal in tests. */
  terminal?: GateTerminal;
}

/** T1+T2. Without a controlling terminal it answers `human-required`; it never reads stdin. */
export async function requireHuman(opts: RequireHumanOptions): Promise<GateResult> {
  if (opts.tier === 'T3') return { ok: false, code: 'unsupported-platform', message: 'Gate tier T3 (OS owner authentication) is not supported yet. Nothing changed.' };
  const ttl = opts.ttlMs ?? DEFAULT_CODE_TTL_MS;
  let term: GateTerminal;
  try { term = opts.terminal ?? openDevTty(opts.tty); }
  catch { return { ok: false, code: 'human-required', message: 'This decision needs a person at a terminal. Nothing changed.' }; }
  try {
    if (!await term.isForeground().catch(() => false)) return { ok: false, code: 'human-required', message: 'This decision needs the terminal in the foreground. Nothing changed.' };
    const code = oneTimeCode();
    term.write(`\n${printable(opts.title)}\nDigest: ${printable(opts.digest)}\nType ${code} to confirm (${Math.round(ttl / 1000)} s): `);
    const started = Date.now();
    const line = await term.readLine(ttl);
    if (line === null || Date.now() - started > ttl) {
      term.write('\nExpired. Nothing changed.\n');
      return { ok: false, code: 'gate-expired', message: 'The code expired. Nothing changed.' };
    }
    if (!sameText(normalizeCode(line), normalizeCode(code))) {
      term.write('The code did not match. Nothing changed.\n');
      return { ok: false, code: 'gate-failed', message: 'The code did not match. Nothing changed.' };
    }
    return { ok: true, proof: { tier: 'T1T2', digest: opts.digest, confirmedAt: new Date().toISOString() } };
  } catch {
    return { ok: false, code: 'human-required', message: 'The terminal failed. Nothing changed.' };
  } finally { term.close(); }
}

/** Single-use, bound to one verb and one digest, and expiring. */
export interface Challenge { id: string; verb: string; digest: string; expiresAtMs: number; mac: string }
export type RedeemResult = { ok: true } | { ok: false; code: 'gate-failed' | 'gate-expired' | 'digest-mismatch'; message: string };

const isChallenge = (c: unknown): c is Challenge => {
  if (typeof c !== 'object' || c === null) return false;
  const { id, verb, digest, expiresAtMs, mac } = c as Record<string, unknown>;
  return typeof id === 'string' && typeof verb === 'string' && typeof digest === 'string' && typeof mac === 'string' && Number.isSafeInteger(expiresAtMs);
};

/** Issues and redeems challenges with a per-process key. */
export class ChallengeIssuer {
  readonly #key = randomBytes(32);
  readonly #used = new Map<string, number>();
  #mac(id: string, verb: string, digest: string, expiresAtMs: number) {
    return createHmac('sha256', this.#key).update(`hraness.challenge/1\0${id}\0${verb}\0${digest}\0${expiresAtMs}`).digest('hex');
  }
  issue(opts: { verb: string; digest: string; ttlMs?: number }): Challenge {
    const id = randomBytes(16).toString('hex');
    const expiresAtMs = Date.now() + (opts.ttlMs ?? DEFAULT_CHALLENGE_TTL_MS);
    return { id, verb: opts.verb, digest: opts.digest, expiresAtMs, mac: this.#mac(id, opts.verb, opts.digest, expiresAtMs) };
  }
  redeem(c: Challenge, verb: string, digest: string, nowMs = Date.now()): RedeemResult {
    // A challenge usually arrives as parsed JSON. Check every field's type
    // before the MAC: `${[id]}` === id, so an id wrapped in an array would
    // pass the MAC and miss the single-use set below.
    if (!isChallenge(c) || !sameText(this.#mac(c.id, c.verb, c.digest, c.expiresAtMs), c.mac)) return { ok: false, code: 'gate-failed', message: 'The challenge is not valid.' };
    if (c.verb !== verb || c.digest !== digest) return { ok: false, code: 'digest-mismatch', message: 'The challenge is for a different decision.' };
    if (nowMs > c.expiresAtMs) return { ok: false, code: 'gate-expired', message: 'The challenge expired.' };
    for (const [id, expires] of this.#used) if (expires < nowMs) this.#used.delete(id);
    if (this.#used.has(c.id)) return { ok: false, code: 'gate-failed', message: 'The challenge was already used.' };
    this.#used.set(c.id, c.expiresAtMs);
    return { ok: true };
  }
}
const defaultIssuer = new ChallengeIssuer();
/** A challenge from this process's issuer. Redeem it with `redeemChallenge`. */
export function ownerChallenge(opts: { verb: string; digest: string; ttlMs?: number }): Challenge { return defaultIssuer.issue(opts); }
export function redeemChallenge(c: Challenge, verb: string, digest: string): RedeemResult { return defaultIssuer.redeem(c, verb, digest); }

/** T3 owner authentication. Reserved: always rejects with `unsupported-platform`. */
export async function ownerAuthorize(_helper: string, _opts: { reason: string; digest: string }): Promise<boolean> {
  const error = new Error('Gate tier T3 (OS owner authentication) is not supported yet.') as Error & { code: string };
  error.code = 'unsupported-platform';
  throw error;
}
