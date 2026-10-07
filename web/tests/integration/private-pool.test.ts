import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { RelayPool } from 'applesauce-relay/pool';
import { createRequire } from 'node:module';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  recheckIdentityOwner
} from '../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  privateSessionSnapshot,
  closePrivateSession
} from '../../src/lib/runtime/private-session.ts';
import {
  getPrivatePool,
  privatePoolOrigins,
  subscribePrivatePage,
  closePrivatePool,
  type PrivatePageMessage
} from '../../src/lib/nostr/private-pool.ts';
import {
  getPublicPool,
  subscribePublicPool,
  closePublicPool
} from '../../src/lib/nostr/public-pool.ts';

// HC_TEST_ONLY_RELAY/provider: genuine pinned SDK, fixed logical origins mapped
// solely here to loopback. Echo cipher and AUTH-state instrumentation qualify
// transport/lifecycle isolation, never encryption, signatures or relay access.
await test('private page transport keeps actual SDK scopes and ownership separate', async (t) => {
  // Resolve the SDK's actual pinned transitive RxJS for teardown fault injection;
  // it is not a new application dependency or replacement Observable.
  const sdkRxjs: unknown = createRequire(
    import.meta.resolve('applesauce-relay/pool')
  )('rxjs');
  const Subscription = (
    sdkRxjs as {
      Subscription: {
        prototype: ReturnType<ReturnType<RelayPool['req']>['subscribe']>;
      };
    }
  ).Subscription;
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  const other =
    'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
  const origins = [
    'wss://relay.example.org',
    'wss://relay-two.example.org',
    'wss://relay-three.example.org'
  ];
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: origins.map((origin) => ({
        origin,
        read: true,
        write: false,
        nip50: false
      })),
      inbox: origins.map((origin) => ({ origin, read: true, write: true })),
      postingEnabled: false,
      messagingEnabled: true,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  assert.equal(
    await createPrivateSession(
      createIdentitySession(),
      'reviewed_private_session'
    ),
    undefined
  );
  assert.equal(getPrivatePool({} as never, policy, origins), undefined);
  const originalSocket = globalThis.WebSocket,
    originalFetch = globalThis.fetch,
    originalReq = Reflect.get(RelayPool.prototype, 'req'),
    originalClose = Reflect.get(RelayPool.prototype, 'close');
  assert.equal(
    Object.getOwnPropertyDescriptor(globalThis, 'window'),
    undefined
  );
  const server = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  const sockets: WebSocket[] = [];
  const frames: unknown[][] = [];
  let delivery:
    | 'normal'
    | 'flood'
    | 'oversize'
    | 'hold'
    | 'byte-flood'
    | 'rejected-byte-flood'
    | 'single' = 'normal';
  let eventWritten = false;
  let heldReady = () => {};
  let heldRequests = 0;
  const raw = {
    id: '1'.repeat(64),
    pubkey: other,
    kind: 1059,
    created_at: 1,
    tags: [['p', owner]],
    content: 'untrusted ciphertext',
    sig: '0'.repeat(128)
  };
  server.on('connection', (socket) => {
    sockets.push(socket);
    socket.send(JSON.stringify(['AUTH', 'unsolicited-challenge']));
    socket.on('message', (bytes) => {
      const buffer = Array.isArray(bytes)
        ? Buffer.concat(bytes)
        : Buffer.isBuffer(bytes)
          ? bytes
          : Buffer.from(bytes);
      const frame: unknown = JSON.parse(buffer.toString('utf8'));
      assert.ok(Array.isArray(frame));
      frames.push(frame);
      if (frame[0] !== 'REQ') return;
      const filter = frame[2] as { kinds: number[] };
      if (filter.kinds[0] === 1059 && delivery === 'hold') {
        heldRequests++;
        if (heldRequests === 3) heldReady();
      } else if (
        filter.kinds[0] === 1059 &&
        (delivery === 'flood' ||
          delivery === 'byte-flood' ||
          delivery === 'rejected-byte-flood')
      ) {
        const candidate =
          delivery === 'flood'
            ? raw
            : {
                ...raw,
                content: 'x'.repeat(delivery === 'byte-flood' ? 20000 : 40000)
              };
        for (let i = 0; i < 501; i++)
          socket.send(JSON.stringify(['EVENT', frame[1], candidate]));
      } else if (filter.kinds[0] === 1059 && delivery === 'single') {
        eventWritten = true;
        socket.send(JSON.stringify(['EVENT', frame[1], raw]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (filter.kinds[0] === 1059 && delivery === 'oversize') {
        socket.send(
          JSON.stringify([
            'EVENT',
            frame[1],
            { ...raw, content: 'x'.repeat(32768) }
          ])
        );
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else socket.send(JSON.stringify(['EOSE', frame[1]]));
    });
  });
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const fixturePort = address.port;
  let key = owner,
    calls = 0,
    signs = 0,
    constructed = 0;
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      assert.ok(origins.includes(new URL(String(url)).origin));
      constructed++;
      super(`ws://127.0.0.1:${fixturePort}`);
    }
  }
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey: () => {
          calls++;
          return Promise.resolve(key);
        },
        signEvent: () => {
          signs++;
          return Promise.reject(new Error('Unexpected signer'));
        },
        nip44: {
          encrypt: (_peer: string, text: string) =>
            Promise.resolve('fixture:' + text),
          decrypt: (_peer: string, text: string) =>
            Promise.resolve(text.slice(8))
        }
      }
    }
  });
  globalThis.WebSocket =
    FixtureSocket as unknown as typeof globalThis.WebSocket;
  globalThis.fetch = () =>
    Promise.reject(new Error('Unexpected information fetch'));
  const sdkScopes = new Map<'public' | 'private', RelayPool>();
  const allScopes = new Set<RelayPool>();
  let requestCalls = 0;
  RelayPool.prototype.req = function (relays, filters, options) {
    requestCalls++;
    allScopes.add(this);
    assert.ok(Array.isArray(filters));
    const filter = filters[0] as { kinds: number[] };
    sdkScopes.set(filter.kinds[0] === 1059 ? 'private' : 'public', this);
    return originalReq.call(this, relays, filters, options);
  };
  const identity = createIdentitySession();
  let privateSession: Awaited<ReturnType<typeof createPrivateSession>>;
  let privatePool: ReturnType<typeof getPrivatePool>;
  let publicPool: ReturnType<typeof getPublicPool>;
  const stops: Array<() => void> = [];
  function publicPage() {
    return new Promise<void>((resolve) => {
      const seen = new Set<string>();
      stops.push(
        subscribePublicPool(
          publicPool!,
          [{ kinds: [30402], limit: 1 }],
          (message) => {
            if (message.type === 'EOSE') {
              seen.add(message.from);
              if (seen.size === 3) resolve();
            }
          }
        )
      );
    });
  }
  async function heldPage() {
    delivery = 'hold';
    heldRequests = 0;
    const ready = new Promise<void>((resolve) => {
      heldReady = resolve;
    });
    stops.push(subscribePrivatePage(privatePool!, 200, () => {}));
    await ready;
  }
  try {
    await t.test(
      'guest, forged and unreviewed session cannot acquire sockets',
      async () => {
        assert.equal(
          await createPrivateSession(identity, 'unreviewed'),
          undefined
        );
        assert.equal(
          await createPrivateSession(identity, 'reviewed_private_session'),
          undefined
        );
        assert.throws(
          () => getPrivatePool({} as never, policy, origins),
          /private_session_invalid/
        );
        assert.equal(constructed, 0);
        assert.equal(calls, 0);
      }
    );
    await connectIdentity(identity);
    await probeIdentityMessaging(identity, 'reviewed_self_copy');
    await t.test(
      'explicit admission rechecks the genuine owner through the shared SDK',
      async () => {
        const before = calls;
        privateSession = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(privateSession);
        assert.equal(calls, before + 1);
        assert.deepEqual(Object.keys(privateSession), []);
        assert.ok(Object.isFrozen(privateSession));
        assert.deepEqual(privateSessionSnapshot(privateSession), {
          owner,
          current: true
        });
        assert.equal(constructed, 0);
      }
    );
    await t.test(
      'selection refuses hints, duplicates and oversized destination sets before egress',
      () => {
        assert.throws(
          () =>
            getPrivatePool(privateSession!, policy, [
              ...origins,
              'wss://unapproved.example.org'
            ]),
          /private_pool_origins_invalid/
        );
        assert.throws(
          () =>
            getPrivatePool(privateSession!, policy, [
              'wss://unapproved.example.org'
            ]),
          /private_pool_origins_invalid/
        );
        assert.throws(
          () =>
            getPrivatePool(privateSession!, policy, [origins[0], origins[0]]),
          /private_pool_origins_invalid/
        );
        const selected = [...origins];
        let reentrant: ReturnType<typeof getPrivatePool>;
        Object.defineProperty(selected, '0', {
          configurable: true,
          get: () => {
            reentrant = getPrivatePool(privateSession!, policy, origins);
            return origins[0];
          }
        });
        privatePool = getPrivatePool(privateSession!, policy, selected);
        assert.ok(privatePool);
        assert.equal(privatePool, reentrant);
        Object.defineProperty(selected, '0', {
          configurable: true,
          writable: true,
          value: origins[0]
        });
        selected[0] = 'wss://unapproved.example.org';
        const detached = privatePoolOrigins(privatePool) as string[];
        detached.push('wss://unapproved.example.org');
        assert.deepEqual(privatePoolOrigins(privatePool), origins);
        assert.equal(
          getPrivatePool(privateSession!, policy, origins),
          privatePool
        );
        assert.throws(
          () => subscribePrivatePage(privatePool!, 201, () => {}),
          /private_page_limit_invalid/
        );
        assert.equal(constructed, 0);
      }
    );
    await t.test(
      'three public and three private same-origin sockets have separate AUTH state',
      async () => {
        publicPool = getPublicPool(policy);
        assert.ok(publicPool);
        await publicPage();
        await heldPage();
        assert.equal(constructed, 6);
        assert.equal(server.clients.size, 6);
        const pub = sdkScopes.get('public'),
          priv = sdkScopes.get('private');
        assert.ok(pub && priv);
        assert.notEqual(pub, priv);
        for (const origin of origins) {
          const a = pub.relay(origin),
            b = priv.relay(origin);
          assert.notEqual(a, b);
          assert.notEqual(a.authentication$, b.authentication$);
          b.authentication$.next({
            ...raw,
            kind: 22242,
            pubkey: owner
          } as never);
          assert.equal(a.authentication, null);
        }
        assert.equal(signs, 0);
        assert.ok(
          frames.every((frame) => frame[0] !== 'AUTH' && frame[0] !== 'EVENT')
        );
        const privateReq = frames.filter(
          (frame) => (frame[2] as { kinds?: number[] })?.kinds?.[0] === 1059
        );
        assert.equal(privateReq.length, 3);
        for (const frame of privateReq)
          assert.deepEqual(frame[2], {
            kinds: [1059],
            '#p': [owner],
            limit: 200
          });
      }
    );
    await t.test(
      'logout closes actual private sockets and public discovery continues',
      async () => {
        const privateSockets = sockets.slice(3, 6);
        const closed = privateSockets.map((socket) => once(socket, 'close'));
        disconnectIdentity(identity);
        await Promise.all(closed);
        assert.equal(server.clients.size, 3);
        assert.equal(privateSessionSnapshot(privateSession!)?.current, false);
        assert.throws(
          () => subscribePrivatePage(privatePool!, 1, () => {}),
          /private_pool_closed/
        );
        await publicPage();
        assert.equal(constructed, 6);
      }
    );
    await t.test(
      'same-author reconnect cannot revive an old scope or consume stale callbacks',
      async () => {
        await connectIdentity(identity);
        await probeIdentityMessaging(identity, 'reviewed_self_copy');
        assert.equal(privateSessionSnapshot(privateSession!)?.current, false);
        const next = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(next);
        assert.notEqual(next, privateSession);
        privateSession = next;
        privatePool = getPrivatePool(next, policy, origins);
        assert.ok(privatePool);
        const before = constructed;
        await heldPage();
        assert.equal(constructed, before + 3);
      }
    );
    await t.test(
      'failed SDK cleanup blocks replacement until exact cleanup retry succeeds',
      () => {
        const before = constructed;
        const held = sdkScopes.get('private');
        assert.ok(held);
        RelayPool.prototype.close = function () {
          if (this === held) throw new Error('HC_TEST_ONLY_CLOSE_FAILURE');
          return originalClose.call(this);
        };
        assert.throws(
          () => closePrivatePool(privatePool!),
          /private_pool_close_failed/
        );
        assert.throws(
          () => getPrivatePool(privateSession!, policy, origins),
          /private_pool_closed/
        );
        assert.equal(constructed, before);
        RelayPool.prototype.close = originalClose;
        closePrivatePool(privatePool!);
      }
    );
    await t.test(
      'observed account loss closes session generation without affecting public scope',
      async () => {
        privatePool = getPrivatePool(privateSession!, policy, origins);
        assert.ok(privatePool);
        await heldPage();
        const newest = [...server.clients].filter(
          (socket) => !sockets.slice(0, 3).includes(socket)
        );
        const closed = newest.map((socket) => once(socket, 'close'));
        key = other;
        await recheckIdentityOwner(identity);
        await Promise.all(closed);
        assert.equal(privateSessionSnapshot(privateSession!)?.current, false);
        assert.equal(
          await createPrivateSession(identity, 'reviewed_private_session'),
          undefined
        );
        await publicPage();
        assert.equal(signs, 0);
      }
    );
    key = owner;
    await connectIdentity(identity);
    await probeIdentityMessaging(identity, 'reviewed_self_copy');
    privateSession = await createPrivateSession(
      identity,
      'reviewed_private_session'
    );
    assert.ok(privateSession);
    privatePool = getPrivatePool(privateSession, policy, origins);
    assert.ok(privatePool);
    await t.test(
      'oversized raw envelope is counted and rejected before consumer delivery',
      async () => {
        delivery = 'oversize';
        let candidates = 0;
        await new Promise<void>((resolve) => {
          stops.push(
            subscribePrivatePage(privatePool!, 200, (message) => {
              if (message.type === 'candidate') candidates++;
              else resolve();
            })
          );
        });
        assert.equal(candidates, 0);
        delivery = 'normal';
      }
    );
    await t.test(
      'duplicate deliveries consume the finite 500-candidate budget',
      async () => {
        delivery = 'flood';
        let candidates = 0;
        const end = await new Promise<PrivatePageMessage>((resolve) => {
          stops.push(
            subscribePrivatePage(privatePool!, 200, (message) => {
              if (message.type === 'candidate') candidates++;
              else resolve(message);
            })
          );
        });
        assert.equal(candidates, 500);
        assert.deepEqual(end, { type: 'end', reason: 'budget' });
        delivery = 'normal';
      }
    );
    await t.test(
      'encoded bytes and rejected envelopes consume the exact finite 8MiB budget',
      async () => {
        delivery = 'byte-flood';
        let candidates = 0,
          bytes = 0;
        const end = await new Promise<PrivatePageMessage>((resolve) => {
          stops.push(
            subscribePrivatePage(privatePool!, 200, (message) => {
              if (message.type === 'candidate') {
                candidates++;
                bytes += Buffer.byteLength(message.wire, 'utf8');
              } else resolve(message);
            })
          );
        });
        const charge = Buffer.byteLength(
          JSON.stringify({ ...raw, content: 'x'.repeat(20000) }),
          'utf8'
        );
        assert.equal(candidates, Math.floor(8388608 / charge));
        assert.ok(bytes <= 8388608);
        assert.deepEqual(end, { type: 'end', reason: 'budget' });
        delivery = 'rejected-byte-flood';
        let rejectedCandidates = 0;
        const rejectedEnd = await new Promise<PrivatePageMessage>((resolve) => {
          stops.push(
            subscribePrivatePage(privatePool!, 200, (message) => {
              if (message.type === 'candidate') rejectedCandidates++;
              else resolve(message);
            })
          );
        });
        assert.equal(rejectedCandidates, 0);
        assert.deepEqual(rejectedEnd, { type: 'end', reason: 'budget' });
        delivery = 'normal';
      }
    );
    await t.test(
      'finite network page enforces the original 15second elapsed boundary',
      async () => {
        delivery = 'hold';
        const original = performance.now.bind(performance),
          descriptor = Object.getOwnPropertyDescriptor(performance, 'now');
        let observations = 0;
        Object.defineProperty(performance, 'now', {
          configurable: true,
          value: () => {
            observations++;
            return original() + (observations > 1 ? 15000 : 0);
          }
        });
        try {
          const end = await new Promise<PrivatePageMessage>((resolve) => {
            stops.push(
              subscribePrivatePage(privatePool!, 200, (message) => {
                if (message.type === 'end') resolve(message);
              })
            );
          });
          assert.ok(observations > 1);
          assert.deepEqual(end, { type: 'end', reason: 'elapsed' });
        } finally {
          if (descriptor) Object.defineProperty(performance, 'now', descriptor);
          else Reflect.deleteProperty(performance, 'now');
          delivery = 'normal';
        }
      }
    );
    await t.test(
      'asynchronous EOSE cleanup failure emits error and retains unavailable page without escaping',
      async () => {
        delivery = 'normal';
        const original = Reflect.get(Subscription.prototype, 'unsubscribe');
        let failNext = true;
        Subscription.prototype.unsubscribe = function () {
          if (failNext && new Error().stack?.includes('at release (')) {
            failNext = false;
            throw new Error('HC_TEST_ONLY_ASYNC_UNSUBSCRIBE_FAILURE');
          }
          return original.call(this);
        };
        try {
          const end = await new Promise<PrivatePageMessage>((resolve) => {
            stops.push(
              subscribePrivatePage(privatePool!, 200, (message) => {
                if (message.type === 'end') resolve(message);
              })
            );
          });
          assert.deepEqual(end, { type: 'end', reason: 'error' });
          assert.throws(
            () => subscribePrivatePage(privatePool!, 1, () => {}),
            /private_page_cleanup_required/
          );
          await new Promise<void>((resolve) => setImmediate(resolve));
        } finally {
          Subscription.prototype.unsubscribe = original;
          closePrivatePool(privatePool!);
          privatePool = getPrivatePool(privateSession!, policy, origins);
          assert.ok(privatePool);
        }
      }
    );
    await t.test(
      'failed page unsubscribe keeps new SDK admission blocked until cleanup retry',
      async () => {
        await heldPage();
        const stop = stops[stops.length - 1];
        const original = Reflect.get(Subscription.prototype, 'unsubscribe');
        let failNext = true;
        Subscription.prototype.unsubscribe = function () {
          if (failNext) {
            failNext = false;
            throw new Error('HC_TEST_ONLY_UNSUBSCRIBE_FAILURE');
          }
          return original.call(this);
        };
        let unexpected: (() => void) | undefined;
        const before = requestCalls;
        try {
          assert.throws(() => stop(), /HC_TEST_ONLY_UNSUBSCRIBE_FAILURE/);
          assert.throws(() => {
            unexpected = subscribePrivatePage(privatePool!, 1, () => {});
          }, /private_pool_closed|private_page_cleanup_required/);
          assert.equal(requestCalls, before);
        } finally {
          unexpected?.();
          Subscription.prototype.unsubscribe = original;
          closePrivatePool(privatePool!);
        }
      }
    );
    await t.test(
      'clock reentry cannot admit an SDK request after owner logout',
      async () => {
        await new Promise<void>((resolve) => setImmediate(resolve));
        assert.equal(
          (await recheckIdentityOwner(identity)).state,
          'messaging_capable'
        );
        privatePool = getPrivatePool(privateSession!, policy, origins);
        assert.ok(privatePool);
        const original = performance.now.bind(performance);
        const descriptor = Object.getOwnPropertyDescriptor(performance, 'now');
        const before = requestCalls;
        let invalidations = 0;
        Object.defineProperty(performance, 'now', {
          configurable: true,
          value: () => {
            invalidations++;
            disconnectIdentity(identity);
            return original();
          }
        });
        try {
          await new Promise<void>((resolve) => {
            stops.push(
              subscribePrivatePage(privatePool!, 1, (message) => {
                if (message.type === 'end') resolve();
              })
            );
          });
          assert.ok(invalidations > 0);
          assert.equal(requestCalls, before);
        } finally {
          if (descriptor) Object.defineProperty(performance, 'now', descriptor);
          else Reflect.deleteProperty(performance, 'now');
        }
      }
    );
    await t.test(
      'candidate clock reentry cannot disclose old-owner ciphertext after logout',
      async () => {
        await connectIdentity(identity);
        await probeIdentityMessaging(identity, 'reviewed_self_copy');
        privateSession = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(privateSession);
        privatePool = getPrivatePool(privateSession, policy, [origins[0]]);
        assert.ok(privatePool);
        delivery = 'single';
        eventWritten = false;
        let candidates = 0,
          invalidations = 0;
        const original = performance.now.bind(performance),
          descriptor = Object.getOwnPropertyDescriptor(performance, 'now');
        Object.defineProperty(performance, 'now', {
          configurable: true,
          value: () => {
            if (eventWritten) {
              invalidations++;
              disconnectIdentity(identity);
            }
            return original();
          }
        });
        try {
          await new Promise<void>((resolve) => {
            stops.push(
              subscribePrivatePage(privatePool!, 1, (message) => {
                if (message.type === 'candidate') candidates++;
                else resolve();
              })
            );
          });
          assert.ok(eventWritten && invalidations > 0);
          assert.equal(candidates, 0);
          assert.equal(privateSessionSnapshot(privateSession)?.current, false);
        } finally {
          if (descriptor) Object.defineProperty(performance, 'now', descriptor);
          else Reflect.deleteProperty(performance, 'now');
          delivery = 'normal';
        }
      }
    );
    await t.test(
      'delayed genuine SDK owner response cannot open old-session sockets after logout',
      async () => {
        await connectIdentity(identity);
        await probeIdentityMessaging(identity, 'reviewed_self_copy');
        privateSession = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(privateSession);
        privatePool = getPrivatePool(privateSession, policy, origins);
        assert.ok(privatePool);
        const browser = Reflect.get(globalThis, 'window');
        const provider: unknown = browser.nostr;
        assert.ok(provider && typeof provider === 'object');
        const descriptor = Object.getOwnPropertyDescriptor(
          provider,
          'getPublicKey'
        );
        assert.ok(descriptor);
        let release = (_key: string) => {
          void _key;
        };
        let entered = () => {};
        const observed = new Promise<void>((resolve) => {
          entered = resolve;
        });
        Object.defineProperty(provider, 'getPublicKey', {
          configurable: true,
          value: () => {
            entered();
            return new Promise<string>((resolve) => {
              release = resolve;
            });
          }
        });
        const before = requestCalls;
        let candidates = 0;
        const end = new Promise<PrivatePageMessage>((resolve) => {
          stops.push(
            subscribePrivatePage(privatePool!, 1, (message) => {
              if (message.type === 'candidate') candidates++;
              else resolve(message);
            })
          );
        });
        try {
          await observed;
          disconnectIdentity(identity);
          assert.deepEqual(await end, { type: 'end', reason: 'closed' });
          Object.defineProperty(provider, 'getPublicKey', descriptor);
          release(owner);
          await new Promise<void>((resolve) => setImmediate(resolve));
          assert.equal(requestCalls, before);
          assert.equal(candidates, 0);
          assert.equal(privateSessionSnapshot(privateSession)?.current, false);
          key = other;
          await connectIdentity(identity);
          await probeIdentityMessaging(identity, 'reviewed_self_copy');
          const next = await createPrivateSession(
            identity,
            'reviewed_private_session'
          );
          assert.ok(next);
          assert.deepEqual(privateSessionSnapshot(next), {
            owner: other,
            current: true
          });
          assert.equal(privateSessionSnapshot(privateSession)?.current, false);
          closePrivateSession(next);
          disconnectIdentity(identity);
          key = owner;
        } finally {
          Object.defineProperty(provider, 'getPublicKey', descriptor);
          release(owner);
        }
      }
    );
    await t.test(
      'capability loss closes actual private transport and signing alone cannot reopen it',
      async () => {
        await connectIdentity(identity);
        await probeIdentityMessaging(identity, 'reviewed_self_copy');
        privateSession = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(privateSession);
        privatePool = getPrivatePool(privateSession, policy, origins);
        assert.ok(privatePool);
        await heldPage();
        const newest = [...server.clients].filter(
          (socket) => !sockets.slice(0, 3).includes(socket)
        );
        const closed = newest.map((socket) => once(socket, 'close'));
        const browser = Reflect.get(globalThis, 'window');
        const provider: unknown = browser.nostr;
        assert.ok(provider && typeof provider === 'object');
        Object.defineProperty(provider, 'nip44', {
          configurable: true,
          value: undefined
        });
        const checked = await recheckIdentityOwner(identity);
        await Promise.all(closed);
        assert.equal(checked.state, 'signing_only');
        assert.equal(privateSessionSnapshot(privateSession)?.current, false);
        const before = constructed;
        assert.equal(
          await createPrivateSession(identity, 'reviewed_private_session'),
          undefined
        );
        assert.equal(constructed, before);
        await publicPage();
      }
    );
  } finally {
    RelayPool.prototype.close = originalClose;
    if (privatePool) closePrivatePool(privatePool);
    if (privateSession) closePrivateSession(privateSession);
    disconnectIdentity(identity);
    for (const stop of stops) stop();
    if (publicPool) closePublicPool(publicPool);
    RelayPool.prototype.req = originalReq;
    for (const scope of allScopes) scope.close();
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    globalThis.WebSocket = originalSocket;
    globalThis.fetch = originalFetch;
    Reflect.deleteProperty(globalThis, 'window');
  }
});
