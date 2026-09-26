// CLI style contract helpers: status symbols, color and ASCII fallbacks, error
// and `Next:` lines, and one redrawn progress line. The rules are in the Hraness
// CLI style contract (symbols ✓ ✗ ⚠ → ● ○ – ↻ 🔐, NO_COLOR, TERM=dumb, non-TTY).
import { detectAudience, type Audience } from './audience.js';

export type CliSymbol = 'ok' | 'fail' | 'warn' | 'next' | 'on' | 'off' | 'skip' | 'progress' | 'notice';

type Tint = 'green' | 'red' | 'yellow' | 'dim' | undefined;
/** Symbol, ASCII fallback and tint. Only the symbol is ever colored, never the sentence. */
export const CLI_SYMBOLS: Readonly<Record<CliSymbol, { glyph: string; ascii: string; tint: Tint }>> = {
  ok: { glyph: '✓', ascii: 'OK', tint: 'green' },
  fail: { glyph: '✗', ascii: 'FAIL', tint: 'red' },
  warn: { glyph: '⚠', ascii: 'WARN', tint: 'yellow' },
  next: { glyph: '→', ascii: '->', tint: 'dim' },
  on: { glyph: '●', ascii: '*', tint: 'green' },
  off: { glyph: '○', ascii: 'o', tint: undefined },
  skip: { glyph: '–', ascii: '-', tint: 'dim' },
  progress: { glyph: '↻', ascii: '...', tint: undefined },
  notice: { glyph: '🔐', ascii: 'NOTE', tint: undefined },
};
const SGR: Record<Exclude<Tint, undefined>, string> = { green: '32', red: '31', yellow: '33', dim: '2' };

export interface CliStyle { color: boolean; ascii: boolean }

/** A writable text stream: `process.stdout`, `process.stderr` or a test double. */
export interface TextStream { write(text: string): unknown; isTTY?: boolean }

/**
 * Color only for a TTY stream, `TERM` other than `dumb`, and `NO_COLOR` unset
 * or empty (`NO_COLOR` wins over `FORCE_COLOR`). ASCII fallbacks replace every
 * symbol for `TERM=dumb`, a locale that names no UTF-8, or `HRANESS_ASCII=1`.
 */
export function cliStyle(stream: { isTTY?: boolean } = {}, env: NodeJS.ProcessEnv = process.env): CliStyle {
  const dumb = env.TERM === 'dumb';
  const noColor = (env.NO_COLOR ?? '') !== '';
  const force = (env.FORCE_COLOR ?? '') !== '' && env.FORCE_COLOR !== '0' && env.FORCE_COLOR !== 'false';
  const color = !noColor && (force || (stream.isTTY === true && !dumb));
  const utf8 = [env.LC_ALL, env.LC_CTYPE, env.LANG].some(value => /utf-?8/i.test(value ?? ''));
  const ascii = dumb || !utf8 || env.HRANESS_ASCII === '1';
  return { color, ascii };
}

/** One symbol, with its ASCII fallback and tint applied. */
export function cliSymbol(name: CliSymbol, style: CliStyle): string {
  const entry = CLI_SYMBOLS[name];
  const text = style.ascii ? entry.ascii : entry.glyph;
  return style.color && entry.tint ? `\u001b[${SGR[entry.tint]}m${text}\u001b[0m` : text;
}

/** `✓ text`, `⚠ text` and so on. */
export function cliLine(name: CliSymbol, text: string, style: CliStyle): string {
  return `${cliSymbol(name, style)} ${text}`;
}

/** The two-line error form: `✗ What happened.` then `→ next command`. */
export function renderCliError(error: { message: string; next?: string }, style: CliStyle): string {
  return cliLine('fail', error.message, style) + '\n' + (error.next ? cliLine('next', error.next, style) + '\n' : '');
}

export interface CliOutputOptions {
  audience?: Audience;
  stdout?: TextStream;
  stderr?: TextStream;
  env?: NodeJS.ProcessEnv;
}

/**
 * Human-facing output with one set of rules. Results go to stdout; errors,
 * notices, progress and `Next:` hints go to stderr. The `quiet` audience gets
 * plain text with no color, progress or hints; `agent` callers should print
 * JSON instead and use this only for errors they cannot express in JSON.
 */
export interface CliOutput {
  readonly audience: Audience;
  readonly style: { stdout: CliStyle; stderr: CliStyle };
  /** A result line on stdout, optionally led by a symbol. */
  result(text: string, symbol?: CliSymbol): void;
  /** An indented detail line under the previous result, on stdout. */
  detail(text: string): void;
  /** `✗ message` and `→ next` on stderr. */
  error(error: { message: string; next?: string }): void;
  /** `⚠ message` on stderr. */
  warn(message: string): void;
  /** `Next: command` on stderr, for the human audience only. */
  next(command: string): void;
  /** One redrawn `↻ text` line on a TTY stderr for the human audience; returns a function that clears it. */
  progress(text: string): () => void;
}

export function createCliOutput(options: CliOutputOptions = {}): CliOutput {
  const stdout = options.stdout ?? process.stdout;
  const stderr = options.stderr ?? process.stderr;
  const env = options.env ?? process.env;
  const audience = options.audience ?? detectAudience({ env, stderrIsTTY: stderr.isTTY === true });
  const plain = audience !== 'human';
  const out = plain ? { ...cliStyle(stdout, env), color: false } : cliStyle(stdout, env);
  const err = plain ? { ...cliStyle(stderr, env), color: false } : cliStyle(stderr, env);
  let progressLine = false;
  const clearProgress = () => { if (progressLine) { stderr.write('\r\u001b[2K'); progressLine = false; } };
  return {
    audience,
    style: { stdout: out, stderr: err },
    result(text, symbol) { clearProgress(); stdout.write((symbol ? cliLine(symbol, text, out) : text) + '\n'); },
    detail(text) { clearProgress(); stdout.write(`  ${text}\n`); },
    error(error) { clearProgress(); stderr.write(renderCliError(error, err)); },
    warn(message) { clearProgress(); stderr.write(cliLine('warn', message, err) + '\n'); },
    next(command) { if (audience === 'human') { clearProgress(); stderr.write(`Next: ${command}\n`); } },
    progress(text) {
      if (audience !== 'human' || stderr.isTTY !== true || env.TERM === 'dumb') return () => {};
      clearProgress();
      stderr.write(cliLine('progress', text, err));
      progressLine = true;
      return clearProgress;
    },
  };
}

/** Human file size for progress lines: `1.8 MB`, `640 KB`. */
export function formatBytes(bytes: number): string {
  if (bytes >= 999_500) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${Math.round(bytes / 1_000)} KB`;
  return `${bytes} bytes`;
}

/**
 * Exit quietly when a reader such as `| head -1` closes stdout early, instead
 * of throwing `EPIPE`. Call once at the top of a CLI entry point.
 */
export function exitQuietlyOnBrokenPipe(stream: NodeJS.WriteStream = process.stdout, exit: (code: number) => void = code => process.exit(code)): void {
  stream.on('error', (error: NodeJS.ErrnoException) => { if (error.code === 'EPIPE') exit(0); else throw error; });
}
