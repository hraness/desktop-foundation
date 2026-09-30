// Decides how much of the native matrix a pull request needs.
//
//   docs   every changed file is documentation: no native legs, no package.
//   subset only platform-neutral files changed: Linux x64, macOS arm64 and
//          Windows x64 build and run every native gate, so every OS family
//          (and its paths, process and terminal semantics) is exercised; the
//          other three targets and the package job wait for main, the
//          nightly run and tags.
//   full   anything else: all six targets and the package job.
//
// It fails closed. An unmapped path, an unreadable file, an unexpected git
// status, an empty change list or any error selects `full`. Pushes to main,
// the nightly schedule, manual runs and tags never call this: they are full.
//
// CI runs the copy of this file from the pull request's base commit, so an
// edit to this selector alone cannot narrow the matrix of the pull request
// that makes it, and a change here maps to `full`. The workflow itself still
// comes from the pull request, so workflow edits are reviewed as usual.
//
// Known limit: a file gated only by its parent (`#[cfg(unix)] mod x;`) and
// with no marker of its own is judged by its own content. Windows x64 in the
// subset covers the OS-family split; only an architecture-only difference in
// such a file would wait for main.
//
// Usage: node scripts/native-scope.mjs <base-rev> <head-rev>
// Prints the scope on stdout and one reason per line on stderr.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Documentation that code reads. Rust tests include_str! these (native legs
// only), so an edit runs the subset rather than skipping native.
const ASSERTED_DOCS = new Set(['docs/protocol.md', 'docs/permissions.md']);
// Files scripts/package-smoke.mjs asserts in the installed package (existence,
// and the skill's front matter). The package job runs only on `full`, so an
// edit to the skill, or deleting any of these, runs every target.
export const PACKAGED_DOCS = new Set([
  'README.md', 'LICENSE', 'docs/installation.md', 'docs/platforms.md', 'skills/companion/SKILL.md',
  'docs/architecture.md', 'docs/adoption.md', 'docs/protocol.md', 'docs/migration-1.0.md',
  'docs/permissions.md', 'docs/identity.md', 'docs/control.md', 'docs/human-gate.md',
]);
// Documentation only.
const DOCS = [/\.md$/, /^docs\//, /^LICENSE$/];

// Build, packaging, platform and CI inputs. Always the full matrix.
const FULL = [
  /^\.github\//,
  /^\.gitattributes$/, // line endings differ on Windows checkouts
  /^\.gitignore$/,
  /^scripts\//, // smokes, fixtures, packaging and this selector
  /^Cargo\.(toml|lock)$/,
  /^crates\/[^/]+\/Cargo\.toml$/,
  /^crates\/[^/]+\/LICENSE$/, // part of the published crate archive
  /^build\.rs$/,
  /^rust-toolchain(\.toml)?$/,
  /^tauri\.conf\.json$/,
  /^windows\.manifest\.xml$/,
  /^capabilities\//,
  /^icons\//,
  /^examples\//,
  /^crates\/[^/]+\/tests\/golden\//, // not pinned to LF, so Windows checkouts differ
  /^skills\//, // packaged; package-smoke reads the skill
  /^frontend\//,
  /^package(-lock)?\.json$/,
  /^sdk\/tsconfig\.json$/,
];

// Authorization, credential and process-launch modules. Their behaviour
// rests on OS facilities (ps, /dev/tty, drive-letter and UNC paths, .exe
// resolution) that a marker scan can miss, so they always run every target.
export const SECURITY = new Set([
  'sdk/src/human-gate.ts', 'sdk/src/helper.ts', 'sdk/src/prompt.ts', 'sdk/src/login.ts',
  'crates/hraness-control-kit/src/gate.rs', 'crates/hraness-control-kit/src/crypto.rs',
  'crates/hraness-local-app/src/helper.rs',
]);

// Source whose platform exposure is read from its content: a file that has
// (or had) a platform branch, or names a platform, needs every target.
const SOURCE = [
  /^src\/.+\.rs$/,
  /^tests\/.+\.rs$/,
  /^crates\/[^/]+\/src\/.+\.rs$/,
  /^crates\/[^/]+\/tests\/[^/]+\.rs$/,
  /^sdk\/src\/.+\.ts$/,
  /^sdk\/test\/[^/]+\.ts$/,
];

// Byte-compared data that .gitattributes pins to LF on every platform.
const DATA = [/^contract\//, /^sdk\/test\/golden\//];

// Any hint that code behaves differently per OS or architecture. OS names
// count as quoted literals, not in prose, so a comment that mentions Linux
// does not pull in every target.
export const PLATFORM_MARKER = /\btarget_(?:os|family|arch|env|vendor|pointer_width)\b|cfg!?\(\s*(?:(?:all|any|not)\(\s*)*(?:windows|unix)\b|\bprocess\.(?:platform|arch)\b|\bos\.(?:platform|type|arch|release)\(|['\"`](?:win32|darwin|linux|macos|windows|freebsd)['\"`]|\bwindows_sys\b|\bwinapi\b|\bobjc2?\b|\bcocoa\b|\bgtk\b|\bpowershell\b|\bpwsh\b|\\\\\.\\pipe|\bstd::os::(?:windows|unix|macos|linux|fd)\b|\bwindows::|\blibc::|\bnix::|\bpath\.(?:sep|delimiter|win32|posix)\b|\bos\.EOL\b|\{[^}]*\b(?:platform|arch|type|EOL|release)\b[^}]*\}\s*from\s*['"](?:node:)?os['"]|['"`]\/dev\/|\.exe\b|['"`]ps['"`]/i;
const PLATFORM_NAME = /(?:^|[/_.-])(?:macos|mac|darwin|windows|win|linux|gtk|appkit|cocoa|tray)(?:[/_.-]|$)/i;

const matches = (patterns, path) => patterns.some(p => p.test(path));

/** Which rule set a path falls under: docs, full, source, data or unmapped. */
export function classifyPath(path, status = 'M') {
  if (PACKAGED_DOCS.has(path) && status === 'D') return 'full';
  if (path.startsWith('skills/')) return 'full';
  if (SECURITY.has(path)) return 'full';
  if (ASSERTED_DOCS.has(path)) return 'data';
  if (matches(DOCS, path)) return 'docs';
  if (matches(FULL, path)) return 'full';
  if (matches(SOURCE, path)) return 'source';
  if (matches(DATA, path)) return 'data';
  return 'unmapped';
}

/** The SDK test that pairs with an SDK source file: sdk/src/x.ts -> sdk/test/x.test.ts. */
export function pairedTest(path) {
  const match = /^sdk\/src\/(.+)\.ts$/.exec(path);
  return match ? `sdk/test/${match[1]}.test.ts` : null;
}

/**
 * @param changes [{ status: 'A'|'M'|'D'|'T', path }] relative to the base.
 * @param read (side: 'base'|'head', path) => string; throws when unreadable.
 * @param has (side, path) => boolean; whether the path exists on that side.
 *        Throws on error. Without it, a paired test is read and must exist.
 * @returns { scope: 'docs'|'subset'|'full', reasons: string[] }
 */
export function selectScope(changes, read, has) {
  const reasons = [];
  try {
    if (!Array.isArray(changes) || changes.length === 0) return { scope: 'full', reasons: ['no changed files reported'] };
    let docsOnly = true;
    for (const change of changes) {
      const { status, path } = change ?? {};
      if (typeof path !== 'string' || !path) return { scope: 'full', reasons: ['malformed change entry'] };
      if (!['A', 'M', 'D', 'T'].includes(status)) return { scope: 'full', reasons: [`${path}: unexpected git status ${status}`] };
      const kind = classifyPath(path, status);
      if (kind === 'docs') continue;
      docsOnly = false;
      if (kind === 'full') reasons.push(`${path}: build, packaging, CI, packaged-doc or security input`);
      else if (kind === 'unmapped') reasons.push(`${path}: not mapped to a scope`);
      else if (kind === 'source') {
        if (PLATFORM_NAME.test(path)) { reasons.push(`${path}: platform-specific file`); continue; }
        const sides = status === 'A' ? ['head'] : status === 'D' ? ['base'] : ['base', 'head'];
        let marked = false;
        for (const side of sides) {
          if (PLATFORM_MARKER.test(read(side, path))) { reasons.push(`${path}: platform branch in ${side}`); marked = true; break; }
        }
        // An SDK module whose test branches on the platform behaves per OS
        // even when the module itself names none (e.g. `ps`, /dev/tty).
        const test = pairedTest(path);
        if (!marked && test) {
          for (const side of ['base', 'head']) {
            if (has && !has(side, test)) continue;
            if (PLATFORM_MARKER.test(read(side, test))) { reasons.push(`${path}: ${test} branches on the platform in ${side}`); break; }
          }
        }
      }
    }
    if (docsOnly) return { scope: 'docs', reasons: ['documentation only'] };
    if (reasons.length) return { scope: 'full', reasons };
    return { scope: 'subset', reasons: ['only platform-neutral source and data changed'] };
  } catch (error) {
    return { scope: 'full', reasons: [...reasons, `selector error: ${error?.message ?? error}`] };
  }
}

function git(args) {
  const result = spawnSync('git', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`git ${args[0]} failed: ${(result.stderr || '').trim()}`);
  return result.stdout;
}

export function gitChanges(base, head) {
  const fields = git(['diff', '--name-status', '--no-renames', '-z', `${base}`, `${head}`]).split('\0');
  if (fields.at(-1) === '') fields.pop();
  if (fields.length % 2) throw new Error('odd git diff output');
  const changes = [];
  for (let i = 0; i < fields.length; i += 2) changes.push({ status: fields[i], path: fields[i + 1] });
  return changes;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  let result;
  try {
    const [base, head] = process.argv.slice(2);
    if (!base || !head) throw new Error('usage: native-scope.mjs <base-rev> <head-rev>');
    const revs = { base, head };
    result = selectScope(
      gitChanges(base, head),
      (side, path) => git(['show', `${revs[side]}:${path}`]),
      (side, path) => git(['ls-tree', '--name-only', revs[side], '--', path]).trim() === path,
    );
  } catch (error) {
    result = { scope: 'full', reasons: [`selector error: ${error?.message ?? error}`] };
  }
  for (const reason of result.reasons.slice(0, 50)) process.stderr.write(`${reason}\n`);
  process.stdout.write(`${result.scope}\n`);
}
