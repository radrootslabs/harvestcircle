import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { readFoodEvent } from '../../src/lib/contracts/food-availability-v1/read.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../../src/lib/nostr/verified-envelope.ts';

interface Vector {
  id: string;
  kind: string;
  expected: Record<string, unknown>;
  signed_wires: Record<string, string>;
}
const corpus = JSON.parse(
  await readFile(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: Vector[] };
for (const row of corpus.vectors) {
  if (!row.signed_wires.event) continue;
  void test(`pinned Rust reader outcome: ${row.id}`, () => {
    const actual = readFoodEvent(row.signed_wires.event);
    const expected = row.expected;
    if ('error' in expected) {
      assert.equal(actual.outcome, 'rejected');
      if (actual.outcome === 'rejected')
        assert.deepEqual(actual.error, expected.error);
    } else if ('projection' in expected) {
      assert.equal(actual.outcome, 'admitted');
      if (actual.outcome === 'admitted') {
        assert.equal(actual.event_id, expected.event_id);
        assert.deepEqual(actual.projection, expected.projection);
      }
    } else if ('partition' in expected) {
      assert.equal(actual.outcome, 'excluded');
      if (actual.outcome === 'excluded') {
        assert.equal(actual.event_id, expected.event_id);
        assert.equal(actual.partition, expected.partition);
      }
    } else {
      assert.equal(actual.outcome, 'admitted');
      if (actual.outcome === 'admitted') {
        assert.equal(actual.event_id, expected.event_id);
        assert.equal(actual.projection.images.length, expected.image_count);
        assert.deepEqual(
          actual.projection.images[0].raw_tag,
          expected.first_raw_tag
        );
        assert.deepEqual(
          actual.projection.images.at(-1)?.raw_tag,
          expected.last_raw_tag
        );
        assert.deepEqual(actual.projection.diagnostics, expected.diagnostics);
      }
    }
  });
}
void test('all public signed wires cross actual SDK verification; detached snapshots cannot change retained proof', () => {
  let checked = 0;
  for (const row of corpus.vectors)
    for (const raw of Object.values(row.signed_wires)) {
      const result = verifyEnvelope(raw);
      if (row.id.endsWith('_031')) {
        assert.equal(result.ok, false);
      } else {
        assert.equal(result.ok, true, row.id);
        if (result.ok) {
          const a = verifiedEnvelopeSnapshot(result.value)!;
          a.tags.length = 0;
          a.content = 'mutated';
          a.created_at = 0;
          assert.deepEqual(
            verifiedEnvelopeSnapshot(result.value),
            JSON.parse(raw),
            row.id
          );
          assert.equal(verifiedEnvelopeSnapshot({} as never), undefined);
        }
      }
      checked++;
    }
  assert.equal(checked, 36);
});

const sample = corpus.vectors.find((row) => row.id.endsWith('_014'))!
  .signed_wires.event;
void test('root numeric spelling cannot be laundered by JSON numeric rounding', () => {
  for (const spelling of [
    '1700000060.0',
    '1.700000060e9',
    '1700000060.00000000001'
  ]) {
    const altered = sample.replace(
      /"created_at":\s*[0-9]+/u,
      `"created_at":${spelling}`
    );
    assert.notEqual(altered, sample);
    assert.equal(verifyEnvelope(altered).ok, false, spelling);
  }
  const altered = sample.replace(/"kind":\s*30402/u, '"kind":30402.0');
  assert.notEqual(altered, sample);
  assert.equal(verifyEnvelope(altered).ok, false);
});
void test('root numeric source is separate from nested unknown extension fields', () => {
  const altered = sample.replace(
    /\}$/u,
    ',"extension":{"kind":1.0,"created_at":2e0}}'
  );
  assert.equal(verifyEnvelope(altered).ok, true);
});
void test('extra-field counts and serialized key/value byte charge match pinned wire limits', () => {
  const original = JSON.parse(sample) as Record<string, unknown>;
  const tooMany = {
    ...original,
    ...Object.fromEntries(Array.from({ length: 65 }, (_, i) => [`x${i}`, 0]))
  };
  assert.equal(verifyEnvelope(JSON.stringify(tooMany)).ok, false);
  const value = 'x'.repeat(65536 - 6); // JSON key \"x\": plus JSON value quotes = six bytes.
  assert.equal(
    verifyEnvelope(JSON.stringify({ ...original, x: value })).ok,
    true
  );
  assert.equal(
    verifyEnvelope(JSON.stringify({ ...original, x: value + 'x' })).ok,
    false
  );
  const maximum = {
    ...original,
    ...Object.fromEntries(Array.from({ length: 64 }, (_, i) => [`x${i}`, 0]))
  };
  assert.equal(verifyEnvelope(JSON.stringify(maximum)).ok, true);
});

void test('parsed nonfinite extras and ill formed Unicode are rejected before verified proof', () => {
  for (const suffix of [
    '"extension":1e309',
    '"extension":{"value":-1e309}',
    '"extension":"\\ud800"',
    '"\\ud800":0'
  ]) {
    const altered = sample.replace(/\}$/u, `,` + suffix + '}');
    assert.notEqual(altered, sample);
    assert.equal(verifyEnvelope(altered).ok, false, suffix);
  }
});
