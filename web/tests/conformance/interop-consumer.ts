import assert from 'node:assert/strict';
import { readFoodEvent } from '../../src/lib/contracts/food-availability-v1/read.ts';
import {
  buildFoodTemplate,
  type FoodDraft
} from '../../src/lib/contracts/food-availability-v1/write.ts';
import { webTemplates } from './food-writer-cases.ts';
import {
  validateInteropManifest,
  type InteropInputs,
  buildInteropManifest
} from './interop-manifest.ts';

type Manifest = ReturnType<typeof buildInteropManifest>;
type Recipe = {
  id: string;
  expected: Record<string, unknown>;
  signed_wires: Record<string, string>;
};
type Boundary = {
  id: string;
  kind: 'timestamp' | 'identifier';
  value: string;
  expected: { typescript: Record<string, unknown> };
};
const base: FoodDraft = {
  identifier: 'boundary-id',
  title: 'Fresh food',
  description: 'Fresh food available locally.',
  location: 'Victoria',
  amount: '1',
  currency: 'CAD',
  unit: 'g',
  status: 'active',
  published_at: 1,
  created_at: 1
};

function reader(
  raw: string,
  expected: Record<string, unknown>,
  read: typeof readFoodEvent
) {
  const actual = read(raw);
  if ('error' in expected) {
    assert.equal(actual.outcome, 'rejected');
    assert.ok(actual.outcome === 'rejected');
    assert.deepEqual(actual.error, expected.error);
    return { outcome: actual.outcome, error: actual.error };
  }
  assert.ok(actual.outcome !== 'rejected');
  assert.equal(actual.event_id, expected.event_id);
  if ('partition' in expected) {
    assert.ok(actual.outcome === 'excluded');
    assert.deepEqual(actual.partition, expected.partition);
    return {
      outcome: actual.outcome,
      event_id: actual.event_id,
      partition: actual.partition
    };
  }
  assert.ok(actual.outcome === 'admitted');
  if ('projection' in expected) {
    assert.deepEqual(actual.projection, expected.projection);
    return {
      outcome: actual.outcome,
      event_id: actual.event_id,
      projection: actual.projection
    };
  }
  assert.ok('image_count' in expected);
  const observed = {
    event_id: actual.event_id,
    image_count: actual.projection.images.length,
    first_raw_tag: actual.projection.images[0]?.raw_tag,
    last_raw_tag: actual.projection.images.at(-1)?.raw_tag,
    diagnostics: actual.projection.diagnostics
  };
  for (const [key, value] of Object.entries(observed))
    assert.deepEqual(value, expected[key], key);
  return { outcome: actual.outcome, ...observed };
}

export function consumeInteropTypescript(
  manifest: unknown,
  inputs: InteropInputs,
  options: {
    profile?: string;
    read?: typeof readFoodEvent;
    build?: typeof buildFoodTemplate;
    writers?: typeof webTemplates;
  } = {}
) {
  const writers = options.writers ?? webTemplates;
  validateInteropManifest(manifest, inputs, writers);
  const checked = manifest as Manifest;
  const decode = (path: string): unknown =>
    JSON.parse(
      new TextDecoder('utf-8', { fatal: true }).decode(inputs.get(path))
    );
  const corpus = decode(
    'contracts/interop/food_availability/corpus.v1.json'
  ) as { vectors: Recipe[] };
  const boundaries = decode(
    'contracts/interop/food_availability/numeric_boundaries.v1.json'
  ) as { vectors: Boundary[] };
  const cases: {
    id: string;
    result: 'PASS' | 'UNSUPPORTED';
    actual?: unknown;
    reason?: string;
  }[] = [];
  for (const row of checked.cases) {
    if (
      row.protocol.profile !== (options.profile ?? 'food_availability_profile')
    )
      continue;
    const participation = row.participation.typescript;
    if (participation === 'UNSUPPORTED_CURRENT_ADAPTER') {
      assert.ok(row.participation.unsupported_reason);
      cases.push({
        id: row.id,
        result: 'UNSUPPORTED',
        reason: row.participation.unsupported_reason
      });
    } else if (participation === 'food_reader') {
      const recipe = corpus.vectors[row.fixture.case_index];
      assert.equal(recipe.id, row.id);
      cases.push({
        id: row.id,
        result: 'PASS',
        actual: reader(
          recipe.signed_wires.event,
          recipe.expected,
          options.read ?? readFoodEvent
        )
      });
    } else if (participation === 'food_writer') {
      const actual = writers[row.fixture.case_index];
      assert.equal(actual.id, row.id);
      assert.deepEqual(
        { result: 'accepted', wire_parts: actual.wire_parts },
        row.expected
      );
      cases.push({
        id: row.id,
        result: 'PASS',
        actual: { wire_parts: actual.wire_parts, summary: actual.summary }
      });
    } else {
      const recipe = boundaries.vectors[row.fixture.case_index];
      assert.equal(recipe.id, row.id);
      const build = options.build ?? buildFoodTemplate;
      const value = recipe.kind === 'timestamp' ? BigInt(recipe.value) : null;
      const result = build(
        recipe.kind === 'identifier'
          ? { ...base, identifier: recipe.value }
          : { ...base, published_at: Number(value), created_at: Number(value) }
      );
      if (participation === 'UNSUPPORTED_JAVASCRIPT_U64') {
        assert.ok(value !== null && value > BigInt(Number.MAX_SAFE_INTEGER));
        assert.equal(result.ok, false);
        assert.ok(!result.ok);
        assert.equal(result.error.code, 'timestamp_invalid');
        assert.ok(row.participation.unsupported_reason);
        cases.push({
          id: row.id,
          result: 'UNSUPPORTED',
          reason: row.participation.unsupported_reason,
          actual: { rejected_code: result.error.code }
        });
      } else {
        assert.equal(participation, 'food_boundary');
        const actual = !result.ok
          ? { result: 'rejected', code: result.error.code }
          : recipe.kind === 'identifier'
            ? { result: 'accepted', identifier: result.wire_parts.tags[0][1] }
            : {
                result: 'accepted',
                timestamp: String(Number(value)),
                encoded_published_at: result.wire_parts.tags.find(
                  (tag) => tag[0] === 'published_at'
                )?.[1]
              };
        assert.deepEqual(actual, recipe.expected.typescript);
        cases.push({ id: row.id, result: 'PASS', actual });
      }
    }
  }
  assert.ok(cases.length > 0, 'zero matching TypeScript manifest cases');
  assert.equal(cases.length, 66);
  const executed = cases.filter((row) => row.result === 'PASS').length;
  assert.equal(executed, 40);
  assert.equal(cases.length - executed, 26);
  return {
    consumer: 'actual_typescript_food_adapters',
    matched_cases: cases.length,
    executed_cases: executed,
    unsupported_cases: cases.length - executed,
    revision: checked.oracle.revision,
    qualification: checked.qualification,
    signing: checked.signing,
    cases
  };
}
