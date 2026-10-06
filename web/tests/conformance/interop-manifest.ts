import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { checkFoodReplay } from './food-bidirectional.ts';
import type { webTemplates } from './food-writer-cases.ts';

export type InteropInputs = ReadonlyMap<string, Uint8Array>;
const food = 'contracts/interop/food_availability/';
const revision = '189c49b74b4bafc142b00b76b296477931139e72';
export const interopInputPaths = [
  `${food}corpus.v1.json`,
  `${food}source_profile.v1.json`,
  `${food}provenance.v1.json`,
  `${food}oracle/src/main.rs`,
  `${food}oracle/Cargo.toml`,
  'radroots.lib.source-lock.v1.toml',
  'web/tests/conformance/food-writer-rust.v1.json',
  `${food}web_templates.v1.json`
] as const;
const hash = (bytes: Uint8Array) =>
  createHash('sha256').update(bytes).digest('hex');
interface Vector {
  id: string;
  kind: string;
  input: Record<string, unknown>;
  expected: Record<string, unknown>;
  signed_wires: Record<string, string>;
}
interface Corpus {
  contract_version: string;
  revision: string;
  source_sha256: string;
  vectors: Vector[];
}
export function buildInteropManifest(
  inputs: InteropInputs,
  templates: typeof webTemplates
) {
  assert.deepEqual([...inputs.keys()].sort(), [...interopInputPaths].sort());
  const bytes = (path: string) => {
    const value = inputs.get(path);
    assert.ok(value && value.byteLength > 0 && value.byteLength <= 1024 * 1024);
    return value;
  };
  const text = (path: string) =>
    new TextDecoder('utf-8', { fatal: true }).decode(bytes(path));
  const descriptor = (path: string) => ({
    path,
    sha256: hash(bytes(path)),
    bytes: bytes(path).byteLength
  });
  const corpus = JSON.parse(text(`${food}corpus.v1.json`)) as Corpus;
  const profile = JSON.parse(text(`${food}source_profile.v1.json`)) as {
    contract_version: string;
    suite: string;
    vectors: { id: string; kind: string; input: unknown; expected: unknown }[];
  };
  const provenance = JSON.parse(text(`${food}provenance.v1.json`)) as {
    public_source: { repository: string; revision: string; sha256: string };
    corpus: { path: string; sha256: string; bytes: number };
  };
  assert.equal(corpus.revision, revision);
  assert.equal(provenance.public_source.revision, revision);
  assert.equal(
    provenance.public_source.repository,
    'https://github.com/radrootslabs/lib'
  );
  assert.equal(corpus.contract_version, '1.0.0');
  assert.equal(profile.contract_version, corpus.contract_version);
  assert.equal(profile.suite, 'food_availability_profile');
  assert.equal(
    corpus.source_sha256,
    hash(bytes(`${food}source_profile.v1.json`))
  );
  assert.equal(provenance.public_source.sha256, corpus.source_sha256);
  assert.deepEqual(provenance.corpus, {
    path: 'corpus.v1.json',
    sha256: hash(bytes(`${food}corpus.v1.json`)),
    bytes: bytes(`${food}corpus.v1.json`).byteLength
  });
  assert.equal(corpus.vectors.length, 40);
  assert.equal(profile.vectors.length, 40);
  assert.deepEqual(
    corpus.vectors.map(({ id, kind, input, expected }) => ({
      id,
      kind,
      input,
      expected
    })),
    profile.vectors
  );
  assert.deepEqual(
    [
      ...text('radroots.lib.source-lock.v1.toml').matchAll(
        /^revision = "([0-9a-f]{40})"$/gmu
      )
    ].map((match) => match[1]),
    [revision]
  );
  assert.ok(text(`${food}oracle/Cargo.toml`).includes(`rev = "${revision}"`));
  const replay = JSON.parse(
    text('web/tests/conformance/food-writer-rust.v1.json')
  ) as unknown;
  checkFoodReplay(
    replay,
    templates,
    [
      'corpus.v1.json',
      'source_profile.v1.json',
      'provenance.v1.json',
      'oracle/src/main.rs'
    ].map((name) => [name, bytes(food + name)] as const)
  );
  const templateBytes = new TextEncoder().encode(
    JSON.stringify(templates, null, 2) + '\n'
  );
  assert.deepEqual(bytes(`${food}web_templates.v1.json`), templateBytes);
  const fixture = (path: string, caseIndex: number, caseId: string) => ({
    ...descriptor(path),
    case_index: caseIndex,
    case_id: caseId
  });
  const publicCases = corpus.vectors.map((row, index) => {
    const reader = typeof row.signed_wires.event === 'string';
    return {
      id: row.id,
      protocol: {
        profile: profile.suite,
        contract_version: profile.contract_version
      },
      fixture: fixture(`${food}corpus.v1.json`, index, row.id),
      oracle_revision: revision,
      expected: row.expected,
      participation: {
        rust_oracle: 'REQUIRED',
        native_consumer: 'PENDING_HCR014',
        typescript: reader ? 'food_reader' : 'UNSUPPORTED_CURRENT_ADAPTER',
        unsupported_reason: reader
          ? null
          : row.kind.includes('validate_revision')
            ? 'Current website reader is not a revision-transition adapter; original head-selection checkpoints retain this obligation.'
            : 'Public authored-details recipes include profile capabilities beyond the strict text-only prototype builder; its separate sixteen actual outputs are shared below.'
      }
    };
  });
  const writers = templates.map((row, index) => ({
    id: row.id,
    protocol: {
      profile: profile.suite,
      contract_version: profile.contract_version
    },
    fixture: fixture(`${food}web_templates.v1.json`, index, row.id),
    oracle_revision: revision,
    expected: { result: 'accepted', wire_parts: row.wire_parts },
    participation: {
      rust_oracle: 'REQUIRED',
      native_consumer: 'PENDING_HCR014',
      typescript: 'food_writer',
      unsupported_reason: null
    }
  }));
  const cases = [...publicCases, ...writers];
  assert.equal(cases.length, 56);
  assert.equal(new Set(cases.map((row) => row.id)).size, cases.length);
  assert.equal(
    publicCases.filter((row) => row.participation.typescript === 'food_reader')
      .length,
    18
  );
  return {
    schema_version: 1,
    qualification: 'FIXTURE_ONLY_NOT_DEPLOYED_TERA',
    signing: 'NOT_RUN_NO_KEYS',
    oracle: { repository: provenance.public_source.repository, revision },
    inputs: interopInputPaths.map(descriptor),
    fixture_authority: {
      public_corpus:
        'Existing pinned public corpus/profile/provenance and producer remain immutable.',
      website_templates:
        'Derived from actual source-controlled Food builder recipes; regenerate only by explicit reviewed generate-interop-manifest.mjs --write. No real user data.',
      manifest:
        'Derived metadata and expectations, not a second editable protocol policy.'
    },
    pending_checks: [
      'native_consumer_HCR014',
      'combined_orchestration_HCR015',
      'native_full_u64_vs_unsafe_js_common_boundary_HCR014',
      'Message_adapters_HCP071_HCP130',
      'deployed_Tera_qualification'
    ],
    cases
  };
}
export function validateInteropManifest(
  manifest: unknown,
  inputs: InteropInputs,
  templates: typeof webTemplates
) {
  assert.deepEqual(manifest, buildInteropManifest(inputs, templates));
}
