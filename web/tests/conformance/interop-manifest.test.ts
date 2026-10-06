import assert from 'node:assert/strict';
import { lstat, readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  interopInputPaths,
  validateInteropManifest
} from './interop-manifest.ts';
import { webTemplates } from './food-writer-cases.ts';
const root = new URL('../../../', import.meta.url);
const inputs = new Map<string, Uint8Array>();
for (const path of interopInputPaths) {
  const file = new URL(path, root);
  const info = await lstat(file);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  inputs.set(path, new Uint8Array(await readFile(file)));
}
const manifest = JSON.parse(
  await readFile(new URL('contracts/interop/manifest.json', root), 'utf8')
) as {
  oracle: { revision: string };
  cases: {
    id: string;
    fixture: { path: string; sha256: string };
    expected: unknown;
    participation: {
      native_consumer: string;
      typescript: string;
      unsupported_reason: string | null;
    };
  }[];
};
void test('one manifest binds actual forty public cases and sixteen website outputs to the pinned oracle', () => {
  validateInteropManifest(manifest, inputs, webTemplates);
  assert.equal(manifest.cases.length, 56);
  assert.equal(
    manifest.cases.filter(
      (row) => row.participation.typescript === 'food_reader'
    ).length,
    18
  );
});
for (const mutation of [
  'duplicate',
  'missing',
  'hash',
  'revision',
  'unsafe_path',
  'expected',
  'unqualified_native',
  'unsupported'
] as const)
  void test(`shared manifest rejects ${mutation} without normalizing the difference`, () => {
    const changed = structuredClone(manifest);
    if (mutation === 'duplicate') changed.cases[1].id = changed.cases[0].id;
    else if (mutation === 'missing') changed.cases.pop();
    else if (mutation === 'hash')
      changed.cases[0].fixture.sha256 = '0'.repeat(64);
    else if (mutation === 'revision') changed.oracle.revision = '0'.repeat(40);
    else if (mutation === 'unsafe_path')
      changed.cases[0].fixture.path = 'web/static/private.json';
    else if (mutation === 'expected')
      changed.cases[13].expected = { outcome: 'excluded' };
    else if (mutation === 'unqualified_native')
      changed.cases[0].participation.native_consumer = 'PASS';
    else changed.cases[0].participation.unsupported_reason = null;
    assert.throws(() => validateInteropManifest(changed, inputs, webTemplates));
  });
void test('changed raw bytes, missing input, wrong source pin and stale actual website output all fail', () => {
  for (const mutation of ['bytes', 'missing', 'pin', 'builder'] as const) {
    const changed = new Map(inputs);
    let templates = webTemplates;
    if (mutation === 'missing') changed.delete(interopInputPaths[0]);
    else if (mutation === 'builder') templates = webTemplates.slice(1);
    else if (mutation === 'pin')
      changed.set(
        'radroots.lib.source-lock.v1.toml',
        new TextEncoder().encode('revision = "' + '0'.repeat(40) + '"\n')
      );
    else changed.set(interopInputPaths[0], new TextEncoder().encode('{}'));
    assert.throws(() => validateInteropManifest(manifest, changed, templates));
  }
});
