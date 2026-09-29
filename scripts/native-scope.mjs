// Decides how much of the native matrix a pull request needs.
//
//   docs   every changed file is documentation: no native legs, no package.
//   subset only platform-neutral files changed: Linux x64 and macOS arm64
//          build and run every native gate; the other four targets and the
//          package job wait for main, the nightly run and tags.
//   full   anything else: all six targets and the package job.
//
// It fails closed. An unmapped path, an unreadable file, an unexpected git
// status, an empty change list or any error selects `full`. Pushes to main,
// the nightly schedule, manual runs and tags never call this: they are full.
//
// CI runs the copy of this file from the pull request's base commit, so a
// pull request cannot narrow its own matrix; a change here maps to `full`.
//
// Usage: node scripts/native-scope.mjs <base-rev> <head-rev>
// Prints the scope on stdout and one reason per line on stderr.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Documentation only. Kept identical to the previous docs_only filter.
const DOCS = [/\.md$/, /^docs\//, /^LICENSE$/];

// Build, packaging, platform and CI inputs. Always the full matrix.
const FULL = [
  /^\.github\//,
  /^\.gitattributes$/, // line endings differ on Windows checkouts
  /^\.gitignore$/,
  /^scripts\//, // smokes, fixtures, packaging and this selector
  /^Cargo\.(toml|lock)$/,
  /^crates\/[^/]+\/Cargo\.toml$/,
  /^build\.rs$/,
  /^rust-toolchain(\.toml)?$/,
  /^tauri\.conf\.json$/,
  /^windows\.manifest\.xml$/,
  /^capabilities\//,
  /^icons\//,
  /^examples\//,
  /^crates\/[^/]+\/tests\/golden\//, // not pinned to LF, so Windows checkouts differ
  /^crates\/[^/]+\/tests\/golden\//, // not pinned to LF; Windows checkouts differ
  /^frontend\//,
  /^package(-lock)?\.json$/,
  /^sdk\/tsconfig\.json$/,
];

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
const DATA = [/^contract\//, /^sdk\/test\/golden\//, /^skills\//];

// Any hint that code behaves differently per OS or architecture. OS names
// count as quoted literals, not in prose, so a comment that mentions Linux
// does not pull in every target.
export const PLATFORM_MARKER = /\btarget_(?:os|family|arch|env|vendor|pointer_width)\b|cfg!?\(\s*(?:(?:all|any|not)\(\s*)*(?:windows|unix)\b|\bprocess\.(?:platform|arch)\b|\bos\.(?:platform|type|arch|release)\(|['\"`](?:win32|darwin|linux|macos|windows|freebsd)['\"`]|\bwindows_sys\b|\bwinapi\b|\bobjc2?\b|\bcocoa\b|\bgtk\b|\bpowershell\b|\bpwsh\b|\\\\\.\\pipe/i;
const PLATFORM_NAME = /(?:^|[/_.-])(?:macos|mac|darwin|windows|win|linux|gtk|appkit|cocoa|tray)(?:[/_.-]|$)/i;

const matches = (patterns, path) => patterns.some(p => p.test(path));

/** Which rule set a path falls under: docs, full, source, data or unmapped. */
export function classifyPath(path) {
  if (matches(DOCS, path)) return 'docs';
  if (matches(FULL, path)) return 'full';
  if (matches(SOURCE, path)) return 'source';
  if (matches(DATA, path)) return 'data';
  return 'unmapped';
}

/**
 * @param changes [{ status: 'A'|'M'|'D'|'T', path }] relative to the base.
 * @param read (side: 'base'|'head', path) => string; throws when unreadable.
 * @returns { scope: 'docs'|'subset'|'full', reasons: string[] }
 */
export function selectScope(changes, read) {
  const reasons = [];
  try {
    if (!Array.isArray(changes) || changes.length === 0) return { scope: 'full', reasons: ['no changed files reported'] };
    let docsOnly = true;
    for (const change of changes) {
      const { status, path } = change ?? {};
      if (typeof path !== 'string' || !path) return { scope: 'full', reasons: ['malformed change entry'] };
      if (!['A', 'M', 'D', 'T'].includes(status)) return { scope: 'full', reasons: [`${path}: unexpected git status ${status}`] };
      const kind = classifyPath(path);
      if (kind === 'docs') continue;
      docsOnly = false;
      if (kind === 'full') reasons.push(`${path}: build, packaging or CI input`);
      else if (kind === 'unmapped') reasons.push(`${path}: not mapped to a scope`);
      else if (kind === 'source') {
        if (PLATFORM_NAME.test(path)) { reasons.push(`${path}: platform-specific file`); continue; }
        const sides = status === 'A' ? ['head'] : status === 'D' ? ['base'] : ['base', 'head'];
        for (const side of sides) {
          if (PLATFORM_MARKER.test(read(side, path))) { reasons.push(`${path}: platform branch in ${side}`); break; }
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
    result = selectScope(gitChanges(base, head), (side, path) => git(['show', `${revs[side]}:${path}`]));
  } catch (error) {
    result = { scope: 'full', reasons: [`selector error: ${error?.message ?? error}`] };
  }
  for (const reason of result.reasons.slice(0, 50)) process.stderr.write(`${reason}\n`);
  process.stdout.write(`${result.scope}\n`);
}
