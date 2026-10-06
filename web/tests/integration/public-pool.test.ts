import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { RelayPool } from 'applesauce-relay/pool';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  getPublicPool,
  publicPoolOrigins,
  subscribePublicPool,
  closePublicPool
} from '../../src/lib/nostr/public-pool.ts';

// HC_TEST_ONLY_RELAY: SDK constructor mapping is confined to this test.
await test('anonymous public pool owns one browser lifetime', async (t) => {
  const origins = ['wss://relay.example.org', 'wss://relay-two.example.org'];
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: origins.map((origin) => ({
        origin,
        read: true,
        write: true,
        nip50: false
      })),
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  const windowDescriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    'window'
  );
  const originalSocket = globalThis.WebSocket;
  const originalFetch = globalThis.fetch;
  assert.equal(windowDescriptor, undefined);
  await t.test(
    'SSR import and acquisition perform no browser or socket work',
    () => {
      assert.equal(getPublicPool(policy), undefined);
    }
  );
  const server = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 4096
  });
  const frames: unknown[][] = [];
  const sockets: WebSocket[] = [];
  server.on('connection', (socket) => {
    sockets.push(socket);
    socket.send(JSON.stringify(['AUTH', 'unsolicited-test-challenge']));
    socket.on('message', (bytes) => {
      const buffer = Array.isArray(bytes)
        ? Buffer.concat(bytes)
        : Buffer.isBuffer(bytes)
          ? bytes
          : Buffer.from(bytes);
      const frame: unknown = JSON.parse(buffer.toString('utf8'));
      assert.ok(Array.isArray(frame));
      frames.push(frame);
      if (frame[0] === 'REQ') socket.send(JSON.stringify(['EOSE', frame[1]]));
    });
  });
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const fixturePort = address.port;
  let constructed = 0;
  let extensionCalls = 0;
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      assert.ok(origins.includes(String(url).replace(/\/$/, '')));
      constructed++;
      super(`ws://127.0.0.1:${fixturePort}`);
    }
  }
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey() {
          extensionCalls++;
          throw new Error('Unexpected identity');
        },
        signEvent() {
          extensionCalls++;
          throw new Error('Unexpected signing');
        }
      }
    }
  });
  globalThis.WebSocket =
    FixtureSocket as unknown as typeof globalThis.WebSocket;
  globalThis.fetch = () =>
    Promise.reject(new Error('Unexpected relay information fetch'));
  let pool: NonNullable<ReturnType<typeof getPublicPool>> | undefined;
  const stops: Array<() => void> = [];
  try {
    await t.test(
      'repeated acquisition shares an opaque fixed-origin pool',
      () => {
        pool = getPublicPool(policy);
        assert.ok(pool);
        assert.equal(getPublicPool(policy), pool);
        assert.deepEqual(Object.keys(pool), []);
        assert.ok(Object.isFrozen(pool));
        assert.deepEqual(publicPoolOrigins(pool), origins);
        assert.equal(constructed, 0);
        const detached = publicPoolOrigins(pool) as string[];
        detached.push('wss://unapproved.example.org');
        assert.deepEqual(publicPoolOrigins(pool), origins);
      }
    );
    assert.ok(pool);
    await t.test(
      'two scopes reuse sockets and unsolicited AUTH never signs',
      async () => {
        const eose = () =>
          new Promise<void>((resolve, reject) => {
            const seen = new Set<string>();
            stops.push(
              subscribePublicPool(
                pool!,
                [{ kinds: [30402], limit: 1 }],
                (message) => {
                  if (message.type === 'ERROR')
                    reject(
                      message.error instanceof Error
                        ? message.error
                        : new Error('Fixture relay request failed')
                    );
                  if (message.type === 'EOSE') {
                    seen.add(message.from);
                    if (seen.size === origins.length) resolve();
                  }
                }
              )
            );
          });
        await eose();
        await eose();
        assert.equal(constructed, 2);
        assert.equal(server.clients.size, 2);
        assert.equal(extensionCalls, 0);
        assert.equal(frames.filter((frame) => frame[0] === 'REQ').length, 4);
        assert.ok(
          frames.every((frame) => frame[0] !== 'AUTH' && frame[0] !== 'EVENT')
        );
        assert.throws(
          () => subscribePublicPool(pool!, (() => []) as never, () => {}),
          /public_pool_filters_invalid/
        );
        assert.throws(
          () => publicPoolOrigins({} as never),
          /public_pool_invalid/
        );
      }
    );
    await t.test(
      'terminal shutdown closes owned sockets and rejects stale use',
      async () => {
        const closed = sockets.map((socket) => once(socket, 'close'));
        closePublicPool(pool!);
        await Promise.all(closed);
        assert.equal(server.clients.size, 0);
        assert.ok(
          sockets.every((socket) => socket.readyState === WebSocket.CLOSED)
        );
        for (const stop of stops) stop();
        closePublicPool(pool!);
        assert.throws(() => getPublicPool(policy), /public_pool_closed/);
        assert.throws(
          () => subscribePublicPool(pool!, [{}], () => {}),
          /public_pool_closed/
        );
        assert.equal(constructed, 2);
        assert.equal(extensionCalls, 0);
      }
    );
  } finally {
    if (pool) closePublicPool(pool);
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    globalThis.WebSocket = originalSocket;
    globalThis.fetch = originalFetch;
    Reflect.deleteProperty(globalThis, 'window');
  }
});

