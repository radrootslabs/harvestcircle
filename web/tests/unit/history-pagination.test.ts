import { test, expect } from 'vitest';
import {
  initialOuterCursor,
  advanceOuterPage
} from '../../src/lib/messaging/inbox-state.ts';
const source = 'wss://archive.example.org';
const fact = (n: number, time: number) => ({
  outerId: n.toString(16).padStart(64, '0'),
  outerTime: time
});
test('oldest retained signed outer time anchors inclusive boundary, independent of inner ordering', () => {
  const c = initialOuterCursor(source, [fact(1, 100), fact(2, 200)]);
  expect(c.until).toBe(100);
  expect(c.historyComplete).toBe(false);
});
test('full equal-time bucket stays inclusive and reports capped without skipping one second', () => {
  const rows = Array.from({ length: 200 }, (_, i) => fact(i + 1, 100));
  const c = advanceOuterPage(
    initialOuterCursor(source, []),
    rows,
    200,
    'complete'
  );
  expect(c).toMatchObject({
    until: 100,
    state: 'capped',
    historyComplete: false
  });
});
test('repeated full identical page becomes saturated instead of unbounded automatic traversal', () => {
  const rows = Array.from({ length: 200 }, (_, i) => fact(i + 1, 100));
  const c = initialOuterCursor(source, rows);
  expect(advanceOuterPage(c, rows, 200, 'complete')).toMatchObject({
    until: 100,
    state: 'saturated',
    historyComplete: false
  });
});
test('new older retained ID progresses using outer time while preserving boundary IDs', () => {
  const c = advanceOuterPage(
    initialOuterCursor(source, [fact(1, 100)]),
    [fact(1, 100), fact(2, 99)],
    2,
    'complete'
  );
  expect(c).toMatchObject({ until: 99, state: 'older_available' });
  expect(c.seen).toEqual([fact(1, 100).outerId, fact(2, 99).outerId]);
});
test('underfilled repeated EOSE describes a bounded exhausted window never a global complete inbox', () =>
  expect(
    advanceOuterPage(
      initialOuterCursor(source, [fact(1, 100)]),
      [fact(1, 100)],
      1,
      'complete'
    )
  ).toMatchObject({ state: 'window_exhausted', historyComplete: false }));
test('failed source retains confirmed metadata without mutating independent peer state', () => {
  const left = initialOuterCursor(source, [fact(1, 100)]),
    right = initialOuterCursor('wss://peer.example.org', [fact(2, 50)]);
  expect(advanceOuterPage(left, [fact(3, 90)], 1, 'closed')).toMatchObject({
    state: 'partial',
    until: 90
  });
  expect(right.until).toBe(50);
  expect(left.until).toBe(100);
});
test('aggregate cutoff is capped partial evidence and never resets original confirmed IDs', () => {
  const c = advanceOuterPage(
    initialOuterCursor(source, [fact(1, 100)]),
    [],
    500,
    'budget'
  );
  expect(c).toMatchObject({
    state: 'capped',
    partial: true,
    until: 100,
    historyComplete: false
  });
  expect(c.seen).toEqual([fact(1, 100).outerId]);
});
test('invalid negative noninteger or noncanonical outer metadata cannot become a cursor', () => {
  for (const row of [
    { outerId: 'bad', outerTime: 100 },
    fact(1, -1),
    fact(1, 1.5)
  ])
    expect(() => initialOuterCursor(source, [row])).toThrow();
});
test('metadata quota refuses the next distinct ID without evicting unresolved history state', () => {
  const rows = Array.from({ length: 2000 }, (_, i) => fact(i + 1, 100));
  const c = initialOuterCursor(source, rows);
  expect(() => advanceOuterPage(c, [fact(2001, 99)], 1, 'complete')).toThrow();
  expect(c.seen).toHaveLength(2000);
  expect(c.until).toBe(100);
});
