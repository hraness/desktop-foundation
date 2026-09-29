// Finds the `hraness-helper` binary a product runs for app assembly, launch,
// notices, prompts and permission checks. From v0.9.0 each release carries
// `hraness-helper-<target>` beside `hraness-companion-<target>`. From 1.0 the
// companion is only an alias of the helper (it has no menu bar), so an older
// pinned manifest without helper assets still works through it.

import { constants } from 'node:fs';
import { access, lstat } from 'node:fs/promises';
import { isAbsolute, normalize } from 'node:path';
import { CompanionError } from './errors.js';
import { ensureBinary, releaseAsset, type EnsureBinaryOptions, type ReleaseManifest } from './install.js';
import { resolveTarget, type PlatformTarget } from './platform.js';

export interface ResolveHelperOptions {
  /** An explicit binary, such as the value of `GHOSTGET_MENUBAR`. Must be an absolute executable regular file. */
  override?: string;
  /** The reviewed, pinned release manifest. Needed unless `override` is set. */
  manifest?: ReleaseManifest;
  target?: PlatformTarget;
  cacheDir?: string;
  fetch?: typeof globalThis.fetch;
  maxBytes?: number;
}
export interface ResolvedHelper { path: string; source: 'override' | 'helper' | 'companion' }

/** The override, else the release's helper asset, else its companion alias. Installs and verifies bytes; runs nothing. */
export async function resolveHelper(opts: ResolveHelperOptions = {}): Promise<ResolvedHelper> {
  if (opts.override !== undefined && opts.override !== '') {
    const path = opts.override;
    if (!isAbsolute(path) || normalize(path) !== path) throw new CompanionError('unsafe_path', 'The helper override must be an absolute, normalized path.');
    const info = await lstat(path).catch(() => undefined);
    if (!info?.isFile() || info.isSymbolicLink()) throw new CompanionError('unsafe_path', 'The helper override must be a regular file, not a link or directory.');
    try { await access(path, constants.X_OK); } catch { throw new CompanionError('unsafe_path', 'The helper override is not executable.'); }
    return { path, source: 'override' };
  }
  if (!opts.manifest) throw new CompanionError('invalid_manifest', 'Pass a pinned release manifest or an override.');
  const target = opts.target ?? resolveTarget();
  const kind = releaseAsset(opts.manifest, target, 'helper') ? 'helper' : 'companion';
  const install: EnsureBinaryOptions = { manifest: opts.manifest, target, cacheDir: opts.cacheDir, fetch: opts.fetch, maxBytes: opts.maxBytes, kind };
  const { path } = await ensureBinary(install);
  return { path, source: kind };
}
