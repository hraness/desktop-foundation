import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Compiled to dist/test; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
const script = new URL('scripts/native-scope.mjs', root).href;
const rootPath = fileURLToPath(root);

type Change = { status: string; path: string };
type Result = { scope: 'docs' | 'subset' | 'full'; reasons: string[] };
type Selector = {
  selectScope: (changes: Change[], read: (side: 'base' | 'head', path: string) => string, has?: (side: 'base' | 'head', path: string) => boolean) => Result;
  classifyPath: (path: string, status?: string) => string;
  PACKAGED_DOCS: Set<string>;
};
const load = async () => await import(script) as Selector;

const neutral = 'pub fn parse(line: &str) -> Option<u32> { line.trim().parse().ok() }\n';
const branchy = '#[cfg(target_os = "windows")]\nfn pipe() {}\n';
const files = (map: Record<string, { base?: string; head?: string }>) =>
  (side: 'base' | 'head', path: string) => {
    const text = map[path]?.[side];
    if (text === undefined) throw new Error(`missing ${side}:${path}`);
    return text;
  };

test('documentation-only pull requests skip the native matrix', async () => {
  const { selectScope } = await load();
  const result = selectScope([{ status: 'M', path: 'README.md' }, { status: 'A', path: 'docs/control.md' }, { status: 'M', path: 'LICENSE' }], files({}));
  assert.equal(result.scope, 'docs');
});

test('platform-neutral source runs the Linux x64, macOS arm64 and Windows x64 subset', async () => {
  const { selectScope } = await load();
  const prose = '// Linux trays ignore this field; macOS and Windows show it.\n';
  const read = files({ 'src/protocol_v2.rs': { base: neutral, head: prose + neutral }, 'sdk/src/registry.ts': { head: 'export const x = 1;\n' } });
  const result = selectScope([
    { status: 'M', path: 'src/protocol_v2.rs' },
    { status: 'A', path: 'sdk/src/registry.ts' },
    { status: 'M', path: 'contract/error-codes.json' },
    { status: 'M', path: 'CHANGELOG.md' },
  ], read, () => false);
  assert.equal(result.scope, 'subset');
});

test('a touched platform branch runs every target', async () => {
  const { selectScope } = await load();
  const markers = [
    '#[cfg(target_os = "windows")]', '#[cfg(windows)]', '#[cfg(not(unix))]', 'cfg!(target_arch = "aarch64")',
    "if (process.platform === 'win32') {}", "process.arch === 'arm64'", "const shell = 'pwsh';", "const os = 'darwin';", 'if os == "windows" {}',
    'use std::os::windows::ffi::OsStrExt;', 'use std::os::unix::fs::PermissionsExt;', 'windows::Win32::Foundation::HANDLE', 'libc::getuid()',
    "import { platform } from 'node:os';", "import { EOL, homedir } from 'os';", 'path.sep', 'os.EOL', 'path.win32.normalize(p)',
    "openSync('/dev/tty', 'r')", "spawnSync('ps', ['-o', 'comm='])", "const bin = name + '.exe';",
  ];
  for (const marker of markers) {
    const read = files({ 'src/protocol_v2.rs': { base: neutral, head: `${neutral}${marker}\n` } });
    assert.equal(selectScope([{ status: 'M', path: 'src/protocol_v2.rs' }], read).scope, 'full', marker);
  }
  // Removing a platform branch also needs every target.
  const removed = files({ 'src/protocol_v2.rs': { base: branchy, head: neutral } });
  assert.equal(selectScope([{ status: 'M', path: 'src/protocol_v2.rs' }], removed).scope, 'full');
  const deleted = files({ 'sdk/src/platform.ts': { base: "process.platform === 'darwin'\n" } });
  assert.equal(selectScope([{ status: 'D', path: 'sdk/src/platform.ts' }], deleted).scope, 'full');
  // A file named for a platform needs every target whatever its content.
  const named = files({ 'src/macos_menu.rs': { base: neutral, head: neutral } });
  assert.equal(selectScope([{ status: 'M', path: 'src/macos_menu.rs' }], named).scope, 'full');
});

test('security and process modules run every target, whatever their content', async () => {
  const { selectScope } = await load();
  for (const path of ['sdk/src/human-gate.ts', 'sdk/src/helper.ts', 'sdk/src/prompt.ts', 'crates/hraness-control-kit/src/crypto.rs']) {
    const read = files({ [path]: { base: neutral, head: neutral } });
    assert.equal(selectScope([{ status: 'M', path }], read, () => false).scope, 'full', path);
  }
});

test('an SDK module whose paired test branches on the platform runs every target', async () => {
  const { selectScope } = await load();
  const source = 'export const x = 1;\n';
  const marked = files({ 'sdk/src/widget.ts': { base: source, head: source }, 'sdk/test/widget.test.ts': { base: "if (process.platform === 'win32') {}\n", head: 'x\n' } });
  assert.equal(selectScope([{ status: 'M', path: 'sdk/src/widget.ts' }], marked, () => true).scope, 'full');
  const plain = files({ 'sdk/src/widget.ts': { base: source, head: source }, 'sdk/test/widget.test.ts': { base: 'x\n', head: 'x\n' } });
  assert.equal(selectScope([{ status: 'M', path: 'sdk/src/widget.ts' }], plain, () => true).scope, 'subset');
  // No paired test on either side: judged by the module alone.
  const alone = files({ 'sdk/src/widget.ts': { base: source, head: source } });
  assert.equal(selectScope([{ status: 'M', path: 'sdk/src/widget.ts' }], alone, () => false).scope, 'subset');
  // A failing existence check fails closed.
  const broken = () => { throw new Error('ls-tree failed'); };
  assert.equal(selectScope([{ status: 'M', path: 'sdk/src/widget.ts' }], alone, broken).scope, 'full');
});

