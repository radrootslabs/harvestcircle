import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { after, test } from 'node:test';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  qualifiedDrafts,
  qualifiedTemplates
} from './food-qualified-matrix.ts';
import { checkFoodReplay } from './food-bidirectional.ts';
import { readFoodEvent } from '../../src/lib/contracts/food-availability-v1/read.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import {
  createPublicHeadCandidate,
  selectPublicHead,
  publicHeadSnapshot
} from '../../src/lib/catalog/heads.ts';

const root = new URL(
  '../../../contracts/interop/food_availability/',
  import.meta.url
);
const inputs = await Promise.all(
  [
    'corpus.v1.json',
    'source_profile.v1.json',
    'provenance.v1.json',
    'oracle/src/main.rs'
  ].map(async (name) => [name, await readFile(new URL(name, root))] as const)
);
const receipt: unknown = JSON.parse(
  await readFile(
    new URL('./food-qualified-writer-rust.v1.json', import.meta.url),
    'utf8'
  )
);
const secret = crypto.getRandomValues(new Uint8Array(32));
after(() => secret.fill(0));
const signed = (
  tags: readonly (readonly string[])[],
  created_at: number,
  content: string
) =>
  JSON.stringify(
    finalizeEvent(
      { kind: 30402, tags: tags.map((row) => [...row]), created_at, content },
      secret
    )
  );
function head(raw: string) {
  const proof = verifyEnvelope(raw);
  assert.equal(proof.ok, true);
  if (!proof.ok) throw new Error('Invalid signed test event');
  const candidate = createPublicHeadCandidate(proof.value);
  assert.ok(candidate);
  return candidate;
}

void test('separate sixteen boundary recipes replay actual pinned Rust consumption without changing original shared fixtures', () => {
  checkFoodReplay(receipt, qualifiedTemplates, inputs);
  assert.equal(qualifiedTemplates.length, 16);
  assert.notEqual(
    qualifiedDrafts[9].identifier,
    qualifiedDrafts[10].identifier
  );
  assert.equal(
    qualifiedDrafts[9].identifier.normalize('NFC'),
    qualifiedDrafts[10].identifier.normalize('NFC')
  );
});

void test('actual SDK signed boundary templates preserve exact decimals, Unicode, timestamps and unknown quantity in the reader', () => {
  for (const [index, row] of qualifiedTemplates.entries()) {
    const result = readFoodEvent(
      signed(row.wire_parts.tags, row.created_at, row.wire_parts.content)
    );
    assert.equal(result.outcome, 'admitted', row.id);
    if (result.outcome !== 'admitted') throw new Error(row.id);
    const draft = qualifiedDrafts[index];
    assert.equal(result.projection.identifier, draft.identifier);
    assert.equal(result.projection.title, draft.title);
    assert.equal(result.projection.location, draft.location);
    assert.equal(result.projection.content, row.wire_parts.content);
    assert.equal(result.projection.published_at, draft.published_at);
    assert.equal(result.projection.price.amount, draft.amount);
    assert.equal(result.projection.status, draft.status);
    assert.deepEqual(
      result.projection.quantity,
      draft.quantity === undefined
        ? null
        : { amount: draft.quantity, unit: draft.unit }
    );
  }
});

void test('complete raw marker intersections remain ambiguous before shape checks and incompatible newer heads suppress older food in both arrival orders', () => {
  const base = qualifiedTemplates[0];
  const core = base.wire_parts.tags.filter(
    (row) => !['radroots:price_unit', 'radroots:quantity'].includes(row[0])
  );
  const oldRaw = signed(
    base.wire_parts.tags,
    base.created_at,
    base.wire_parts.content
  );
  assert.equal(readFoodEvent(oldRaw).outcome, 'admitted');
  const old = head(oldRaw);
  let combinations = 0;
  for (const focused of ['radroots:price_unit', 'radroots:quantity'])
    for (const operational of [
      'radroots:primary_bin',
      'radroots:bin',
      'radroots:price'
    ])
      for (const focusedArity of [1, 2, 3])
        for (const operationalArity of [1, 2, 3]) {
          const marker = (name: string, length: number) =>
            [name, 'kg', 'extra'].slice(0, length);
          const raw = signed(
            [
              ...core,
              marker(focused, focusedArity),
              marker(operational, operationalArity)
            ],
            base.created_at + 1,
            base.wire_parts.content
          );
          const read = readFoodEvent(raw);
          assert.equal(read.outcome, 'rejected');
          if (read.outcome !== 'rejected')
            throw new Error('Mixed marker admitted');
          assert.equal(read.error.code, 'food_profile_ambiguous');
          const incompatible = head(raw);
          assert.equal(selectPublicHead(old, incompatible).head, incompatible);
          assert.equal(selectPublicHead(incompatible, old).head, incompatible);
          assert.equal(
            publicHeadSnapshot(incompatible).created_at,
            base.created_at + 1
          );
          combinations++;
        }
  assert.equal(combinations, 54);
});

void test('equal-time incompatible replacements use the lower actual event ID and never resurrect an older food view', () => {
  const base = qualifiedTemplates[0];
  const old = head(
    signed(base.wire_parts.tags, base.created_at, base.wire_parts.content)
  );
  const aRaw = signed(
    [...base.wire_parts.tags, ['radroots:bin']],
    base.created_at + 1,
    'First incompatible head'
  );
  const bRaw = signed(
    [...base.wire_parts.tags, ['radroots:primary_bin']],
    base.created_at + 1,
    'Second incompatible head'
  );
  for (const raw of [aRaw, bRaw])
    assert.notEqual(readFoodEvent(raw).outcome, 'admitted');
  const a = head(aRaw),
    b = head(bRaw);
  const winner = publicHeadSnapshot(a).id < publicHeadSnapshot(b).id ? a : b;
  for (const order of [
    [old, a, b],
    [b, old, a],
    [a, b, old]
  ]) {
    let current = order[0];
    for (const candidate of order.slice(1))
      current = selectPublicHead(current, candidate).head;
    assert.equal(current, winner);
  }
});

void test('maximum-safe signed envelope cannot launder fractional numeric source spelling through JSON rounding', () => {
  const base = qualifiedTemplates[5];
  const raw = signed(
    base.wire_parts.tags,
    base.created_at,
    base.wire_parts.content
  );
  assert.equal(verifyEnvelope(raw).ok, true);
  for (const spelling of [
    '9007199254740991.0',
    '9007199254740991.0000000001',
    '9.007199254740991e15'
  ]) {
    const altered = raw.replace(
      '"created_at":9007199254740991',
      `"created_at":${spelling}`
    );
    assert.notEqual(altered, raw);
    assert.equal(
      (JSON.parse(altered) as { created_at: number }).created_at,
      base.created_at
    );
    assert.equal(verifyEnvelope(altered).ok, false, spelling);
  }
});
