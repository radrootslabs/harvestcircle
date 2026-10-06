import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { buildFoodTemplate } from '../../src/lib/contracts/food-availability-v1/write.ts';
const fixture = JSON.parse(
  await readFile(
    new URL(
      '../../../contracts/interop/food_availability/numeric_boundaries.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as {
  vectors: {
    id: string;
    kind: 'timestamp' | 'identifier';
    value: string;
    expected: { typescript: Record<string, unknown> };
  }[];
};
const base = {
  identifier: 'boundary-id',
  title: 'Fresh food',
  description: 'Fresh food available locally.',
  location: 'Victoria',
  amount: '1',
  currency: 'CAD',
  unit: 'g',
  status: 'active' as const,
  published_at: 1,
  created_at: 1
};
assert.equal(fixture.vectors.length, 10);
for (const row of fixture.vectors)
  void test(`actual TS Food boundary ${row.id}`, () => {
    if (row.kind === 'identifier') {
      const result = buildFoodTemplate({ ...base, identifier: row.value });
      assert.ok(result.ok);
      assert.deepEqual(
        { result: 'accepted', identifier: result.wire_parts.tags[0][1] },
        row.expected.typescript
      );
      return;
    }
    assert.match(row.value, /^(0|[1-9][0-9]*)$/u);
    const value = BigInt(row.value);
    const result = buildFoodTemplate({
      ...base,
      published_at: Number(value),
      created_at: Number(value)
    });
    if (value > BigInt(Number.MAX_SAFE_INTEGER)) {
      assert.equal(result.ok, false);
      if (!result.ok) assert.equal(result.error.code, 'timestamp_invalid');
      assert.deepEqual(row.expected.typescript, {
        result: 'unsupported',
        reason:
          'Canonical timestamp exceeds Number.MAX_SAFE_INTEGER; current JavaScript adapter must reject it without rounding.'
      });
    } else if (!result.ok) {
      assert.deepEqual(
        { result: 'rejected', code: result.error.code },
        row.expected.typescript
      );
    } else {
      assert.deepEqual(
        {
          result: 'accepted',
          timestamp: String(Number(value)),
          encoded_published_at: result.wire_parts.tags.find(
            (tag) => tag[0] === 'published_at'
          )?.[1]
        },
        row.expected.typescript
      );
    }
  });
void test('canonical raw identifier spellings remain distinct in actual unsigned outputs', () => {
  const results = fixture.vectors
    .filter((row) => row.kind === 'identifier')
    .map((row) => buildFoodTemplate({ ...base, identifier: row.value }));
  assert.equal(results.length, 2);
  assert.ok(results[0].ok && results[1].ok);
  assert.notEqual(
    results[0].wire_parts.tags[0][1],
    results[1].wire_parts.tags[0][1]
  );
});
