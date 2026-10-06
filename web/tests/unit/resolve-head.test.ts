import { afterAll, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import { PUBLIC_INGRESS_BUDGETS } from '../../src/lib/config/budgets.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  publicRunSnapshot,
  closePublicScheduler,
  type RequestClock
} from '../../src/lib/nostr/request-scope.ts';
import {
  createPublicHeadCandidate,
  publicHeadKey
} from '../../src/lib/catalog/heads.ts';
import { headResolutionQueries } from '../../src/lib/nostr/product-queries.ts';
import {
  createHeadResolver,
  resolveHeads,
  headResolutionSnapshot,
  closeHeadResolver,
  type HeadSubscriber
} from '../../src/lib/catalog/resolve-head.ts';

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
const cleanup: (() => void)[] = [];
afterAll(() => {
  for (const stop of cleanup) stop();
  for (const key of keys) key.fill(0);
});
function fixture() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const event = (
    time = base.created_at,
    tags = base.tags,
    content = base.content,
    kind = 30402
  ) => finalizeEvent({ kind, created_at: time, tags, content }, key);
  const proof = (value: ReturnType<typeof event>) => {
    const p = verifyEnvelope(JSON.stringify(value));
    if (!p.ok) throw Error('fixture proof');
    return p.value;
  };
  const head = (value = event()) => {
    const h = createPublicHeadCandidate(proof(value));
    if (!h) throw Error('fixture head');
    return h;
  };
  const origin = 'wss://one.example.org';
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin, read: true, write: false, nip50: false }],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  let now = 0;
  const timers = new Map<() => void, { at: number; callback: () => void }>();
  const clock: RequestClock = {
    now: () => now,
    schedule(callback, delay) {
      const cancel = () => {
        timers.delete(cancel);
      };
      timers.set(cancel, { at: now + delay, callback });
      return cancel;
    }
  };
  const scheduler = createPublicScheduler(clock),
    run = createPublicRun(scheduler, policy);
  cleanup.push(() => closePublicScheduler(scheduler));
  const channels: {
    kind: string;
    filters: unknown;
    emit: (message: PublicPoolMessage) => void;
    stopped: () => boolean;
  }[] = [];
  const subscribe: HeadSubscriber = (kind, filters, onVerified) =>
    openPublicRequest(
      run,
      kind,
      (next) => {
        let stopped = false;
        channels.push({ kind, filters, emit: next, stopped: () => stopped });
        return () => {
          stopped = true;
        };
      },
      onVerified
    );
  const resolver = createHeadResolver(run, subscribe, {
    nowSeconds: () => base.created_at + 10
  });
  const emit = (i: number, value: ReturnType<typeof event>) =>
    channels[i].emit({
      type: 'EVENT',
      id: 'fixture',
      from: origin,
      event: value
    });
  const eose = (i: number) =>
    channels[i].emit({ type: 'EOSE', id: 'fixture', from: origin });
  const initial = head();
  return {
    event,
    head,
    initial,
    run,
    resolver,
    channels,
    emit,
    eose,
    proof,
    origin,
    timers,
    advance: (value: number) => {
      now = value;
      for (const t of [...timers.values()]) if (t.at <= now) t.callback();
    }
  };
}
describe('bounded exact-coordinate head resolution', () => {
  it('batches unique exact coordinates and author deletion lookups without keyword constraints', () => {
    const f = fixture(),
      other = f.head(
        f.event(
          base.created_at,
          base.tags.map((t) => (t[0] === 'd' ? ['d', 'other'] : [...t]))
        )
      );
    const queries = headResolutionQueries([f.initial, other, f.initial]);
    expect(queries.head).toHaveLength(2);
    expect(queries.deletion).toHaveLength(1);
    expect(queries.head[0]).toEqual({
      kinds: [30402],
      authors: [f.event().pubkey],
      '#d': [base.tags.find((t) => t[0] === 'd')![1]],
      limit: 200
    });
    for (const filter of [...queries.head, ...queries.deletion]) {
      expect(filter).not.toHaveProperty('search');
      expect(filter).not.toHaveProperty('q');
      expect(filter).not.toHaveProperty('since');
    }
    expect(queries.deletion[0]).toMatchObject({ kinds: [5], limit: 200 });
  });
  it('empty generic coordinates omit d filters so missing first d is discoverable', () => {
    const f = fixture();
    const h = f.head(
      f.event(
        base.created_at,
        base.tags.filter((t) => t[0] !== 'd')
      )
    );
    expect(headResolutionQueries([h]).head[0]).not.toHaveProperty('#d');
  });
  it('renamed and sold revisions supersede the original keyword match before projection', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const tags = base.tags.map((t) =>
      t[0] === 'title'
        ? ['title', 'Renamed celery']
        : t[0] === 'status'
          ? ['status', 'sold']
          : [...t]
    );
    const newer = f.event(base.created_at + 1, tags);
    f.emit(0, newer);
    f.eose(0);
    f.eose(1);
    const row = headResolutionSnapshot(f.resolver)[0];
    expect(row.state.head.id).toBe(newer.id);
    expect(row.state.food?.title).toBe('Renamed celery');
    expect(row.state.food?.status).toBe('sold');
    expect(row.coverage).toBe('bounded-eose');
    expect(row.lastKnown).toBe(false);
    expect(row.definitiveAbsence).toBe(false);
  });
  it('newer incompatible heads prevent an older focused fallback', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const newer = f.event(
      base.created_at + 1,
      base.tags.map((t) => (t[0] === 'status' ? ['status', 'unknown'] : [...t]))
    );
    f.emit(0, newer);
    f.emit(0, f.event());
    const row = headResolutionSnapshot(f.resolver)[0];
    expect(row.state.head.id).toBe(newer.id);
    expect(row.state.display).toBe('unsupported');
    expect(row.state.food).toBeUndefined();
  });
  it('incomplete or unavailable lookup retains honest last-known source outcomes', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    f.eose(0);
    let row = headResolutionSnapshot(f.resolver)[0];
    expect(row.lastKnown).toBe(true);
    expect(row.deletionSources?.result.sources[0].state).toBe('pending');
    f.channels[1].emit({
      type: 'CLOSED',
      id: 'fixture',
      from: f.origin,
      reason: 'untrusted relay text'
    });
    row = headResolutionSnapshot(f.resolver)[0];
    expect(row.coverage).toBe('partial');
    expect(row.lastKnown).toBe(true);
    expect(row.state.head.id).toBe(
      headResolutionSnapshot(f.resolver)[0].state.head.id
    );
    expect(JSON.stringify(row)).not.toContain('untrusted relay text');
  });
  it('coalesces repeated coordinates and updates retained winners without extra requests', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial, f.initial]);
    resolveHeads(f.resolver, [f.initial]);
    expect(f.channels).toHaveLength(2);
    expect(publicRunSnapshot(f.run).activeRequests).toBe(2);
    const newer = f.head(f.event(base.created_at + 1));
    resolveHeads(f.resolver, [newer]);
    expect(f.channels).toHaveLength(2);
    expect(headResolutionSnapshot(f.resolver)[0].key).toBe(
      publicHeadKey(newer)
    );
  });
  it('same-author deletion of a newly discovered ID suppresses that winner and retains its evidence', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const newer = f.event(base.created_at + 1);
    f.emit(0, newer);
    f.emit(1, f.event(base.created_at - 1, [['e', newer.id]], '', 5));
    f.eose(0);
    f.eose(1);
    const row = headResolutionSnapshot(f.resolver)[0];
    expect(row.state.head.id).toBe(newer.id);
    expect(row.deletion.outcome).toBe('suppressed');
    expect(row.state.food).toBeUndefined();
    expect(row.deletionProofs).toHaveLength(1);
    expect(verifyEnvelope(JSON.stringify(newer)).ok).toBe(true);
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(2);
  });
  it('rejects out-of-coordinate responses locally and marks malformed deletion coverage partial', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    f.emit(
      0,
      f.event(
        base.created_at + 1,
        base.tags.map((t) => (t[0] === 'd' ? ['d', 'wrong'] : [...t]))
      )
    );
    f.emit(1, f.event(base.created_at, [['e', 'bad']], '', 5));
    f.eose(0);
    f.eose(1);
    const row = headResolutionSnapshot(f.resolver)[0];
    expect(row.coverage).toBe('partial');
    expect(row.state.head.id).toBe(f.event().id);
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(2);
  });
  it('auxiliary deliveries exhaust the same run budget and stop both queries', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const value = f.event();
    for (let n = 0; n < PUBLIC_INGRESS_BUDGETS.deliveries; n++)
      f.emit(n % 2, value);
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(2000);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
    expect(headResolutionSnapshot(f.resolver)[0].lastKnown).toBe(true);
  });
  it('deadline and explicit teardown stop effects while retaining last-known evidence', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    f.advance(10000);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
    expect(f.timers.size).toBe(0);
    expect(headResolutionSnapshot(f.resolver)[0].headSources?.state).toBe(
      'deadline'
    );
    closeHeadResolver(f.resolver);
    expect(headResolutionSnapshot(f.resolver)[0].lastKnown).toBe(true);
    expect(() => resolveHeads(f.resolver, [f.initial])).toThrow();
  });
  it('a second scope admission failure closes the first without cancelling unrelated work', () => {
    const f = fixture();
    for (let n = 0; n < 5; n++)
      openPublicRequest(
        f.run,
        'profile',
        () => () => {},
        () => {}
      );
    resolveHeads(f.resolver, [f.initial]);
    expect(f.channels).toHaveLength(1);
    expect(f.channels[0].stopped()).toBe(true);
    expect(publicRunSnapshot(f.run).activeRequests).toBe(5);
    expect(headResolutionSnapshot(f.resolver)[0].coverage).toBe('partial');
  });
  it('detached result metadata cannot alter winner, deletion evidence or later coverage', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const row = headResolutionSnapshot(f.resolver)[0];
    (row.state.head as unknown as { id: string }).id = 'forged';
    const source = row.headSources!.result.sources[0];
    (source as unknown as { state: string }).state = 'eose';
    expect(headResolutionSnapshot(f.resolver)[0].state.head.id).toBe(
      f.event().id
    );
    expect(
      headResolutionSnapshot(f.resolver)[0].headSources?.result.sources[0].state
    ).toBe('pending');
  });
  it('late callbacks after resolver teardown cannot replace retained evidence', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    closeHeadResolver(f.resolver);
    f.emit(0, f.event(base.created_at + 1));
    expect(headResolutionSnapshot(f.resolver)[0].state.head.id).toBe(
      f.event().id
    );
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(0);
  });
  it('first d identity prevents a matching secondary d from advancing another coordinate', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    f.emit(0, f.event(base.created_at + 1, [['d', 'other'], ...base.tags]));
    expect(headResolutionSnapshot(f.resolver)[0].state.head.id).toBe(
      f.event().id
    );
    expect(headResolutionSnapshot(f.resolver)[0].coverage).toBe('partial');
  });
  it('publishes the accepted final head delivery before the shared budget stops the run', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const value = f.event();
    for (let n = 0; n < 1999; n++) f.emit(0, value);
    const newer = f.event(base.created_at + 1);
    f.emit(0, newer);
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(2000);
    expect(headResolutionSnapshot(f.resolver)[0].state.head.id).toBe(newer.id);
    expect(headResolutionSnapshot(f.resolver)[0].lastKnown).toBe(true);
  });
  it('retains the accepted final tombstone delivery before the shared budget stops the run', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const value = f.event();
    for (let n = 0; n < 1999; n++) f.emit(0, value);
    f.emit(1, f.event(base.created_at - 1, [['e', value.id]], '', 5));
    expect(publicRunSnapshot(f.run).ingress.deliveries).toBe(2000);
    const row = headResolutionSnapshot(f.resolver)[0];
    expect(row.deletion.outcome).toBe('suppressed');
    expect(row.state.food).toBeUndefined();
    expect(row.lastKnown).toBe(true);
  });
  it('a tombstone already retained by an earlier batch suppresses a subsequently resolved coordinate without relay redelivery', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    const next = f.head(
      f.event(
        base.created_at + 1,
        base.tags.map((t) => (t[0] === 'd' ? ['d', 'next'] : [...t]))
      )
    );
    const author = f.event().pubkey;
    f.emit(
      1,
      f.event(base.created_at + 2, [['a', `30402:${author}:next`]], '', 5)
    );
    f.eose(0);
    f.eose(1);
    resolveHeads(f.resolver, [next]);
    f.eose(2);
    f.eose(3);
    const row = headResolutionSnapshot(f.resolver).find(
      (r) => r.key === publicHeadKey(next)
    )!;
    expect(row.deletion.outcome).toBe('suppressed');
    expect(row.state.food).toBeUndefined();
    expect(row.coverage).toBe('bounded-eose');
  });
  it('deletion learned in a later batch also suppresses an earlier retained coordinate', () => {
    const f = fixture();
    resolveHeads(f.resolver, [f.initial]);
    f.eose(0);
    f.eose(1);
    const next = f.head(
      f.event(
        base.created_at + 1,
        base.tags.map((t) => (t[0] === 'd' ? ['d', 'next'] : [...t]))
      )
    );
    resolveHeads(f.resolver, [next]);
    f.emit(3, f.event(base.created_at - 1, [['e', f.event().id]], '', 5));
    f.eose(2);
    f.eose(3);
    const row = headResolutionSnapshot(f.resolver).find(
      (r) => r.key === publicHeadKey(f.initial)
    )!;
    expect(row.deletion.outcome).toBe('suppressed');
    expect(row.state.food).toBeUndefined();
    expect(row.deletionProofs).toHaveLength(1);
  });
  it('refuses oversized batches before effects', () => {
    const f = fixture();
    expect(() =>
      resolveHeads(
        f.resolver,
        Array.from({ length: 2001 }, () => f.initial)
      )
    ).toThrow();
    expect(f.channels).toHaveLength(0);
  });
});
