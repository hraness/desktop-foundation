// Retires a product's old menu bar login item. Generalised from Textbutler's
// `retireMenuLoginItem`. Only a regular file you own, at one of the exact
// labels given, whose contents `accepts` recognises, is touched: its label is
// booted out and the file is renamed to `<name>.retired-<ms>` (launchd skips
// names that do not end in .plist). Nothing is deleted and no process is
// signalled by pid. To undo, rename the file back and run
// `launchctl bootstrap gui/<uid> <path>`.

import { execFile } from 'node:child_process';
import { constants } from 'node:fs';
import { lstat, open, rename } from 'node:fs/promises';
import { join } from 'node:path';

export const MAX_LOGIN_ITEM_BYTES = 64 * 1024;
const LABEL = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

export interface LegacyLoginItem { label: string; path: string; text: string }
export interface RetireOptions {
  home: string;
  /** Exact labels to look for, such as `app.hraness.companion.textbutler`. */
  labels: readonly string[];
  /** True only for contents that are the product's old menu bar item. */
  accepts: (item: LegacyLoginItem) => boolean;
  bootout?: (label: string) => Promise<void>;
  now?: () => Date;
  /** The owning uid. Defaults to this process's. */
  uid?: number;
}
export interface Retired { label: string; from: string; to: string }

/** Finds the first matching item, boots it out and renames it aside. Returns null when there is none. */
export async function retireLegacyLoginItem(opts: RetireOptions): Promise<Retired | null> {
  const uid = opts.uid ?? process.getuid?.();
  if (uid === undefined) return null;
  for (const label of opts.labels) {
    if (!LABEL.test(label)) throw new Error(`Invalid login item label: ${label}`);
    const path = join(opts.home, 'Library', 'LaunchAgents', `${label}.plist`);
    const text = await readOwned(path, uid);
    if (text === undefined || !opts.accepts({ label, path, text })) continue;
    await (opts.bootout ?? defaultBootout)(label);
    const to = `${path}.retired-${(opts.now?.() ?? new Date()).getTime()}`;
    await rename(path, to);
    return { label, from: path, to };
  }
  return null;
}

/** The file's text when it is a small regular file owned by `uid`, read without following a link. */
async function readOwned(path: string, uid: number): Promise<string | undefined> {
  try {
    const info = await lstat(path);
    if (!info.isFile() || info.size > MAX_LOGIN_ITEM_BYTES || info.uid !== uid) return undefined;
    const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
      const opened = await file.stat();
      if (opened.ino !== info.ino || opened.dev !== info.dev) return undefined;
      const { buffer, bytesRead } = await file.read(Buffer.alloc(MAX_LOGIN_ITEM_BYTES + 1), 0, MAX_LOGIN_ITEM_BYTES + 1, 0);
      if (bytesRead > MAX_LOGIN_ITEM_BYTES) return undefined;
      return new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, bytesRead));
    } finally { await file.close(); }
  } catch { return undefined; }
}

/** `launchctl bootout gui/<uid>/<label>`. An item that is not loaded is the common case and is fine. */
export function defaultBootout(label: string): Promise<void> {
  if (!LABEL.test(label) || process.platform !== 'darwin') return Promise.resolve();
  return new Promise(resolve => execFile('/bin/launchctl', ['bootout', `gui/${process.getuid!()}/${label}`],
    { env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin' }, timeout: 20_000, killSignal: 'SIGKILL' }, () => resolve()));
}

/** A matcher for the common case: the plist runs this program with this argument. */
export function launches(program: string | RegExp, argument?: string): (item: LegacyLoginItem) => boolean {
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return ({ text }) => {
    const strings = [...text.matchAll(/<string>([^<]*)<\/string>/g)].map(match => match[1]);
    const hit = strings.some(s => (typeof program === 'string' ? new RegExp(`${esc(program)}$`) : program).test(s));
    return hit && (argument === undefined || strings.includes(argument));
  };
}
