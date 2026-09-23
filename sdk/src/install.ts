import { createHash, randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import { link, lstat, mkdir, open, readlink, unlink } from 'node:fs/promises';
import { isAbsolute, join, normalize, parse, sep } from 'node:path';
import { CompanionError } from './errors.js';
import { resolveTarget, TARGETS, userPaths, type PlatformTarget } from './platform.js';

export interface ReleaseAsset { target: PlatformTarget; name: string; size: number; sha256: string }
export interface ReleaseManifest {
  schemaVersion: 1;
  version: string;
  repository: string;
  tag: string;
  assets: ReleaseAsset[];
}
const MAX_ASSET_BYTES = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES = 64 * 1024;
const DOWNLOAD_TIMEOUT_MS = 120_000;
const invalid = (message: string): never => { throw new CompanionError('invalid_manifest', message); };

/** The caller must supply a reviewed, pinned manifest; never fetch a mutable latest manifest. */
export function parseReleaseManifest(bytes: string | Uint8Array): ReleaseManifest {
  if (Buffer.byteLength(bytes) > MAX_MANIFEST_BYTES) invalid('Release manifest exceeds its size limit.');
  let manifest: unknown;
  try { manifest = JSON.parse(typeof bytes === 'string' ? bytes : new TextDecoder('utf-8', { fatal: true }).decode(bytes)); }
  catch { return invalid('Release manifest is not valid UTF-8 JSON.'); }
  return validateManifest(manifest);
}
function validateManifest(value: unknown): ReleaseManifest {
  if (!value || typeof value !== 'object') return invalid('Missing release manifest.');
  const m = value as ReleaseManifest;
  if (Object.keys(m).some(key => !['schemaVersion', 'version', 'repository', 'tag', 'assets'].includes(key))) return invalid('Release manifest contains unknown fields.');
  if (m.schemaVersion !== 1 || typeof m.version !== 'string' || !/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(m.version)
      || m.tag !== `v${m.version}` || typeof m.repository !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9_-]*\/[a-zA-Z0-9][a-zA-Z0-9_.-]*$/.test(m.repository)) {
    return invalid('Release manifest needs schemaVersion 1, a fixed version/tag, and an owner/repository.');
  }
  if (!Array.isArray(m.assets) || m.assets.length === 0 || m.assets.length > TARGETS.length) return invalid('Release manifest has no assets or too many assets.');
  const targets = new Set<string>();
  for (const asset of m.assets) {
    if (!asset || !TARGETS.includes(asset.target) || targets.has(asset.target)) return invalid('Invalid or repeated release target.');
    if (Object.keys(asset).some(key => !['target', 'name', 'size', 'sha256'].includes(key))) return invalid('Release asset contains unknown fields.');
    const expectedName = `hraness-companion-${asset.target}${asset.target.includes('windows') ? '.exe' : ''}`;
    if (asset.name !== expectedName || !Number.isSafeInteger(asset.size) || asset.size <= 0 || asset.size > MAX_ASSET_BYTES
        || typeof asset.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(asset.sha256)) return invalid('Invalid release asset name, size, or SHA-256.');
    targets.add(asset.target);
  }
  // Copy to prevent the caller mutating a manifest while an install awaits IO.
  return { schemaVersion: 1, version: m.version, repository: m.repository, tag: m.tag, assets: m.assets.map(a => ({ ...a })) };
}

export interface EnsureBinaryOptions {
  manifest: ReleaseManifest;
  target?: PlatformTarget;
  cacheDir?: string;
  fetch?: typeof globalThis.fetch;
  maxBytes?: number;
}
export interface InstalledBinary { path: string; version: string; target: PlatformTarget; reused: boolean }
/** Read-only identity and integrity evidence for agents helping a human approve an OS prompt. */
export async function inspectBinary(options: Omit<EnsureBinaryOptions, 'fetch' | 'maxBytes'>) {
  const manifest = validateManifest(options.manifest);
  const target = options.target ?? resolveTarget();
  const asset = manifest.assets.find(value => value.target === target);
  if (!asset) throw new CompanionError('unsupported_target', `This release has no companion asset for ${target}.`);
  const path = join(options.cacheDir ?? userPaths().cacheDir, ...manifest.repository.split('/'), manifest.version, target, asset.name);
  await assertPhysicalPath(path);
  const installed = await verifyFile(path, asset);
  return { path, installed, integrity: installed ? 'verified' : 'missing', version: manifest.version, tag: manifest.tag, repository: manifest.repository, target, sha256: asset.sha256, size: asset.size };
}
function errno(error: unknown, code: string) { return (error as NodeJS.ErrnoException)?.code === code; }

