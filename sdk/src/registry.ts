// Verbs, the JSON envelope and exit codes shared by every Hraness CLI. The
// Rust twin is `hraness_control_kit::{envelope, registry}`; both read the
// same contract/ files. See docs/control.md.

import { detectAudience, type Audience } from './audience.js';
import { requireHuman, type GateProof } from './human-gate.js';

export type OpClass = 'read' | 'operate' | 'decide' | 'decide-legacy';
export type GateTier = 'T1T2' | 'T3';
export type NextAudience = 'agent' | 'human';

export const ERROR_SCHEMA = 'hraness.error/1';
export const COMMANDS_SCHEMA = 'hraness.commands/1';
export const EXIT = { ok: 0, failure: 1, usage: 2, humanRequired: 3, ownerUnavailable: 4, conflict: 5 } as const;

/** Shared codes and their exit statuses (contract/error-codes.json). */
export const ERROR_CODES = {
  'usage': 2, 'not-found': 1, 'permission-denied': 1, 'human-required': 3, 'gate-failed': 3, 'gate-expired': 3,
  'owner-unavailable': 4, 'control-already-running': 5, 'conflict': 5, 'digest-mismatch': 5,
  'unsupported-platform': 1, 'internal': 1,
} as const;
export type SharedErrorCode = keyof typeof ERROR_CODES;
/** A shared code, or a product code under the product's own prefix (`ghostget.policy-locked`). */
export type ErrorCode = SharedErrorCode | `${string}.${string}`;
const PRODUCT_CODE = /^[a-z][a-z0-9-]*\.[a-z0-9][a-z0-9.-]*$/;
const PRODUCT_NAME = /^[a-z][a-z0-9-]{0,63}$/;

export interface NextStep { command: string; why: string; audience: NextAudience }
export interface ErrorBody { code: ErrorCode; message: string; detail?: string; next?: NextStep[] }
export type Envelope<T> =
  | { ok: true; schema: string; generatedAt: string; data: T; next?: NextStep[] }
  | { ok: false; schema: typeof ERROR_SCHEMA; generatedAt: string; error: ErrorBody };

export function isErrorCode(code: string, product?: string): code is ErrorCode {
  if (Object.hasOwn(ERROR_CODES, code)) return true;
  return product !== undefined && PRODUCT_CODE.test(code) && code.startsWith(`${product}.`);
}
export function exitCodeFor(code: ErrorCode): number {
  return Object.hasOwn(ERROR_CODES, code) ? ERROR_CODES[code as SharedErrorCode] : EXIT.failure;
}

/** An error a verb throws to answer with a specific code. */
export class HranessError extends Error {
  constructor(readonly code: ErrorCode, message: string, readonly detail?: string, readonly next: NextStep[] = []) {
    super(message);
    this.name = 'HranessError';
  }
  toBody(): ErrorBody {
    return { code: this.code, message: this.message, ...(this.detail ? { detail: this.detail } : {}), ...(this.next.length ? { next: this.next } : {}) };
  }
}

const now = () => new Date().toISOString();
export function okEnvelope<T>(schema: string, data: T, next?: NextStep[], at: Date = new Date()): Envelope<T> {
  return { ok: true, schema, generatedAt: at.toISOString(), data, ...(next?.length ? { next } : {}) };
}
export function errorEnvelope(error: ErrorBody, at?: Date): Envelope<never> {
  return { ok: false, schema: ERROR_SCHEMA, generatedAt: at ? at.toISOString() : now(), error };
}
export function envelopeExitCode(envelope: Envelope<unknown>): number {
  return envelope.ok ? EXIT.ok : exitCodeFor(envelope.error.code);
}

