import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createCliOutput } from '../src/cli-style.js';
import { lintMenuCommand } from '../src/lint-menu-command.js';
import { layout } from '../src/menu-kit.js';
import type { SnapshotV2 } from '../src/protocol-v2.js';

const cli = fileURLToPath(new URL('../src/cli.js', import.meta.url));
const UTF8 = { LANG: 'en_US.UTF-8' };

const good: SnapshotV2 = {
  version: 2, type: 'snapshot', appId: 'demo', name: 'Demo', revision: 1, mark: { symbol: 'mark.agent', letters: 'Hc' },
  items: layout({ name: 'Demo', status: { kind: 'status', symbol: 'status.running', label: 'Running' }, primary: { kind: 'action', id: 'open', label: 'Open dashboard', opens: 'browser' } }),
};
const bad: SnapshotV2 = { ...good, items: [...good.items.slice(0, -1), { kind: 'action', id: 'x', label: 'Open Logs Folder' }, good.items.at(-1)!] };

function capture() {
  let out = '', err = '';
  const output = createCliOutput({ audience: 'quiet', env: UTF8, stdout: { write: (text: string) => { out += text; } }, stderr: { write: (text: string) => { err += text; } } });
  return { output, get out() { return out; }, get err() { return err; } };
}
const files: Record<string, string> = { 'good.json': JSON.stringify(good), 'bad.json': JSON.stringify(bad), 'broken.json': '{', 'v1.json': JSON.stringify({ version: 1, type: 'snapshot', appId: 'demo', name: 'Demo', title: 'Hc', revision: 1, items: [{ kind: 'quit', label: 'Quit Demo' }] }) };
const readText = async (path: string) => { if (!(path in files)) throw Object.assign(new Error('missing'), { code: 'ENOENT' }); return files[path]!; };

test('lint-menu passes clean fixtures and reports findings with the tree', async () => {
  const ok = capture();
  assert.equal(await lintMenuCommand(['--strict', 'good.json', 'v1.json'], ok.output, readText), 0);
  assert.equal(ok.out, '✓ good.json\n✓ v1.json\n✓ No menu problems in 2 fixtures.\n');
  const warn = capture();
  assert.equal(await lintMenuCommand(['bad.json'], warn.output, readText), 0);
  assert.match(warn.out, /^⚠ bad\.json\n  sentence-case at items\[5\]: "Open Logs Folder" capitalizes "Logs"/);
  assert.match(warn.out, /\n    Open Logs Folder\n/);
  assert.match(warn.out, /1 warning\.\n$/);
  const strict = capture();
  assert.equal(await lintMenuCommand(['--strict', 'bad.json'], strict.output, readText), 1);
  assert.match(strict.out, /^✗ bad\.json/);
  const nouns = capture();
  assert.equal(await lintMenuCommand(['--strict', '--proper-noun', 'Logs Folder', 'bad.json'], nouns.output, readText), 0);
  const invalid = capture();
  assert.equal(await lintMenuCommand(['broken.json', 'missing.json'], invalid.output, readText), 1);
  assert.equal(invalid.out, '✗ broken.json: not a valid menu snapshot (invalid-json).\n✗ missing.json: not a valid menu snapshot (not-found).\n2 errors.\n');
  const json = capture();
  assert.equal(await lintMenuCommand(['--json', '--strict', 'bad.json'], json.output, readText), 1);
  const parsed = JSON.parse(json.out);
  assert.equal(parsed.ok, false);
  assert.equal(parsed.results[0].findings[0].rule, 'sentence-case');
});

test('lint-menu usage errors exit 2 with one next step', async () => {
  const none = capture();
  assert.equal(await lintMenuCommand([], none.output, readText), 2);
  assert.equal(none.err, '✗ Name at least one fixture file.\n→ companion lint-menu --help\n');
  const unknown = capture();
  assert.equal(await lintMenuCommand(['--fix', 'good.json'], unknown.output, readText), 2);
  assert.match(unknown.err, /Unknown option "--fix"/);
  const json = capture();
  assert.equal(await lintMenuCommand(['--json'], json.output, readText), 2);
  assert.equal(JSON.parse(json.out).error.code, 'usage');
});

test('companion CLI: help exits 0, pipes close quietly, NO_COLOR and TERM=dumb', () => {
  const dir = mkdtempSync(join(tmpdir(), 'lint-menu-'));
  writeFileSync(join(dir, 'good.json'), JSON.stringify(good));
  const run = (args: string[], env: NodeJS.ProcessEnv = {}) => spawnSync(process.execPath, [cli, ...args], { encoding: 'utf8', env: { ...process.env, HRANESS_AUDIENCE: '', ...UTF8, ...env } });
  for (const args of [['--help'], ['-h'], ['help'], ['lint-menu', '--help']]) {
    const result = run(args);
    assert.equal(result.status, 0, args.join(' '));
    assert.match(result.stdout, /^Usage: companion/);
    assert.equal(result.stderr, '');
  }
  const help = run(['--help']).stdout;
  assert.ok(help.split('\n').length <= 25);
  assert.ok(help.split('\n').every(line => line.length <= 80));
  const colorless = run(['lint-menu', join(dir, 'good.json')], { NO_COLOR: '1', FORCE_COLOR: '1' });
  assert.equal(colorless.status, 0);
  assert.ok(!colorless.stdout.includes('\u001b['));
  const dumb = run(['lint-menu', join(dir, 'good.json')], { TERM: 'dumb' });
  assert.match(dumb.stdout, /^OK /);
  if (process.platform !== 'win32') {
    const piped = spawnSync('/bin/sh', ['-c', `"${process.execPath}" "${cli}" --help | head -1`], { encoding: 'utf8' });
    assert.equal(piped.status, 0);
    assert.equal(piped.stdout, 'Usage: companion [command] [--json]\n');
    assert.equal(piped.stderr, '');
  }
});
