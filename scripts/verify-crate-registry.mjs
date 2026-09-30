import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export function verifyRegistryArchive(response, name, version, checksum) {
  assert.match(checksum, /^[0-9a-f]{64}$/, 'expected archive checksum must be SHA-256');
  const published = response.version;
  assert.equal(published?.crate, name, 'registry crate name does not match');
  assert.equal(published?.num, version, 'registry version does not match');
  assert.equal(published?.yanked, false, 'registry version is yanked or lacks yanked status');
  assert.equal(published?.checksum, checksum, 'registry archive differs from the verified source');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [name, version, checksum, responsePath] = process.argv.slice(2);
  verifyRegistryArchive(JSON.parse(readFileSync(responsePath, 'utf8')), name, version, checksum);
  console.log(`Verified ${name} ${version} registry archive`);
}
