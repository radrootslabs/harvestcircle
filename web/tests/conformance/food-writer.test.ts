import assert from 'node:assert/strict';
import test from 'node:test';
import { webTemplates } from './food-writer-cases.ts';
import { projectFoodParts } from '../../src/lib/contracts/food-availability-v1/read.ts';
for (const row of webTemplates)
  void test(`strict website template source: ${row.id}`, () => {
    const projected = projectFoodParts({
      ...row.wire_parts,
      created_at: row.created_at
    });
    assert.equal(projected.outcome, 'focused');
    if (projected.outcome === 'focused') {
      assert.equal(projected.projection.summary, row.summary);
      assert.deepEqual(projected.projection.images, []);
    }
    assert.deepEqual(
      row.wire_parts.tags.map((tag) => tag[0]),
      [
        'd',
        'title',
        'summary',
        'published_at',
        'location',
        'price',
        'radroots:price_unit',
        ...(row.wire_parts.tags.some((t) => t[0] === 'radroots:quantity')
          ? ['radroots:quantity']
          : []),
        'status'
      ]
    );
  });
