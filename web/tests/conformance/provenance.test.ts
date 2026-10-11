import assert from 'node:assert/strict';
import { lstat, readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  provenanceInputPaths,
  validateFixtureProvenance,
  fixtureQualification
} from './provenance.ts';
const root = new URL('../../../', import.meta.url);
const inputs = new Map<string, Uint8Array>();
for (const path of provenanceInputPaths) {
  const file = new URL(path, root);
  const info = await lstat(file);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.nlink === 1 &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  const bytes = new Uint8Array(await readFile(file));
  assert.equal(bytes.byteLength, info.size);
  inputs.set(path, bytes);
}
const manifest = JSON.parse(
  await readFile(
    new URL('../fixtures/provenance/qualified-inputs.v1.json', import.meta.url),
    'utf8'
  )
) as {
  inputs: { path: string; bytes: number; sha256: string }[];
  producer: {
    node: string;
    rust_selection: string;
    applesauce: Record<string, string>;
  };
  qualification: Record<string, string>;
};
void test('frozen public and qualified fixture inputs and outputs bind exact producer selection without claiming execution or independent interoperability', () => {
  validateFixtureProvenance(manifest, inputs);
  assert.equal(manifest.inputs.length, provenanceInputPaths.length);
  assert.equal(manifest.producer.rust_selection, '1.99.0');
  assert.deepEqual(manifest.qualification, fixtureQualification);
  for (const name of [
    'food-qualified-writer-rust.v1.json',
    'message-qualified-writer-rust.v1.json'
  ])
    assert.ok(manifest.inputs.some((row) => row.path.endsWith(name)));
});
void test('every changed raw fixture or producer input byte fails against the frozen manifest even when JSON meaning is unchanged', () => {
  for (const path of provenanceInputPaths) {
    const changed = new Map(inputs);
    const value = inputs.get(path)!;
    const altered = new Uint8Array(value.byteLength + 1);
    altered.set(value);
    altered[value.byteLength] = 32;
    changed.set(path, altered);
    assert.throws(() => validateFixtureProvenance(manifest, changed), path);
  }
});
void test('missing, extra, empty and oversized source inputs fail before fixture qualification', () => {
  for (const mutation of ['missing', 'extra', 'empty', 'oversized']) {
    const changed = new Map(inputs);
    if (mutation === 'missing') changed.delete(provenanceInputPaths[0]);
    else if (mutation === 'extra')
      changed.set('../outside.json', new Uint8Array([1]));
    else
      changed.set(
        provenanceInputPaths[0],
        new Uint8Array(mutation === 'empty' ? 0 : 1024 * 1024 + 1)
      );
    assert.throws(() => validateFixtureProvenance(manifest, changed), mutation);
  }
});
void test('fixture descriptor, producer-version and false deployment or independent-client claims are refused', () => {
  for (const mutation of [
    'hash',
    'size',
    'path',
    'node',
    'rust',
    'sdk',
    'deployment',
    'client',
    'roundtrip'
  ]) {
    const changed = structuredClone(manifest);
    if (mutation === 'hash') changed.inputs[0].sha256 = '0'.repeat(64);
    else if (mutation === 'size') changed.inputs[0].bytes++;
    else if (mutation === 'path')
      changed.inputs[0].path = 'web/static/fixture.json';
    else if (mutation === 'node') changed.producer.node = '26.0.0';
    else if (mutation === 'rust') changed.producer.rust_selection = '1.97.1';
    else if (mutation === 'sdk')
      changed.producer.applesauce['applesauce-core'] = '0.0.0';
    else if (mutation === 'deployment')
      changed.qualification.deployment = 'PASS';
    else if (mutation === 'client')
      changed.qualification.independent_client_receive_reply = 'PASS';
    else changed.qualification.sdk_roundtrips = 'FULL_INTEROP_PASS';
    assert.throws(() => validateFixtureProvenance(changed, inputs), mutation);
  }
});
