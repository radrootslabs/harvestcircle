import { describe, expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import { publisherProfileQuery } from '../../src/lib/nostr/profile-query.ts';
function author() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    return finalizeEvent(
      { kind: 0, created_at: 1, tags: [], content: '{}' },
      key
    ).pubkey;
  } finally {
    key.fill(0);
  }
}
describe('bounded publisher profile query', () => {
  it('coalesces twenty page authors into one bounded generic metadata filter', () => {
    const authors = Array.from({ length: 20 }, () => author());
    expect(publisherProfileQuery(authors)).toEqual([
      { kinds: [0], authors, limit: 200 }
    ]);
    expect(publisherProfileQuery([authors[0], authors[0]])).toEqual([
      { kinds: [0], authors: [authors[0]], limit: 200 }
    ]);
    const query = publisherProfileQuery(authors);
    query[0].authors![0] = authors[1];
    expect(publisherProfileQuery(authors)[0].authors![0]).toBe(authors[0]);
    expect(publisherProfileQuery([])).toEqual([]);
  });
  it('rejects oversized or malformed admission before request effects', () => {
    const key = author();
    for (const values of [
      Array.from({ length: 21 }, () => key),
      ['wrong'],
      [key.toUpperCase()],
      ['0'.repeat(64)]
    ])
      expect(() => publisherProfileQuery(values)).toThrow();
  });
});
