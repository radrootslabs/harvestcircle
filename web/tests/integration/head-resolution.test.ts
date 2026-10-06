import test from 'node:test';
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
  beginPublicViewRun,
  closePublicRuntime
} from '../../src/lib/runtime/public-runtime.ts';
import { publicRunSnapshot } from '../../src/lib/nostr/request-scope.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import { createPublicHeadCandidate } from '../../src/lib/catalog/heads.ts';
import {
  createPublicViewHeadResolver,
  resolveHeads,
  headResolutionSnapshot
} from '../../src/lib/catalog/resolve-head.ts';

await test('actual SDK exact-coordinate and author deletion queries share one bounded run and retain suppressed newest evidence', async () => {
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
        (query.kinds as number[])[0] === 30402
          ? [previous, current]
          : [deletion];
      for (const value of values)
        socket.send(JSON.stringify(['EVENT', id, value]));
      socket.send(JSON.stringify(['EOSE', id]));
      if (filters.length === 2) sent();
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
      assert.equal(String(url).replace(/\/$/, ''), origin);
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
        public: [{ origin, read: true, write: false, nip50: false }],
        inbox: [],
        postingEnabled: false,
        messagingEnabled: false,
        operatorDenylist: []
      })
    )!;
    const runtime = mountPublicRuntime(context, policy)!;
    const view = createPublicView(runtime),
      run = beginPublicViewRun(view);
    const proof = verifyEnvelope(JSON.stringify(previous));
    assert.ok(proof.ok);
    const initial = createPublicHeadCandidate(proof.value);
    assert.ok(initial);
    const resolver = createPublicViewHeadResolver(view, run, {
      nowSeconds: () => base.created_at + 10
    });
    resolveHeads(resolver, [initial, initial]);
    resolveHeads(resolver, [initial]);
    await sentBoth;
    for (
      let n = 0;
      n < 100 &&
      headResolutionSnapshot(resolver)[0].coverage !== 'bounded-eose';
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    assert.equal(filters.length, 2);
    const head = filters.find((q) => (q.kinds as number[])[0] === 30402)!;
    const requests = filters.find((q) => (q.kinds as number[])[0] === 5)!;
    assert.deepEqual(head.authors, [previous.pubkey]);
    assert.deepEqual(head['#d'], [base.tags.find((t) => t[0] === 'd')![1]]);
    assert.deepEqual(requests.authors, [previous.pubkey]);
    for (const q of filters) {
      assert.equal(q.limit, 200);
      assert.equal('q' in q, false);
      assert.equal('search' in q, false);
      assert.equal('since' in q, false);
      assert.equal('#k' in q, false);
    }
    const row = headResolutionSnapshot(resolver)[0];
    assert.equal(row.coverage, 'bounded-eose');
    assert.equal(row.lastKnown, false);
    assert.equal(row.state.head.id, current.id);
    assert.equal(row.state.food, undefined);
    assert.equal(row.deletion.outcome, 'suppressed');
    assert.equal(row.deletionProofs.length, 1);
    assert.equal(row.definitiveAbsence, false);
    assert.equal(publicRunSnapshot(run).ingress.deliveries, 3);
    assert.equal(publicRunSnapshot(run).activeRequests, 0);
    closePublicRuntime(context);
    assert.equal(headResolutionSnapshot(resolver)[0].lastKnown, true);
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
