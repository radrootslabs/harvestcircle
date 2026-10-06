import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import WebSocket, { WebSocketServer } from 'ws';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  getPublicPool,
  subscribePublicPool,
  closePublicPool
} from '../../src/lib/nostr/public-pool.ts';
import {
  createPublicIngress,
  admitPublicEvent,
  publicIngressStats
} from '../../src/lib/nostr/ingress.ts';

// HC_TEST_ONLY_RELAY: actual SDK sockets map only to an isolated loopback server.
await test('actual relay batch isolates hostile candidates before trusted projection', async () => {
  const corpus = JSON.parse(
    await readFile(
      new URL(
        '../../../contracts/interop/food_availability/corpus.v1.json',
        import.meta.url
      ),
      'utf8'
    )
  ) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
  const raw = corpus.vectors.find((row) => row.id.endsWith('_014'))!
    .signed_wires.event;
  const valid = JSON.parse(raw) as Record<string, unknown>;
  const candidates = [
    { ...valid, sig: '0'.repeat(128) },
    { ...valid, content: 'untrusted mutation' },
    { ...valid, created_at: Number.MAX_SAFE_INTEGER + 1 },
    valid,
    valid,
    { ...valid, tags: [['d', 'a'], {}] }
  ];
  const server = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 4096
  });
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
      for (const event of candidates)
        socket.send(JSON.stringify(['EVENT', frame[1], event]));
      socket.send(JSON.stringify(['EOSE', frame[1]]));
    })
  );
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const port = address.port;
  const originalSocket = globalThis.WebSocket;
  assert.equal(
    Object.getOwnPropertyDescriptor(globalThis, 'window'),
    undefined
  );
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      assert.equal(String(url).replace(/\/$/, ''), 'wss://hostile.example.org');
      super(`ws://127.0.0.1:${port}`);
    }
  }
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {}
  });
  globalThis.WebSocket =
    FixtureSocket as unknown as typeof globalThis.WebSocket;
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://hostile.example.org',
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
  const pool = getPublicPool(policy)!;
  const owner = createPublicIngress();
  const outcomes: string[] = [];
  try {
    await new Promise<void>((resolve) =>
      subscribePublicPool(pool, [{ kinds: [30402], limit: 200 }], (message) => {
        if (message.type === 'EVENT')
          outcomes.push(admitPublicEvent(owner, message.event).status);
        if (message.type === 'EOSE') resolve();
      })
    );
    assert.deepEqual(outcomes, [
      'rejected',
      'rejected',
      'rejected',
      'accepted',
      'duplicate',
      'rejected'
    ]);
    assert.equal(publicIngressStats(owner).deliveries, 6);
    assert.ok(publicIngressStats(owner).chargedBytes > 0);
    assert.equal(publicIngressStats(owner).stopped, false);
  } finally {
    closePublicPool(pool);
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    globalThis.WebSocket = originalSocket;
    Reflect.deleteProperty(globalThis, 'window');
  }
});