export interface ParsedArgs {
  positionals: string[];
  /** `--name` is `true`; `--name=value` is the value. */
  flags: Record<string, string | true>;
}
export interface CliIO {
  stdout: { write(text: string): unknown };
  stderr: { write(text: string): unknown };
  env?: NodeJS.ProcessEnv;
  audience?: Audience;
  /** Replaces the /dev/tty gate in tests. */
  gate?: typeof requireHuman;
}
export interface VerbContext {
  product: string;
  json: boolean;
  audience: Audience;
  /** Set when the verb's gate was satisfied. */
  proof?: GateProof;
  io: CliIO;
}
export interface Verb<I = unknown, O = unknown> {
  path: readonly string[];
  opClass: OpClass;
  schema: string;
  summary: string;
  /** Parses arguments; throw `HranessError('usage', ...)` on bad input. */
  input: (argv: ParsedArgs) => I;
  run: (input: I, ctx: VerbContext) => Promise<O>;
  gate?: { tier: GateTier; describe: (input: I) => { title: string; digest: string; command?: string } };
  /** Plain text for people; JSON is printed when absent. */
  text?: (output: O) => string;
}
export interface VerbDescriptor { path: string[]; opClass: OpClass; schema: string; summary: string; gate?: GateTier }
export interface Registry { product: string; verbs: readonly Verb<any, any>[] }

const WORD = /^[a-z][a-z0-9-]{0,63}$/;
/** Rejects a missing op class, a `decide` verb without a gate, a gate on a `read` verb and repeated paths. */
export function defineRegistry(product: string, verbs: Verb<any, any>[]): Registry {
  if (!PRODUCT_NAME.test(product)) throw new Error(`Invalid product name: ${product}`);
  const seen = new Set<string>();
  for (const verb of verbs) {
    const name = verb.path.join(' ');
    if (!verb.path.length || !verb.path.every(w => WORD.test(w)) || verb.path[0] === 'commands') throw new Error(`Invalid verb path: ${name}`);
    if (!['read', 'operate', 'decide', 'decide-legacy'].includes(verb.opClass)) throw new Error(`Verb ${name} has no operation class.`);
    if (verb.opClass === 'decide' && !verb.gate) throw new Error(`Decide verb ${name} needs a gate.`);
    if (verb.opClass === 'read' && verb.gate) throw new Error(`Read verb ${name} cannot have a gate.`);
    if (verb.gate && !['T1T2', 'T3'].includes(verb.gate.tier)) throw new Error(`Verb ${name} has an unknown gate tier.`);
    if (seen.has(name)) throw new Error(`Verb ${name} is registered twice.`);
    seen.add(name);
  }
  return { product, verbs: [...verbs] };
}

export function describeVerb(verb: Verb<any, any>): VerbDescriptor {
  return { path: [...verb.path], opClass: verb.opClass, schema: verb.schema, summary: verb.summary, ...(verb.gate ? { gate: verb.gate.tier } : {}) };
}
export function commandsJson(reg: Registry, at?: Date): Envelope<{ product: string; verbs: VerbDescriptor[] }> {
  return okEnvelope(COMMANDS_SCHEMA, { product: reg.product, verbs: reg.verbs.map(describeVerb) }, undefined, at);
}

export function parseArgs(args: readonly string[]): ParsedArgs {
  const positionals: string[] = [];
  const flags: Record<string, string | true> = {};
  let rest = false;
  for (const arg of args) {
    if (rest || !arg.startsWith('--') || arg === '--') {
      if (arg === '--' && !rest) { rest = true; continue; }
      positionals.push(arg);
      continue;
    }
    const eq = arg.indexOf('=');
    const name = arg.slice(2, eq < 0 ? undefined : eq);
    if (!/^[a-z][a-z0-9-]*$/.test(name)) throw new HranessError('usage', `Unknown option ${arg}.`);
    flags[name] = eq < 0 ? true : arg.slice(eq + 1);
  }
  return { positionals, flags };
}

/** The verb with the longest path that prefixes `words`. */
export function lookupVerb(reg: Registry, words: readonly string[]): Verb<any, any> | undefined {
  return reg.verbs
    .filter(v => v.path.length <= words.length && v.path.every((w, i) => words[i] === w))
    .sort((a, b) => b.path.length - a.path.length)[0];
}