export async function assertPhysicalPath(path: string) {
  if (!isAbsolute(path) || normalize(path) !== path) throw new CompanionError('unsafe_path', 'Companion paths must be absolute and normalized.');
  let current = parse(path).root;
  for (const segment of path.slice(current.length).split(sep).filter(Boolean)) {
    current = join(current, segment);
    let info;
    try { info = await lstat(current); } catch (error) { if (errno(error, 'ENOENT')) return; throw error; }
    if (info.isSymbolicLink()) {
      const systemAlias = process.platform === 'darwin' && ['/tmp', '/var', '/etc'].includes(current)
        && await readlink(current) === `private${current}`;
      if (!systemAlias) throw new CompanionError('unsafe_path', 'Companion paths cannot traverse symbolic links or junctions.');
    }
  }
}
export async function ensurePrivateDirectory(path: string) {
  await assertPhysicalPath(path);
  await mkdir(path, { recursive: true, mode: 0o700 });
  await assertPhysicalPath(path);
  const info = await lstat(path);
  if (!info.isDirectory() || info.isSymbolicLink()) throw new CompanionError('unsafe_path', 'Companion cache directories must be real directories, not symbolic links.');
  if (process.platform !== 'win32' && ((info.mode & 0o077) !== 0 || info.uid !== process.getuid?.())) throw new CompanionError('unsafe_path', 'The companion directory must belong to the current user and have owner-only permissions.');
}
async function verifyFile(path: string, asset: ReleaseAsset): Promise<boolean> {
  let info;
  try { info = await lstat(path); } catch (error) { if (errno(error, 'ENOENT')) return false; throw error; }
  if (!info.isFile() || info.isSymbolicLink()) throw new CompanionError('unsafe_path', 'The cached companion is not a regular file.');
  if (process.platform !== 'win32' && (info.uid !== process.getuid?.() || (info.mode & 0o022) !== 0 || (info.mode & 0o100) === 0)) throw new CompanionError('unsafe_path', 'The cached companion must belong to the current user, be executable, and not be writable by other users.');
  if (info.size !== asset.size) throw new CompanionError('integrity_failed', 'The cached companion has an unexpected size.', 'Keep the failed artifact for inspection or explicitly remove this version from the cache before retrying.');
  const file = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const opened = await file.stat();
    if (opened.dev !== info.dev || opened.ino !== info.ino || opened.size !== info.size || opened.mode !== info.mode || opened.uid !== info.uid) throw new CompanionError('integrity_failed', 'The cached companion changed before verification.');
    const hash = createHash('sha256');
    let size = 0;
    for await (const chunk of file.createReadStream({ autoClose: false })) { size += chunk.length; if (size > asset.size) break; hash.update(chunk); }
    if (size !== asset.size || hash.digest('hex') !== asset.sha256) throw new CompanionError('integrity_failed', 'The cached companion failed SHA-256 verification.', 'Do not launch this file. Explicitly remove this version from the cache before retrying.');
    const current = await file.stat();
    const named = await lstat(path);
    if (current.size !== asset.size || current.mtimeMs !== opened.mtimeMs || current.ctimeMs !== opened.ctimeMs
        || named.isSymbolicLink() || named.dev !== current.dev || named.ino !== current.ino || named.mode !== opened.mode || named.uid !== opened.uid) throw new CompanionError('integrity_failed', 'The cached companion changed while it was verified.');
  } finally { await file.close(); }
  return true;
}
function allowedRedirect(url: URL) {
  return url.protocol === 'https:' && !url.username && !url.password && !url.port
    && ['github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'].includes(url.hostname);
}
async function download(asset: ReleaseAsset, url: URL, temp: string, fetcher: typeof fetch, signal: AbortSignal) {
  let response: Response | undefined;
  for (let hop = 0; hop <= 5; hop++) {
    if (!allowedRedirect(url)) throw new CompanionError('download_failed', 'Release download redirected outside approved GitHub HTTPS hosts.');
    response = await fetcher(url, { redirect: 'manual', signal, headers: { Accept: 'application/octet-stream' } });
    if (![301, 302, 303, 307, 308].includes(response.status)) break;
    const location = response.headers.get('location');
    await response.body?.cancel();
    if (!location || hop === 5) throw new CompanionError('download_failed', 'Release download returned an invalid redirect chain.');
    url = new URL(location, url);
  }
  if (!response?.ok || !response.body) throw new CompanionError('download_failed', `Release download failed with HTTP ${response?.status ?? 'unknown'}.`);
  const length = response.headers.get('content-length');
  if (length !== null && (!/^\d+$/.test(length) || Number(length) !== asset.size)) {
    await response.body.cancel();
    throw new CompanionError('integrity_failed', 'Release download has an unexpected Content-Length.');
  }
  const file = await open(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o600);
  const reader = response.body.getReader();
  try {
    let total = 0;
    const hash = createHash('sha256');
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.length;
      if (total > asset.size) throw new CompanionError('integrity_failed', 'Release download exceeded its pinned byte limit.');
      hash.update(value);
      let offset = 0;
      while (offset < value.length) { const result = await file.write(value, offset, value.length - offset); if (!result.bytesWritten) throw new CompanionError('download_failed', 'Unable to write release download.'); offset += result.bytesWritten; }
    }
    if (total !== asset.size || hash.digest('hex') !== asset.sha256) throw new CompanionError('integrity_failed', 'Release download failed pinned size or SHA-256 verification.');
    await file.chmod(0o755);
    await file.sync();
  } finally {
    await reader.cancel().catch(() => undefined);
    await file.close();
  }
}
/** Installs bytes only. Does not launch, enable autostart, or change any OS security setting. */
async function ensureBinaryImpl(options: EnsureBinaryOptions): Promise<InstalledBinary> {
  const manifest = validateManifest(options.manifest);
  const target = options.target ?? resolveTarget();
  const asset = manifest.assets.find(a => a.target === target);
  if (!asset) throw new CompanionError('unsupported_target', `This release has no companion asset for ${target}.`);
  if (options.maxBytes !== undefined && (!Number.isSafeInteger(options.maxBytes) || options.maxBytes <= 0 || asset.size > options.maxBytes)) throw new CompanionError('invalid_manifest', 'Release asset exceeds the caller download limit.');
  const cache = options.cacheDir ?? userPaths().cacheDir;
  if (!isAbsolute(cache)) throw new CompanionError('unsafe_path', 'The companion cache directory must be absolute.');
  await ensurePrivateDirectory(cache);
  let directory = cache;
  for (const segment of [...manifest.repository.split('/'), manifest.version, target]) { directory = join(directory, segment); await ensurePrivateDirectory(directory); }
  const path = join(directory, asset.name);
  const result = { path, version: manifest.version, target };
  if (await verifyFile(path, asset)) return { ...result, reused: true };
  const temp = join(directory, `.${asset.name}.${randomUUID()}.tmp`);
  try {
    await download(asset, new URL(`https://github.com/${manifest.repository}/releases/download/${manifest.tag}/${asset.name}`), temp, options.fetch ?? globalThis.fetch, AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS));
    try { await link(temp, path); }
    catch (error) {
      if (!errno(error, 'EEXIST')) throw error;
      // Another installer won the atomic publish. Never replace its file.
      if (!await verifyFile(path, asset)) throw new CompanionError('cache_conflict', 'Concurrent install disappeared before it could be verified.');
      return { ...result, reused: true };
    }
    return { ...result, reused: false };
  } catch (error) {
    if (error instanceof CompanionError) throw error;
    if (errno(error, 'EPERM') || errno(error, 'EACCES')) throw new CompanionError('os_approval_required', 'The operating system denied the companion install.', 'Check the destination permissions and OS policy, and ask the user to approve the executable if the OS requires it. Do not disable the policy or clear quarantine automatically.');
    throw new CompanionError('download_failed', 'Unable to install the pinned companion release.', 'Check network access and cache permissions, then retry.');
  } finally { await unlink(temp).catch(error => { if (!errno(error, 'ENOENT')) throw error; }); }
}

export async function ensureBinary(options: EnsureBinaryOptions): Promise<InstalledBinary> {
  try { return await ensureBinaryImpl(options); }
  catch (error) {
    if (error instanceof CompanionError) throw error;
    if (errno(error, 'EPERM') || errno(error, 'EACCES')) throw new CompanionError('os_approval_required', 'The operating system denied access to the companion cache.', 'Check the cache permissions and OS policy. Do not disable security policy to get past this.');
    throw new CompanionError('download_failed', 'Unable to prepare the companion cache.');
  }
}
