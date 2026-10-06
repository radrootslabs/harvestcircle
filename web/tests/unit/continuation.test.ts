import { afterEach, describe, expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot
} from '../../src/lib/catalog/heads.ts';
import {
  createChronologicalContinuation,
  localSearchPage
} from '../../src/lib/catalog/continuation.ts';
const keys: Uint8Array[] = [];
afterEach(() => {
  for (const key of keys.splice(0)) key.fill(0);
});
function head(time: number, name: string, kind = 30402) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const proof = verifyEnvelope(
    JSON.stringify(
      finalizeEvent(
        { kind, created_at: time, tags: [['d', name]], content: '' },
        key
      )
    )
  );
  if (!proof.ok) throw Error('fixture verification');
  const candidate = createPublicHeadCandidate(proof.value);
  if (!candidate) throw Error('fixture head');
  return candidate;
}
describe('inclusive chronological continuation and local pages', () => {
  it('does not cross a saturated source bucket because another source returned an older item', () => {
    const owner = createChronologicalContinuation(),
      a = head(100, 'a'),
      b = head(99, 'b');
    const first = owner.begin();
    first.observe(a);
    first.observe(b);
    first.finish(true, 'bounded-eose', [
      { source: 'one', saturated: true, eventIds: [publicHeadSnapshot(a).id] },
      { source: 'two', saturated: false, eventIds: [publicHeadSnapshot(b).id] }
    ]);
    expect(owner.snapshot().until).toBe(100);
    const next = owner.begin();
    next.observe(a);
    next.observe(b);
    next.finish(true, 'bounded-eose', [
      { source: 'one', saturated: true, eventIds: [publicHeadSnapshot(a).id] },
      { source: 'two', saturated: false, eventIds: [publicHeadSnapshot(b).id] }
    ]);
    expect(owner.snapshot()).toMatchObject({
      until: 100,
      gap: true,
      coverage: 'partial'
    });
  });

  it('repeats the observed boundary and stops a repeated saturated equal-time bucket with an explicit gap', () => {
    const owner = createChronologicalContinuation(),
      item = head(100, 'a');
    const first = owner.begin();
    first.observe(item);
    first.observe(item);
    first.finish(true, 'bounded-eose');
    expect(owner.snapshot()).toMatchObject({
      until: 100,
      windows: 1,
      observed: 1,
      gap: false,
      definitiveAbsence: false
    });
    const next = owner.begin();
    expect(next.until).toBe(100);
    next.observe(item);
    next.finish(true, 'bounded-eose');
    expect(owner.snapshot()).toMatchObject({
      until: 100,
      gap: true,
      coverage: 'partial'
    });
    expect(() => owner.restart()).toThrow('food_search_chronological_gap');
  });
  it('accepts new boundary IDs as progress and advances only to an actually observed older timestamp', () => {
    const owner = createChronologicalContinuation(),
      a = head(100, 'a'),
      b = head(100, 'b'),
      c = head(99, 'c');
    const first = owner.begin();
    first.observe(a);
    first.finish(true, 'bounded-eose');
    const next = owner.begin();
    next.observe(a);
    next.observe(b);
    next.finish(true, 'bounded-eose');
    expect(owner.snapshot()).toMatchObject({
      until: 100,
      gap: false,
      windows: 2
    });
    expect(() => owner.begin()).toThrow(
      'food_search_chronological_window_limit'
    );
    const older = owner.restart(),
      window = older.begin();
    expect(window.until).toBe(100);
    window.observe(c);
    window.finish(false, 'bounded-eose');
    expect(older.snapshot()).toMatchObject({
      until: 99,
      windows: 1,
      gap: false
    });
  });
  it('ignores wrong-kind/out-of-bound/forged candidates for cursor purposes and preserves partial coverage', () => {
    const owner = createChronologicalContinuation(),
      first = owner.begin();
    first.observe(head(10, 'a'));
    first.finish(false, 'partial');
    const next = owner.begin();
    next.observe(head(11, 'future'));
    next.observe(head(0, 'profile', 0));
    expect(() => next.observe({} as ReturnType<typeof head>)).toThrow(
      'public_head_invalid'
    );
    next.finish(true, 'bounded-eose');
    expect(owner.snapshot()).toMatchObject({
      until: 10,
      gap: true,
      coverage: 'partial'
    });
  });
  it('does not invent an older boundary for an empty window, and refuses parallel windows', () => {
    const owner = createChronologicalContinuation(),
      first = owner.begin();
    expect(() => owner.begin()).toThrow('food_search_chronological_pending');
    first.finish(false, 'bounded-eose');
    expect(owner.snapshot().until).toBeUndefined();
    expect(() => owner.restart()).toThrow(
      'food_search_chronological_boundary_missing'
    );
    expect(() => first.observe(head(0, 'late'))).toThrow(
      'food_search_chronological_window_closed'
    );
  });
  it('restarts with known boundary evidence without mutating the predecessor or erasing partial coverage', () => {
    const owner = createChronologicalContinuation(),
      item = head(0, 'zero'),
      first = owner.begin();
    first.observe(item);
    first.finish(true, 'partial');
    const next = owner.restart(),
      window = next.begin();
    window.observe(item);
    window.finish(true, 'bounded-eose');
    expect(next.snapshot()).toMatchObject({
      until: 0,
      gap: true,
      coverage: 'partial'
    });
    expect(owner.snapshot()).toMatchObject({ windows: 1, gap: false });
    const copy = owner.snapshot();
    Object.assign(copy, { until: 100 });
    expect(owner.snapshot().until).toBe(0);
  });
  it('shows20 more local rows per page without changing order or creating network work', () => {
    const rows = Array.from({ length: 45 }, (_, n) => n);
    expect(localSearchPage(rows, 1)).toEqual({
      rows: rows.slice(0, 20),
      hasMore: true
    });
    expect(localSearchPage(rows, 2)).toEqual({
      rows: rows.slice(0, 40),
      hasMore: true
    });
    expect(localSearchPage(rows, 3)).toEqual({ rows, hasMore: false });
    for (const value of [0, -1, 1.5, NaN, 101])
      expect(() => localSearchPage(rows, value)).toThrow(
        'food_search_local_page_invalid'
      );
    expect(rows).toHaveLength(45);
  });
});
