// Credentials for a process started at login. A login item starts without
// the terminal's environment, so a product that reads a token from, say,
// SPONGE_API_TOKEN would come up signed out. `saveLoginEnvironment` saves the
// values of the variables the product names to a private file in its state
// directory; the login-started process calls `loadLoginEnvironment` to fill in
// any of those variables that are unset. Values never go into the login item
// file, argv or logs.
import { randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open, rename, unlink } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { ensurePrivateDirectory } from './install.js';

const FILE = 'login-environment.json';
const NAME = /^[A-Za-z_][A-Za-z0-9_]{0,127}$/;
const MAX_VALUE = 16 * 1024;
const MAX_FILE = 64 * 1024;

export interface LoginEnvResult {
  /** Names whose current values were saved. */
  saved: string[];
  /** Names that are unset here, so a login-started process won't have them. */
  missing: string[];
}

function checkNames(names: readonly string[]): void {
  if (names.length > 32 || names.some(name => !NAME.test(name))) throw new Error('invalid-login-env-name');
}

/** Path of the private file, for diagnostics only. */
export function loginEnvironmentPath(stateDir: string): string { return join(stateDir, FILE); }

/** Saves the named variables' current values (0600, atomic replace). With nothing to save, removes the file. */
export async function saveLoginEnvironment(stateDir: string, names: readonly string[], env: NodeJS.ProcessEnv = process.env): Promise<LoginEnvResult> {
  checkNames(names);
  if (!isAbsolute(stateDir)) throw new Error('absolute-state-directory-required');
  const values: Record<string, string> = {};
  const saved: string[] = [], missing: string[] = [];
  for (const name of names) {
    const value = env[name];
    if (typeof value === 'string' && value !== '' && value.length <= MAX_VALUE && !value.includes('\0')) { values[name] = value; saved.push(name); }
    else missing.push(name);
  }
  if (!saved.length) { await removeLoginEnvironment(stateDir); return { saved, missing }; }
  await ensurePrivateDirectory(stateDir);
  const temp = join(stateDir, `.${FILE}.${randomUUID()}.tmp`);
  try {
    const handle = await open(temp, 'wx', 0o600);
    try { await handle.writeFile(JSON.stringify({ version: 1, env: values })); await handle.sync(); }
    finally { await handle.close(); }
    await rename(temp, loginEnvironmentPath(stateDir));
  } finally { await unlink(temp).catch(() => {}); }
  return { saved, missing };
}

/**
 * Fills in any of `names` that are unset in `env` from the saved file.
 * Returns the names it filled. Ignores a file that is not a private regular
 * file owned by this user, or that does not parse.
 */
export async function loadLoginEnvironment(stateDir: string, names: readonly string[], env: NodeJS.ProcessEnv = process.env): Promise<string[]> {
  checkNames(names);
  if (!names.length || !isAbsolute(stateDir)) return [];
  const path = loginEnvironmentPath(stateDir);
  let handle;
  try {
    const stat = await lstat(path);
    if (!stat.isFile() || stat.size > MAX_FILE) return [];
    if (process.platform !== 'win32' && ((stat.mode & 0o077) !== 0 || stat.uid !== process.getuid?.())) return [];
    handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const actual = await handle.stat();
    if (actual.ino !== stat.ino || actual.dev !== stat.dev) return [];
    const parsed = JSON.parse(await handle.readFile('utf8')) as { version?: unknown; env?: Record<string, unknown> };
    if (parsed.version !== 1 || !parsed.env || typeof parsed.env !== 'object') return [];
    const filled: string[] = [];
    for (const name of names) {
      const value = parsed.env[name];
      if ((env[name] ?? '') === '' && typeof value === 'string' && value !== '' && value.length <= MAX_VALUE && !value.includes('\0')) { env[name] = value; filled.push(name); }
    }
    return filled;
  } catch { return []; }
  finally { await handle?.close(); }
}

/** Removes the saved file, if any. */
export async function removeLoginEnvironment(stateDir: string): Promise<boolean> {
  if (!isAbsolute(stateDir)) return false;
  try { await unlink(loginEnvironmentPath(stateDir)); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false; throw error; }
}
