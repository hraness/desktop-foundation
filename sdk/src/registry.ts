// Verbs, the JSON envelope and exit codes shared by every Hraness CLI. The
// Rust twin is `hraness_control_kit::{envelope, registry}`; both read the
// same contract/ files. See docs/control.md.

import { detectAudience, type Audience } from './audience.js';
import { requireHuman, type GateProof } from './human-gate.js';
import { closest, cliLine, cliStyle } from './cli-style.js';
import { isAllowedSettingsUrl } from './permissions.js';

export type OpClass = 'read' | 'operate' | 'decide' | 'decide-legacy';
export type GateTier = 'T1T2' | 'T3';
export type NextAudience = 'agent' | 'human';

export const ERROR_SCHEMA = 'hraness.error/1';
export const COMMANDS_SCHEMA = 'hraness.commands/1';
/** `--help --json`: the same verb descriptors, for the verbs the help covers. */
export const HELP_SCHEMA = 'hraness.help/1';
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
/**
 * A macOS permission behind the failure: its kind (`full-disk-access`,
 * `keychain`, ...) and, when it has one, the System Settings pane to open.
 * Added in 1.1.0; readers that do not know it can ignore it.
 */
export interface ErrorPermission { kind: string; settingsUrl?: string | null }
/** `^[a-z][a-z0-9-]*$`: a valid `error.permission.kind` (contract/names.json `permissionKind`). */
export const validPermissionKind = (kind: string) => /^[a-z][a-z0-9-]*$/.test(kind);
/**
 * The `error.permission` member, or undefined for a kind outside
 * `^[a-z][a-z0-9-]*$`. A settings link that is not one of the known System
 * Settings panes is dropped, so an envelope never carries a link a client
 * should not open. The Rust `ErrorBody::with_permission` gives the same
 * bytes. `permissionError()` from `./permissions` gives both fields:
 * `errorPermission(info.kind, info.settingsUrl)`.
 */
export function errorPermission(kind: string, settingsUrl?: string | null): ErrorPermission | undefined {
  if (!validPermissionKind(kind)) return undefined;
  return settingsUrl && isAllowedSettingsUrl(settingsUrl) ? { kind, settingsUrl } : { kind };
}
export interface ErrorBody { code: ErrorCode; message: string; detail?: string; next?: NextStep[]; permission?: ErrorPermission }
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
  constructor(readonly code: ErrorCode, message: string, readonly detail?: string, readonly next: NextStep[] = [], readonly permission?: ErrorPermission) {
    super(message);
    this.name = 'HranessError';
  }
  toBody(): ErrorBody {
    const permission = this.permission && errorPermission(this.permission.kind, this.permission.settingsUrl);
    return {
      code: this.code, message: this.message, ...(this.detail ? { detail: this.detail } : {}), ...(this.next.length ? { next: this.next } : {}),
      ...(permission ? { permission } : {}),
    };
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
  /** `isTTY` turns on color for the `✗` and `→` symbols. */
  stderr: { write(text: string): unknown; isTTY?: boolean };
  env?: NodeJS.ProcessEnv;
  /** Defaults to `detectAudience`. An agent gets the JSON envelope with or without `--json`. */
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
  /**
   * Flags that take no value, such as `--snapshot`. Only these, the
   * `valueFlags`, `--json` and `--help` are accepted; any other flag is a
   * usage error, so a typo never runs the verb.
   */
  flags?: readonly string[];
  /** Positional arguments for `--help`, such as `<id> <allow-once|deny>`. */
  usage?: string;
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
    if (verb.valueFlags?.some(flag => !FLAG.test(flag) || RESERVED_FLAGS.has(flag))) throw new Error(`Verb ${name} has an invalid value flag.`);
    if (verb.flags?.some(flag => !FLAG.test(flag) || RESERVED_FLAGS.has(flag) || verb.valueFlags?.includes(flag))) throw new Error(`Verb ${name} has an invalid flag.`);
    if (verb.output !== undefined && verb.output !== 'envelope' && verb.output !== 'raw') throw new Error(`Verb ${name} has an unknown output mode.`);
    if (seen.has(name)) throw new Error(`Verb ${name} is registered twice.`);
    seen.add(name);
  }
  return { product, verbs: [...verbs] };
}

