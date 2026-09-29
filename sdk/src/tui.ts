// A small text UI for a product's `tui` verb. The Rust twin is
// `hraness_control_kit::tui` (ratatui). Off a terminal the same views print
// as a deterministic snapshot; `--json` prints the envelope that `status
// --json` prints. Snapshots are golden-tested at widths 40, 80 and 120.

import { stripVTControlCharacters } from 'node:util';
import { envelopeExitCode, EXIT, type Envelope } from './registry.js';

export type Mode = 'interactive' | 'snapshot' | 'json';
export const DEFAULT_SNAPSHOT_WIDTH = 80;

export interface View<S> {
  /** A stable id such as `status`. */
  id: string;
  title: string;
  /** Lines no wider than `width` columns. Use `box` and `table` to build them. */
  render(state: S, width: number): string[];
}

/** `--json` wins, then `--snapshot`; with neither, a terminal gets the interactive view. */
export function chooseMode(json: boolean, snapshot: boolean, stdoutIsTerminal: boolean): Mode {
  return json ? 'json' : snapshot || !stdoutIsTerminal ? 'snapshot' : 'interactive';
}

/** Removes escape sequences and other control characters so product data cannot drive the terminal. */
export function clean(text: string): string {
  return stripVTControlCharacters(String(text)).replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
}
/** Cuts or pads `text` to exactly `width` columns (one column per code point). */
export function fit(text: string, width: number): string {
  const chars = [...clean(text)];
  return chars.length >= width ? chars.slice(0, Math.max(0, width)).join('') : chars.join('') + ' '.repeat(width - chars.length);
}
/** A titled box `width` columns wide around `lines`. */
export function box(title: string, lines: readonly string[], width: number): string[] {
  const inner = Math.max(0, width - 2);
  const label = inner >= 4 ? fit(` ${clean(title)} `, Math.min(inner, [...clean(title)].length + 2)) : '';
  return [
    `┌${label}${'─'.repeat(inner - [...label].length)}┐`,
    ...lines.map(line => `│${fit(line, inner)}│`),
    `└${'─'.repeat(inner)}┘`,
  ];
}
/** Columns sized to their widest cell, with the last column taking the rest. */
export function table(headers: readonly string[], rows: readonly (readonly string[])[], width: number): string[] {
  const widths = headers.map((header, i) => Math.max(...[header, ...rows.map(row => row[i] ?? '')].map(cell => [...clean(cell)].length)));
  const line = (cells: readonly string[]) => {
    let out = '';
    cells.forEach((cell, i) => {
      const last = i === cells.length - 1;
      const room = width - [...out].length;
      if (room <= 0) return;
      out += last ? fit(cell, room) : fit(cell, Math.min(room, widths[i] + 2));
    });
    return out.trimEnd();
  };
  return [line(headers), ...rows.map(line)];
}

/** Every view under a `== title ==` heading, the same text the Rust kit prints. */
export function renderSnapshot<S>(views: readonly View<S>[], state: S, width = DEFAULT_SNAPSHOT_WIDTH): string {
  return views.map(view => `== ${clean(view.title)} ==\n${view.render(state, width).map(line => `${fit(line, width).trimEnd()}\n`).join('')}`).join('\n');
}

export interface TuiIO {
  stdout: { write(text: string): unknown; columns?: number; rows?: number };
  stdin?: NodeJS.ReadStream;
}
export interface RunTuiOptions<S> {
  load: () => Promise<Envelope<S>>;
  views: readonly View<S>[];
  mode: Mode;
  width?: number;
  io?: TuiIO;
}

export type Action = 'next' | 'previous' | 'reload' | 'quit' | 'none';
/** Tab and Shift-Tab switch views, r reloads, q, Esc and Ctrl-C quit. */
export function actionFor(key: string): Action {
  switch (key) {
    case '\t': return 'next';
    case '\x1b[Z': return 'previous';
    case 'r': case 'R': return 'reload';
    case 'q': case 'Q': case '\x1b': case '\x03': return 'quit';
    default: return 'none';
  }
}

/** Shows the views and returns the exit status. */
export async function runTui<S>(opts: RunTuiOptions<S>): Promise<number> {
  const io = opts.io ?? { stdout: process.stdout, stdin: process.stdin };
  let loaded = await opts.load();
  if (opts.mode === 'json') { io.stdout.write(`${JSON.stringify(loaded)}\n`); return envelopeExitCode(loaded); }
  const text = (envelope: Envelope<S>, width: number, only?: View<S>) => envelope.ok
    ? renderSnapshot(only ? [only] : opts.views, envelope.data, width)
    : `${envelope.error.code}: ${clean(envelope.error.message)}\n`;
  if (opts.mode === 'snapshot' || !io.stdin?.isTTY) {
    io.stdout.write(text(loaded, opts.width ?? DEFAULT_SNAPSHOT_WIDTH));
    return envelopeExitCode(loaded);
  }
  const stdin = io.stdin;
  let index = 0;
  const draw = () => {
    const width = opts.width ?? io.stdout.columns ?? DEFAULT_SNAPSHOT_WIDTH;
    const tabs = opts.views.map((view, i) => (i === index ? `[${clean(view.title)}]` : ` ${clean(view.title)} `)).join(' ');
    const footer = 'Tab next · Shift-Tab back · r reload · q quit';
    io.stdout.write(`\x1b[H\x1b[2J${fit(tabs, width).trimEnd()}\n${text(loaded, width, opts.views[index]).replace(/\n/g, '\r\n')}\r\n${fit(footer, width).trimEnd()}`);
  };
  io.stdout.write('\x1b[?1049h\x1b[?25l');
  stdin.setRawMode(true);
  stdin.resume();
  try {
    draw();
    for await (const chunk of stdin) {
      const action = actionFor(chunk.toString('utf8'));
      if (action === 'quit') break;
      if (action === 'next') index = (index + 1) % opts.views.length;
      if (action === 'previous') index = (index + opts.views.length - 1) % opts.views.length;
      if (action === 'reload') loaded = await opts.load();
      if (action !== 'none') draw();
    }
  } finally {
    stdin.setRawMode(false);
    stdin.pause();
    io.stdout.write('\x1b[?25h\x1b[?1049l');
  }
  return loaded.ok ? EXIT.ok : envelopeExitCode(loaded);
}
