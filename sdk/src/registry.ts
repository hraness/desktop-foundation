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
// contract/names.json; the Rust kit uses the same limits.
const PRODUCT_NAME = /^[a-z][a-z0-9-]{0,31}$/;
const VERB_SEGMENT = /^[a-z][a-z0-9-]{0,31}$/;
const SCHEMA_ID = /^[a-z][a-z0-9-]{0,31}(\.[a-z0-9][a-z0-9-]{0,31})+\/[0-9]{1,9}$/;
const PRODUCT_CODE = /^[a-z][a-z0-9-]{0,31}\.[a-z0-9][a-z0-9.-]{0,63}$/;
/** `[a-z][a-z0-9-]{0,31}`, from contract/names.json. */
export const validProductName = (name: string) => PRODUCT_NAME.test(name);
/** One word of a verb path, such as `approvals`. */
export const validVerbSegment = (word: string) => VERB_SEGMENT.test(word);
/** A schema id such as `example.status/1`. */
export const validSchemaId = (schema: string) => SCHEMA_ID.test(schema);
/** A product code such as `example.policy-locked`, under any product's prefix. */
export const validProductCode = (code: string) => PRODUCT_CODE.test(code);

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
  /** `--name` is `true`; `--name=value`, or `--name value` for a declared value flag, is the value. */
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
  /** Flags that take the next argument as their value, so `--digest abc` works like `--digest=abc`. */
  valueFlags?: readonly string[];
  /** Parses arguments; throw `HranessError('usage', ...)` on bad input. */
  input: (argv: ParsedArgs) => I;
  run: (input: I, ctx: VerbContext) => Promise<O>;
  gate?: { tier: GateTier; describe: (input: I) => { title: string; digest: string; command?: string } };
  /**
   * For a gated verb whose class depends on its input, such as
   * `approvals decide <id> --digest <d> deny|allow-once`: `test` returns
   * true for inputs an agent may run on its own (deny), which then run as
   * `operate` with no gate. Every other input keeps the gate.
   * `commands --json` lists the verb under its gated class with `summary`.
   */
  operateWhen?: { summary: string; test: (input: I) => boolean };
  /**
   * `raw`: the verb writes its own stdout and `run` returns the exit
   * status, as `tui` and `control serve` do. `runCli` prints nothing on
   * success; errors thrown before or by `run` still print an envelope.
   */
  output?: 'envelope' | 'raw';
  /** Plain text for people; JSON is printed when absent. */
  text?: (output: O) => string;
}
export interface VerbDescriptor { path: string[]; opClass: OpClass; schema: string; summary: string; gate?: GateTier; operateWhen?: string }
export interface Registry { product: string; verbs: readonly Verb<any, any>[] }

/**
 * Rejects a missing op class, a `decide` verb without a gate, a gate on a
 * `read` or `operate` verb, `operateWhen` on a verb without a gate, bad
 * names and schemas (contract/names.json) and repeated paths.
 */
export function defineRegistry(product: string, verbs: Verb<any, any>[]): Registry {
  if (!validProductName(product)) throw new Error(`Invalid product name: ${product}`);
  const seen = new Set<string>();
  for (const verb of verbs) {
    const name = verb.path.join(' ');
    if (!verb.path.length || !verb.path.every(validVerbSegment) || verb.path[0] === 'commands') throw new Error(`Invalid verb path: ${name}`);
    if (!validSchemaId(verb.schema)) throw new Error(`Verb ${name} has an invalid schema: ${verb.schema}`);
    if (!['read', 'operate', 'decide', 'decide-legacy'].includes(verb.opClass)) throw new Error(`Verb ${name} has no operation class.`);
    if (verb.opClass === 'decide' && !verb.gate) throw new Error(`Decide verb ${name} needs a gate.`);
    if ((verb.opClass === 'read' || verb.opClass === 'operate') && verb.gate) throw new Error(`${verb.opClass === 'read' ? 'Read' : 'Operate'} verb ${name} cannot have a gate.`);
    if (verb.gate && !['T1T2', 'T3'].includes(verb.gate.tier)) throw new Error(`Verb ${name} has an unknown gate tier.`);
    if (verb.operateWhen && (!verb.gate || typeof verb.operateWhen.test !== 'function' || !verb.operateWhen.summary)) throw new Error(`Verb ${name} has operateWhen without a gate, a test or a summary.`);
    if (verb.valueFlags?.some(flag => !FLAG.test(flag) || flag === 'json')) throw new Error(`Verb ${name} has an invalid value flag.`);
    if (verb.output !== undefined && verb.output !== 'envelope' && verb.output !== 'raw') throw new Error(`Verb ${name} has an unknown output mode.`);
    if (seen.has(name)) throw new Error(`Verb ${name} is registered twice.`);
    seen.add(name);
  }
  return { product, verbs: [...verbs] };
}

