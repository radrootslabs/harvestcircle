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
async function fixture() {
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
      public: [{ origin, read: true, write: false, nip50: false }],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const wire = (title = 'Carrots', d?: string, time = base.created_at) =>
    finalizeEvent(
      {
        kind: 30402,
        created_at: time,
        content: base.content,
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
  it('bounds primary scope history to the approved source-round and chronological-window maximum', async () => {
    const f = await fixture(),
      token = f.begin('');
    for (let i = 0; i < 5; i++) {
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
      f.channels[i].emit({ type: 'EOSE', id: 'fixture', from: f.origin });
    }
    expect(() =>
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
    ).toThrow('food_search_primary_limit');
    expect(f.channels).toHaveLength(5);
    expect(f.snapshot().scopes).toHaveLength(5);
    expect(f.snapshot().refresh).toBe('limit');
  });
  it('charges failed primary attempts before provider effects without unlimited retries', async () => {
    const f = await fixture(),
      token = f.begin('');
    f.provider.mockImplementation(() => {
      throw new Error('isolated provider failure');
    });
    for (let i = 0; i < 5; i++) {
      expect(() =>
        search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
      ).toThrow('food_search_request_failed');
    }
    expect(f.provider).toHaveBeenCalledTimes(5);
    expect(() =>
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
    ).toThrow('food_search_primary_limit');
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
    const f = await fixture(),
      token = f.begin('', [f.head()]);
    for (let i = 0; i < 4; i++)
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }]);
    expect(f.snapshot().run?.activeRequests).toBe(6);
    expect(() =>
      search.subscribeFoodSearch(token, [{ kinds: [30402], limit: 200 }])
    ).toThrow('food_search_request_failed');
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
