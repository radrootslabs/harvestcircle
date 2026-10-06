import { afterAll, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent, verifiedSymbol } from 'applesauce-core/helpers';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../../src/lib/nostr/verified-envelope.ts';
import { readFoodEvent } from '../../src/lib/contracts/food-availability-v1/read.ts';
import {
  createPublicHeadCandidate,
  selectPublicHead,
  publicHeadSnapshot,
  publicHeadKey,
  publicHeadEnvelope,
  type PublicHead
} from '../../src/lib/catalog/heads.ts';

const keys: Uint8Array[] = [];
afterAll(() => {
  for (const key of keys) key.fill(0);
});
// Ephemeral test-only signing; only verified public wires enter the selector.
function author() {
  const secret = crypto.getRandomValues(new Uint8Array(32));
  keys.push(secret);
  return (
    created_at: number,
    tags: string[][] = [['d', 'stock']],
    content = 'Same title',
    kind = 30402
  ) =>
    JSON.stringify(finalizeEvent({ created_at, tags, content, kind }, secret));
}
function proof(raw: string): VerifiedEnvelope {
  const result = verifyEnvelope(raw);
  if (!result.ok) throw new Error('Invalid signed test event');
  return result.value;
}
function head(raw: string): PublicHead {
  const result = createPublicHeadCandidate(proof(raw));
  if (!result) throw new Error('Expected public head candidate');
  return result;
}
function fold(raws: string[]) {
  const known = new Map<string, PublicHead>();
  for (const raw of raws) {
    const candidate = head(raw),
      key = publicHeadKey(candidate);
    known.set(key, selectPublicHead(known.get(key), candidate).head);
  }
  return [...known.values()].map(publicHeadSnapshot);
}
describe('signature-verified public coordinate heads', () => {
  it('owns frozen opaque candidates with exact original proof', () => {
    const signed = author(),
      value = proof(signed(10));
    const candidate = createPublicHeadCandidate(value)!;
    expect(Object.keys(candidate)).toEqual([]);
    expect(Object.isFrozen(candidate)).toBe(true);
    expect(publicHeadEnvelope(candidate)).toBe(value);
    expect(publicHeadSnapshot(candidate).id).toBe(
      verifiedEnvelopeSnapshot(value)!.id
    );
    expect(selectPublicHead(undefined, candidate)).toEqual({
      decision: 'applied',
      head: candidate
    });
  });
  it('selects the newest timestamp regardless of arrival order', () => {
    const signed = author(),
      old = signed(10),
      newest = signed(20);
    const expected = publicHeadSnapshot(head(newest));
    expect(fold([old, newest])).toEqual([expected]);
    expect(fold([newest, old])).toEqual([expected]);
  });
  it('uses the lower canonical event ID for equal timestamps in both orders', () => {
    const signed = author(),
      a = head(signed(10, [['d', 'stock']], 'a')),
      b = head(signed(10, [['d', 'stock']], 'b'));
    const [winner, loser] =
      publicHeadSnapshot(a).id < publicHeadSnapshot(b).id ? [a, b] : [b, a];
    expect(selectPublicHead(loser, winner)).toEqual({
      decision: 'applied',
      head: winner
    });
    expect(selectPublicHead(winner, loser)).toEqual({
      decision: 'higher_id',
      head: winner
    });
  });
  it('retains one head across fresh duplicate observations', () => {
    const signed = author(),
      raw = signed(10),
      a = head(raw),
      b = head(raw);
    expect(a).not.toBe(b);
    expect(selectPublicHead(a, b)).toEqual({ decision: 'duplicate', head: a });
    expect(fold([raw, raw, raw])).toHaveLength(1);
  });
  it('keeps distinct authors and identifiers despite identical titles', () => {
    const a = author(),
      b = author();
    const raws = [a(10), b(10), a(10, [['d', 'other']])];
    expect(fold(raws)).toHaveLength(3);
    const current = head(raws[0]),
      candidate = head(raws[1]);
    expect(selectPublicHead(current, candidate)).toEqual({
      decision: 'coordinate_mismatch',
      head: current
    });
    expect(publicHeadKey(current)).not.toBe(publicHeadKey(candidate));
  });
  it('preserves raw Unicode, case, whitespace and colon identity', () => {
    const signed = author();
    const ids = [
      'Stock',
      'stock',
      ' stock ',
      'a:b',
      '\ufeffstock',
      'é',
      'e\u0301'
    ];
    const selected = fold(
      ids.map((identifier) => signed(10, [['d', identifier]]))
    );
    expect(selected.map((value) => value.identifier)).toEqual(ids);
    expect(new Set(selected.map((value) => value.identifier)).size).toBe(
      ids.length
    );
  });
  it('uses the first d even when later d tags disagree', () => {
    const signed = author();
    expect(
      publicHeadSnapshot(
        head(
          signed(10, [
            ['d', 'first'],
            ['d', 'second']
          ])
        )
      ).identifier
    ).toBe('first');
  });
  it('uses the empty address for missing empty or malformed first d values', () => {
    const signed = author();
    const cases = [[], [['d', '']], [['d']], [['d'], ['d', 'later']]];
    const candidates = cases.map((tags, index) =>
      head(signed(10 + index, tags))
    );
    expect(new Set(candidates.map(publicHeadKey)).size).toBe(1);
    for (const candidate of candidates)
      expect(publicHeadSnapshot(candidate).identifier).toBe('');
  });
  it('does not apply focused identifier limits before generic head selection', () => {
    const signed = author(),
      raw = signed(20, [['d', 'x'.repeat(300)]]);
    expect(publicHeadSnapshot(head(raw)).identifier).toBe('x'.repeat(300));
    expect(readFoodEvent(raw).outcome).not.toBe('admitted');
  });
  it('supports approved replaceable public metadata without d identity', () => {
    const signed = author();
    for (const kind of [0, 10050]) {
      const a = head(signed(10, [['d', 'first']], '{}', kind));
      const b = head(signed(20, [['d', 'second']], '{}', kind));
      expect(publicHeadSnapshot(a).identifier).toBeNull();
      expect(publicHeadKey(a)).toBe(publicHeadKey(b));
      expect(selectPublicHead(a, b).head).toBe(b);
    }
    expect(
      fold([signed(10, [], '{}', 0), signed(10, [], '', 10050), signed(10)])
    ).toHaveLength(3);
  });
  it('excludes regular deletion, unapproved public, private and connection events', () => {
    const signed = author();
    for (const kind of [5, 1, 1059, 22242, 30023])
      expect(
        createPublicHeadCandidate(proof(signed(10, [], '', kind)))
      ).toBeUndefined();
  });
  it('selects a verified incompatible newer product before focused filtering', () => {
    const corpus = JSON.parse(
      readFileSync(
        new URL(
          '../../../contracts/interop/food_availability/corpus.v1.json',
          import.meta.url
        ),
        'utf8'
      )
    ) as {
      vectors: { id: string; signed_wires: Record<string, string> }[];
    };
    const wires = corpus.vectors.find((value) =>
      value.id.endsWith('_032')
    )!.signed_wires;
    const base = JSON.parse(wires.previous) as {
      created_at: number;
      tags: string[][];
      content: string;
    };
    const signed = author();
    const oldRaw = signed(base.created_at, base.tags, base.content);
    const nextRaw = signed(
      base.created_at + 1,
      [...base.tags, ['radroots:primary_bin', 'storage']],
      base.content
    );
    const old = head(oldRaw),
      newest = head(nextRaw);
    expect(selectPublicHead(old, newest).head).toBe(newest);
    expect(readFoodEvent(oldRaw).outcome).toBe('admitted');
    expect(readFoodEvent(nextRaw).outcome).not.toBe('admitted');
  });
  it('rejects forged verification flags and invalid signatures without replacing a good head', () => {
    const signed = author(),
      raw = signed(10),
      current = head(raw);
    const invalid = JSON.parse(raw) as Record<string | symbol, unknown>;
    invalid.sig = '0'.repeat(128);
    invalid[verifiedSymbol] = true;
    expect(verifyEnvelope(JSON.stringify(invalid)).ok).toBe(false);
    expect(
      createPublicHeadCandidate(invalid as unknown as VerifiedEnvelope)
    ).toBeUndefined();
    expect(
      createPublicHeadCandidate(Object.freeze({}) as VerifiedEnvelope)
    ).toBeUndefined();
    expect(publicHeadSnapshot(current).id).toBe(
      (JSON.parse(raw) as { id: string }).id
    );
  });
  it('exposes detached snapshots and cannot forge either selection operand', () => {
    const signed = author(),
      candidate = head(signed(10));
    const copy = publicHeadSnapshot(candidate) as {
      id: string;
      identifier: string | null;
      created_at: number;
    };
    copy.id = '0'.repeat(64);
    copy.identifier = 'changed';
    copy.created_at = 99;
    expect(publicHeadSnapshot(candidate)).not.toMatchObject(copy);
    expect(() => publicHeadSnapshot({} as PublicHead)).toThrow(
      'public_head_invalid'
    );
    expect(() => selectPublicHead(candidate, {} as PublicHead)).toThrow(
      'public_head_invalid'
    );
    expect(() => selectPublicHead({} as PublicHead, candidate)).toThrow(
      'public_head_invalid'
    );
  });
  it('preserves protocol ordering for future and maximum safe timestamps', () => {
    const signed = author(),
      a = head(signed(0)),
      b = head(signed(Number.MAX_SAFE_INTEGER));
    expect(selectPublicHead(a, b).head).toBe(b);
    expect(selectPublicHead(b, a)).toEqual({ decision: 'older', head: b });
    const view = verifiedEnvelopeSnapshot(publicHeadEnvelope(b))!;
    view.tags[0][1] = 'changed';
    view.created_at = 0;
    expect(publicHeadSnapshot(b).created_at).toBe(Number.MAX_SAFE_INTEGER);
    expect(publicHeadSnapshot(b).identifier).toBe('stock');
  });
});
