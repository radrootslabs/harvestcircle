import test from 'node:test';
import {
  publicRequestScopeSnapshot,
  publicRunSnapshot
} from '../../src/lib/nostr/request-scope.ts';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { finalizeEvent } from 'applesauce-core/helpers';
import WebSocket, { WebSocketServer } from 'ws';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  closePublicRuntime
} from '../../src/lib/runtime/public-runtime.ts';
import {
  createFoodSearchCoordinator,
  beginFoodSearch,
  subscribeFoodSearch,
  sampleFoodSearch,
  foodSearchRequestOwner,
  foodSearchSnapshot,
  cancelFoodSearch
} from '../../src/lib/catalog/search-run.ts';

await test('actual SDK qualified relevance sample closes at100 while ordinary fallback and latest/deletion work share the remaining run', async () => {
  const corpus = JSON.parse(
    await readFile(
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
  const key = crypto.getRandomValues(new Uint8Array(32));
  const previous = finalizeEvent(
    {
      kind: 30402,
      created_at: base.created_at,
      tags: base.tags,
      content: base.content
    },
    key
  );
  const current = finalizeEvent(
    {
      kind: 30402,
      created_at: base.created_at + 1,
      tags: base.tags.map((t) =>
        t[0] === 'status'
          ? ['status', 'sold']
          : t[0] === 'title'
            ? ['title', 'Renamed celery']
            : [...t]
      ),
      content: base.content
    },
    key
  );
  const deletion = finalizeEvent(
    {
      kind: 5,
      created_at: base.created_at - 1,
      tags: [['e', current.id]],
      content: ''
    },
    key
  );
  key.fill(0);
  const origin = 'wss://one.example.org',
    server = new WebSocketServer({
      host: '127.0.0.1',
      port: 0,
      maxPayload: 4096
    });
  const sockets: WebSocket[] = [];
  const filters: Record<string, unknown>[] = [];
  let sent = () => {};
  const sentBoth = new Promise<void>((resolve) => {
    sent = resolve;
  });
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
      const id = String(frame[1]),
        query = frame[2] as Record<string, unknown>;
      filters.push(query);
      const values =
        query.search !== undefined
          ? Array(101).fill(previous)
          : (query.kinds as number[])[0] === 30402
            ? query.authors === undefined
              ? [previous]
              : [current]
            : [deletion];
      for (const value of values)
        socket.send(JSON.stringify(['EVENT', id, value]));
      socket.send(JSON.stringify(['EOSE', id]));
      if (filters.length === 1) sent();
    });
  });
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const port = address.port;
  const oldWindow = Object.getOwnPropertyDescriptor(globalThis, 'window'),
    oldSocket = globalThis.WebSocket;
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      assert.ok(
        [origin, 'wss://two.example.org'].includes(
          String(url).replace(/\/$/, '')
        )
      );
      super(`ws://127.0.0.1:${port}`);
    }
  }
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {}
  });
  globalThis.WebSocket =
    FixtureSocket as unknown as typeof globalThis.WebSocket;
  const context = createPublicRuntimeContext();
  try {
    const policy = validateRelayPolicy(
      JSON.stringify({
        schemaVersion: 1,
        public: [
          { origin, read: true, write: false, nip50: true },
          {
            origin: 'wss://two.example.org',
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
    )!;
    const runtime = mountPublicRuntime(context, policy)!;
    const view = createPublicView(runtime);
    const coordinator = createFoodSearchCoordinator(view, {
      nowSeconds: () => base.created_at + 10
    });
    const generation = beginFoodSearch(coordinator, 'carrots');
    const samples = sampleFoodSearch(generation);
    assert.equal(samples.length, 1);
    await sentBoth;
    for (
      let n = 0;
      n < 100 && publicRequestScopeSnapshot(samples[0]).state === 'active';
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    assert.equal(
      publicRunSnapshot(foodSearchRequestOwner(generation)).ingress.deliveries,
      100
    );
    subscribeFoodSearch(generation, [{ kinds: [30402], limit: 200 }]);
    for (
      let n = 0;
      n < 100 &&
      (foodSearchSnapshot(coordinator)?.run?.activeRequests ?? 0) > 0;
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    assert.equal(filters.length, 7);
    assert.deepEqual(filters[0], {
      kinds: [30402],
      search: 'carrots',
      limit: 100
    });
    const head = filters.find(
      (q) => (q.kinds as number[])[0] === 30402 && q.authors !== undefined
    )!;
    const requests = filters.find((q) => (q.kinds as number[])[0] === 5)!;
    assert.deepEqual(head.authors, [previous.pubkey]);
    assert.deepEqual(head['#d'], [base.tags.find((t) => t[0] === 'd')![1]]);
    assert.deepEqual(requests.authors, [previous.pubkey]);
    for (const q of filters.slice(1)) {
      assert.equal(q.limit, 200);
      assert.equal('q' in q, false);
      assert.equal('search' in q, false);
      assert.equal('since' in q, false);
      assert.equal('#k' in q, false);
    }
    const snapshot = foodSearchSnapshot(coordinator)!;
    assert.equal(snapshot.query, 'carrots');
    assert.equal(snapshot.refresh, 'partial');
    assert.equal(snapshot.rows.length, 0);
    assert.equal(snapshot.definitiveAbsence, false);
    assert.equal(snapshot.run?.ingress.deliveries, 106);
    assert.equal(snapshot.run?.activeRequests, 0);
    assert.equal(snapshot.scopes.length, 4);
    cancelFoodSearch(generation);
  } finally {
    closePublicRuntime(context);
    key.fill(0);
    globalThis.WebSocket = oldSocket;
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow);
    else delete (globalThis as { window?: unknown }).window;
    for (const socket of sockets) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
  }
});
