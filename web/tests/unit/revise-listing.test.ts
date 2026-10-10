import { describe, expect, it } from 'vitest';
import {
  nextOwnedRevisionTime,
  reviewOwnedRevision
} from '../../src/lib/publishing/revise-listing.ts';
import { reviewMarkSold } from '../../src/lib/publishing/mark-sold.ts';
import type { OwnedEditSource } from '../../src/lib/publishing/edit-source.ts';
import type { PublicDraftController } from '../../src/lib/publishing/draft-controller.ts';

describe('HCP119 owned revision clock and original custody', () => {
  it('uses actual forward wall time and advances the same-second head once', () => {
    expect(nextOwnedRevisionTime(100, 200)).toBe(200);
    expect(nextOwnedRevisionTime(100, 100)).toBe(101);
  });
  it('stops before advancing beyond the original future-clock allowance', () => {
    expect(nextOwnedRevisionTime(399, 100)).toBe(400);
    expect(nextOwnedRevisionTime(400, 100)).toBeUndefined();
    expect(nextOwnedRevisionTime(1000, 100)).toBeUndefined();
  });
  it('repeated updates cannot indefinitely beat a frozen/backward clock', () => {
    let head = 100;
    for (let index = 0; index < 300; index++) {
      const next = nextOwnedRevisionTime(head, 100);
      expect(next).toBe(head + 1);
      if (next === undefined) throw Error('clock stopped before its limit');
      head = next;
    }
    expect(head).toBe(400);
    expect(nextOwnedRevisionTime(head, 100)).toBeUndefined();
    expect(nextOwnedRevisionTime(head, 0)).toBeUndefined();
  });
  it.each([NaN, Infinity, -1, Number.MAX_SAFE_INTEGER + 1])(
    'unavailable wall clock %s cannot prepare a revision',
    (now) => expect(nextOwnedRevisionTime(100, now)).toBeUndefined()
  );
  it.each([-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER])(
    'unsupported or overflowing head %s cannot advance',
    (head) => expect(nextOwnedRevisionTime(head, 100)).toBeUndefined()
  );
  it('copied source/controller and boolean review cannot mint revision custody', async () => {
    expect(
      await reviewOwnedRevision(
        {} as OwnedEditSource,
        {} as PublicDraftController,
        '3d030bf5-901d-45e1-8251-41cbdf805e96',
        true
      )
    ).toEqual({ status: 'blocked', reason: 'invalid_scope' });
  });
  it('sold also requires original retained source and explicit reviewed consequence', async () => {
    expect(
      await reviewMarkSold(
        {} as OwnedEditSource,
        {} as PublicDraftController,
        '3d030bf5-901d-45e1-8251-41cbdf805e96',
        true
      )
    ).toEqual({ status: 'blocked', reason: 'invalid_scope' });
  });
});
