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
  getPublicStore,
  insertPublicEnvelope,
  closePublicStore,
  publicStoreEnvelope
} from '../../src/lib/nostr/public-store.ts';
import {
  createPublicIngress,
  publicIngressStats
} from '../../src/lib/nostr/ingress.ts';
import {
  createObservationJournal,
  publicObservations,
  exactPublicObservation
} from '../../src/lib/catalog/observations.ts';
import {
  createPublicRequestResult,
  handlePublicRequestMessage,
  publicRequestSnapshot,
  disposePublicRequestResult
} from '../../src/lib/nostr/request-result.ts';
await test('actual SDK sources retain exact observations before store dedup and sanitize partial outcomes', async () => {
  const corpus = JSON.parse(
    await readFile(
      new URL(
        '../../../contracts/interop/food_availability/corpus.v1.json',
        import.meta.url
      ),
      'utf8'
    )
  ) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
  const wires = corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires;
  const current = JSON.parse(wires.current) as { id: string; sig: string };
  const previous = JSON.parse(wires.previous) as { id: string };
  const origins = ['wss://one.example.org', 'wss://two.example.org'];
  const servers = origins.map(
    () => new WebSocketServer({ host: '127.0.0.1', port: 0, maxPayload: 4096 })
  );
  const sockets: WebSocket[] = [];
  for (const [index, server] of servers.entries()) {
    server.on('connection', (socket) => {
      sockets.push(socket);
      socket.on('message', (bytes) => {
        const buffer = Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.isBuffer(bytes)
            ? bytes
            : Buffer.from(bytes);
        const frame = JSON.parse(buffer.toString('utf8')) as unknown[];
        if (frame[0] !== 'REQ') return;
        const id = frame[1];
        if (index === 0) {
          socket.send(JSON.stringify(['EVENT', id, current]));
          socket.send(JSON.stringify(['EVENT', id, current]));
          socket.send(
            JSON.stringify(['EVENT', id, { ...current, sig: '0'.repeat(128) }])
          );
          socket.send(JSON.stringify(['EOSE', id]));
        } else {
          socket.send(JSON.stringify(['EVENT', id, previous]));
          socket.send(
            JSON.stringify([
              'CLOSED',
              id,
              'HC_TEST_ONLY_PRIVATE_RAW_REASON <script>unsafe</script>'
            ])
          );
        }
      });
    });
  }
  await Promise.all(servers.map((server) => once(server, 'listening')));
  const ports = servers.map((server) => {
    const address = server.address();
    assert.ok(address && typeof address !== 'string');
    return address.port;
  });
  const windowDescriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    'window'
  );
  const originalSocket = globalThis.WebSocket;
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      const origin = String(url).replace(/\/$/, '');
      const index = origins.indexOf(origin);
      assert.notEqual(index, -1);
      super(`ws://127.0.0.1:${ports[index]}`);
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
      public: origins.map((origin) => ({
        origin,
        read: true,
        write: false,
        nip50: false
      })),
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  const pool = getPublicPool(policy)!;
  const store = getPublicStore()!;
  const ingress = createPublicIngress();
  const journal = createObservationJournal(policy);
  const result = createPublicRequestResult(policy, ingress, journal);
  const statuses: string[] = [];
  try {
    await new Promise<void>((resolve) =>
      subscribePublicPool(pool, [{ kinds: [30402], limit: 200 }], (message) => {
        const outcome = handlePublicRequestMessage(result, message);
        if (message.type === 'EVENT') statuses.push(outcome.status);
        if (outcome.status === 'accepted' || outcome.status === 'duplicate')
          insertPublicEnvelope(store, outcome.value);
        if (
          publicRequestSnapshot(result).sources.every(
            (row) => row.state === 'eose' || row.state === 'closed'
          )
        )
          resolve();
      })
    );
    const snapshot = publicRequestSnapshot(result);
    assert.equal(snapshot.coverage, 'partial');
    assert.equal(snapshot.definitiveAbsence, false);
    assert.equal(publicObservations(journal, snapshot.context).length, 2);
    assert.equal(
      exactPublicObservation(journal, snapshot.context, current.id, origins[0]),
      true
    );
    assert.equal(
      exactPublicObservation(journal, snapshot.context, current.id, origins[1]),
      false
    );
    assert.equal(
      exactPublicObservation(
        journal,
        snapshot.context,
        previous.id,
        origins[1]
      ),
      true
    );
    assert.ok(publicStoreEnvelope(store, current.id));
    assert.ok(publicStoreEnvelope(store, previous.id));
    assert.equal(publicIngressStats(ingress).deliveries, 4);
    assert.equal(statuses.filter((x) => x === 'accepted').length, 2);
    assert.equal(statuses.filter((x) => x === 'duplicate').length, 1);
    assert.equal(statuses.filter((x) => x === 'rejected').length, 1);
    assert.doesNotMatch(
      JSON.stringify(snapshot),
      /HC_TEST_ONLY_PRIVATE_RAW_REASON|script|unsafe/
    );
  } finally {
    disposePublicRequestResult(result);
    closePublicStore(store);
    closePublicPool(pool);
    for (const socket of sockets) socket.terminate();
    await Promise.all(
      servers.map(
        (server) =>
          new Promise<void>((resolve, reject) =>
            server.close((error) => (error ? reject(error) : resolve()))
          )
      )
    );
    globalThis.WebSocket = originalSocket;
    if (windowDescriptor)
      Object.defineProperty(globalThis, 'window', windowDescriptor);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});
