import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { RelayPool } from 'applesauce-relay/pool';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  recheckIdentityOwner
} from '../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession,
  privateSessionSnapshot,
  privateSessionCleanupRequired,
  subscribePrivateSessionClose
} from '../../src/lib/runtime/private-session.ts';
import {
  getPrivateVisibilityScope,
  privateVisibilitySnapshot,
  closePrivateVisibilityScope
} from '../../src/lib/runtime/dispose.ts';
import {
  getPrivatePool,
  subscribePrivatePage,
  closePrivatePool
} from '../../src/lib/nostr/private-pool.ts';
import {
  getPublicPool,
  subscribePublicPool,
  closePublicPool
} from '../../src/lib/nostr/public-pool.ts';
import {
  getPrivateCacheScope,
  retainPrivateCacheWire,
  privateCacheWire,
  privateCacheSnapshot
} from '../../src/lib/nostr/private-cache-scope.ts';
import {
  getPrivateEnvelopeCapability,
  privateEnvelopeConstructionOwnership
} from '../../src/lib/nostr/private-envelope-capability.ts';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';

// HC_TEST_ONLY_PROVIDER/RELAY/DOCUMENT: actual SDK sockets on isolated loopback;
// echo probe and controlled EventTarget qualify lifetime, not crypto/browser Q.
await test('private disposal owns real SDK sockets, retryable cleanup and terminal visibility while public browsing continues', async (t) => {
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  const other =
    'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
  const origin = 'wss://relay.example.org';
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin, read: true, write: false, nip50: false }],
      inbox: [{ origin, read: true, write: true }],
      postingEnabled: false,
      messagingEnabled: true,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  const server = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  const requests = new Map<WebSocket, { kind: number; id: string }>();
  let privateReady = () => {};
  server.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const buffer = Array.isArray(bytes)
        ? Buffer.concat(bytes)
        : Buffer.isBuffer(bytes)
          ? bytes
          : Buffer.from(bytes);
      const frame: unknown = JSON.parse(buffer.toString('utf8'));
      assert.ok(Array.isArray(frame));
      if (frame[0] !== 'REQ') return;
      const filter = frame[2] as { kinds: number[] };
      requests.set(socket, { kind: filter.kinds[0], id: frame[1] as string });
      if (filter.kinds[0] === 1059) privateReady();
      else socket.send(JSON.stringify(['EOSE', frame[1]]));
    })
  );
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const fixturePort = address.port;
  const originalSocket = globalThis.WebSocket,
    originalFetch = globalThis.fetch,
    originalReq = Reflect.get<RelayPool, 'req'>(RelayPool.prototype, 'req'),
    originalClose = Reflect.get<RelayPool, 'close'>(
      RelayPool.prototype,
      'close'
    );
  const documentTarget = new EventTarget();
  Object.defineProperty(documentTarget, 'visibilityState', {
    configurable: true,
    writable: true,
    value: 'visible'
  });
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: documentTarget
  });
  let key = owner,
    prompts = 0,
    signatures = 0;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey: () => {
          prompts++;
          return Promise.resolve(key);
        },
        signEvent: () => {
          signatures++;
          return Promise.reject(new Error('HC_TEST_ONLY_UNEXPECTED_SIGN'));
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
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      assert.equal(new URL(String(url)).origin, origin);
      super(`ws://127.0.0.1:${fixturePort}`);
    }
  }
  globalThis.WebSocket =
    FixtureSocket as unknown as typeof globalThis.WebSocket;
  globalThis.fetch = () =>
    Promise.reject(new Error('HC_TEST_ONLY_UNEXPECTED_FETCH'));
  const sdkScopes = new Map<'public' | 'private', RelayPool>();
  RelayPool.prototype.req = function (relays, filters, options) {
    assert.ok(Array.isArray(filters));
    const filter = filters[0] as { kinds: number[] };
    sdkScopes.set(filter.kinds[0] === 1059 ? 'private' : 'public', this);
    return originalReq.call(this, relays, filters, options);
  };
  const identity = createIdentitySession();
  let pool: ReturnType<typeof getPrivatePool>,
    publicPool: ReturnType<typeof getPublicPool>;
  const stops: Array<() => void> = [];
  async function connect() {
    assert.equal((await connectIdentity(identity)).state, 'signing_only');
    assert.equal(
      (await probeIdentityMessaging(identity, 'reviewed_self_copy')).state,
      'messaging_capable'
    );
    const session = await createPrivateSession(
      identity,
      'reviewed_private_session'
    );
    assert.ok(session);
    return session;
  }
  async function held(session: Awaited<ReturnType<typeof connect>>) {
    pool = getPrivatePool(session, policy!, [origin]);
    assert.ok(pool);
    const ready = new Promise<void>((resolve) => {
      privateReady = resolve;
    });
    stops.push(subscribePrivatePage(pool, 1, () => {}));
    await ready;
    return [...server.clients].filter((s) => requests.get(s)?.kind === 1059);
  }
  async function publicPage() {
    assert.ok(publicPool);
    await new Promise<void>((resolve) => {
      stops.push(
        subscribePublicPool(
          publicPool!,
          [{ kinds: [0], limit: 1 }],
          (message) => {
            if (message.type === 'EOSE') resolve();
          }
        )
      );
    });
  }
  function visible(value: 'visible' | 'hidden') {
    Object.defineProperty(documentTarget, 'visibilityState', {
      configurable: true,
      writable: true,
      value
    });
    documentTarget.dispatchEvent(new Event('visibilitychange'));
  }
  try {
    await t.test(
      'guest and forged scopes allocate no sockets or provider prompts',
      () => {
        assert.throws(
          () => getPrivateVisibilityScope({} as never),
          /private_session_invalid/
        );
        assert.equal(prompts, 0);
        assert.equal(server.clients.size, 0);
      }
    );
    publicPool = getPublicPool(policy);
    assert.ok(publicPool);
    await publicPage();
    const publicSdk = sdkScopes.get('public');
    assert.ok(publicSdk);
    let session = await connect();
    await t.test(
      'hidden document closes actual private sockets, caches and permissions without affecting public SDK',
      async () => {
        const cache = getPrivateCacheScope(session, 'projection_ownership');
        assert.ok(cache);
        assert.equal(
          retainPrivateCacheWire(
            cache,
            'a'.repeat(64),
            '"HC_TEST_ONLY_SYNTHETIC_OWNERSHIP"'
          ),
          'accepted'
        );
        const capability = getPrivateEnvelopeCapability(session);
        assert.ok(capability);
        const scope = getPrivateVisibilityScope(session);
        assert.ok(scope);
        assert.equal(privateVisibilitySnapshot(scope).active, true);
        const sockets = await held(session),
          closed = sockets.map((s) => once(s, 'close'));
        assert.equal(sockets.length, 1);
        const late = () =>
          retainPrivateCacheWire(
            cache,
            'b'.repeat(64),
            '"HC_TEST_ONLY_LATE_OWNERSHIP"'
          );
        visible('hidden');
        await Promise.all(closed);
        assert.deepEqual(privateVisibilitySnapshot(scope), {
          active: false,
          cleanupRequired: false
        });
        assert.equal(privateSessionSnapshot(session)?.current, false);
        assert.equal(privateCacheWire(cache, 'a'.repeat(64)), undefined);
        assert.equal(privateCacheSnapshot(cache).count, 0);
        assert.equal(late(), 'closed');
        assert.equal(
          privateEnvelopeConstructionOwnership(capability),
          undefined
        );
        const before = prompts;
        visible('visible');
        assert.equal(prompts, before);
        assert.equal(privateVisibilitySnapshot(scope).active, false);
        await publicPage();
        assert.equal(sdkScopes.get('public'), publicSdk);
      }
    );
    session = await connect();
    await t.test(
      'failed physical SDK close is retained, drains other resources and blocks replacement before provider prompts',
      async () => {
        const cache = getPrivateCacheScope(session, 'projection_ownership');
        assert.ok(cache);
        retainPrivateCacheWire(
          cache,
          'c'.repeat(64),
          '"HC_TEST_ONLY_SYNTHETIC_OWNERSHIP"'
        );
        const sockets = await held(session);
        const sdk = sdkScopes.get('private');
        assert.ok(sdk);
        let closeCalls = 0;
        RelayPool.prototype.close = function () {
          if (this === sdk) {
            closeCalls++;
            throw new Error('HC_TEST_ONLY_CLOSE_FAILURE');
          }
          return originalClose.call(this);
        };
        disconnectIdentity(identity);
        assert.equal(privateSessionSnapshot(session)?.current, false);
        assert.equal(privateSessionCleanupRequired(session), true);
        assert.equal(privateCacheSnapshot(cache).count, 0);
        assert.equal(closePrivateSession(session), false);
        assert.ok(closeCalls >= 2);
        assert.equal((await connectIdentity(identity)).state, 'signing_only');
        assert.equal(
          (await probeIdentityMessaging(identity, 'reviewed_self_copy')).state,
          'messaging_capable'
        );
        const before = prompts;
        assert.equal(
          await createPrivateSession(identity, 'reviewed_private_session'),
          undefined
        );
        assert.equal(prompts, before);
        await publicPage();
        assert.equal(sdkScopes.get('public'), publicSdk);
        const closed = sockets.map((s) => once(s, 'close'));
        RelayPool.prototype.close = originalClose;
        assert.equal(closePrivateSession(session), true);
        await Promise.all(closed);
        assert.equal(privateSessionCleanupRequired(session), false);
        const next = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(next);
        assert.notEqual(next, session);
        session = next;
      }
    );
    await t.test(
      'successful callbacks drain once and failed registrations retry without reviving authority',
      () => {
        let failed = 0,
          success = 0;
        subscribePrivateSessionClose(session, () => {
          failed++;
          if (failed === 1) throw new Error('HC_TEST_ONLY_CALLBACK_FAILURE');
        });
        subscribePrivateSessionClose(session, () => {
          success++;
        });
        assert.equal(closePrivateSession(session), false);
        assert.equal(success, 1);
        assert.equal(failed, 1);
        assert.equal(closePrivateSession(session), true);
        assert.equal(success, 1);
        assert.equal(failed, 2);
      }
    );
    session = await connect();
    await t.test(
      'explicit route disposal and observed account loss close actual scopes with no hidden restart',
      async () => {
        const scope = getPrivateVisibilityScope(session);
        assert.ok(scope);
        const sockets = await held(session);
        const closed = sockets.map((s) => once(s, 'close'));
        assert.equal(closePrivateVisibilityScope(scope), true);
        await Promise.all(closed);
        assert.equal(privateVisibilitySnapshot(scope).active, false);
        visible('hidden');
        visible('visible');
        assert.equal(privateVisibilitySnapshot(scope).active, false);
        await publicPage();
        session = await connect();
        const replacement = getPrivateVisibilityScope(session);
        assert.ok(replacement);
        const nextSockets = await held(session),
          nextClosed = nextSockets.map((s) => once(s, 'close'));
        key = other;
        assert.equal((await recheckIdentityOwner(identity)).state, 'guest');
        await Promise.all(nextClosed);
        assert.equal(privateVisibilitySnapshot(replacement).active, false);
        await publicPage();
        assert.equal(signatures, 0);
      }
    );
    await t.test(
      'initially hidden visibility admission suspends without provider or socket effects',
      async () => {
        session = await connect();
        visible('hidden');
        const before = prompts;
        const socketCount = server.clients.size;
        const scope = getPrivateVisibilityScope(session);
        assert.ok(scope);
        assert.deepEqual(privateVisibilitySnapshot(scope), {
          active: false,
          cleanupRequired: false
        });
        assert.equal(prompts, before);
        assert.equal(server.clients.size, socketCount);
        visible('visible');
        assert.equal(privateVisibilitySnapshot(scope).active, false);
        await publicPage();
      }
    );
    await t.test(
      'failed visibility listener removal stays terminal and blocks new admission until actual retry',
      async () => {
        session = await connect();
        const scope = getPrivateVisibilityScope(session);
        assert.ok(scope);
        const remove = Reflect.get<EventTarget, 'removeEventListener'>(
          EventTarget.prototype,
          'removeEventListener'
        );
        Object.defineProperty(documentTarget, 'removeEventListener', {
          configurable: true,
          value: () => {
            throw new Error('HC_TEST_ONLY_LISTENER_REMOVAL_FAILURE');
          }
        });
        try {
          assert.equal(closePrivateVisibilityScope(scope), false);
          assert.deepEqual(privateVisibilitySnapshot(scope), {
            active: false,
            cleanupRequired: true
          });
          const before = prompts;
          assert.equal(
            await createPrivateSession(identity, 'reviewed_private_session'),
            undefined
          );
          assert.equal(prompts, before);
          await publicPage();
        } finally {
          Object.defineProperty(documentTarget, 'removeEventListener', {
            configurable: true,
            value: remove
          });
        }
        assert.equal(closePrivateVisibilityScope(scope), true);
        assert.deepEqual(privateVisibilitySnapshot(scope), {
          active: false,
          cleanupRequired: false
        });
        const next = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        assert.ok(next);
        assert.notEqual(next, session);
        session = next;
      }
    );
  } finally {
    RelayPool.prototype.close = originalClose;
    disconnectIdentity(identity);
    if (pool) closePrivatePool(pool);
    for (const stop of stops) stop();
    if (publicPool) closePublicPool(publicPool);
    RelayPool.prototype.req = originalReq;
    globalThis.WebSocket = originalSocket;
    globalThis.fetch = originalFetch;
    Reflect.deleteProperty(globalThis, 'window');
    Reflect.deleteProperty(globalThis, 'document');
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
  }
});
