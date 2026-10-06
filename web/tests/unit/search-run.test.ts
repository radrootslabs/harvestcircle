import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import type { RequestClock } from '../../src/lib/nostr/request-scope.ts';
let search: typeof import('../../src/lib/catalog/search-run.ts');
let runtime: typeof import('../../src/lib/runtime/public-runtime.ts');
let requests: typeof import('../../src/lib/nostr/request-scope.ts');
let context: ReturnType<typeof runtime.createPublicRuntimeContext>;
const keys: Uint8Array[] = [];
beforeEach(async () => {
  vi.resetModules();
  vi.stubGlobal('window', {});
  search = await import('../../src/lib/catalog/search-run.ts');
  runtime = await import('../../src/lib/runtime/public-runtime.ts');
  requests = await import('../../src/lib/nostr/request-scope.ts');
});
afterEach(() => {
  if (context) runtime.closePublicRuntime(context);
  for (const key of keys.splice(0)) key.fill(0);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
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
async function fixture(nip50 = false, sourceCount = 1) {
  const { RelayPool } = await import('applesauce-relay/pool');
  const { validateRelayPolicy } =
    await import('../../src/lib/config/relays.ts');
  const { verifyEnvelope } =
    await import('../../src/lib/nostr/verified-envelope.ts');
  const { createPublicHeadCandidate } =
    await import('../../src/lib/catalog/heads.ts');
  const channels: {
    emit: (value: PublicPoolMessage) => void;
    stopped: () => boolean;
  }[] = [];
  const provider = vi.spyOn(RelayPool.prototype, 'req').mockImplementation(
    () =>
      ({
        subscribe(subscriber: { next?: (value: PublicPoolMessage) => void }) {
          let stopped = false;
          channels.push({
            emit: (value: PublicPoolMessage) => {
              if (!stopped) subscriber.next?.(value);
            },
            stopped: () => stopped
          });
          return {
            get closed() {
              return stopped;
            },
            unsubscribe() {
              stopped = true;
            }
          };
        }
      }) as unknown as ReturnType<InstanceType<typeof RelayPool>['req']>
  );
  const origin = 'wss://one.example.org';
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: Array.from({ length: sourceCount }, (_, n) => ({
        origin: n === 0 ? origin : `wss://source${n}.example.org`,
        read: true,
        write: false,
        nip50
      })),
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const wire = (
    title = 'Carrots',
    d?: string,
    time = base.created_at,
    content = base.content
  ) =>
    finalizeEvent(
      {
        kind: 30402,
        created_at: time,
        content,
        tags: base.tags.map((tag) =>
          tag[0] === 'title'
            ? ['title', title]
            : tag[0] === 'd' && d !== undefined
              ? ['d', d]
              : [...tag]
        )
      },
      key
    );
  const head = (event = wire()) => {
    const p = verifyEnvelope(JSON.stringify(event));
    if (!p.ok) throw Error('fixture proof');
    const h = createPublicHeadCandidate(p.value);
    if (!h) throw Error('fixture head');
    return h;
  };
  let now = 0,
    reads = 0;
  let onNow = () => {};
  const timers = new Map<() => void, { at: number; callback: () => void }>();
  const clock: RequestClock = {
    now: () => {
      reads++;
      onNow();
      return now;
    },
    schedule: (callback, delay) => {
      const cancel = () => {
        timers.delete(cancel);
      };
      timers.set(cancel, { at: now + delay, callback });
      return cancel;
    }
  };
  context = runtime.createPublicRuntimeContext();
  const owner = runtime.mountPublicRuntime(context, policy, clock)!;
  const view = runtime.createPublicView(owner);
  const wallClock = {
    nowSeconds: () => base.created_at + 10
  };
  const coordinator = search.createFoodSearchCoordinator(view, wallClock);
  const begin = (query = '', heads: readonly ReturnType<typeof head>[] = []) =>
    search.beginFoodSearch(coordinator, query, heads);
  const snapshot = () => search.foodSearchSnapshot(coordinator)!;
  const emit = (i: number, event: ReturnType<typeof wire>) =>
    channels[i].emit({ type: 'EVENT', id: 'fixture', from: origin, event });
  return {
    eose: (i: number) => {
      const selected = provider.mock.calls[i][0];
      if (!Array.isArray(selected)) throw new Error('fixture source array');
      for (const source of selected)
        channels[i].emit({ type: 'EOSE', id: 'fixture', from: String(source) });
    },
    coordinator,
    view,
    wallClock,
    begin,
    snapshot,
    head,
    wire,
    emit,
    channels,
    provider,
    origin,
    timers,
    reads: () => reads,
    reenter: (callback: () => void) => {
      onNow = callback;
    },
    advance: (value: number) => {
      now = value;
      for (const timer of [...timers.values()])
        if (timer.at <= now) timer.callback();
    }
  };
}
describe('finite food search coordination (isolated SDK provider, not qualification)', () => {
  it('reserves at most two chronological windows even after clean EOSE', async () => {
    const f = await fixture(),
      token = f.begin('');
    for (let n = 0; n < 2; n++) {
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
      f.channels[n].emit({ type: 'EOSE', id: 'fixture', from: f.origin });
    }
    expect(() =>
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
    ).toThrow('food_search_chronological_window_limit');
    expect(f.provider).toHaveBeenCalledTimes(2);
    expect(f.snapshot().definitiveAbsence).toBe(false);
  });

  it('keeps a saturated source boundary even when another readable source returns an older head', async () => {
    const f = await fixture(false, 2),
      token = f.begin('carrots');
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    const item = f.wire('Carrots', 'saturated', base.created_at),
      older = f.wire('Carrots', 'other', base.created_at - 1);
    for (let n = 0; n < 200; n++) f.emit(0, item);
    f.channels[0].emit({
      type: 'EVENT',
      id: 'fixture',
      from: 'wss://source1.example.org',
      event: older
    });
    f.eose(0);
    expect(f.snapshot().continuation.until).toBe(base.created_at);
    search.chronologicalFoodSearch(token);
    expect(f.provider.mock.calls.at(-1)![1]).toEqual([
      { kinds: [30402], limit: 200, until: base.created_at }
    ]);
  });
  it('uses only chronological verified times for the inclusive cursor despite earlier relevance samples', async () => {
    const f = await fixture(true),
      token = f.begin('carrots');
    search.sampleFoodSearch(token);
    f.emit(0, f.wire('Carrots', 'sample', base.created_at - 100));
    f.channels[0].emit({ type: 'EOSE', id: 'sample', from: f.origin });
    const request = search.chronologicalFoodSearch(token);
    f.emit(1, f.wire('Carrots', 'chronological', base.created_at));
    f.channels[1].emit({ type: 'EOSE', id: 'chronological', from: f.origin });
    expect(requests.publicRequestScopeSnapshot(request).state).toBe('eose');
    expect(f.snapshot().continuation.until).toBe(base.created_at);
    search.chronologicalFoodSearch(token);
    expect(f.provider.mock.calls.at(-1)![1]).toEqual([
      { kinds: [30402], limit: 200, until: base.created_at }
    ]);
    expect(() =>
      search.subscribeFoodSearch(token, [
        { kinds: [30402], limit: 200, until: base.created_at - 1 }
      ])
    ).toThrow('food_search_chronological_filter');
  });
  it('shows another local20row page without opening a source and keeps the current generation', async () => {
    const f = await fixture(),
      heads = Array.from({ length: 26 }, (_, n) =>
        f.head(f.wire('Carrots', String(n)))
      );
    const token = f.begin('carrots', heads),
      first = f.snapshot();
    expect(first.rows).toHaveLength(20);
    expect(first.hasMore).toBe(true);
    const count = f.provider.mock.calls.length;
    search.showMoreFoodSearch(token);
    expect(f.provider).toHaveBeenCalledTimes(count);
    const next = f.snapshot();
    expect(next.generation).toBe(token);
    expect(next.rows).toHaveLength(26);
    expect(next.hasMore).toBe(false);
  });
  it('starts an explicit older successor run at the same inclusive boundary and preserves known results', async () => {
    const f = await fixture(),
      token = f.begin('carrots');
    search.chronologicalFoodSearch(token);
    f.emit(0, f.wire());
    f.channels[0].emit({ type: 'EOSE', id: 'chronological', from: f.origin });
    const first = f.snapshot();
    expect(first.continuation.until).toBe(base.created_at);
    const older = search.searchOlderFoodSearch(f.coordinator);
    expect(older).not.toBe(token);
    expect(() => search.showMoreFoodSearch(token)).toThrow(
      'food_search_superseded'
    );
    expect(f.provider.mock.calls.at(-1)![1]).toEqual([
      { kinds: [30402], limit: 200, until: base.created_at }
    ]);
    expect(f.snapshot()).toMatchObject({
      query: 'carrots',
      generation: older,
      definitiveAbsence: false
    });
    expect(f.snapshot().rows).toHaveLength(1);
  });
  it('requires the one-round sampler for relevance filters even after a completed sample', async () => {
    const f = await fixture(true),
      token = f.begin('carrots');
    search.sampleFoodSearch(token);
    f.channels[0].emit({ type: 'EOSE', id: 'sample', from: f.origin });
    // A JavaScript caller can supply extra arguments despite the public type.
    const unsafeSubscribe = search.subscribeFoodSearch as unknown as (
      run: typeof token,
      filters: Parameters<typeof search.subscribeFoodSearch>[1],
      source: string
    ) => unknown;
    expect(() =>
      unsafeSubscribe(
        token,
        [{ kinds: [30402], search: 'carrots', limit: 100 }],
        f.origin
      )
    ).toThrow('food_search_sample_required');
    expect(f.provider).toHaveBeenCalledTimes(1);
  });
  it('locally rejects irrelevant or ignored relevance matches and accepts matching fallback evidence', async () => {
    const f = await fixture(true),
      token = f.begin('carrots');
    search.sampleFoodSearch(token);
    f.emit(
      0,
      f.wire('Celery', 'irrelevant', base.created_at, 'Celery available')
    );
    f.channels[0].emit({ type: 'EOSE', id: 'sample', from: f.origin });
    expect(f.snapshot().rows).toHaveLength(0);
    const fallback = search.subscribeFoodSearch(token, [
      { kinds: [30402], limit: 200 }
    ]);
    expect(requests.publicRequestScopeSnapshot(fallback).state).toBe('active');
    f.emit(f.channels.length - 1, f.wire('Carrots', 'wanted'));
    const snapshot = f.snapshot();
    expect(snapshot.rows).toHaveLength(1);
    expect(snapshot.rows[0].state.food?.title).toBe('Carrots');
    expect(snapshot.rows[0].lastKnown).toBe(true);
    expect(snapshot.definitiveAbsence).toBe(false);
    expect('olderCursor' in snapshot).toBe(false);
  });

  it('samples a qualified source once and leaves ordinary fallback available', async () => {
    const f = await fixture(true),
      token = f.begin(' ＣＡＲＲＯＴＳ ');
    const samples = search.sampleFoodSearch(token);
    expect(samples).toHaveLength(1);
    expect(f.provider.mock.calls[0][0]).toEqual([f.origin]);
    expect(f.provider.mock.calls[0][1]).toEqual([
      { kinds: [30402], search: 'carrots', limit: 100 }
    ]);
    f.channels[0].emit({ type: 'EOSE', id: 'sample', from: f.origin });
    expect(f.snapshot()).toMatchObject({
      refresh: 'bounded-eose',
      definitiveAbsence: false,
      rows: []
    });
    expect(() => search.sampleFoodSearch(token)).toThrow(
      'food_search_sample_round_limit'
    );
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    expect(f.provider.mock.calls[1][1]).toEqual([
      { kinds: [30402], limit: 200 }
    ]);
    expect(f.snapshot().run?.activeRequests).toBe(1);
  });
  it('skips sampling for empty queries and unqualified sources without preventing fallback', async () => {
    const f = await fixture(false),
      token = f.begin('carrots');
    expect(search.sampleFoodSearch(token)).toEqual([]);
    expect(f.provider).not.toHaveBeenCalled();
    const browse = f.begin('');
    expect(search.sampleFoodSearch(browse)).toEqual([]);
    search.subscribeFoodSearch(browse, [{ kinds: [30402], limit: 200 }]);
    expect(f.provider).toHaveBeenCalledTimes(1);
  });
  it('records sample provider failure and still permits ordinary chronological discovery', async () => {
    const f = await fixture(true),
      token = f.begin('carrots');
    f.provider.mockImplementationOnce(() => {
      throw new Error('isolated sample refused');
    });
    expect(search.sampleFoodSearch(token)).toEqual([]);
    expect(f.snapshot()).toMatchObject({
      refresh: 'error',
      definitiveAbsence: false
    });
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    expect(f.channels).toHaveLength(1);
    expect(f.snapshot().run?.activeRequests).toBe(1);
  });

  it('uses one coordinator per genuine view and refuses a changed clock owner', async () => {
    const f = await fixture();
    expect(search.createFoodSearchCoordinator(f.view, f.wallClock)).toBe(
      f.coordinator
    );
    expect(() =>
      search.createFoodSearchCoordinator(f.view, {
        nowSeconds: () => base.created_at + 10
      })
    ).toThrow('food_search_clock_changed');
  });
  it('bounds primary scope history to one three-source sample round plus two chronological windows', async () => {
    const f = await fixture(true, 3),
      token = f.begin('carrots');
    search.sampleFoodSearch(token);
    for (let i = 0; i < 3; i++) f.eose(i);
    for (let i = 0; i < 2; i++) {
      search.chronologicalFoodSearch(token);
      f.eose(i + 3);
    }
    expect(() => search.chronologicalFoodSearch(token)).toThrow(
      'food_search_chronological_window_limit'
    );
    expect(f.channels).toHaveLength(5);
    expect(f.snapshot().scopes).toHaveLength(5);
    expect(f.snapshot().refresh).toBe('limit');
  });
  it('charges failed sample and chronological attempts before provider effects without unlimited retries', async () => {
    const f = await fixture(true, 3),
      token = f.begin('carrots');
    f.provider.mockImplementation(() => {
      throw new Error('isolated provider failure');
    });
    expect(search.sampleFoodSearch(token)).toEqual([]);
    for (let i = 0; i < 2; i++)
      expect(() => search.chronologicalFoodSearch(token)).toThrow(
        'food_search_request_failed'
      );
    expect(f.provider).toHaveBeenCalledTimes(5);
    expect(() => search.chronologicalFoodSearch(token)).toThrow(
      'food_search_chronological_window_limit'
    );
    expect(f.provider).toHaveBeenCalledTimes(5);
    expect(f.channels).toHaveLength(0);
    expect(f.snapshot().scopes).toHaveLength(0);
    expect(f.snapshot().refresh).toBe('limit');
  });
  it('rejects invalid query before effects and preserves the current generation', async () => {
    const f = await fixture(),
      token = f.begin(' carrots '),
      reads = f.reads();
    expect(() => f.begin('x'.repeat(513))).toThrow('query_too_long');
    expect(f.reads()).toBe(reads);
    expect(f.snapshot()).toMatchObject({
      generation: token,
      query: 'carrots',
      definitiveAbsence: false
    });
  });
  it('browses retained active heads and keeps source uncertainty explicit', async () => {
    const f = await fixture();
    f.begin('', [f.head(), f.head()]);
    expect(f.snapshot()).toMatchObject({
      query: '',
      available: true,
      refresh: 'refreshing',
      definitiveAbsence: false
    });
    expect(f.snapshot().rows).toHaveLength(1);
    expect(f.snapshot().rows[0].lastKnown).toBe(true);
    expect(f.channels).toHaveLength(2);
  });
  it('bounds and validates the entire cached seed before changing the generation', async () => {
    const f = await fixture(),
      token = f.begin('carrots'),
      h = f.head(),
      reads = f.reads();
    expect(() => f.begin('', Array(2001).fill(h))).toThrow(
      'head_query_input_limit'
    );
    expect(() => f.begin('', [{} as typeof h])).toThrow('public_head_invalid');
    expect(f.reads()).toBe(reads);
    expect(f.snapshot().generation).toBe(token);
  });
  it('cancels superseded scopes and refuses an old handle while retaining known evidence', async () => {
    const f = await fixture(),
      first = f.begin('carrots', [f.head()]);
    const second = f.begin('celery');
    expect(f.channels.slice(0, 2).every((c) => c.stopped())).toBe(true);
    expect(() => search.foodSearchRequestOwner(first)).toThrow(
      'food_search_superseded'
    );
    f.emit(0, f.wire('Carrots', undefined, base.created_at + 2));
    f.emit(2, f.wire('Celery', undefined, base.created_at + 1));
    expect(f.snapshot().generation).toBe(second);
    expect(f.snapshot().rows[0].state.food?.title).toBe('Celery');
  });
  it('cannot publish a generation superseded during an injected clock read', async () => {
    const f = await fixture();
    f.begin('carrots', [f.head()]);
    let armed = true;
    f.reenter(() => {
      if (armed) {
        armed = false;
        f.begin('celery');
      }
    });
    expect(search.foodSearchSnapshot(f.coordinator)).toBeUndefined();
    expect(f.snapshot().query).toBe('celery');
  });
  it('shares all six scope slots with cached latest and deletion lookups', async () => {
    const f = await fixture(true, 3),
      token = f.begin('carrots', [f.head()]);
    search.sampleFoodSearch(token);
    search.chronologicalFoodSearch(token);
    expect(f.snapshot().run?.activeRequests).toBe(6);
    expect(() =>
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
    ).toThrow('food_search_chronological_pending');
    expect(f.snapshot().run?.activeRequests).toBe(6);
    search.cancelFoodSearch(token);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
    expect(f.timers.size).toBe(0);
    expect(f.snapshot().rows).toHaveLength(1);
  });
  it('truncates later requests at the original fifteen-second run deadline', async () => {
    const f = await fixture(),
      token = f.begin('', [f.head()]);
    f.advance(8000);
    const request = search.subscribeFoodSearch(token, [
      { kinds: [30402], limit: 200 }
    ]);
    expect(requests.publicRequestScopeSnapshot(request).deadline).toBe(15000);
    f.advance(15000);
    expect(f.snapshot()).toMatchObject({
      refresh: 'deadline',
      run: { deadline: 15000, active: false },
      definitiveAbsence: false
    });
    expect(f.snapshot().rows).toHaveLength(1);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
  });
  it('charges duplicates and rejects across primary and auxiliary work to one two-thousand delivery cap', async () => {
    const f = await fixture(),
      token = f.begin('', [f.head()]);
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    const valid = f.wire(),
      invalid = { ...valid, id: '0'.repeat(64) };
    for (let i = 0; i < 2000; i++)
      f.emit(i % 2 === 0 ? 2 : 0, i < 1000 ? valid : invalid);
    expect(f.snapshot().run?.ingress).toMatchObject({
      deliveries: 2000,
      stopped: true
    });
    expect(f.snapshot().refresh).toBe('limit');
    expect(f.channels.every((c) => c.stopped())).toBe(true);
    expect(f.snapshot().rows).toHaveLength(1);
  });
  it('charges rejected bytes across primary and auxiliary work to the same eight-MiB ceiling', async () => {
    const f = await fixture(),
      token = f.begin('', [f.head()]);
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    const invalid = { ...f.wire(), content: 'x'.repeat(128000) };
    for (let i = 0; i < 100 && !f.snapshot().run?.ingress.stopped; i++)
      f.emit(i % 2 === 0 ? 2 : 0, invalid);
    expect(f.snapshot().run?.ingress).toMatchObject({
      chargedBytes: 8388608,
      stopped: true
    });
    expect(f.snapshot().run!.ingress.deliveries).toBeLessThan(2000);
    expect(f.snapshot().refresh).toBe('limit');
    expect(f.snapshot().rows).toHaveLength(1);
  });
  it('materializes a newly discovered cached coordinate after deadline without starting new requests', async () => {
    const f = await fixture(),
      token = f.begin('');
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    f.emit(0, f.wire('New carrots', 'fresh-coordinate'));
    f.advance(15000);
    const count = f.channels.length,
      row = f.snapshot().rows[0];
    expect(row.state.food?.title).toBe('New carrots');
    expect(row).toMatchObject({
      coverage: 'partial',
      lastKnown: true,
      definitiveAbsence: false
    });
    expect(f.channels).toHaveLength(count);
  });
  it('retains bounded results despite relay closure and never exposes global completeness', async () => {
    const f = await fixture(),
      token = f.begin('', [f.head()]);
    search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    for (const channel of f.channels)
      channel.emit({
        type: 'CLOSED',
        id: 'fixture',
        from: f.origin,
        reason: 'unavailable'
      });
    const snapshot = f.snapshot();
    expect(snapshot.rows).toHaveLength(1);
    expect(snapshot.refresh).toBe('partial');
    expect(snapshot.definitiveAbsence).toBe(false);
    expect(Object.hasOwn(snapshot, 'complete')).toBe(false);
  });
  it('disposes the owned view and denies further query effects', async () => {
    const f = await fixture();
    f.begin('', [f.head()]);
    search.closeFoodSearchCoordinator(f.coordinator);
    expect(search.foodSearchSnapshot(f.coordinator)).toBeUndefined();
    expect(() => f.begin('')).toThrow('food_search_closed');
    expect(f.channels.every((c) => c.stopped())).toBe(true);
  });
});