test('documentation that code or the package reads is not docs-only', async () => {
  const { selectScope } = await load();
  // include_str! by Rust tests that run only on native legs.
  for (const path of ['docs/protocol.md', 'docs/permissions.md'])
    assert.equal(selectScope([{ status: 'M', path }], files({})).scope, 'subset', path);
  // The packaged skill, and deleting any packaged doc, needs the package job.
  assert.equal(selectScope([{ status: 'M', path: 'skills/companion/SKILL.md' }], files({})).scope, 'full');
  for (const path of ['docs/installation.md', 'README.md', 'LICENSE'])
    assert.equal(selectScope([{ status: 'D', path }], files({})).scope, 'full', path);
  assert.equal(selectScope([{ status: 'M', path: 'docs/installation.md' }], files({})).scope, 'docs');
});

test('the doc lists match what the code reads', async (t) => {
  const { classifyPath, PACKAGED_DOCS } = await load();
  const listed = spawnSync('git', ['ls-files', '*.rs'], { cwd: rootPath, encoding: 'utf8' });
  if (listed.status !== 0) return t.skip('git is not available');
  let included = 0;
  for (const file of listed.stdout.split('\n').filter(Boolean)) {
    for (const [, target] of readFileSync(new URL(file, root), 'utf8').matchAll(/include_str!\("([^"]+\.md)"\)/g)) {
      // URL arithmetic keeps forward slashes on Windows too.
      const doc = new URL(target, new URL(file, root)).href.slice(root.href.length);
      assert.notEqual(classifyPath(doc), 'docs', `${file} includes ${doc}; add it to ASSERTED_DOCS`);
      included += 1;
    }
  }
  assert.ok(included > 0, 'expected Rust tests to include documentation');
  let packaged = 0;
  const smoke = readFileSync(new URL('scripts/package-smoke.mjs', root), 'utf8');
  for (const [, doc] of smoke.matchAll(/'((?:[\w-]+\/)*[\w.-]+\.md|LICENSE)'/g))
  {
    assert.ok(PACKAGED_DOCS.has(doc), `package-smoke asserts ${doc}; add it to PACKAGED_DOCS`);
    packaged += 1;
  }
  assert.ok(packaged > 0, 'expected package-smoke to assert packaged documentation');
});

test('build, packaging, CI and selector changes run every target', async () => {
  const { selectScope } = await load();
  for (const path of [
    '.github/workflows/companion.yml', 'scripts/native-scope.mjs', 'scripts/alias-smoke.mjs', 'scripts/prompt-smoke.mjs',
    'Cargo.toml', 'Cargo.lock', 'crates/hraness-local-app/Cargo.toml', 'crates/hraness-cli-kit/LICENSE', 'build.rs', 'tauri.conf.json', 'windows.manifest.xml',
    'capabilities/README.json', 'icons/icon.ico', 'examples/stdio_fixture.rs', 'package.json', 'package-lock.json',
    'sdk/tsconfig.json', '.gitattributes', 'crates/hraness-control-kit/tests/golden/tui-status-80.txt',
  ]) {
    assert.equal(selectScope([{ status: 'M', path }, { status: 'M', path: 'README.md' }], files({})).scope, 'full', path);
  }
});

test('the selector fails closed', async () => {
  const { selectScope } = await load();
  assert.equal(selectScope([], files({})).scope, 'full', 'empty change list');
  assert.equal(selectScope([{ status: 'M', path: 'somewhere/new.bin' }], files({})).scope, 'full', 'unmapped path');
  assert.equal(selectScope([{ status: 'R100', path: 'src/protocol.rs' }], files({})).scope, 'full', 'unexpected status');
  assert.equal(selectScope([{ status: 'M', path: 'src/protocol_v2.rs' }], files({})).scope, 'full', 'unreadable file');
  assert.equal(selectScope(null as unknown as Change[], files({})).scope, 'full', 'no list');
  const reasons = selectScope([{ status: 'M', path: 'src/protocol_v2.rs' }], files({})).reasons.join('\n');
  assert.match(reasons, /selector error/);
});

test('the command line reports full when git cannot compute the change', () => {
  const run = spawnSync(process.execPath, [fileURLToPath(script), 'no-such-rev', 'HEAD'], { cwd: rootPath, encoding: 'utf8' });
  assert.equal(run.status, 0);
  assert.equal(run.stdout.trim(), 'full');
  assert.match(run.stderr, /selector error/);
});

test('every tracked file maps to a scope rule', async (t) => {
  const { classifyPath } = await load();
  const listed = spawnSync('git', ['ls-files'], { cwd: rootPath, encoding: 'utf8' });
  if (listed.status !== 0) return t.skip('git is not available');
  const unmapped = listed.stdout.split('\n').filter(Boolean).filter(path => classifyPath(path) === 'unmapped');
  assert.deepEqual(unmapped, [], 'new top-level inputs need a rule in scripts/native-scope.mjs');
});