/** The refusal for a gated verb where no person can answer. It never prompts. */
export function humanRequired(reg: Registry, verb: Verb<any, any>, command: string): ErrorBody {
  return {
    code: 'human-required',
    message: `\`${reg.product} ${verb.path.join(' ')}\` is a decision for a person. Nothing changed.`,
    next: [{ command, why: 'Run this in your own terminal to decide.', audience: 'human' }],
  };
}

function shellWord(word: string): string {
  return /^[A-Za-z0-9_./:=@%+-]+$/.test(word) ? word : `'${word.replaceAll("'", `'\\''`)}'`;
}

/**
 * Runs one command line and returns its exit status. `commands` lists the
 * verbs. `--json` prints one envelope. A gated verb run with `--json` by an
 * agent (or with no one at the terminal) answers `human-required` (exit 3)
 * and never prompts; `HRANESS_AUDIENCE` and `--confirm` never satisfy a
 * gate. Otherwise the gate asks at `/dev/tty`.
 */
export async function runCli(reg: Registry, argv: readonly string[], io: CliIO): Promise<number> {
  const env = io.env ?? process.env;
  const audience = io.audience ?? detectAudience({ env });
  let parsed: ParsedArgs;
  let json = argv.includes('--json');
  const emit = (envelope: Envelope<unknown>, text?: string): number => {
    if (json || text === undefined) io.stdout.write(`${JSON.stringify(envelope)}\n`);
    else if (envelope.ok) io.stdout.write(text.endsWith('\n') ? text : `${text}\n`);
    if (!envelope.ok && !json) {
      const { error } = envelope;
      io.stderr.write(`${error.code}: ${error.message}\n${(error.next ?? []).map(n => `  next: ${n.command}  (${n.why})\n`).join('')}`);
    }
    return envelopeExitCode(envelope);
  };
  const fail = (error: unknown): number => {
    const body: ErrorBody = error instanceof HranessError ? error.toBody() : { code: 'internal', message: 'An unexpected failure.', detail: error instanceof Error ? error.message : String(error) };
    if (!isErrorCode(body.code, reg.product)) return emit(errorEnvelope({ code: 'internal', message: `The verb answered an undeclared code ${body.code}.` }));
    return emit(errorEnvelope(body));
  };
  try { parsed = parseArgs(argv); }
  catch (error) { return fail(error); }
  json = parsed.flags.json === true;
  const words = parsed.positionals;
  if (words[0] === 'commands') return emit(commandsJson(reg), reg.verbs.map(v => `${reg.product} ${v.path.join(' ')}  [${v.opClass}${v.gate ? ` ${v.gate.tier}` : ''}]  ${v.summary}`).join('\n'));
  const verb = lookupVerb(reg, words);
  if (!verb) return fail(new HranessError('usage', words.length ? `Unknown command: ${words.join(' ')}.` : 'Name a command.', undefined, [{ command: `${reg.product} commands --json`, why: 'List every verb', audience: 'agent' }]));
  const flags = { ...parsed.flags };
  delete flags.json;
  const args: ParsedArgs = { positionals: words.slice(verb.path.length), flags };
  const ctx: VerbContext = { product: reg.product, json, audience, io };
  try {
    const input = verb.input(args);
    if (verb.gate) {
      const described = verb.gate.describe(input);
      const command = described.command ?? [reg.product, ...verb.path, ...args.positionals].map(shellWord).join(' ');
      if (verb.gate.tier === 'T3') throw new HranessError('unsupported-platform', 'Gate tier T3 (OS owner authentication) is not supported yet. Nothing changed.');
      if (json && audience !== 'human') return emit(errorEnvelope(humanRequired(reg, verb, command)));
      const gate = await (io.gate ?? requireHuman)({ title: described.title, digest: described.digest, tier: verb.gate.tier });
      if (!gate.ok) {
        const body = gate.code === 'human-required' ? humanRequired(reg, verb, command) : { code: gate.code, message: gate.message };
        return emit(errorEnvelope(body));
      }
      ctx.proof = gate.proof;
    }
    const output = await verb.run(input, ctx);
    return emit(okEnvelope(verb.schema, output), verb.text?.(output));
  } catch (error) { return fail(error); }
}
