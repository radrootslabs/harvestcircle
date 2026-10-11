import assert from 'node:assert/strict';
import { test } from 'node:test';
import { generateSecretKey, finalizeEvent } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  PUBLIC_INGRESS_BUDGETS,
  PUBLIC_REQUEST_BUDGETS
} from '../../src/lib/config/budgets.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  publicRunSnapshot,
  publicRequestScopeSnapshot,
  publicSchedulerSnapshot,
  closePublicScheduler,
  type RequestClock,
  type RequestKind
} from '../../src/lib/nostr/request-scope.ts';
import {
  createExtensionScheduler,
  runExtensionAction,
  callExtension,
  markExtensionWaitExpired,
  extensionSchedulerSnapshot,
  type ExtensionAction
} from '../../src/lib/nostr/extension-scheduler.ts';
import type { PublicPoolMessage } from '../../src/lib/nostr/exports.ts';
const origin = 'wss://aggregate.example.org';
const policy = validateRelayPolicy(
  JSON.stringify({
    schemaVersion: 1,
    public: [{ origin, read: true, write: false, nip50: false }],
    inbox: [],
    postingEnabled: false,
    messagingEnabled: false,
    operatorDenylist: []
  })
);
assert.ok(policy);
function channels() {
  let now = 0;
  const timers = new Map<symbol, () => void>();
  const clock: RequestClock = {
    now: () => now,
    schedule(callback) {
      const id = Symbol();
      timers.set(id, callback);
      return () => {
        timers.delete(id);
      };
    }
  };
  const scheduler = createPublicScheduler(clock);
  const run = createPublicRun(scheduler, policy!);
  let closed = 0,
    verified = 0;
  const sinks: ((message: PublicPoolMessage) => void)[] = [];
  const kinds: RequestKind[] = [
    'search',
    'head',
    'deletion',
    'profile',
    'search',
    'head'
  ];
  const requests = kinds.map((kind) =>
    openPublicRequest(
      run,
      kind,
      (next) => {
        sinks.push(next);
        return () => {
          closed++;
        };
      },
      () => {
        verified++;
      }
    )
  );
  return {
    scheduler,
    run,
    requests,
    sinks,
    timers,
    counts: () => ({ closed, verified }),
    advance(value: number) {
      now = value;
    },
    close: () => closePublicScheduler(scheduler)
  };
}
await test('aggregate primary and auxiliary bytes, rejects and duplicate costs stop all six scopes while an outstanding extension call remains occupied', async () => {
  const key = generateSecretKey(),
    f = channels(),
    extension = createExtensionScheduler();
  const capture = { owner: null, session: Symbol(), operation: Symbol() };
  let action: ExtensionAction | undefined,
    release: (() => void) | undefined,
    invoked = 0;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const pending = runExtensionAction(
    extension,
    capture,
    () => true,
    async (value) => {
      action = value;
      return callExtension(value, 'decrypt', () => {
        invoked++;
        return held;
      });
    }
  );
  try {
    assert.equal(invoked, 1);
    assert.ok(action);
    assert.throws(
      () =>
        openPublicRequest(
          f.run,
          'profile',
          () => {
            throw Error('seventh effect');
          },
          () => {}
        ),
      /public_request_concurrency_limit/u
    );
    const good = finalizeEvent(
      {
        kind: 30402,
        created_at: 1700000000,
        tags: [['d', 'unicode']],
        content: '😀é'.repeat(20)
      },
      key
    );
    const bad = { ...good, sig: '0'.repeat(128) };
    const event = (candidate: typeof good): PublicPoolMessage => ({
      type: 'EVENT',
      id: 'bounded',
      from: origin,
      event: candidate
    });
    f.sinks[0](event(good));
    f.sinks[1](event(good));
    f.sinks[2](event(bad));
    assert.equal(f.counts().verified, 2);
    const first = publicRunSnapshot(f.run).ingress;
    assert.equal(first.deliveries, 3);
    assert.ok(
      first.chargedBytes > new TextEncoder().encode(good.content).length * 3
    );
    assert.equal(first.stopped, false);
    const rawOversize = {
      ...good,
      content: '😀'.repeat(PUBLIC_INGRESS_BUDGETS.eventBytes / 4 + 1)
    };
    for (let count = 0; !publicRunSnapshot(f.run).ingress.stopped; count++) {
      assert.ok(count < 33, 'finite actual accounting reaches 8MiB');
      f.sinks[count % f.sinks.length](event(rawOversize));
    }
    const final = publicRunSnapshot(f.run);
    assert.equal(
      final.ingress.chargedBytes,
      PUBLIC_INGRESS_BUDGETS.processedBytes
    );
    assert.equal(final.ingress.stopped, true);
    assert.equal(final.activeRequests, 0);
    assert.deepEqual(publicSchedulerSnapshot(f.scheduler), {
      activeRequests: 0,
      closed: false
    });
    assert.equal(f.counts().closed, 6);
    assert.equal(f.timers.size, 0);
    for (const sink of f.sinks) sink(event(good));
    assert.deepEqual(publicRunSnapshot(f.run).ingress, final.ingress);
    markExtensionWaitExpired(action);
    assert.deepEqual(extensionSchedulerSnapshot(extension), {
      state: 'active',
      pending: 'decrypt',
      phase: 'wait_expired'
    });
    const blocked = await runExtensionAction(
      extension,
      capture,
      () => true,
      () => {
        invoked++;
        return Promise.resolve(undefined);
      }
    );
    assert.equal(blocked.status, 'busy');
    assert.equal(invoked, 1);
    release!();
    const settled = await pending;
    assert.equal(settled.status, 'wait_expired');
    assert.deepEqual(extensionSchedulerSnapshot(extension), { state: 'idle' });
    console.log(
      JSON.stringify({
        fixture:
          'HCP132_LOGICAL_APPLICATION_COMBINED_NO_PREALLOCATION_OR_LIVE_OPERATOR_CLAIM',
        public: final.ingress,
        requestsClosed: f.counts().closed,
        controlled_slot_calls: invoked
      })
    );
  } finally {
    release?.();
    await pending;
    f.close();
    key.fill(0);
  }
});
await test('all primary and auxiliary callbacks enforce captured ten and fifteen second deadlines even when timer dispatch is delayed', () => {
  const f = channels(),
    key = generateSecretKey();
  try {
    assert.equal(
      publicRunSnapshot(f.run).deadline,
      PUBLIC_REQUEST_BUDGETS.runMilliseconds
    );
    assert.ok(
      f.requests.every(
        (request) =>
          publicRequestScopeSnapshot(request).deadline ===
          PUBLIC_REQUEST_BUDGETS.requestMilliseconds
      )
    );
    f.advance(PUBLIC_REQUEST_BUDGETS.requestMilliseconds);
    const good = finalizeEvent(
      {
        kind: 30402,
        created_at: 1700000000,
        tags: [['d', 'deadline']],
        content: 'bounded'
      },
      key
    );
    for (const sink of f.sinks)
      sink({ type: 'EVENT', id: 'late', from: origin, event: good });
    assert.equal(f.counts().verified, 0);
    assert.equal(f.counts().closed, 6);
    assert.equal(f.timers.size, 0);
    assert.equal(publicRunSnapshot(f.run).ingress.deliveries, 0);
    assert.ok(
      f.requests.every(
        (request) => publicRequestScopeSnapshot(request).state === 'deadline'
      )
    );
    f.advance(PUBLIC_REQUEST_BUDGETS.runMilliseconds - 1);
    const late = openPublicRequest(
      f.run,
      'profile',
      () => () => {},
      () => {}
    );
    assert.equal(
      publicRequestScopeSnapshot(late).deadline,
      PUBLIC_REQUEST_BUDGETS.runMilliseconds
    );
    f.advance(PUBLIC_REQUEST_BUDGETS.runMilliseconds);
    assert.equal(publicRunSnapshot(f.run).active, false);
    assert.throws(
      () =>
        openPublicRequest(
          f.run,
          'search',
          () => () => {},
          () => {}
        ),
      /public_run_inactive/u
    );
  } finally {
    f.close();
    key.fill(0);
  }
});