/** `commands` is built in, not a verb, so `help commands --json` describes it with this. */
const COMMANDS_DESCRIPTOR: VerbDescriptor = { path: ['commands'], opClass: 'read', schema: COMMANDS_SCHEMA, summary: 'List every command.' };
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
/** Every verb takes these; a verb cannot declare them. */
const RESERVED_FLAGS = new Set(['json', 'help']);
/** Splits `--help` and `-h` (before any `--`) out of `argv`. */
function takeHelp(argv: readonly string[]): { help: boolean; rest: string[] } {
  const end = argv.indexOf('--');
  const rest = argv.filter((arg, i) => (end >= 0 && i >= end) || (arg !== '--help' && arg !== '-h'));
  return { help: rest.length !== argv.length, rest };
}
/**
 * Splits `--debug` (before any `--`) out of `argv`: CLI_MENU_STYLE D5 makes it
 * the same as `HRANESS_DEBUG=1`. A registry whose verbs declare their own
 * `debug` flag keeps it as that flag.
 */
function takeDebug(reg: Registry, argv: readonly string[]): { debug: boolean; rest: string[] } {
  if (reg.verbs.some(v => v.flags?.includes('debug') || v.valueFlags?.includes('debug'))) return { debug: false, rest: [...argv] };
  const end = argv.indexOf('--');
  const rest = argv.filter((arg, i) => (end >= 0 && i >= end) || arg !== '--debug');
  return { debug: rest.length !== argv.length, rest };
}
function verbLines(product: string, verbs: readonly Verb<any, any>[]): string[] {
  const names = verbs.map(v => `${product} ${v.path.join(' ')}`);
  const width = Math.max(0, ...names.map(n => n.length));
  return verbs.map((v, i) => `  ${names[i].padEnd(width)}  ${v.summary}`);
}
/** What a verb's class means for the person reading its help, in plain words. */
function personLine(verb: Verb<any, any>): string | undefined {
  if (!verb.gate) return undefined;
  if (verb.gate.tier === 'T3') return 'A person decides this with their macOS login, which is not supported yet.';
  return `A person decides this at their own terminal${verb.operateWhen ? `, except for ${verb.operateWhen.summary}` : ''}.`;
}
/** Help for one verb: `Usage:` line, summary, who decides, and every flag it accepts. */
export function verbHelp(reg: Registry, verb: Verb<any, any>): string {
  const flags: [string, string][] = [
    ...(verb.valueFlags ?? []).map((f): [string, string] => [`--${f} <value>`, '']),
    ...(verb.flags ?? []).map((f): [string, string] => [`--${f}`, '']),
    ['--json', 'Print machine-readable output'],
    ['-h, --help', 'Print this help'],
  ];
  const width = Math.max(...flags.map(([f]) => f.length));
  const person = personLine(verb);
  return [
    `Usage: ${reg.product} ${verb.path.join(' ')}${verb.usage ? ` ${verb.usage}` : ''} [options]`,
    '',
    verb.summary,
    ...(person ? [person] : []),
    '',
    'Options',
    ...flags.map(([f, why]) => (why ? `  ${f.padEnd(width)}  ${why}` : `  ${f}`)),
  ].join('\n');
}
/** Help for a group of verbs, or every verb at the top level. */
function groupHelp(reg: Registry, words: readonly string[], under: readonly Verb<any, any>[]): string {
  const prefix = words.length ? `${words.join(' ')} ` : '';
  return [
    `Usage: ${reg.product} ${prefix}<command> [options]`,
    '',
    'Commands',
    ...verbLines(reg.product, under),
    '',
    'Options',
    '  --json      Print machine-readable output',
    '  -h, --help  Print help',
    '',
    `Run \`${reg.product} help ${prefix}<command>\` for one command.`,
  ].join('\n');
}
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
    if (!FLAG.test(name)) throw new HranessError('usage', `Unknown option "${arg}", so nothing ran.`);
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

