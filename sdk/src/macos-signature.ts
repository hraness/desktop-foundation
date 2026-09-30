import { execFile } from 'node:child_process';
import { CompanionError } from './errors.js';
import type { BinaryKind, ReleaseManifest } from './install.js';
import type { PlatformTarget } from './platform.js';

export const APPLE_TEAM = '8AAP53VTW3';
export const MACOS_IDENTIFIERS = {
  helper: 'dev.hraness.desktop-foundation.helper',
  companion: 'dev.hraness.desktop-foundation.companion',
} as const;

/** Historical pins and explicit local builds retain their original trust model. */
export function requiresMacosSignature(manifest: ReleaseManifest, target: PlatformTarget): boolean {
  if (manifest.repository.toLowerCase() !== 'hraness/desktop-foundation' || !target.endsWith('-apple-darwin')) return false;
  const [major, minor, patch] = manifest.version.split(/[.-]/).slice(0, 3).map(Number);
  return major > 1 || major === 1 && (minor > 1 || minor === 1 && patch >= 3);
}

export function macosRequirement(kind: BinaryKind): string {
  return `identifier "${MACOS_IDENTIFIERS[kind]}" and anchor apple generic `
    + 'and certificate 1[field.1.2.840.113635.100.6.2.6] exists '
    + 'and certificate leaf[field.1.2.840.113635.100.6.1.13] exists '
    + `and certificate leaf[subject.OU] = "${APPLE_TEAM}"`;
}

const rejected = () => new CompanionError('integrity_failed', 'The Mac release does not have the expected Hraness Developer ID signature.', 'Preserve the failed artifact. Install an intact official release; do not re-sign it or change macOS trust settings.');
export type SignatureRunner = (args: readonly string[]) => Promise<string>;

/** Internal test seam; production always uses the absolute system codesign tool. */
export async function verifyMacosSignatureWith(path: string, kind: BinaryKind, run: SignatureRunner): Promise<void> {
  try {
    await run(['--verify', '--strict', '--test-requirement', '=' + macosRequirement(kind), path]);
    const metadata = await run(['--display', '--verbose=4', path]);
    const lines = metadata.split(/\r?\n/);
    if (!lines.includes(`Identifier=${MACOS_IDENTIFIERS[kind]}`) || !lines.includes(`TeamIdentifier=${APPLE_TEAM}`)
        || !/^CodeDirectory .*flags=.*\([^\n)]*\bruntime\b[^\n)]*\)/m.test(metadata)
        || !/^Timestamp=.+/m.test(metadata)) throw rejected();
  } catch { throw rejected(); }
}

function codesign(args: readonly string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile('/usr/bin/codesign', [...args], {
      timeout: 15_000, maxBuffer: 64 * 1024, encoding: 'utf8',
      env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: process.env.HOME, LC_ALL: 'C' },
    }, (error, stdout, stderr) => error ? reject(rejected()) : resolve(stdout + stderr));
  });
}

export async function verifyMacosRelease(path: string, manifest: ReleaseManifest, target: PlatformTarget, kind: BinaryKind): Promise<void> {
  if (!requiresMacosSignature(manifest, target)) return;
  // Preparing another platform's executable cannot establish macOS trust.
  // Refuse to report it admitted; ordinary Linux/Windows installs are unchanged.
  if (process.platform !== 'darwin') throw rejected();
  await verifyMacosSignatureWith(path, kind, codesign);
}
