import { afterAll, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import { normalizePublicQuery } from '../../src/lib/catalog/query-input.ts';
import { searchResolvedFood } from '../../src/lib/catalog/search-match.ts';
import {
  createPublicHeadCandidate,
  publicHeadEnvelope
} from '../../src/lib/catalog/heads.ts';
import {
  createHeadResolver,
  resolveHeads,
  headResolutionSnapshot
} from '../../src/lib/catalog/resolve-head.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../../src/lib/nostr/verified-envelope.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  closePublicScheduler
} from '../../src/lib/nostr/request-scope.ts';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicRetention,
  retainPublicEnvelope,
  publicRetentionKnown
} from '../../src/lib/catalog/retention.ts';
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
const cleanup: (() => void)[] = [];
afterAll(() => {
  for (const stop of cleanup) stop();
});
function fixture() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  cleanup.push(() => key.fill(0));
  const proof = (
    time = base.created_at,
    tags = base.tags,
    content = base.content,
    kind = 30402
  ) => {
    const p = verifyEnvelope(
      JSON.stringify(
        finalizeEvent({ kind, created_at: time, tags, content }, key)
      )
    );
    if (!p.ok) throw Error('search fixture');
    return p.value;
  };
  const head = (p: ReturnType<typeof proof>) => {
    const h = createPublicHeadCandidate(p);
    if (!h) throw Error('search head');
    return h;
  };
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    }),
    run = createPublicRun(scheduler, policy);
  cleanup.push(() => closePublicScheduler(scheduler));
  const retention = createPublicRetention();
  let available = true,
    now = base.created_at + 10,
    assessments = 0;
  const model = createHeadResolver(
    run,
    (kind, filters, next) => openPublicRequest(run, kind, () => () => {}, next),
    {
      nowSeconds: () => {
        assessments++;
        return now;
      }
    },
    {
      retain: (heads) => {
        for (const h of heads)
          retainPublicEnvelope(retention, publicHeadEnvelope(h));
        return true;
      },
      read: (h) => publicRetentionKnown(retention, h),
      available: () => available
    }
  );
  return {
    proof,
    head,
    model,
    retention,
    add: (p: ReturnType<typeof proof>) => resolveHeads(model, [head(p)]),
    retain: (p: ReturnType<typeof proof>) => retainPublicEnvelope(retention, p),
    unavailable: () => {
      available = false;
    },
    clock: (value: number) => {
      now = value;
    },
    clockSamples: () => assessments
  };
}
describe('deterministic admitted public food matching', () => {
  it('requires all terms across title summary description and public location', () => {
    const f = fixture();
    f.add(f.proof());
    const result = searchResolvedFood(f.model, 'NANTES fresh week SAANICH');
    expect(result.ok).toBe(true);
    if (!result.ok) throw Error('query');
    expect(result.rows).toHaveLength(1);
    expect(searchResolvedFood(f.model, 'Nantes missing')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('normalizes NFKC case and whitespace copies while preserving signed food fields', () => {
    const f = fixture(),
      tags = base.tags.map((t) =>
        t[0] === 'title' ? ['title', 'ＣＡＲＲＯＴＳ'] : [...t]
      ),
      p = f.proof(base.created_at, tags);
    f.add(p);
    const before = verifiedEnvelopeSnapshot(p);
    const result = searchResolvedFood(f.model, '  carrots\n ＳＡＡＮＩＣＨ  ');
    expect(result).toMatchObject({ ok: true, query: 'carrots saanich' });
    if (!result.ok) throw Error('query');
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0].state.food!.title).toBe('ＣＡＲＲＯＴＳ');
    expect(verifiedEnvelopeSnapshot(p)).toEqual(before);
  });
  it('empty queries browse only known active supply and preserve last-known uncertainty', () => {
    const f = fixture();
    f.add(f.proof());
    const result = searchResolvedFood(f.model, ' \n ');
    if (!result.ok) throw Error('query');
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0].lastKnown).toBe(true);
    expect(result.rows[0].definitiveAbsence).toBe(false);
  });
  it('sold revisions exclude the prior active keyword match', () => {
    const f = fixture();
    f.add(f.proof());
    f.retain(
      f.proof(
        base.created_at + 1,
        base.tags.map((t) => (t[0] === 'status' ? ['status', 'sold'] : [...t]))
      )
    );
    expect(searchResolvedFood(f.model, 'carrots')).toMatchObject({
      ok: true,
      rows: []
    });
    expect(searchResolvedFood(f.model, '')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('unsupported newest heads block older focused matches', () => {
    const f = fixture();
    f.add(f.proof());
    f.retain(
      f.proof(
        base.created_at + 1,
        base.tags.map((t) =>
          t[0] === 'status' ? ['status', 'unknown'] : [...t]
        )
      )
    );
    expect(searchResolvedFood(f.model, 'carrots')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('authorized retained deletion evidence excludes matches without older fallback', () => {
    const f = fixture(),
      p = f.proof();
    f.add(p);
    f.retain(
      f.proof(
        base.created_at - 1,
        [['e', verifiedEnvelopeSnapshot(p)!.id]],
        '',
        5
      )
    );
    expect(searchResolvedFood(f.model, 'carrots')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('future and clock-unavailable heads cannot match or expose older rows', () => {
    const f = fixture();
    f.add(f.proof());
    f.retain(f.proof(base.created_at + 1000));
    expect(searchResolvedFood(f.model, '')).toMatchObject({
      ok: true,
      rows: []
    });
    const unavailable = fixture();
    unavailable.clock(NaN);
    unavailable.add(unavailable.proof());
    expect(headResolutionSnapshot(unavailable.model)[0].state.display).toBe(
      'clock_unavailable'
    );
    expect(searchResolvedFood(unavailable.model, 'carrots')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('terminal projection unavailability differs from a bounded empty match', () => {
    const f = fixture();
    f.add(f.proof());
    f.unavailable();
    expect(searchResolvedFood(f.model, 'carrots')).toEqual({
      ok: true,
      query: 'carrots',
      available: false,
      rows: [],
      definitiveAbsence: false
    });
  });
  it('plain punctuation is literal and does not activate regex stemming or a geocoder', () => {
    const f = fixture();
    f.add(f.proof());
    for (const q of ['.*', 'carrot.*', '[a-z]', 'Central.*Saanich', 'carro']) {
      const result = searchResolvedFood(f.model, q);
      expect(result.ok).toBe(true);
      if (!result.ok) throw Error('query');
      expect(result.rows.length).toBe(q === 'carro' ? 1 : 0);
    }
    expect(searchResolvedFood(f.model, 'carroted')).toMatchObject({
      ok: true,
      rows: []
    });
  });
  it('enforces actual512UTF8 bytes and12 normalized terms before matching', () => {
    const f = fixture();
    f.add(f.proof());
    expect(searchResolvedFood(f.model, '菜'.repeat(170) + 'aa').ok).toBe(true);
    expect(searchResolvedFood(f.model, '菜'.repeat(171))).toEqual({
      ok: false,
      error: 'query_too_long'
    });
    expect(
      searchResolvedFood(f.model, Array(12).fill('carrots').join(' ')).ok
    ).toBe(true);
    expect(
      searchResolvedFood(f.model, Array(13).fill('carrots').join(' '))
    ).toEqual({ ok: false, error: 'too_many_terms' });
    expect(searchResolvedFood(f.model, 'ﷺ'.repeat(100)).ok).toBe(false);
  });
  it('rejects malformed query inputs without materializing a projection', () => {
    const f = fixture();
    f.add(f.proof());
    const assessments = f.clockSamples();
    for (const input of [undefined, {}, '\ud800', '\u0000'])
      expect(searchResolvedFood(f.model, input)).toEqual({
        ok: false,
        error: 'invalid_query'
      });
    expect(normalizePublicQuery('')).toMatchObject({
      ok: true,
      text: '',
      terms: []
    });
    expect(f.clockSamples()).toBe(assessments);
  });
  it('returns exact decimal and unknown quantity values without conversion or mutation', () => {
    const f = fixture(),
      tags = base.tags
        .filter((t) => t[0] !== 'radroots:quantity')
        .map((t) =>
          t[0] === 'price' ? ['price', '9007199254740993.125', t[2]] : [...t]
        );
    f.add(f.proof(base.created_at, tags));
    const result = searchResolvedFood(f.model, 'carrots');
    if (!result.ok) throw Error('query');
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0].state.food!.price.amount).toBe(
      '9007199254740993.125'
    );
    expect(result.rows[0].state.food!.quantity).toBeNull();
    expect(headResolutionSnapshot(f.model)[0].state.food!.price.amount).toBe(
      '9007199254740993.125'
    );
  });
});
