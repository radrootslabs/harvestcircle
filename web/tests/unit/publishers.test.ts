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
let publishers: typeof import('../../src/lib/catalog/publishers.ts');
beforeEach(async () => {
  publishers = await import('../../src/lib/catalog/publishers.ts');
});
describe('genuine bounded public publisher views', () => {
  it('coalesces displayed authors and preserves key fallback without an account', async () => {
    const f = await fixture(),
      token = f.begin(),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock),
      run = search.foodSearchRequestOwner(token);
    const reads = publishers.readPublisherPage(owner, run, [key, key]);
    expect(reads).toHaveLength(1);
    expect(f.provider).toHaveBeenCalledTimes(1);
    expect(publishers.readPublisherPage(owner, run, [key])).toEqual([]);
    expect(publishers.publisherSnapshot(owner, key)).toEqual({
      pubkey: key,
      label: key,
      assertedName: false
    });
    const profile = finalizeEvent(
      {
        kind: 0,
        created_at: base.created_at,
        tags: [],
        content: JSON.stringify({
          display_name: '<b>Farmer</b>',
          pubkey: 'attacker',
          nip05: 'claims@example.org',
          picture: 'https://tracking.example.org'
        })
      },
      keys[0]
    );
    f.emit(0, profile);
    expect(publishers.publisherSnapshot(owner, key)).toEqual({
      pubkey: key,
      label: '<b>Farmer</b>',
      assertedName: true
    });
    expect(f.provider).toHaveBeenCalledTimes(1);
    expect(f.snapshot().run?.ingress.deliveries).toBe(1);
  });
  it('selects generic latest before unsupported/future parsing and never resurrects an old name', async () => {
    const f = await fixture(),
      token = f.begin(),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock);
    publishers.readPublisherPage(owner, search.foodSearchRequestOwner(token), [
      key
    ]);
    const send = (time: number, content: string) =>
      f.emit(
        0,
        finalizeEvent({ kind: 0, created_at: time, tags: [], content }, keys[0])
      );
    send(base.created_at, '{"name":"Old"}');
    expect(publishers.publisherSnapshot(owner, key).label).toBe('Old');
    send(base.created_at + 1, '{"name":123}');
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
    send(base.created_at, '{"name":"Old"}');
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
    send(base.created_at + 311, '{"name":"Future"}');
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
  });
  it('ignores corrupt signatures and nonrequested authors, and rejects a foreign run before effects', async () => {
    const f = await fixture(),
      token = f.begin(),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock),
      run = search.foodSearchRequestOwner(token);
    publishers.readPublisherPage(owner, run, [key]);
    const profile = finalizeEvent(
      {
        kind: 0,
        created_at: base.created_at,
        tags: [],
        content: '{"name":"Spoof"}'
      },
      keys[0]
    );
    f.emit(0, { ...profile, id: '0'.repeat(64) });
    const other = crypto.getRandomValues(new Uint8Array(32));
    keys.push(other);
    f.emit(
      0,
      finalizeEvent(
        {
          kind: 0,
          created_at: base.created_at,
          tags: [],
          content: '{"name":"Other"}'
        },
        other
      )
    );
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
    expect(() =>
      publishers.readPublisherPage(owner, {} as typeof run, [key])
    ).toThrow();
    expect(f.provider).toHaveBeenCalledTimes(1);
    search.cancelFoodSearch(token);
    f.emit(0, profile);
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
    expect(f.channels[0].stopped()).toBe(true);
  });
  it('reserves a failed provider read without retry or losing known food', async () => {
    const f = await fixture(),
      token = f.begin('', [f.head()]),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock),
      run = search.foodSearchRequestOwner(token);
    f.provider.mockImplementationOnce(() => {
      throw Error('isolated provider failure');
    });
    expect(() => publishers.readPublisherPage(owner, run, [key])).toThrow();
    expect(publishers.readPublisherPage(owner, run, [key])).toEqual([]);
    expect(publishers.publisherSnapshot(owner, key)).toEqual({
      pubkey: key,
      label: key,
      assertedName: false
    });
    expect(f.snapshot().rows).toHaveLength(1);
    expect(f.provider).toHaveBeenCalledTimes(3);
    expect(search.foodSearchRequestOwner(token)).toBe(run);
  });
  it('does not publish a stale name when an injected clock delivers a newer unsupported head', async () => {
    const f = await fixture(),
      token = f.begin(),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock);
    publishers.readPublisherPage(owner, search.foodSearchRequestOwner(token), [
      key
    ]);
    f.emit(
      0,
      finalizeEvent(
        {
          kind: 0,
          created_at: base.created_at,
          tags: [],
          content: '{"name":"Old"}'
        },
        keys[0]
      )
    );
    const newer = finalizeEvent(
      {
        kind: 0,
        created_at: base.created_at + 1,
        tags: [],
        content: '{"name":123}'
      },
      keys[0]
    );
    let armed = true;
    f.wallClock.nowSeconds = () => {
      if (armed) {
        armed = false;
        f.emit(0, newer);
      }
      return base.created_at + 10;
    };
    expect(publishers.publisherSnapshot(owner, key).label).toBe(key);
  });
  it('enforces twenty publisher admissions per page across row reordering', async () => {
    const f = await fixture(),
      token = f.begin(),
      owner = publishers.createPublisherViews(f.view, f.wallClock),
      run = search.foodSearchRequestOwner(token);
    const authors = Array.from({ length: 21 }, () => {
      const key = crypto.getRandomValues(new Uint8Array(32));
      keys.push(key);
      return finalizeEvent(
        { kind: 0, created_at: base.created_at, tags: [], content: '{}' },
        key
      ).pubkey;
    });
    publishers.readPublisherPage(owner, run, authors.slice(0, 20));
    expect(publishers.readPublisherPage(owner, run, [authors[20]])).toEqual([]);
    expect(f.provider).toHaveBeenCalledTimes(1);
    expect(publishers.publisherSnapshot(owner, authors[20]).label).toBe(
      authors[20]
    );
    publishers.readPublisherPage(owner, run, [authors[20]], 2);
    expect(f.provider).toHaveBeenCalledTimes(2);
    for (const page of [0, 101, 1.5, NaN])
      expect(() =>
        publishers.readPublisherPage(owner, run, [authors[20]], page)
      ).toThrow('publisher_page_invalid');
    expect(f.provider).toHaveBeenCalledTimes(2);
  });
  it('retains fallback after a failed read and shares all global work and deadlines', async () => {
    const f = await fixture(),
      token = f.begin(),
      key = f.wire().pubkey;
    const owner = publishers.createPublisherViews(f.view, f.wallClock),
      run = search.foodSearchRequestOwner(token);
    publishers.readPublisherPage(owner, run, [key]);
    const profile = finalizeEvent(
      {
        kind: 0,
        created_at: base.created_at,
        tags: [],
        content: '{"name":"Known"}'
      },
      keys[0]
    );
    for (let n = 0; n < 2000; n++) f.emit(0, profile);
    expect(f.snapshot().run?.ingress).toMatchObject({
      deliveries: 2000,
      stopped: true
    });
    expect(f.channels[0].stopped()).toBe(true);
    expect(() => publishers.readPublisherPage(owner, run, [key])).toThrow();
    expect(publishers.publisherSnapshot(owner, key).label).toBe('Known');
  });
});
