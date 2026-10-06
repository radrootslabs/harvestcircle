import { describe, expect, it } from 'vitest';
import {
  chronologicalSearchQuery,
  requireChronologicalSearchQuery
} from '../../src/lib/nostr/chronological-query.ts';

describe('bounded chronological queries', () => {
  it('requests a detached kind30402 window of200 without keyword or tag constraints', () => {
    expect(chronologicalSearchQuery()).toEqual([
      { kinds: [30402], limit: 200 }
    ]);
    expect(chronologicalSearchQuery(123)).toEqual([
      { kinds: [30402], limit: 200, until: 123 }
    ]);
    const one = chronologicalSearchQuery();
    one[0].kinds![0] = 5;
    expect(chronologicalSearchQuery()[0].kinds).toEqual([30402]);
  });
  it('preserves zero/inclusive safe integer boundaries and refuses unsupported time', () => {
    expect(chronologicalSearchQuery(0)[0].until).toBe(0);
    for (const value of [-1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])
      expect(() => chronologicalSearchQuery(value)).toThrow(
        'chronological_boundary_invalid'
      );
  });
  it('refuses externally supplied skip cursors and extra keyword constraints', () => {
    expect(
      requireChronologicalSearchQuery(
        [{ limit: 200, kinds: [30402], until: 123 }],
        123
      )
    ).toEqual(chronologicalSearchQuery(123));
    for (const value of [
      [],
      [{ kinds: [30402], limit: 201 }],
      [{ kinds: [5], limit: 200 }],
      [{ kinds: [30402], limit: 200, search: 'carrots' }],
      [{ kinds: [30402], limit: 200, since: 1 }],
      [{ kinds: [30402], limit: 200, until: 122 }]
    ])
      expect(() => requireChronologicalSearchQuery(value, 123)).toThrow(
        'food_search_chronological_filter'
      );
  });
});
