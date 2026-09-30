import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { syncBuiltinESMExports } from 'node:module';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ensureBinary, inspectBinary, type ReleaseManifest } from '../src/install.js';

for (const corruptOnRetry of [false, true]) {
  test(`temporary hard-link removal restarts full verification${corruptOnRetry ? ' and still rejects changed bytes' : ''}`, { skip: process.platform === 'win32' }, async t => {
    const cacheDir = await fs.mkdtemp(join(tmpdir(), 'install-link-race-'));
    const bytes = Buffer.from('never-executed-cache-fixture');
    const target = 'aarch64-apple-darwin' as const;
    const manifest: ReleaseManifest = { schemaVersion: 1, version: '0.5.0', tag: 'v0.5.0', repository: 'hraness/desktop-foundation',
      assets: [{ target, name: `hraness-companion-${target}`, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }] };
    try {
      const options = { manifest, target, cacheDir };
      const installed = await ensureBinary({ ...options, fetch: (async () => new Response(bytes)) as typeof fetch });
      const temporaryName = join(cacheDir, 'owned-install.tmp');
      await fs.link(installed.path, temporaryName);
      let opens = 0;
      let unlinked = false;
      const originalOpen = fs.open;
      t.mock.method(fs, 'open', async (...args: Parameters<typeof fs.open>) => {
        const handle = await originalOpen(...args);
        if (args[0] === installed.path) {
          opens++;
          if (corruptOnRetry && opens === 2) await fs.writeFile(installed.path, Buffer.alloc(bytes.length, 65));
          const originalStat = handle.stat.bind(handle);
          t.mock.method(handle, 'stat', (async () => {
            const info = await originalStat();
            if (!unlinked) { unlinked = true; await fs.unlink(temporaryName); }
            return info;
          }) as typeof handle.stat);
        }
        return handle;
      });
      syncBuiltinESMExports();
      if (corruptOnRetry) await assert.rejects(inspectBinary(options), { code: 'integrity_failed' });
      else assert.equal((await inspectBinary(options)).installed, true);
      assert.equal(opens, 2, 'the complete descriptor/hash verification must restart after unlink');
    } finally {
      t.mock.restoreAll();
      syncBuiltinESMExports();
      await fs.rm(cacheDir, { recursive: true, force: true });
    }
  });
}
