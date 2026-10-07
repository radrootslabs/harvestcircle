import { describe, it, expect } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  openPublicRequest,
  closePublicScheduler,
  publicRunSnapshot,
  publicRunObservations,
  publicSchedulerSnapshot,
  type RequestClock
} from '../../src/lib/nostr/request-scope.ts';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
import {
  createInboxResolver,
  inboxResolutionSnapshot,
  closeInboxResolver
} from '../../src/lib/messaging/resolve-inbox.ts';
import {
  inboxPreferenceWire,
  inboxPreferenceSnapshot
} from '../../src/lib/nostr/inbox-preferences.ts';
import { inboxPreferenceQueries } from '../../src/lib/nostr/inbox-queries.ts';
const origins = ['wss://one.example.org', 'wss://two.example.org'];
function signed() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const old = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [
          ['relay', origins[0]],
          ['unknown', 'keep']
        ],
        content: 'preserve'
      },
      key
    );
    const newer = finalizeEvent(
      { kind: 10050, created_at: 200, tags: [['relay']], content: '' },
      key
    );
    const tie = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [['relay', origins[1]]],
        content: ''
      },
      key
    );
    const unrelated = finalizeEvent(
      { kind: 10002, created_at: 300, tags: [['r', origins[0]]], content: '' },
      key
    );
    return { old, newer, tie, unrelated };
  } finally {
    key.fill(0);
  }
}
function setup(author: string, sources = origins, stopFailures = 0) {
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: sources.map((origin) => ({
        origin,
        read: true,
        write: false,
        nip50: false
      })),
      inbox: [{ origin: 'wss://archive.example.org', read: true, write: true }],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  let now = 0,
    stops = 0;
  let clockHook: (() => void) | undefined;
  const timers = new Map<() => void, () => void>();
  const clock: RequestClock = {
    now: () => {
      const hook = clockHook;
      clockHook = undefined;
      hook?.();
      return now;
    },
    schedule(callback) {
      const cancel = () => {
        timers.delete(cancel);
      };
      timers.set(cancel, callback);
      return cancel;
    }
  };
  const scheduler = createPublicScheduler(clock),
    run = createPublicRun(scheduler, policy);
  let sink: (message: PublicPoolMessage) => void = () => {},
    current = true;
  const resolver = createInboxResolver(
    run,
    author,
    (next) =>
      openInboxRequest(
        run,
        author,
        (callback) => {
          sink = callback;
          return () => {
            stops++;
            if (stops <= stopFailures)
              throw new Error('fixture_cleanup_failed');
          };
        },
        next
      ),
    () => current
  );
  return {
    resolver,
    scheduler,
    run,
    emit(message: PublicPoolMessage) {
      sink(message);
    },
    eose() {
      for (const from of sources) sink({ type: 'EOSE', from, id: 'fixture' });
    },
    stale() {
      current = false;
    },
    duringClock(callback: () => void) {
      clockHook = callback;
    },
    expire() {
      now = 10001;
      for (const callback of [...timers.values()]) callback();
    },
    stops: () => stops
  };
}
function event(
  from: string,
  value: ReturnType<typeof signed>['old']
): PublicPoolMessage {
  return { type: 'EVENT', from, id: 'fixture', event: value };
}
describe('bounded recipient preference discovery', () => {
  it('rechecks current generation after the injected clock synchronously invalidates discovery ownership', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      s.emit(event(origins[0], f.old));
      s.eose();
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('ready');
      s.duringClock(() => s.stale());
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('cleanup failure cannot enable ready and explicit close retries retained admission', () => {
    const f = signed(),
      s = setup(f.old.pubkey, origins, 1);
    try {
      s.emit(event(origins[0], f.old));
      s.eose();
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
      expect(publicSchedulerSnapshot(s.scheduler).activeRequests).toBe(1);
      closeInboxResolver(s.resolver);
      expect(publicSchedulerSnapshot(s.scheduler).activeRequests).toBe(0);
      expect(s.stops()).toBe(2);
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('rejects non-JSON extras, cycles, huge decoded fields and reentrant descriptor traps under shared ingress bounds', () => {
    const f = signed();
    let getterCalls = 0;
    const cycle: Record<string, unknown> = {};
    cycle.self = cycle;
    for (const extra of [
      () => {
        getterCalls++;
        return 'unsafe';
      },
      cycle,
      'x'.repeat(262145),
      {
        get nested() {
          getterCalls++;
          return 'unsafe';
        }
      }
    ]) {
      const s = setup(f.old.pubkey);
      try {
        const candidate = { ...f.old, extra };
        s.emit(event(origins[0], candidate));
        s.eose();
        expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
        expect(publicRunSnapshot(s.run).ingress.deliveries).toBe(1);
      } finally {
        closePublicScheduler(s.scheduler);
      }
    }
    expect(getterCalls).toBe(0);
    const s = setup(f.old.pubkey);
    let entered = false;
    try {
      const hostile = new Proxy(
        { ...f.old },
        {
          getOwnPropertyDescriptor(target, name) {
            if (!entered) {
              entered = true;
              s.emit(event(origins[0], f.old));
            }
            return Reflect.getOwnPropertyDescriptor(target, name);
          }
        }
      );
      s.emit(event(origins[0], hostile));
      expect(publicRunSnapshot(s.run).ingress).toMatchObject({
        stopped: true,
        deliveries: 2
      });
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('fixes exact kind and author without time filters or sender routing', () => {
    const f = signed();
    expect(inboxPreferenceQueries(f.old.pubkey)).toEqual([
      { kinds: [10050], authors: [f.old.pubkey], limit: 200 }
    ]);
    expect(() => inboxPreferenceQueries('f'.repeat(64))).toThrow(
      'inbox_author_invalid'
    );
  });
  it('requires every admitted fixed source and never caches degraded empty lookup as missing', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
      s.emit({ type: 'EOSE', from: origins[0], id: 'fixture' });
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
      s.eose();
      expect(inboxResolutionSnapshot(s.resolver)).toMatchObject({
        status: 'missing',
        coverage: 'bounded-eose',
        definitiveAbsence: false
      });
      s.stale();
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('empty discovery manifest stays inconclusive without falling back to configured archive', () => {
    const f = signed(),
      s = setup(f.old.pubkey, []);
    try {
      expect(inboxResolutionSnapshot(s.resolver)).toMatchObject({
        status: 'inconclusive',
        sources: []
      });
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('new verified incompatible head blocks older compatible preference and retains both source versions', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      s.emit(event(origins[0], f.old));
      s.emit(event(origins[1] + '/', f.newer));
      s.eose();
      const r = inboxResolutionSnapshot(s.resolver);
      expect(r.status).toBe('unsupported');
      expect(inboxPreferenceSnapshot(r.head!)?.id).toBe(f.newer.id);
      expect(r.knownBase).toEqual({
        author: f.old.pubkey,
        id: f.newer.id,
        createdAt: 200
      });
      expect(r.sources.map((row) => row.head?.id)).toEqual([
        f.old.id,
        f.newer.id
      ]);
      expect(publicRunObservations(s.run, r.request)).toEqual([]);
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('selects timestamp then lower ID deterministically independently of arrival order', () => {
    const f = signed();
    for (const rows of [
      [f.old, f.tie],
      [f.tie, f.old]
    ]) {
      const s = setup(f.old.pubkey);
      try {
        for (const row of rows) s.emit(event(origins[0], row));
        s.eose();
        const r = inboxResolutionSnapshot(s.resolver);
        expect(r.status).toBe('ready');
        expect(r.knownBase?.id).toBe([f.old.id, f.tie.id].sort()[0]);
      } finally {
        closePublicScheduler(s.scheduler);
      }
    }
  });
  it('preserves decoded unknown JSON and signed tags without trusting getters or cached SDK metadata', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      const extra = {
        ...f.old,
        extra: { nested: [null, true, 1, 'untrusted'] }
      };
      s.emit(event(origins[0], extra));
      s.eose();
      const r = inboxResolutionSnapshot(s.resolver);
      expect(r.status).toBe('ready');
      const raw: unknown = JSON.parse(inboxPreferenceWire(r.head!)!);
      expect(raw).toEqual(JSON.parse(JSON.stringify(extra)));
      expect(Object.getOwnPropertySymbols(raw as object)).toHaveLength(0);
      r.sources.pop();
      expect(inboxResolutionSnapshot(s.resolver).sources).toHaveLength(2);
      expect(r.wireProvenance).toBe('decoded-sdk-json');
    } finally {
      closePublicScheduler(s.scheduler);
    }
    const t = setup(f.old.pubkey);
    let reads = 0;
    try {
      const hostile = {
        ...f.old,
        get extra() {
          reads++;
          throw new Error('unsafe');
        }
      };
      t.emit(event(origins[0], hostile));
      t.eose();
      expect(reads).toBe(0);
      expect(inboxResolutionSnapshot(t.resolver).status).toBe('inconclusive');
    } finally {
      closePublicScheduler(t.scheduler);
    }
  });
  it('rejects wrong author, wrong kind and bad signature without turning a partial lookup ready or missing', () => {
    const f = signed(),
      other = signed();
    for (const bad of [
      other.old,
      f.unrelated,
      { ...f.old, sig: '0'.repeat(128) }
    ]) {
      const s = setup(f.old.pubkey);
      try {
        s.emit(event(origins[0], bad));
        s.eose();
        expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
        expect(publicRunSnapshot(s.run).ingress.deliveries).toBe(1);
      } finally {
        closePublicScheduler(s.scheduler);
      }
    }
  });
  it('saturation and failed source prevent bounded readiness despite a valid known head', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      for (let i = 0; i < 200; i++) s.emit(event(origins[0], f.old));
      s.eose();
      const r = inboxResolutionSnapshot(s.resolver);
      expect(r.status).toBe('inconclusive');
      expect(r.knownBase?.id).toBe(f.old.id);
      expect(r.sources[0]).toMatchObject({ state: 'limit', candidates: 200 });
      expect(publicRunSnapshot(s.run).ingress.deliveries).toBe(200);
    } finally {
      closePublicScheduler(s.scheduler);
    }
    const t = setup(f.old.pubkey);
    try {
      t.emit(event(origins[0], f.old));
      t.emit({
        type: 'ERROR',
        from: origins[1],
        error: new Error('untrusted')
      });
      t.emit({ type: 'EOSE', from: origins[0], id: 'fixture' });
      expect(inboxResolutionSnapshot(t.resolver).status).toBe('inconclusive');
    } finally {
      closePublicScheduler(t.scheduler);
    }
  });
  it('unknown source never supplies preference authority and deadline/close revoke readiness', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    try {
      s.emit(event('wss://hint.example.org', f.old));
      s.expire();
      expect(inboxResolutionSnapshot(s.resolver).status).toBe('inconclusive');
      expect(publicRunSnapshot(s.run).ingress.deliveries).toBe(1);
      s.emit(event(origins[0], f.old));
      expect(inboxResolutionSnapshot(s.resolver).head).toBeUndefined();
      closeInboxResolver(s.resolver);
      expect(s.stops()).toBe(1);
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
  it('shares six-scope concurrency and forbids generic inbox mode before effects', () => {
    const f = signed(),
      s = setup(f.old.pubkey);
    let effects = 0;
    try {
      for (let i = 0; i < 5; i++)
        openPublicRequest(
          s.run,
          'profile',
          () => {
            effects++;
            return () => {};
          },
          () => {}
        );
      expect(() =>
        openInboxRequest(
          s.run,
          f.old.pubkey,
          () => {
            effects++;
            return () => {};
          },
          () => {}
        )
      ).toThrow('public_request_concurrency_limit');
      expect(effects).toBe(5);
      expect(publicSchedulerSnapshot(s.scheduler).activeRequests).toBe(6);
      expect(() =>
        openPublicRequest(
          s.run,
          'inbox',
          () => {
            effects++;
            return () => {};
          },
          () => {}
        )
      ).toThrow('inbox_author_invalid');
    } finally {
      closePublicScheduler(s.scheduler);
    }
  });
});