export function describeVerb(verb: Verb<any, any>): VerbDescriptor {
  return {
    path: [...verb.path], opClass: verb.opClass, schema: verb.schema, summary: verb.summary,
    ...(verb.gate ? { gate: verb.gate.tier } : {}), ...(verb.operateWhen ? { operateWhen: verb.operateWhen.summary } : {}),
  };
}
export function commandsJson(reg: Registry, at?: Date): Envelope<{ product: string; verbs: VerbDescriptor[] }> {
  return okEnvelope(COMMANDS_SCHEMA, { product: reg.product, verbs: reg.verbs.map(describeVerb) }, undefined, at);
}

const FLAG = /^[a-z][a-z0-9-]*$/;
/**
 * Splits `args` into positionals and flags. A flag named in `valueFlags`
 * takes the next argument as its value when written without `=`; a
 * missing value is a usage error.
 */
export function parseArgs(args: readonly string[], valueFlags: Iterable<string> = []): ParsedArgs {
  const takesValue = new Set(valueFlags);
  const positionals: string[] = [];
  const flags: Record<string, string | true> = {};
  let rest = false;
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (rest || !arg.startsWith('--') || arg === '--') {
      if (arg === '--' && !rest) { rest = true; continue; }
      positionals.push(arg);
      continue;
    }
    const eq = arg.indexOf('=');
    const name = arg.slice(2, eq < 0 ? undefined : eq);
    if (!FLAG.test(name)) throw new HranessError('usage', `Unknown option ${arg}.`);
    if (eq >= 0) { flags[name] = arg.slice(eq + 1); continue; }
    if (!takesValue.has(name)) { flags[name] = true; continue; }
    const value = args[i + 1];
    if (value === undefined || value.startsWith('--')) throw new HranessError('usage', `--${name} needs a value.`);
    flags[name] = value;
    i++;
  }
  return { positionals, flags };
}
/** The command line that `parseArgs` reads back as `path` plus `args`. Drops `--json`. */
export function formatCommand(product: string, path: readonly string[], args: ParsedArgs): string {
  const flags = Object.entries(args.flags).filter(([name]) => name !== 'json').map(([name, value]) => (value === true ? `--${name}` : `--${name}=${value}`));
  const dashed = args.positionals.some(word => word.startsWith('--'));
  const words = dashed ? [product, ...path, ...flags, '--', ...args.positionals] : [product, ...path, ...args.positionals, ...flags];
  return words.map(shellWord).join(' ');
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
  // Find the verb from a plain parse, then parse again with its value flags.
  try { parsed = parseArgs(argv); }
  catch (error) { return fail(error); }
  json = parsed.flags.json === true;
  let words = parsed.positionals;
  if (words[0] === 'commands') return emit(commandsJson(reg), reg.verbs.map(v => `${reg.product} ${v.path.join(' ')}  [${v.opClass}${v.gate ? ` ${v.gate.tier}` : ''}]  ${v.summary}`).join('\n'));
  const verb = lookupVerb(reg, words);
  if (!verb) return fail(new HranessError('usage', words.length ? `Unknown command: ${words.join(' ')}.` : 'Name a command.', undefined, [{ command: `${reg.product} commands --json`, why: 'List every verb', audience: 'agent' }]));
  try { parsed = parseArgs(argv, verb.valueFlags ?? []); }
  catch (error) { return fail(error); }
  json = parsed.flags.json === true;
  words = parsed.positionals;
  if (lookupVerb(reg, words) !== verb) return fail(new HranessError('usage', `Put options after \`${reg.product} ${verb.path.join(' ')}\`.`));
  const flags = { ...parsed.flags };
  delete flags.json;
  const args: ParsedArgs = { positionals: words.slice(verb.path.length), flags };
  const ctx: VerbContext = { product: reg.product, json, audience, io };
  try {
    const input = verb.input(args);
    if (verb.gate && !verb.operateWhen?.test(input)) {
      const described = verb.gate.describe(input);
      const command = described.command ?? formatCommand(reg.product, verb.path, args);
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
    if (verb.output === 'raw') {
      if (!Number.isInteger(output) || output < 0 || output > 255) throw new Error(`Raw verb ${verb.path.join(' ')} returned no exit status.`);
      return output as number;
    }
    return emit(okEnvelope(verb.schema, output), verb.text?.(output));
  } catch (error) { return fail(error); }
}
