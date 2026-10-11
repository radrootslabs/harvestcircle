import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { interopInputPaths } from './interop-manifest.ts';

export const provenanceInputPaths = [
  ...interopInputPaths,
  'contracts/interop/manifest.json',
  'contracts/interop/food_availability/oracle/Cargo.lock',
  ...[
    'corpus.v1.json',
    'source_profile.v1.json',
    'provenance.v1.json',
    'oracle/Cargo.toml',
    'oracle/Cargo.lock',
    'oracle/src/main.rs'
  ].map((name) => 'contracts/interop/message/' + name),
  ...[
    'food-qualified-matrix.ts',
    'food-qualified-writer-rust.v1.json',
    'message-cases.ts',
    'message-qualified-matrix.ts',
    'message-qualified-writer-rust.v1.json',
    'provenance.ts'
  ].map((name) => 'web/tests/conformance/' + name),
  'web/tests/fixtures/independent-nip17/public-outer-wires.v1.json',
  'web/tools/check-food-interop.mjs',
  'web/tools/check-message-interop.mjs',
  'web/tools/generate-qualified-fixture-manifest.mjs',
  'web/package.json',
  'web/pnpm-lock.yaml',
  'core/rust-toolchain.toml'
] as const;
export const fixtureQualification = {
  frozen_fixtures: 'HASHED_SOURCE_AND_OUTPUT_BYTES',
  rust_replays: 'CHECKED_VECTORS_NOT_FRESH_NATIVE_EXECUTION',
  sdk_roundtrips: 'SAME_LIBRARY_NOT_INDEPENDENT_CLIENT',
  published_nip17: 'PUBLIC_OUTER_SIGNATURES_ONLY_NO_DECRYPTION_CUSTODY',
  deployment: 'NOT_QUALIFIED',
  independent_client_receive_reply: 'NOT_QUALIFIED_HCM027_REQUIRED',
  signing: 'NO_FIXTURE_KEYS_OR_OPERATOR_AUTHORIZATION'
} as const;
export function buildFixtureProvenance(
  inputs: ReadonlyMap<string, Uint8Array>
) {
  assert.deepEqual([...inputs.keys()].sort(), [...provenanceInputPaths].sort());
  assert.equal(new Set(provenanceInputPaths).size, provenanceInputPaths.length);
  const bytes = (path: string) => {
    const value = inputs.get(path);
    assert.ok(value && value.byteLength > 0 && value.byteLength <= 1024 * 1024);
    return value;
  };
  const text = (path: string) =>
    new TextDecoder('utf-8', { fatal: true }).decode(bytes(path));
  const json = (path: string): unknown => JSON.parse(text(path));
  const pkg = json('web/package.json') as {
    name: string;
    version: string;
    engines: { node: string; pnpm: string };
    packageManager: string;
    dependencies: Record<string, string>;
    devDependencies: Record<string, string>;
  };
  assert.equal(pkg.name, 'harvestcircle_web');
  assert.equal(pkg.engines.node, '24.21.0');
  assert.equal(pkg.engines.pnpm, '12.9.1');
  assert.equal(pkg.packageManager, 'pnpm@12.9.1');
  assert.equal(pkg.devDependencies.prettier, '3.9.9');
  const channel = [
    ...text('core/rust-toolchain.toml').matchAll(/^channel = "([^"]+)"$/gmu)
  ].map((match) => match[1]);
  assert.deepEqual(channel, ['1.99.0']);
  const revision = '189c49b74b4bafc142b00b76b296477931139e72';
  assert.deepEqual(
    [
      ...text('radroots.lib.source-lock.v1.toml').matchAll(
        /^revision = "([0-9a-f]{40})"$/gmu
      )
    ].map((match) => match[1]),
    [revision]
  );
  for (const family of ['food_availability', 'message']) {
    assert.ok(
      text(`contracts/interop/${family}/oracle/Cargo.toml`).includes(
        `rev = "${revision}"`
      )
    );
    const corpus = json(`contracts/interop/${family}/corpus.v1.json`) as {
      revision: string;
    };
    assert.equal(corpus.revision, revision);
  }
  const publicOuter = json(
    'web/tests/fixtures/independent-nip17/public-outer-wires.v1.json'
  ) as { source: { revision: string; scope: string }; cases: unknown[] };
  assert.equal(
    publicOuter.source.revision,
    '0046368a747c5c25ae2bec28bae0e537744c8f10'
  );
  assert.match(publicOuter.source.scope, /outer ciphertext only/);
  assert.equal(publicOuter.cases.length, 2);
  return {
    schema_version: 1,
    producer: {
      module: 'web/tests/conformance/provenance.ts',
      generator: 'web/tools/generate-qualified-fixture-manifest.mjs',
      node: pkg.engines.node,
      pnpm: pkg.engines.pnpm,
      package: pkg.name,
      package_version: pkg.version,
      rust_selection: channel[0],
      rust_version_meaning: 'CHECKED_IN_SELECTION_NOT_AN_EXECUTION_RECEIPT',
      applesauce: Object.fromEntries(
        ['applesauce-core', 'applesauce-relay', 'applesauce-signers'].map(
          (name) => [name, pkg.dependencies[name]]
        )
      ),
      formatter: pkg.devDependencies.prettier
    },
    oracle: { repository: 'https://github.com/radrootslabs/lib', revision },
    qualification: fixtureQualification,
    inputs: provenanceInputPaths.map((path) => ({
      path,
      bytes: bytes(path).byteLength,
      sha256: createHash('sha256').update(bytes(path)).digest('hex')
    }))
  };
}
export function validateFixtureProvenance(
  manifest: unknown,
  inputs: ReadonlyMap<string, Uint8Array>
) {
  assert.deepEqual(manifest, buildFixtureProvenance(inputs));
}
