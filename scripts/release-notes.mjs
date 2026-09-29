// Renders the GitHub Release body for a tag, per hraness/.github RELEASES.md:
// the CHANGELOG.md section for the version (summary and `## Changes`), then
// generated `## Install` and `## Verify`, then the identity record as the
// final bytes. Fails when the section is missing, empty or says Unreleased.
//
//   node scripts/release-notes.mjs v0.9.0 <commit> > notes.md
//
// Reads CHANGELOG.md and artifacts/release-manifest.json from the working
// directory.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

const REPOSITORY = 'hraness/desktop-foundation';

export function changelogSection(changelog, version) {
  const lines = changelog.split('\n');
  const heading = new RegExp(`^## v?${version.replaceAll('.', '\\.')}(?: - \\d{4}-\\d{2}-\\d{2})?\\s*$`);
  const start = lines.findIndex(line => heading.test(line));
  if (start < 0) throw new Error(`CHANGELOG.md has no section for ${version}`);
  let end = lines.findIndex((line, index) => index > start && /^## /.test(line));
  if (end < 0) end = lines.length;
  // Sub-headings inside the section are one level down (`### Changes`).
  const body = lines.slice(start + 1, end).join('\n').trim().replace(/^### /gm, '## ');
  if (!body) throw new Error(`CHANGELOG.md section ${version} is empty`);
  if (/unreleased/i.test(lines[start]) || /^unreleased\b/i.test(body)) throw new Error(`CHANGELOG.md section ${version} still says Unreleased`);
  if (!/^## Changes$/m.test(body) || !/^- /m.test(body)) throw new Error(`CHANGELOG.md section ${version} needs a summary and a ## Changes list`);
  return body;
}

export function renderNotes({ changelog, tag, commit, manifestSha256 }) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error(`Not a release tag: ${tag}`);
  if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error('Commit must be a full SHA');
  const version = tag.slice(1);
  const section = changelogSection(changelog, version);
  const identity = { repository: REPOSITORY, tag, commit, ...(manifestSha256 ? { releaseManifestSha256: manifestSha256 } : {}) };
  return `${section}

## Install

\`\`\`sh
npm install https://github.com/${REPOSITORY}/releases/download/${tag}/hraness-desktop-foundation-${version}.tgz
\`\`\`

Rust products pin the tag: \`desktop-foundation = { git = "https://github.com/${REPOSITORY}", tag = "${tag}" }\`.

## Verify

Checksums for every asset are in \`SHA256SUMS\`. The source commit is \`${commit}\`. Each asset has a GitHub build attestation: \`gh attestation verify <file> --repo ${REPOSITORY}\`. See [the installation guide](https://github.com/${REPOSITORY}/blob/${tag}/docs/installation.md).

<!-- hraness-release ${JSON.stringify(identity)} -->`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [tag, commit] = process.argv.slice(2);
  const manifestSha256 = createHash('sha256').update(readFileSync('artifacts/release-manifest.json')).digest('hex');
  process.stdout.write(renderNotes({ changelog: readFileSync('CHANGELOG.md', 'utf8'), tag, commit, manifestSha256 }));
}
