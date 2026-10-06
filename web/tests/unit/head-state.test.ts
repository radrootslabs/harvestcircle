import { afterAll, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  verifyEnvelope,
  type VerifiedEnvelope
} from '../../src/lib/nostr/verified-envelope.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot
} from '../../src/lib/catalog/heads.ts';
import { assessFutureTimestamp } from '../../src/lib/catalog/clock-policy.ts';
import {
  createFoodHeadState,
  advanceFoodHeadState,
  refreshFoodHeadState,
  foodHeadStateSnapshot,
  foodHeadStateHead,
  type FoodHeadState
} from '../../src/lib/catalog/head-state.ts';

const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const base = JSON.parse(
  corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires.previous
) as { tags: string[][]; content: string; created_at: number };
const keys: Uint8Array[] = [];
afterAll(() => {
  for (const key of keys) key.fill(0);
});
function fixture() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const signed = (
    time = base.created_at,
    tags = base.tags,
    content = base.content,
    kind = 30402
  ) =>
    JSON.stringify(
      finalizeEvent({ kind, created_at: time, tags, content }, key)
    );
  const proof = (raw: string) => {
    const result = verifyEnvelope(raw);
    if (!result.ok) throw new Error('Invalid signed test event');
    return result.value;
  };
  const candidate = (raw: string) => {
    const value = createPublicHeadCandidate(proof(raw));
    if (!value) throw new Error('Expected public head');
    return value;
  };
  let now = base.created_at;
  let calls = 0;
  const clock = {
    nowSeconds: () => {
      calls++;
      return now;
    }
  };
  const state = createFoodHeadState(candidate(signed()), clock);
  return {
    signed,
    proof,
    candidate,
    state,
    clock,
    setNow: (v: number) => {
      now = v;
    },
    calls: () => calls
  };
}
function status(value: string) {
  return base.tags.map((t) => (t[0] === 'status' ? ['status', value] : [...t]));
}
describe('verified food head display uncertainty', () => {
  it('reserves a new winner before its clock callback and never overwrites a still newer accepted head', () => {
    const f = fixture();
    const newer = f.proof(f.signed(base.created_at + 2, status('unknown')));
    let armed = false;
    let entered = false;
    const clock = {
      nowSeconds: () => {
        if (armed && !entered) {
          entered = true;
          advanceFoodHeadState(state, newer);
        }
        return base.created_at;
      }
    };
    const state: FoodHeadState = createFoodHeadState(
      foodHeadStateHead(f.state),
      clock
    );
    armed = true;
    advanceFoodHeadState(state, f.proof(f.signed(base.created_at + 1)));
    expect(foodHeadStateSnapshot(state)).toMatchObject({
      display: 'unsupported',
      head: { created_at: base.created_at + 2 }
    });
    expect(foodHeadStateSnapshot(state).food).toBeUndefined();
    expect(publicHeadSnapshot(foodHeadStateHead(state)).created_at).toBe(
      base.created_at + 2
    );
  });
  it('cannot publish an older focused assessment after clock callback accepts a newer unsupported winner', () => {
    const f = fixture();
    const newer = f.proof(f.signed(base.created_at + 2, status('unknown')));
    let armed = false;
    let entered = false;
    const clock = {
      nowSeconds: () => {
        if (armed && !entered) {
          entered = true;
          expect(advanceFoodHeadState(state, newer)).toBe('applied');
        }
        return base.created_at;
      }
    };
    const state: FoodHeadState = createFoodHeadState(
      foodHeadStateHead(f.state),
      clock
    );
    armed = true;
    refreshFoodHeadState(state);
    expect(foodHeadStateSnapshot(state)).toMatchObject({
      display: 'unsupported',
      head: { created_at: base.created_at + 2 }
    });
    expect(foodHeadStateSnapshot(state).food).toBeUndefined();
    expect(publicHeadSnapshot(foodHeadStateHead(state)).created_at).toBe(
      base.created_at + 2
    );
  });
  it('projects the focused known head without claiming assured stock', () => {
    const f = fixture();
    const view = foodHeadStateSnapshot(f.state);
    expect(view.display).toBe('focused_known');
    expect(view.food?.status).toBe('active');
    expect(view.clockDirection).toBe('initial');
    expect(view.head).toEqual(publicHeadSnapshot(foodHeadStateHead(f.state)));
  });
  it('keeps a newer unsupported status and never returns the older active projection', () => {
    const f = fixture();
    const raw = f.signed(base.created_at + 1, status('unknown'));
    expect(advanceFoodHeadState(f.state, f.proof(raw))).toBe('applied');
    const view = foodHeadStateSnapshot(f.state);
    expect(view.display).toBe('unsupported');
    expect(view.food).toBeUndefined();
    expect(view.head.id).toBe((JSON.parse(raw) as { id: string }).id);
    expect(view.unsupportedReason).toBe('food_status_invalid');
  });
  it('blocks generic operational and ambiguous newer heads before focused filtering', () => {
    for (const tags of [
      base.tags.filter((t) => !t[0].startsWith('radroots:')),
      [
        ['d', base.tags.find((t) => t[0] === 'd')![1]],
        ['radroots:primary_bin', 'stock']
      ],
      [...base.tags, ['radroots:primary_bin', 'stock']]
    ]) {
      const f = fixture();
      advanceFoodHeadState(
        f.state,
        f.proof(f.signed(base.created_at + 1, tags))
      );
      expect(foodHeadStateSnapshot(f.state).classification).toBe('unsupported');
      expect(foodHeadStateSnapshot(f.state).food).toBeUndefined();
    }
  });
  it('cannot let a bad signature or forged proof suppress good evidence', () => {
    const f = fixture();
    const before = foodHeadStateSnapshot(f.state);
    const raw = JSON.parse(
      f.signed(base.created_at + 1, status('unknown'))
    ) as { sig: string };
    raw.sig = '0'.repeat(128);
    expect(verifyEnvelope(JSON.stringify(raw)).ok).toBe(false);
    expect(
      advanceFoodHeadState(f.state, raw as unknown as VerifiedEnvelope)
    ).toBe('ignored');
    expect(foodHeadStateSnapshot(f.state)).toEqual(before);
  });
  it('does not resurrect an older focused head arriving after unsupported evidence', () => {
    const f = fixture();
    advanceFoodHeadState(
      f.state,
      f.proof(f.signed(base.created_at + 1, status('unknown')))
    );
    const before = foodHeadStateSnapshot(f.state);
    expect(advanceFoodHeadState(f.state, f.proof(f.signed()))).toBe('older');
    expect(foodHeadStateSnapshot(f.state)).toEqual(before);
  });
  it('uses strict greater-than-five-minute quarantine with fractional wall clock support', () => {
    expect(assessFutureTimestamp(1300, 1000)).toBe('within_policy');
    expect(assessFutureTimestamp(1301, 1000)).toBe('future_quarantined');
    expect(assessFutureTimestamp(1301, 1000.5)).toBe('future_quarantined');
    expect(assessFutureTimestamp(Number.MAX_SAFE_INTEGER, 0)).toBe(
      'future_quarantined'
    );
  });
  it('quarantines the protocol winner without changing its signed timestamp', () => {
    const f = fixture();
    const raw = f.signed(base.created_at + 301);
    advanceFoodHeadState(f.state, f.proof(raw));
    const view = foodHeadStateSnapshot(f.state);
    expect(view.display).toBe('future_quarantined');
    expect(view.classification).toBe('focused');
    expect(view.food).toBeUndefined();
    expect(view.head.created_at).toBe(base.created_at + 301);
  });
  it('explicit refresh exposes forward and backward clock transitions without changing winner', () => {
    const f = fixture();
    advanceFoodHeadState(f.state, f.proof(f.signed(base.created_at + 301)));
    const id = foodHeadStateSnapshot(f.state).head.id;
    f.setNow(base.created_at + 1);
    refreshFoodHeadState(f.state);
    expect(foodHeadStateSnapshot(f.state)).toMatchObject({
      display: 'focused_known',
      clockDirection: 'forward'
    });
    f.setNow(base.created_at);
    refreshFoodHeadState(f.state);
    expect(foodHeadStateSnapshot(f.state)).toMatchObject({
      display: 'future_quarantined',
      clockDirection: 'backward'
    });
    expect(foodHeadStateSnapshot(f.state).head.id).toBe(id);
  });
  it('cannot restore an older current view while the winner is quarantined', () => {
    const f = fixture();
    advanceFoodHeadState(f.state, f.proof(f.signed(base.created_at + 301)));
    expect(advanceFoodHeadState(f.state, f.proof(f.signed()))).toBe('older');
    expect(foodHeadStateSnapshot(f.state).display).toBe('future_quarantined');
    expect(foodHeadStateSnapshot(f.state).food).toBeUndefined();
  });
  it('retains incompatible evidence while future and still denies food after clock catches up', () => {
    const f = fixture();
    advanceFoodHeadState(
      f.state,
      f.proof(f.signed(base.created_at + 301, status('unknown')))
    );
    expect(foodHeadStateSnapshot(f.state)).toMatchObject({
      display: 'future_quarantined',
      classification: 'unsupported'
    });
    f.setNow(base.created_at + 1);
    refreshFoodHeadState(f.state);
    expect(foodHeadStateSnapshot(f.state).display).toBe('unsupported');
    expect(foodHeadStateSnapshot(f.state).food).toBeUndefined();
  });
  it('unavailable clock preserves the new winner and fails closed for display', () => {
    const f = fixture();
    f.setNow(NaN);
    advanceFoodHeadState(f.state, f.proof(f.signed(base.created_at + 1)));
    expect(foodHeadStateSnapshot(f.state)).toMatchObject({
      display: 'clock_unavailable',
      clockDirection: 'unavailable',
      assessedAtSeconds: null
    });
    expect(foodHeadStateSnapshot(f.state).head.created_at).toBe(
      base.created_at + 1
    );
    expect(foodHeadStateSnapshot(f.state).food).toBeUndefined();
    f.setNow(base.created_at + 1);
    refreshFoodHeadState(f.state);
    expect(foodHeadStateSnapshot(f.state).display).toBe('focused_known');
  });
  it('clock throwing or invalid numeric samples never manufacture a display time', () => {
    const f = fixture();
    const state = createFoodHeadState(foodHeadStateHead(f.state), {
      nowSeconds: () => {
        throw new Error('clock failure');
      }
    });
    expect(foodHeadStateSnapshot(state).display).toBe('clock_unavailable');
    for (const now of [NaN, Infinity, -1, Number.MAX_SAFE_INTEGER + 1])
      expect(assessFutureTimestamp(base.created_at, now)).toBe(
        'clock_unavailable'
      );
  });
  it('duplicates and other coordinates do not sample clock or replace state', () => {
    const f = fixture();
    const before = foodHeadStateSnapshot(f.state),
      calls = f.calls();
    expect(advanceFoodHeadState(f.state, f.proof(f.signed()))).toBe(
      'duplicate'
    );
    const tags = base.tags.map((t) => (t[0] === 'd' ? ['d', 'other'] : t));
    expect(
      advanceFoodHeadState(
        f.state,
        f.proof(f.signed(base.created_at + 1, tags))
      )
    ).toBe('coordinate_mismatch');
    expect(f.calls()).toBe(calls);
    expect(foodHeadStateSnapshot(f.state)).toEqual(before);
  });
  it('protects detached projection and head metadata from mutation and rejects forged owners', () => {
    const f = fixture();
    const view = foodHeadStateSnapshot(f.state);
    if (!view.food) throw new Error('Expected focused food');
    (view.food as { title: string }).title = 'mutated';
    (view.head as { created_at: number }).created_at = 0;
    expect(foodHeadStateSnapshot(f.state).food?.title).not.toBe('mutated');
    expect(foodHeadStateSnapshot(f.state).head.created_at).toBe(
      base.created_at
    );
    expect(() => foodHeadStateSnapshot({} as FoodHeadState)).toThrow(
      'food_head_state_invalid'
    );
  });
  it('does not treat public metadata coordinates as food state', () => {
    const f = fixture();
    for (const kind of [0, 10050])
      expect(() =>
        createFoodHeadState(
          f.candidate(f.signed(base.created_at, [], '{}', kind)),
          f.clock
        )
      ).toThrow('food_head_kind_invalid');
  });
});
