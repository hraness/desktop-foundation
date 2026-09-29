import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// Compiled to dist/test; the repository root is two levels up.
const root = new URL('../../', import.meta.url);
const script = new URL('scripts/release-notes.mjs', root).href;
const manifest = JSON.parse(readFileSync(new URL('package.json', root), 'utf8')) as { version: string };

test('the release page comes from the CHANGELOG section for this version', async () => {
  const { renderNotes } = await import(script) as { renderNotes: (o: object) => string };
  const changelog = readFileSync(new URL('CHANGELOG.md', root), 'utf8');
  const tag = `v${manifest.version}`;
  const commit = '0'.repeat(40);
  const notes = renderNotes({ changelog, tag, commit, manifestSha256: 'f'.repeat(64) });
  const headings = [...notes.matchAll(/^## (.+)$/gm)].map(m => m[1]);
  assert.deepEqual(headings, ['Changes', 'Install', 'Verify'], 'summary, then Changes, Install, Verify');
  assert.ok(!notes.startsWith('#'), 'the summary comes first');
  assert.ok(notes.includes(`releases/download/${tag}/hraness-desktop-foundation-${manifest.version}.tgz`));
  assert.ok(notes.includes(commit));
  assert.match(notes, /\n<!-- hraness-release \{[^\n]*\} -->$/, 'identity comment is the final bytes');
  const identity = JSON.parse(/<!-- hraness-release (\{.*\}) -->$/.exec(notes)![1]);
  assert.deepEqual(identity, { repository: 'hraness/desktop-foundation', tag, commit, releaseManifestSha256: 'f'.repeat(64) });
  assert.ok(!/What's Changed|Full Changelog|Generated with/i.test(notes));
  for (const bad of ['# Changelog\n', `# C\n\n## ${manifest.version}\n\n`, `# C\n\n## ${manifest.version} Unreleased\n\nText.\n\n### Changes\n\n- x\n`, `# C\n\n## ${manifest.version}\n\nOnly a summary.\n`]) {
    assert.throws(() => renderNotes({ changelog: bad, tag, commit }));
  }
  assert.throws(() => renderNotes({ changelog, tag: 'latest', commit }));
});
