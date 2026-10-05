import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstat, readFile } from 'node:fs/promises';
import test from 'node:test';
import { getEventHash, verifyEvent } from 'applesauce-core/helpers';
import type { NostrEvent } from 'applesauce-core/helpers';

const root = new URL(
  '../../../contracts/interop/food_availability/',
  import.meta.url
);
const revision = '189c49b74b4bafc142b00b76b296477931139e72';
const sourceHash =
  'dede1eb1f682ecd548e8cd3ccb251d298762e504e2588a73528e5f7a5b9eff21';
const digest = (bytes: Uint8Array) =>
  createHash('sha256').update(bytes).digest('hex');

interface Vector {
  id: string;
  kind: string;
  input: Record<string, unknown>;
  expected: Record<string, unknown>;
  signed_wires: Record<string, string>;
  unsigned_wire_parts: {
    kind: number;
    content: string;
    tags: string[][];
  } | null;
}
interface Corpus {
  revision: string;
  source_sha256: string;
  vectors: Vector[];
}
interface Descriptor {
  path: string;
  sha256: string;
  bytes: number;
}
interface Provenance {
  schema_version: number;
  public_source: { revision: string; repository: string; sha256: string };
  source_copy: Descriptor;
  corpus: Descriptor;
  producer: { files: Descriptor[]; rust_toolchain: string };
  protocol_references: {
    nip: string;
    revision: string;
    url: string;
    sha256: string;
  }[];
}

async function input(name: string) {
  const file = new URL(name, root);
  const info = await lstat(file);
  assert.ok(info.isFile() && !info.isSymbolicLink());
  assert.ok(info.size > 0 && info.size <= 1024 * 1024);
  const bytes = await readFile(file);
  assert.equal(bytes.length, info.size);
  return bytes;
}

const sourceBytes = await input('source_profile.v1.json');
const corpusBytes = await input('corpus.v1.json');
const source = JSON.parse(sourceBytes.toString()) as Corpus;
const corpus = JSON.parse(corpusBytes.toString()) as Corpus;
const provenance = JSON.parse(
  (await input('provenance.v1.json')).toString()
) as Provenance;

void test('public Food corpus and isolated producer retain exact pinned byte provenance', async () => {
  assert.equal(provenance.schema_version, 1);
  assert.equal(
    provenance.public_source.repository,
    'https://github.com/radrootslabs/lib'
  );
  assert.equal(provenance.public_source.revision, revision);
  assert.equal(provenance.public_source.sha256, sourceHash);
  assert.equal(digest(sourceBytes), sourceHash);
  assert.equal(corpus.revision, revision);
  assert.equal(corpus.source_sha256, sourceHash);
  assert.equal(provenance.source_copy.path, 'source_profile.v1.json');
  assert.equal(provenance.source_copy.sha256, digest(sourceBytes));
  assert.equal(provenance.source_copy.bytes, sourceBytes.length);
  assert.equal(provenance.corpus.path, 'corpus.v1.json');
  assert.equal(provenance.corpus.sha256, digest(corpusBytes));
  assert.equal(provenance.corpus.bytes, corpusBytes.length);
  assert.equal(provenance.producer.rust_toolchain, '1.97.1');
  assert.deepEqual(provenance.producer.files.map((row) => row.path).sort(), [
    'oracle/Cargo.lock',
    'oracle/Cargo.toml',
    'oracle/rust-toolchain.toml',
    'oracle/src/main.rs'
  ]);
  for (const row of provenance.producer.files) {
    const bytes = await input(row.path);
    assert.equal(digest(bytes), row.sha256, row.path);
    assert.equal(bytes.length, row.bytes, row.path);
  }
  assert.deepEqual(
    provenance.protocol_references.map((row) => row.nip),
    ['01', '07', '09', '17', '19', '42', '44', '50', '59', '99']
  );
  for (const row of provenance.protocol_references) {
    assert.equal(row.revision, '0046368a747c5c25ae2bec28bae0e537744c8f10');
    assert.equal(
      row.url,
      `https://raw.githubusercontent.com/nostr-protocol/nips/${row.revision}/${row.nip}.md`
    );
    assert.match(row.sha256, /^[0-9a-f]{64}$/u);
  }
});

void test('all forty public cases retain original input and Rust outcome coverage', () => {
  assert.equal(source.vectors.length, 40);
  assert.equal(corpus.vectors.length, 40);
  assert.equal(new Set(corpus.vectors.map((row) => row.id)).size, 40);
  for (const [index, row] of corpus.vectors.entries()) {
    const original = source.vectors[index];
    assert.equal(row.id, original.id);
    assert.equal(row.kind, original.kind);
    assert.deepEqual(row.input, original.input, row.id);
    assert.deepEqual(row.expected, original.expected, row.id);
  }
  for (const id of [
    'food_admission_normalizes_decimal_currency_014',
    'food_admission_excludes_operational_before_validation_016',
    'food_admission_excludes_generic_nip99_017',
    'food_admission_rejects_ambiguous_markers_018',
    'food_admission_rejects_invalid_signature_031',
    'food_revision_equal_time_a_current_039',
    'food_revision_equal_time_b_current_040'
  ])
    assert.ok(
      corpus.vectors.some((row) => row.id === id),
      id
    );
});

void test('actual Applesauce verifies the Rust-read signed wire bytes and rejects the signature control', () => {
  let wires = 0;
  let rejected = 0;
  for (const row of corpus.vectors) {
    for (const [field, raw] of Object.entries(row.signed_wires)) {
      const event = JSON.parse(raw) as NostrEvent;
      assert.deepEqual(event, row.input[field], row.id);
      assert.ok(
        Number.isSafeInteger(event.created_at) && event.created_at >= 0
      );
      assert.ok(
        Number.isSafeInteger(event.kind) &&
          event.kind >= 0 &&
          event.kind <= 65535
      );
      assert.equal(getEventHash(event), event.id, row.id);
      const invalid = row.id === 'food_admission_rejects_invalid_signature_031';
      assert.equal(verifyEvent(event), !invalid, row.id);
      if (invalid) rejected++;
      wires++;
    }
  }
  assert.equal(wires, 36);
  assert.equal(rejected, 1);
});

void test('eleven actual Rust unsigned outputs preserve complete content and tags', () => {
  let outputs = 0;
  for (const row of corpus.vectors) {
    const wire = row.unsigned_wire_parts;
    if (wire === null) continue;
    assert.equal(row.kind, 'food_availability.build_authored_draft.valid');
    assert.equal(wire.kind, 30402);
    const expected = row.expected.wire_parts as Record<string, unknown>;
    assert.deepEqual(wire.tags, expected.tags);
    if ('content_length' in expected) {
      assert.equal(
        new TextEncoder().encode(wire.content).length,
        expected.content_length
      );
    } else {
      assert.equal(wire.content, expected.content);
    }
    outputs++;
  }
  assert.equal(outputs, 11);
});
