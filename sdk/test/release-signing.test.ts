import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = new URL('../../', import.meta.url);
test('non-executing release signing and artifact-custody behavioral suite', () => {
  const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-I', 'scripts/sign-macos-release.test.py'], {
    cwd: fileURLToPath(root), encoding: 'utf8', timeout: 60_000, maxBuffer: 256 * 1024,
  });
  assert.equal(result.status, 0, `${result.error?.message ?? ''}\n${result.stdout}\n${result.stderr}`);
});

test('Apple credentials occur only in the separate tag-gated signer and before packaging', () => {
  const workflow = readFileSync(new URL('.github/workflows/companion.yml', root), 'utf8');
  const signer = workflow.slice(workflow.indexOf('  macos_sign:'), workflow.indexOf('  package:'));
  assert.ok(signer.includes("if: github.event_name == 'push' && github.ref_type == 'tag'"));
  assert.ok(signer.includes('environment: hraness-apple-release'));
  assert.ok(signer.includes('needs: [release_identity, native, sdk, cli-kit, control-kit]'));
  assert.ok(!/npm |cargo |bun |\.\/artifacts/.test(signer), 'credential-bearing job must not build or execute payloads');
  const outside = workflow.replace(signer, '');
  assert.ok(!outside.includes('secrets.APPLE_'));
  assert.ok(workflow.indexOf('fetch-signed artifacts') < workflow.indexOf('run: node scripts/release-manifest.mjs'));
  assert.ok(workflow.includes('macos_sign, package, package_smoke]'));
  const pack = workflow.slice(workflow.indexOf('  package:'), workflow.indexOf('  package_smoke:'));
  assert.ok(!pack.includes('package-smoke.mjs'));
  assert.ok(pack.includes('steps.distribution.outputs.artifact-digest'));
  assert.equal(workflow.split('macos-release-artifacts.py fetch-distribution artifacts').length, 3);
  assert.ok(workflow.includes('DISTRIBUTION_ARTIFACT_ID: ${{ needs.package.outputs.artifact_id }}'));
  assert.ok(workflow.indexOf('macos-release-artifacts.py verify-tag') < workflow.indexOf('gh release create'));
});
