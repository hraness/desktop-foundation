import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ensureBinary, inspectBinary, type ReleaseManifest } from '../src/install.js';
import { APPLE_TEAM, MACOS_IDENTIFIERS, macosRequirement, requiresMacosSignature, verifyMacosSignatureWith } from '../src/macos-signature.js';

const target = 'aarch64-apple-darwin' as const;
const bytes = Buffer.from('synthetic unsigned file, never executed');
function manifest(version = '1.1.3'): ReleaseManifest {
  return { schemaVersion: 1, version, tag: `v${version}`, repository: 'hraness/desktop-foundation',
    assets: [{ target, name: `hraness-companion-${target}`, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }] };
}
const metadata = (kind: 'helper' | 'companion') => `Identifier=${MACOS_IDENTIFIERS[kind]}\nTeamIdentifier=${APPLE_TEAM}\nCodeDirectory v=20500 flags=0x10000(runtime) hashes=2\nTimestamp=Oct 1, 2026\n`;

test('signature floor retains historical pins and applies to future official Mac releases', () => {
  for (const version of ['0.9.0', '1.0.0', '1.1.2']) assert.equal(requiresMacosSignature(manifest(version), target), false);
  for (const version of ['1.1.3', '1.1.3-rc.1', '1.2.0', '2.0.0']) assert.equal(requiresMacosSignature(manifest(version), target), true);
  assert.equal(requiresMacosSignature(manifest(), 'x86_64-unknown-linux-gnu'), false);
  assert.equal(requiresMacosSignature({ ...manifest(), repository: 'example/local-build' }, target), false);
});

test('both executable identities require cryptographic Developer ID verification and runtime metadata', async () => {
  for (const kind of ['helper', 'companion'] as const) {
    const commands: readonly string[][] = [];
    await verifyMacosSignatureWith('/private/example', kind, async args => {
      (commands as string[][]).push([...args]);
      return args.includes('--display') ? metadata(kind) : '';
    });
    assert.deepEqual(commands[0], ['--verify', '--strict', '--test-requirement', '=' + macosRequirement(kind), '/private/example']);
    assert.ok(macosRequirement(kind).includes('anchor apple generic'));
    assert.ok(macosRequirement(kind).includes(APPLE_TEAM));
    for (const changed of [metadata(kind).replace(APPLE_TEAM, 'AAAAAAAAAA'), metadata(kind).replace(MACOS_IDENTIFIERS[kind], 'other.id'),
      metadata(kind).replace('(runtime)', '(none)'), metadata(kind).replace(/Timestamp=.*/, '')]) {
      await assert.rejects(verifyMacosSignatureWith('/private/example', kind, async () => changed), { code: 'integrity_failed' });
    }
    await assert.rejects(verifyMacosSignatureWith('/private/example', kind, async () => { throw new Error('untrusted tool output'); }),
      error => (error as Error).message.includes('Developer ID') && !(error as Error).message.includes('untrusted'));
  }
});

test('real codesign parses the SDK requirement and rejects an ad-hoc identity', { skip: process.platform !== 'darwin' }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'sdk-requirement-parser-'));
  try {
    const path = join(directory, 'owned-fixture');
    await copyFile('/usr/bin/true', path); await chmod(path, 0o755);
    const options = { encoding: 'utf8' as const, timeout: 15_000,
      env: { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: directory, LC_ALL: 'C' }, stdio: 'pipe' as const };
    execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', '--identifier', MACOS_IDENTIFIERS.helper, path], options);
    let diagnostic = '';
    await assert.rejects(verifyMacosSignatureWith(path, 'helper', async args => {
      try { return execFileSync('/usr/bin/codesign', [...args], options); }
      catch (error) { diagnostic = String((error as { stderr?: string }).stderr); throw error; }
    }), { code: 'integrity_failed' });
    assert.match(diagnostic, /code failed to satisfy specified code requirement/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('unsigned new Mac download is rejected before publication and its staging file is removed', async () => {
  const cacheDir = await mkdtemp(join(tmpdir(), 'signature-download-'));
  try {
    await assert.rejects(ensureBinary({ manifest: manifest(), cacheDir, target, fetch: (async () => new Response(bytes)) as typeof fetch }), { code: 'integrity_failed' });
    assert.deepEqual(await readdir(join(cacheDir, 'hraness', 'desktop-foundation', '1.1.3', target)), []);
  } finally { await rm(cacheDir, { recursive: true, force: true }); }
});

test('cached unsigned new Mac artifact is rejected by offline reuse and inspection, and preserved', async () => {
  const cacheDir = await mkdtemp(join(tmpdir(), 'signature-cache-'));
  try {
    let directory = cacheDir;
    for (const segment of ['hraness', 'desktop-foundation', '1.1.3', target]) { directory = join(directory, segment); await mkdir(directory, { mode: 0o700 }); }
    const path = join(directory, manifest().assets[0]!.name);
    await writeFile(path, bytes); await chmod(path, 0o755);
    const options = { manifest: manifest(), cacheDir, target };
    await assert.rejects(ensureBinary({ ...options, fetch: (async () => { throw new Error('no download expected'); }) as typeof fetch }), { code: 'integrity_failed' });
    await assert.rejects(inspectBinary(options), { code: 'integrity_failed' });
    assert.deepEqual(await readFile(path), bytes);
  } finally { await rm(cacheDir, { recursive: true, force: true }); }
});
