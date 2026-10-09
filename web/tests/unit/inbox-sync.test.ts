import { test, expect } from 'vitest';
import { privateInboxQueries } from '../../src/lib/nostr/inbox-queries.ts';
const owner = 'a'.repeat(64);
test('live filter requests no retained history and admits future backdated outer events', () =>
  expect(privateInboxQueries(owner, 'live')).toEqual([
    { kinds: [1059], '#p': [owner], limit: 0 }
  ]));
test('finite backfill requests the bounded existing 200 without inner time cursor', () =>
  expect(privateInboxQueries(owner, 'backfill')).toEqual([
    { kinds: [1059], '#p': [owner], limit: 200 }
  ]));
test('noncanonical owner cannot construct a private query', () =>
  expect(() => privateInboxQueries('not a key', 'live')).toThrow());
test('unknown mode cannot silently make a live or history request', () =>
  expect(() => privateInboxQueries(owner, 'unreviewed' as 'live')).toThrow());
test('query result mutation cannot alter later private filter scope', () => {
  const a = privateInboxQueries(owner, 'live');
  a[0].kinds!.push(14);
  expect(privateInboxQueries(owner, 'live')).toEqual([
    { kinds: [1059], '#p': [owner], limit: 0 }
  ]);
});
