import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// Compiled to dist/test/readme.test.js; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
const read = (path: string): string => readFileSync(new URL(path, root), 'utf8');
const manifest = JSON.parse(read('package.json')) as { version: string };
const cargo = read('Cargo.toml');
const readme = read('README.md');
const protocol = read('docs/protocol.md');
const version = manifest.version;

test('Cargo and npm versions agree', () => {
  assert.match(cargo, new RegExp(`^version = "${version.replaceAll('.', '\\.')}"$`, 'm'));
});

test('README install lines pin the current release', () => {
  assert.ok(readme.includes(
    `npm install https://github.com/hraness/desktop-foundation/releases/download/v${version}/hraness-desktop-foundation-${version}.tgz`,
  ), 'README npm install line must use the package version');
  assert.ok(readme.includes(
    `desktop-foundation = { git = "https://github.com/hraness/desktop-foundation", tag = "v${version}" }`,
  ), 'README Cargo dependency must use the package version');
  for (const match of readme.matchAll(/releases\/download\/v(\d+\.\d+\.\d+)\/|tag = "v(\d+\.\d+\.\d+)"/g)) {
    assert.equal(match[1] ?? match[2], version, 'README pins a release other than the package version');
  }
});

test('protocol sample prints the current version', () => {
  assert.ok(protocol.includes(`hraness-companion ${version} protocol/1`));
});

test('README uses no em dashes', () => {
  assert.ok(!readme.includes('—'));
});
