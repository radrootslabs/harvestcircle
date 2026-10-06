import { afterAll, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
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
import {
  orderFoodRows,
  listingView
} from '../../src/lib/catalog/listing-view.ts';
describe('exact advertised listing views', () => {
  it('keeps signed text and decimals beyond safe integers, and unknown quantity', () => {
    const f = fixture();
    const tags = base.tags
      .filter((t) => t[0] !== 'radroots:quantity')
      .map((t) =>
        t[0] === 'price'
          ? ['price', '9007199254740993.123456789', 'CAD']
          : t[0] === 'title'
            ? ['title', '<script>ＣＡＲＲＯＴＳ</script>']
            : [...t]
      );
    const p = f.proof(base.created_at, tags);
    f.add(p);
    const row = headResolutionSnapshot(f.model)[0];
    const value = listingView(row)!;
    expect(value.title).toBe('<script>ＣＡＲＲＯＴＳ</script>');
    expect(value.price).toEqual(row.state.food!.price);
    expect(value.price.amount).toBe('9007199254740993.123456789');
    expect(value.quantity).toBeNull();
    expect(value.href).toMatch(/^\/products\/naddr1/);
    expect(value.href).not.toContain('?');
    (value.price as { amount: string }).amount = '0';
    expect(listingView(row)!.price.amount).toBe('9007199254740993.123456789');
    expect(verifiedEnvelopeSnapshot(p)?.content).toBe(base.content);
  });
  it('orders revision time then lower event ID without mutating the input', () => {
    const f = fixture();
    for (const [d, t] of [
      ['a', 0],
      ['b', 1],
      ['c', 1]
    ] as const)
      f.add(
        f.proof(
          base.created_at + t,
          base.tags.map((v) => (v[0] === 'd' ? ['d', d] : [...v]))
        )
      );
    const rows = headResolutionSnapshot(f.model),
      before = rows.map((v) => v.state.head.id);
    const ordered = orderFoodRows(rows);
    expect(ordered[0].state.head.created_at).toBe(base.created_at + 1);
    expect(ordered[0].state.head.id < ordered[1].state.head.id).toBe(true);
    expect(rows.map((v) => v.state.head.id)).toEqual(before);
  });
  it('does not promote older keyword matches after sold, renamed or withdrawn latest evidence', () => {
    for (const outcome of ['sold', 'renamed', 'withdrawn']) {
      const f = fixture(),
        old = f.proof();
      f.add(old);
      const tags = base.tags.map((t) =>
        outcome === 'sold' && t[0] === 'status'
          ? ['status', 'sold']
          : outcome === 'renamed' && t[0] === 'title'
            ? ['title', 'Celery']
            : [...t]
      );
      const newer = f.proof(
        base.created_at + 1,
        tags,
        outcome === 'renamed' ? 'Celery' : base.content
      );
      f.retain(newer);
      if (outcome === 'withdrawn')
        f.retain(
          f.proof(
            base.created_at,
            [['e', verifiedEnvelopeSnapshot(newer)!.id]],
            '',
            5
          )
        );
      const matched = searchResolvedFood(
        f.model,
        outcome === 'renamed' ? 'nantes' : ''
      );
      expect(matched.ok && matched.rows).toEqual([]);
      if (outcome !== 'renamed')
        expect(listingView(headResolutionSnapshot(f.model)[0])).toBeUndefined();
    }
  });
});