// Actual SDK lifecycle with an inert socket, not real relay qualification.
await test('callback and failed-close cleanup preserve pool ownership', async (t) => {
  const originalSocket = globalThis.WebSocket;
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://relay.example.org',
          read: true,
          write: false,
          nip50: false
        }
      ],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  class InertSocket {
    static OPEN = 1;
    static CLOSED = 3;
    readyState = 0;
    onopen?: () => void;
    onclose?: (event: { wasClean: boolean }) => void;
    readonly timer: ReturnType<typeof setTimeout>;
    constructor(url: string) {
      assert.equal(new URL(url).origin, 'wss://relay.example.org');
      this.timer = setTimeout(() => {
        this.readyState = 1;
        this.onopen?.();
      }, 0);
    }
    send() {}
    close() {
      clearTimeout(this.timer);
      this.readyState = 3;
      this.onclose?.({ wasClean: true });
    }
  }
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {}
  });
  globalThis.WebSocket = InertSocket as unknown as typeof globalThis.WebSocket;
  try {
    await t.test(
      'synchronous OPEN shutdown retains no group subscriber',
      async () => {
        const adapter = (await import(
          new URL('../../src/lib/nostr/public-pool.ts', import.meta.url).href +
            '?hcp028-reentrant'
        )) as typeof import('../../src/lib/nostr/public-pool.ts');
        const groups: ReturnType<RelayPool['group']>[] = [];
        const group = Reflect.get<RelayPool, 'group'>(
          RelayPool.prototype,
          'group'
        );
        RelayPool.prototype.group = function (
          this: RelayPool,
          ...args: Parameters<RelayPool['group']>
        ) {
          const value = group.apply(this, args);
          groups.push(value);
          return value;
        };
        let stop: (() => void) | undefined;
        try {
          const pool = adapter.getPublicPool(policy);
          assert.ok(pool);
          let opens = 0;
          stop = adapter.subscribePublicPool(
            pool,
            [{ limit: 1 }],
            (message) => {
              if (message.type === 'OPEN') {
                opens++;
                adapter.closePublicPool(pool);
              }
            }
          );
          assert.equal(opens, 1);
          assert.equal(groups.length, 1);
          // Test-only inspection of the actual SDK's retained subscription.
          const stream = (groups[0] as unknown as { readonly relays$: unknown })
            .relays$;
          assert.ok(
            stream &&
              typeof stream === 'object' &&
              'observers' in stream &&
              Array.isArray(stream.observers)
          );
          assert.equal(stream.observers.length, 0);
        } finally {
          stop?.();
          RelayPool.prototype.group = group;
        }
      }
    );
    await t.test(
      'failed SDK shutdown stays closed to traffic and can retry cleanup',
      async () => {
        const adapter = (await import(
          new URL('../../src/lib/nostr/public-pool.ts', import.meta.url).href +
            '?hcp028-close-retry'
        )) as typeof import('../../src/lib/nostr/public-pool.ts');
        const close = Reflect.get<RelayPool, 'close'>(
          RelayPool.prototype,
          'close'
        );
        let calls = 0;
        const instances: RelayPool[] = [];
        RelayPool.prototype.close = function (this: RelayPool) {
          instances.push(this);
          calls++;
          if (calls === 1) throw new Error('HC_TEST_ONLY_SDK_CLOSE_FAILURE');
          close.call(this);
        };
        try {
          const pool = adapter.getPublicPool(policy);
          assert.ok(pool);
          const stop = adapter.subscribePublicPool(
            pool,
            [{ limit: 1 }],
            () => {}
          );
          assert.throws(
            () => adapter.closePublicPool(pool),
            /public_pool_close_failed/
          );
          assert.throws(
            () => adapter.getPublicPool(policy),
            /public_pool_closed/
          );
          assert.throws(
            () => adapter.subscribePublicPool(pool, [{}], () => {}),
            /public_pool_closed/
          );
          adapter.closePublicPool(pool);
          stop();
          assert.equal(calls, 2);
          assert.equal(instances[0]?.relays.size, 0);
          assert.equal(instances[1], instances[0]);
        } finally {
          RelayPool.prototype.close = close;
          if (instances[0]) close.call(instances[0]);
        }
      }
    );
  } finally {
    globalThis.WebSocket = originalSocket;
    Reflect.deleteProperty(globalThis, 'window');
  }
});