/**
 * The first single-dash option before `--`, such as `-x`, skipping the values
 * of value flags. `-h` never reaches here and a bare `-` or `-5` is a word.
 */
function shortOption(args: readonly string[], valueFlags: readonly string[]): string | undefined {
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--') return undefined;
    if (arg.startsWith('--')) {
      if (!arg.includes('=') && valueFlags.includes(arg.slice(2))) i++;
      continue;
    }
    if (/^-[A-Za-z]/.test(arg)) return arg;
  }
  return undefined;
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
    message: `\`${reg.product} ${verb.path.join(' ')}\` is a decision for a person, so nothing changed.`,
    next: [{ command, why: 'Run this in your own terminal to decide.', audience: 'human' }],
  };
}

function shellWord(word: string): string {
  return /^[A-Za-z0-9_./:=@%+-]+$/.test(word) ? word : `'${word.replaceAll("'", `'\\''`)}'`;
}

/**
 * The unknown-command error for `words`: names the first unknown word,
 * suggests the closest command at that level, and points at the help for
 * the level it reached.
 */
function unknownCommand(reg: Registry, words: readonly string[]): HranessError {
  let known = 0;
  while (known < words.length && reg.verbs.some(v => v.path.length > known && words.slice(0, known + 1).every((w, i) => v.path[i] === w))) known++;
  const prefix = words.slice(0, known);
  const help = [reg.product, ...prefix, '--help'].join(' ');
  // The agent step stays first, as in 1.0.0; text output picks the human one.
  const next: NextStep[] = [
    { command: `${reg.product} commands --json`, why: 'List every verb', audience: 'agent' },
    { command: help, why: 'List the commands', audience: 'human' },
  ];
  if (!words.length) return new HranessError('usage', 'Name a command.', undefined, next);
  if (known === words.length) return new HranessError('usage', `Name a command after "${prefix.join(' ')}".`, undefined, next);
  const typed = words.slice(0, known + 1).join(' ');
  const seen = new Set<string>();
  const candidates: [string, string][] = [];
  for (const v of reg.verbs) {
    if (v.path.length <= known || !prefix.every((w, i) => v.path[i] === w)) continue;
    const word = v.path[known];
    if (!seen.has(word)) { seen.add(word); candidates.push([word, [...prefix, word].join(' ')]); }
  }
  if (!known) candidates.push(['commands', 'commands'], ['help', 'help']);
  const found = closest(words[known], candidates);
  const suggestion = found === words[known] ? undefined : found;
  return new HranessError('usage', `Unknown command "${typed}".${suggestion ? ` Did you mean "${suggestion}"?` : ''}`, undefined, next);
}

/**
 * Text errors follow the Hraness CLI style: `✗` and one sentence, then `→`
 * and one next command (the first step meant for a person, else the first
 * step, else `fallback`). `HRANESS_DEBUG=1` adds the code and detail.
 * `options.code` puts the code before the message, for the quiet audience.
 */
export function renderTextError(error: ErrorBody, fallback: string, stream: { isTTY?: boolean } = {}, env: NodeJS.ProcessEnv = process.env, options: { code?: boolean } = {}): string {
  const style = cliStyle(stream, env);
  const next = (error.next ?? []).find(n => n.audience === 'human') ?? error.next?.[0];
  // `code: message`, as 1.0 printed it, for scripts that match on the code.
  const message = options.code ? `${error.code}: ${error.message}` : error.message;
  const lines = [cliLine('fail', message, style), cliLine('next', next?.command ?? fallback, style)];
  if (env.HRANESS_DEBUG === '1') lines.push(`  code: ${error.code}`, ...(error.detail ? [`  detail: ${error.detail}`] : []));
  return `${lines.join('\n')}\n`;
}

/**
 * Runs one command line and returns its exit status. `commands` lists the
 * verbs. `--help`, `-h` and `help <command>` print help and never run a
 * verb; a flag the verb did not declare is a usage error. `--json`, or an
 * agent audience, prints one envelope; otherwise errors are two lines on
 * stderr. A verb that needs a person, run with `--json` or by an agent (or
 * with no one at the terminal), answers `human-required` (exit 3) and never
 * prompts; `HRANESS_AUDIENCE` and `--confirm` never stand in for the
 * person. Otherwise the person answers at `/dev/tty`.
 */
export async function runCli(reg: Registry, fullArgv: readonly string[], io: CliIO): Promise<number> {
  const { debug, rest: undebugged } = takeDebug(reg, fullArgv);
  let { help, rest: argv } = takeHelp(undebugged);
  const env = debug ? { ...(io.env ?? process.env), HRANESS_DEBUG: '1' } : io.env ?? process.env;
  const audience = io.audience ?? detectAudience({ env, stderrIsTTY: io.stderr.isTTY });
  // An agent reads JSON whether or not it asked for it.
  const agent = audience === 'agent';
  let parsed: ParsedArgs;
  let explicitJson = argv.slice(0, argv.indexOf('--') < 0 ? undefined : argv.indexOf('--')).includes('--json');
  let json = explicitJson || agent;
  let fallback = `${reg.product} --help`;
  const emit = (envelope: Envelope<unknown>, text?: string): number => {
    // A person reads two lines on stderr. A script (the quiet audience) also
    // gets the error envelope on stdout and the code on stderr, as in 1.0.
    const quiet = audience === 'quiet';
    if (json || (envelope.ok && text === undefined) || (!envelope.ok && quiet)) io.stdout.write(`${JSON.stringify(envelope)}\n`);
    else if (envelope.ok) io.stdout.write(text!.endsWith('\n') ? text! : `${text}\n`);
    if (!envelope.ok && !json) io.stderr.write(renderTextError(envelope.error, fallback, io.stderr, quiet ? { ...env, HRANESS_ASCII: '1', NO_COLOR: '1' } : env, { code: quiet }));
    return envelopeExitCode(envelope);
  };
  const fail = (error: unknown): number => {
    const body: ErrorBody = error instanceof HranessError ? error.toBody() : { code: 'internal', message: 'An unexpected failure.', detail: error instanceof Error ? error.message : String(error) };
    // The code goes in `detail`, which text mode shows only with `--debug` or `HRANESS_DEBUG=1`.
    if (!isErrorCode(body.code, reg.product)) return emit(errorEnvelope({ code: 'internal', message: 'The command failed with an error it did not declare.', detail: `Undeclared code ${body.code}.` }));
    return emit(errorEnvelope(body));
  };
  // Find the verb from a plain parse, then parse again with its value flags.
  try { parsed = parseArgs(argv); }
  catch (error) {
    const guess = lookupVerb(reg, argv.filter(arg => !arg.startsWith('-')));
    if (guess) fallback = `${reg.product} ${guess.path.join(' ')} --help`;
    return fail(error);
  }
  explicitJson = parsed.flags.json === true;
  json = explicitJson || agent;
  let words = parsed.positionals;
  // `help <command>` is `<command> --help`, unless a product registered `help`.
  // Only a leading `help` counts (after `--json` at most), so it is never a
  // flag's value or a word after `--`.
  const lead = argv.findIndex(arg => arg !== '--json');
  if (lead >= 0 && argv[lead] === 'help' && words[0] === 'help' && !reg.verbs.some(v => v.path[0] === 'help')) {
    help = true;
    argv = [...argv.slice(0, lead), ...argv.slice(lead + 1)];
    words = words.slice(1);
    // `help help` is the root help.
    if (words.length === 1 && words[0] === 'help') words = [];
  }
  if (words[0] === 'commands' && !help) return emit(commandsJson(reg), [`Usage: ${reg.product} <command> [options]`, '', 'Commands', ...verbLines(reg.product, reg.verbs)].join('\n'));
  // Help never runs a verb. It answers for the verb named, or lists the
  // verbs under the words given (all of them at the top level).
  if (help) {
    const found = lookupVerb(reg, words);
    if (found) return emit(okEnvelope(HELP_SCHEMA, { product: reg.product, verbs: [describeVerb(found)] }), verbHelp(reg, found));
    if (words[0] === 'commands') return emit(okEnvelope(HELP_SCHEMA, { product: reg.product, verbs: [COMMANDS_DESCRIPTOR] }), [`Usage: ${reg.product} commands [options]`, '', 'List every command.', '', 'Options', '  --json      Print machine-readable output', '  -h, --help  Print this help'].join('\n'));
    const under = reg.verbs.filter(v => words.every((w, i) => v.path[i] === w));
    if (!under.length) return fail(unknownCommand(reg, words));
    return emit(okEnvelope(HELP_SCHEMA, { product: reg.product, verbs: under.map(describeVerb) }), groupHelp(reg, words, under));
  }
  const verb = lookupVerb(reg, words);
  if (!verb) {
    const lead = /^-[A-Za-z]/.test(words[0] ?? '') ? words[0] : undefined;
    if (lead) return fail(new HranessError('usage', `Unknown option "${lead}", so nothing ran.`, undefined, [{ command: `${reg.product} --help`, why: 'List the commands', audience: 'human' }]));
    return fail(unknownCommand(reg, words));
  }
  const name = `${reg.product} ${verb.path.join(' ')}`;
  fallback = `${name} --help`;
  try { parsed = parseArgs(argv, verb.valueFlags ?? []); }
  catch (error) { return fail(error); }
  explicitJson = parsed.flags.json === true;
  json = explicitJson || agent;
  words = parsed.positionals;
  if (lookupVerb(reg, words) !== verb) return fail(new HranessError('usage', `Put options after "${name}".`));
  const accepted = new Set(['json', ...(verb.valueFlags ?? []), ...(verb.flags ?? [])]);
  const unknown = Object.keys(parsed.flags).find(flag => !accepted.has(flag));
  const short = shortOption(argv, verb.valueFlags ?? []);
  if (short) return fail(new HranessError('usage', `Unknown option "${short}" for "${name}", so nothing ran.`, undefined, [{ command: `${name} --help`, why: 'List its options', audience: 'human' }]));
  if (unknown) return fail(new HranessError('usage', `Unknown option "--${unknown}" for "${name}", so nothing ran.`, undefined, [{ command: `${name} --help`, why: 'List its options', audience: 'human' }]));
  if (verb.flags?.some(flag => typeof parsed.flags[flag] === 'string')) return fail(new HranessError('usage', `--${verb.flags.find(flag => typeof parsed.flags[flag] === 'string')} takes no value.`));
  const flags = { ...parsed.flags };
  delete flags.json;
  const args: ParsedArgs = { positionals: words.slice(verb.path.length), flags };
  // A raw verb owns stdout, so it sees only the --json the caller asked for.
  const ctx: VerbContext = { product: reg.product, json: verb.output === 'raw' ? explicitJson : json, audience, io };
  try {
    const input = verb.input(args);
    if (verb.gate && !verb.operateWhen?.test(input)) {
      const described = verb.gate.describe(input);
      const command = described.command ?? formatCommand(reg.product, verb.path, args);
      if (verb.gate.tier === 'T3') throw new HranessError('unsupported-platform', 'Deciding with your macOS login is not supported yet, so nothing changed.');
      if (json && audience !== 'human') return emit(errorEnvelope(humanRequired(reg, verb, command)));
      const gate = await (io.gate ?? requireHuman)({ title: described.title, digest: described.digest, tier: verb.gate.tier });
      if (!gate.ok) {
        // A wrong or expired code: running the same command again asks for a new one.
        const body: ErrorBody = gate.code === 'human-required' ? humanRequired(reg, verb, command)
          : gate.code === 'gate-failed' || gate.code === 'gate-expired' ? { code: gate.code, message: gate.message, next: [{ command, why: 'Run it again for a new code.', audience: 'human' }] }
          : { code: gate.code, message: gate.message };
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
