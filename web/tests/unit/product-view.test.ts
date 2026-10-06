import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import type { RequestClock } from '../../src/lib/nostr/request-scope.ts';
let product: typeof import('../../src/lib/catalog/product-view.ts');
let runtime: typeof import('../../src/lib/runtime/public-runtime.ts');
let context: ReturnType<typeof runtime.createPublicRuntimeContext>;
const keys: Uint8Array[] = [];
beforeEach(async () => {
  vi.resetModules();
  vi.stubGlobal('window', {});
  product = await import('../../src/lib/catalog/product-view.ts');
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
async function fixture(nip50 = false, sourceCount = 1, throws = false) {
  const { RelayPool } = await import('applesauce-relay/pool');
  const { validateRelayPolicy } =
    await import('../../src/lib/config/relays.ts');
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
  let now = 0;
  const timers = new Map<() => void, { at: number; callback: () => void }>();
  const clock: RequestClock = {
    now: () => {
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
  if (throws)
    provider.mockImplementation(() => {
      throw Error('fixture provider denied');
    });
  context = runtime.createPublicRuntimeContext();
  const owner = runtime.mountPublicRuntime(context, policy, clock)!;
  const view = runtime.createPublicView(owner);
  let onWall = () => {};
  const wallClock = {
    nowSeconds: () => {
      onWall();
      return base.created_at + 10;
    }
  };
  const { encodeProductReference } =
    await import('../../src/lib/nostr/references.ts');
  const initial = wire();
  const naddr = encodeProductReference({
    kind: 30402,
    pubkey: initial.pubkey,
    identifier: initial.tags.find((t) => t[0] === 'd')![1]
  })!;
  const observer = new Map<() => void, () => void>();
  const schedule = (callback: () => void) => {
    const cancel = () => {
      observer.delete(cancel);
    };
    observer.set(cancel, callback);
    return cancel;
  };
  const updated: ReturnType<typeof product.productViewSnapshot>[] = [];
  const token = product.createProductView(
    view,
    naddr,
    wallClock,
    (value) => updated.push(value),
    schedule
  );
  return {
    token,
    armWall: (callback: () => void) => {
      onWall = callback;
    },
    naddr,
    view,
    provider,
    channels,
    wire,
    initial,
    updated,
    observer,
    emit: (i: number, event: ReturnType<typeof wire>) =>
      channels[i].emit({ type: 'EVENT', id: 'fixture', from: origin, event }),
    eose: (i: number) =>
      channels[i].emit({ type: 'EOSE', id: 'fixture', from: origin }),
    snapshot: () => product.productViewSnapshot(token),
    tick: () => {
      const pending = [...observer.values()];
      observer.clear();
      for (const callback of pending) callback();
    },
    fail: (i: number) =>
      channels[i].emit({
        type: 'CLOSED',
        id: 'fixture',
        from: origin,
        reason: 'fixture failure'
      }),
    advance: () => {
      now = 16000;
      for (const timer of [...timers.values()]) timer.callback();
    }
  };
}
describe('genuine direct coordinate product view', () => {
  it('rechecks publication ownership after a snapshot clock creates a successor', async () => {
    const f = await fixture();
    f.emit(0, f.initial);
    const profileIndex = f.provider.mock.calls.findIndex((call) =>
      JSON.stringify(call[1]).includes('[0]')
    );
    expect(profileIndex).toBeGreaterThan(0);
    f.emit(
      profileIndex,
      finalizeEvent(
        {
          kind: 0,
          created_at: base.created_at,
          tags: [],
          content: JSON.stringify({ display_name: 'Actual asserted publisher' })
        },
        keys.at(-1)!
      )
    );
    const previous = f.updated.length;
    let successor: ReturnType<typeof product.createProductView> | undefined;
    f.armWall(() => {
      f.armWall(() => {});
      successor = product.createProductView(
        f.view,
        f.naddr,
        { nowSeconds: () => base.created_at + 10 },
        () => {},
        () => () => {}
      );
    });
    f.tick();
    expect(successor).toBeDefined();
    expect(f.updated.length).toBe(previous);
    expect(f.snapshot().outcome).toBe('unavailable');
    expect(f.snapshot().food).toBeUndefined();
    product.closeProductView(f.token);
    expect(product.productViewSnapshot(successor!).outcome).toBe('checking');
    product.closeProductView(successor!);
  });

  it('reports zero qualified sources unavailable without relay delivery or polling', async () => {
    const f = await fixture(false, 0);
    expect(f.snapshot().outcome).toBe('unavailable');
    expect(f.snapshot().sources[0]?.result.sources).toEqual([]);
    expect(f.observer.size).toBe(0);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
  });
  it('ignores superseded retained callbacks and old cleanup cannot close successor view work', async () => {
    const f = await fixture();
    const pending = [...f.observer.values()];
    const count = f.updated.length;
    const nextUpdates: ReturnType<typeof product.productViewSnapshot>[] = [];
    const next = product.createProductView(
      f.view,
      f.naddr,
      { nowSeconds: () => base.created_at + 10 },
      (value) => nextUpdates.push(value),
      () => () => {}
    );
    for (const callback of pending) callback();
    expect(f.updated.length).toBe(count);
    const channel = f.channels.at(-1)!;
    expect(channel.stopped()).toBe(false);
    product.closeProductView(f.token);
    expect(channel.stopped()).toBe(false);
    expect(product.productViewSnapshot(next).outcome).toBe('checking');
    expect(nextUpdates.length).toBe(1);
    product.closeProductView(next);
    expect(channel.stopped()).toBe(true);
  });
  it('reports initial provider failure unavailable without retries', async () => {
    const f = await fixture(false, 1, true);
    expect(f.snapshot().outcome).toBe('unavailable');
    expect(f.updated.at(-1)?.outcome).toBe('unavailable');
    expect(f.observer.size).toBe(0);
    expect(f.provider).toHaveBeenCalledTimes(1);
  });

  it('publishes terminal deadline state once and never restarts reads', async () => {
    const f = await fixture();
    expect(f.updated.at(-1)?.outcome).toBe('checking');
    const calls = f.provider.mock.calls.length;
    f.advance();
    f.tick();
    expect(f.updated.at(-1)?.outcome).toBe('unobserved');
    expect(f.updated.at(-1)?.lastKnown).toBe(true);
    expect(f.observer.size).toBe(0);
    expect(f.provider.mock.calls.length).toBe(calls);
  });
  it('distinguishes failed sources from bounded empty observation', async () => {
    const f = await fixture();
    f.fail(0);
    f.tick();
    expect(f.snapshot().outcome).toBe('unavailable');
    expect(f.updated.at(-1)?.outcome).toBe('unavailable');
  });

  it('distinguishes bounded unobserved from invalid before effects', async () => {
    const f = await fixture();
    expect(f.snapshot().outcome).toBe('checking');
    const calls = f.provider.mock.calls.length;
    expect(() =>
      product.createProductView(
        f.view,
        'bad',
        { nowSeconds: () => 0 },
        () => {}
      )
    ).toThrow('product_reference_invalid');
    expect(f.provider.mock.calls.length).toBe(calls);
    f.eose(0);
    expect(f.snapshot().outcome).toBe('unobserved');
    expect(f.snapshot().lastKnown).toBe(true);
    product.closeProductView(f.token);
    expect(f.observer.size).toBe(0);
  });
  it('admits only actual exact coordinates and latest unsupported never revives older food', async () => {
    const f = await fixture();
    f.emit(0, f.wire('Other', 'other'));
    expect(f.snapshot().outcome).toBe('checking');
    f.emit(0, f.initial);
    expect(f.snapshot().outcome).toBe('active');
    expect(f.snapshot().food?.title).toBe('Carrots');
    const key = keys.at(-1)!;
    const unsupported = finalizeEvent(
      {
        kind: 30402,
        created_at: base.created_at + 1,
        content: 'other format',
        tags: [
          ['d', f.initial.tags.find((t) => t[0] === 'd')![1]],
          ['title', 'Unsupported']
        ]
      },
      key
    );
    f.emit(0, unsupported);
    f.emit(0, f.initial);
    expect(f.snapshot().outcome).toBe('unsupported');
    expect(f.snapshot().food).toBeUndefined();
    expect(f.snapshot().eventId).toBe(unsupported.id);
  });
  it('shows sold, future quarantine and authorized deletion without fabricated actions', async () => {
    const f = await fixture();
    const key = keys.at(-1)!;
    const sold = finalizeEvent(
      {
        kind: 30402,
        created_at: base.created_at + 1,
        content: base.content,
        tags: base.tags.map((t) =>
          t[0] === 'status' ? ['status', 'sold'] : [...t]
        )
      },
      key
    );
    f.emit(0, sold);
    expect(f.snapshot().outcome).toBe('sold');
    expect(f.snapshot().food?.status).toBe('sold');
    const deletion = finalizeEvent(
      {
        kind: 5,
        created_at: base.created_at + 2,
        content: '',
        tags: [['e', sold.id]]
      },
      key
    );
    const deletionIndex = f.provider.mock.calls.findIndex((c) =>
      JSON.stringify(c[1]).includes('[5]')
    );
    expect(deletionIndex).toBeGreaterThan(0);
    f.emit(deletionIndex, deletion);
    expect(f.snapshot().outcome).toBe('withdrawn');
    expect(f.snapshot().food).toBeUndefined();
    const future = f.wire('Future', undefined, base.created_at + 36000);
    f.emit(0, future);
    expect(f.snapshot().outcome).toBe('future_quarantined');
    expect(f.snapshot().food).toBeUndefined();
  });
  it('preserves uncertainty and scoped cleanup without closing another public view', async () => {
    const f = await fixture();
    f.emit(0, f.initial);
    f.fail(0);
    expect(f.snapshot().lastKnown).toBe(true);
    const retained = runtime.publicRuntime(context)!;
    const other = runtime.createPublicView(retained);
    const otherRun = runtime.beginPublicViewRun(other);
    const calls = f.provider.mock.calls.length;
    product.closeProductView(f.token);
    expect(f.observer.size).toBe(0);
    expect(f.channels.every((c) => c.stopped())).toBe(true);
    expect(runtime.publicViewRunCurrent(other, otherRun)).toBe(true);
    f.channels[0].emit({
      type: 'EVENT',
      id: 'fixture',
      from: 'wss://one.example.org',
      event: f.initial
    });
    f.tick();
    expect(f.provider.mock.calls.length).toBe(calls);
    expect(f.snapshot().outcome).toBe('unavailable');
    runtime.disposePublicView(other);
  });
});
