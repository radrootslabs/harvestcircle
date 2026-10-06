import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import type { RequestClock } from '../../src/lib/nostr/request-scope.ts';
let search: typeof import('../../src/lib/catalog/search-run.ts');
let runtime: typeof import('../../src/lib/runtime/public-runtime.ts');
let context: ReturnType<typeof runtime.createPublicRuntimeContext>;
const keys: Uint8Array[] = [];
beforeEach(async () => {
  vi.resetModules();
  vi.stubGlobal('window', {});
  search = await import('../../src/lib/catalog/search-run.ts');
  runtime = await import('../../src/lib/runtime/public-runtime.ts');
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
let views: typeof import('../../src/lib/catalog/search-view.ts');
beforeEach(async () => {
  views = await import('../../src/lib/catalog/search-view.ts');
});
function renderClock() {
  const callbacks = new Map<() => void, true>();
  return {
    schedule: (callback: () => void) => {
      callbacks.set(callback, true);
      return () => {
        callbacks.delete(callback);
      };
    },
    tick: () => {
      const values = Array.from(callbacks.keys());
      callbacks.clear();
      for (const callback of values) callback();
    },
    size: () => callbacks.size
  };
}
describe('bounded foreground search presentation owner', () => {
  it('rejects invalid queries before losing retained rows or starting effects', async () => {
    const f = await fixture(),
      clock = renderClock(),
      updates: unknown[] = [];
    const owner = views.createSearchView(
      f.view,
      f.wallClock,
      (value) => {
        updates.push(value);
      },
      clock.schedule
    );
    views.startSearchView(owner, 'carrots', [f.head()]);
    const before = f.provider.mock.calls.length;
    expect(() => views.startSearchView(owner, 'x'.repeat(513))).toThrow();
    expect(f.provider).toHaveBeenCalledTimes(before);
    expect(views.searchViewSnapshot(owner)?.rows).toHaveLength(1);
    expect(updates.length).toBeGreaterThan(0);
    views.closeSearchView(owner);
    expect(clock.size()).toBe(0);
  });
  it('does not schedule older discovery when showing another local page', async () => {
    const f = await fixture(),
      clock = renderClock();
    const owner = views.createSearchView(
      f.view,
      f.wallClock,
      () => {},
      clock.schedule
    );
    const heads = Array.from({ length: 26 }, (_, n) =>
      f.head(f.wire('Carrots', `view_${n}`, base.created_at - n))
    );
    views.startSearchView(owner, 'carrots', heads);
    const before = f.provider.mock.calls.length;
    views.moreSearchView(owner);
    expect(views.searchViewSnapshot(owner)?.rows).toHaveLength(26);
    expect(f.provider).toHaveBeenCalledTimes(before);
    views.closeSearchView(owner);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
  });
  it('cancels superseded callbacks and terminates foreground updates at the original deadline', async () => {
    const f = await fixture(),
      clock = renderClock();
    const owner = views.createSearchView(
      f.view,
      f.wallClock,
      () => {},
      clock.schedule
    );
    views.startSearchView(owner, 'carrots', [f.head()]);
    const old = views.searchViewSnapshot(owner)!.generation;
    views.startSearchView(owner, 'celery');
    expect(views.searchViewSnapshot(owner)!.generation).not.toBe(old);
    expect(f.channels.slice(0, 3).every((c) => c.stopped())).toBe(true);
    f.advance(15000);
    clock.tick();
    expect(clock.size()).toBe(0);
    expect(views.searchViewSnapshot(owner)?.query).toBe('celery');
    views.closeSearchView(owner);
  });
  it('closes only its own view while another public view remains active', async () => {
    const f = await fixture(),
      clock = renderClock();
    const other = runtime.createPublicView(runtime.publicRuntime(context)!);
    const run = runtime.beginPublicViewRun(other);
    runtime.subscribePublicView(
      other,
      run,
      'search',
      [{ kinds: [30402], limit: 200 }],
      () => {}
    );
    const owner = views.createSearchView(
      f.view,
      f.wallClock,
      () => {},
      clock.schedule
    );
    views.startSearchView(owner, 'carrots');
    views.closeSearchView(owner);
    expect(f.channels[0].stopped()).toBe(false);
    expect(f.channels.slice(1).every((c) => c.stopped())).toBe(true);
    expect(() => views.startSearchView(owner, '')).toThrow();
    runtime.disposePublicView(other);
  });
});

it('preserves a successor foreground observer started by a local-page update callback', async () => {
  const f = await fixture(),
    clock = renderClock();
  let reenter = false;
  const owner = views.createSearchView(
    f.view,
    f.wallClock,
    () => {
      if (reenter) {
        reenter = false;
        views.startSearchView(owner, 'turnips');
      }
    },
    clock.schedule
  );
  views.startSearchView(owner, 'carrots', [f.head()]);
  reenter = true;
  views.moreSearchView(owner);
  expect(views.searchViewSnapshot(owner)?.query).toBe('turnips');
  expect(clock.size()).toBe(1);
  views.closeSearchView(owner);
});
