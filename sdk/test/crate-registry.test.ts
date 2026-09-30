import { test } from 'node:test';
import assert from 'node:assert/strict';

const script = new URL('../../scripts/verify-crate-registry.mjs', import.meta.url).href;
type Published = { crate: string; num: string; checksum: string; yanked: boolean };
const checksum = 'ab'.repeat(32);
const valid = { crate: 'hraness-cli-kit', num: '1.1.2', checksum, yanked: false };
const load = async () => await import(script) as {
  verifyRegistryArchive: (response: { version?: Partial<Published> }, name: string, version: string, checksum: string) => void;
};

test('the registry archive must match the prepared crate and remain available', async () => {
  const { verifyRegistryArchive } = await load();
  const verify = (version?: Partial<Published>) => verifyRegistryArchive({ version }, valid.crate, valid.num, checksum);
  assert.doesNotThrow(() => verify(valid));
  for (const changed of [
    { crate: 'another-crate' }, { num: '1.1.1' }, { checksum: 'cd'.repeat(32) }, { yanked: true },
  ]) assert.throws(() => verify({ ...valid, ...changed }));
  assert.throws(() => verify());
  const { yanked: _, ...withoutYanked } = valid;
  assert.throws(() => verify(withoutYanked));
  assert.throws(() => verifyRegistryArchive({ version: valid }, valid.crate, valid.num, 'not-a-checksum'));
});
