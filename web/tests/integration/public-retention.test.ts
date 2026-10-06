import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { finalizeEvent } from 'applesauce-core/helpers';
import WebSocket, { WebSocketServer } from 'ws';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  beginPublicViewRun,
  closePublicRuntime,
  publicViewProjectionAvailable
} from '../../src/lib/runtime/public-runtime.ts';
import {
  getPublicStore,
  insertPublicEnvelope,
  publicStoreRetention
} from '../../src/lib/nostr/public-store.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import { publicRunSnapshot } from '../../src/lib/nostr/request-scope.ts';
import { createPublicHeadCandidate } from '../../src/lib/catalog/heads.ts';
import {
  createPublicViewHeadResolver,
  resolveHeads,
  headResolutionSnapshot,
  headResolverRetentionState
} from '../../src/lib/catalog/resolve-head.ts';

await test('actual shared SDK retention carries lifecycle evidence across runs and coherently stops all public views at 32MiB', async () => {
  const key = crypto.getRandomValues(new Uint8Array(32));
  const signed = (time: number, tags: string[][], content = '', kind = 30402) =>
    finalizeEvent({ kind, created_at: time, tags, content }, key);
  const proof = (event: ReturnType<typeof signed>) => {
    const p = verifyEnvelope(JSON.stringify(event));
    assert.ok(p.ok);
    return p.value;
  };
  const old = signed(100, [['d', 'item']]),
    newer = signed(101, [
      ['d', 'item'],
      ['status', 'unknown']
    ]);
  const deleted = signed(1, [['e', newer.id]], '', 5);
  const privateWork = {
    unsent: 'private memory owned separately',
    outbox: ['unfinished']
  };
  const originalPrivate = structuredClone(privateWork);
  const origin = 'wss://one.example.org',
    server = new WebSocketServer({
      host: '127.0.0.1',
      port: 0,
      maxPayload: 4096
    });
  const sockets: WebSocket[] = [],
    requests: { socket: WebSocket; id: string; kind: number }[] = [],
    closes: string[] = [];
  server.on('connection', (socket) => {
    sockets.push(socket);
    socket.on('message', (bytes) => {
      const buffer = Array.isArray(bytes)
        ? Buffer.concat(bytes)
        : Buffer.isBuffer(bytes)
          ? bytes
          : Buffer.from(bytes);
      const frame = JSON.parse(buffer.toString('utf8')) as unknown[];
      if (frame[0] === 'CLOSE') closes.push(String(frame[1]));
      if (frame[0] !== 'REQ') return;
      const kind = (frame[2] as { kinds: number[] }).kinds[0],
        id = String(frame[1]);
      requests.push({ socket, id, kind });
      if (kind === 30402) socket.send(JSON.stringify(['EVENT', id, old]));
      if (requests.length <= 4) socket.send(JSON.stringify(['EOSE', id]));
    });
  });
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const port = address.port;
  const priorWindow = Object.getOwnPropertyDescriptor(globalThis, 'window'),
    priorSocket = globalThis.WebSocket;
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
  const settle = async (predicate: () => boolean) => {
    for (let n = 0; n < 200 && !predicate(); n++)
      await new Promise((r) => setTimeout(r, 5));
    assert.ok(predicate());
  };
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
    const runtime = mountPublicRuntime(context, policy)!,
      store = getPublicStore()!;
    for (const event of [old, deleted, newer])
      assert.equal(insertPublicEnvelope(store, proof(event)), 'accepted');
    const head = createPublicHeadCandidate(proof(old))!;
    const views = [createPublicView(runtime), createPublicView(runtime)],
      runs = views.map((v) => beginPublicViewRun(v));
    const resolvers = views.map((v, i) =>
      createPublicViewHeadResolver(v, runs[i], { nowSeconds: () => 200 })
    );
    for (const resolver of resolvers) {
      resolveHeads(resolver, [head]);
      const row = headResolutionSnapshot(resolver)[0];
      assert.equal(row.state.head.id, newer.id);
      assert.equal(row.deletion.outcome, 'suppressed');
    }
    await settle(
      () =>
        requests.length === 4 &&
        resolvers.every(
          (r) => headResolutionSnapshot(r)[0].coverage === 'bounded-eose'
        )
    );
    for (const resolver of resolvers)
      assert.equal(
        headResolutionSnapshot(resolver)[0].deletion.outcome,
        'suppressed'
      );
    // Fill the genuine shared owner to its real inclusive limit; no toy cap.
    const cap = 33554432;
    let time = 1000;
    for (;;) {
      const tags = [['d', `bulk-${time}`]],
        empty = signed(time, tags),
        remaining = cap - publicStoreRetention(store).payloadBytes,
        overhead = Buffer.byteLength(JSON.stringify(empty)),
        length = Math.min(131000, remaining - overhead);
      assert.ok(length >= 0);
      assert.equal(
        insertPublicEnvelope(
          store,
          proof(signed(time, tags, 'a'.repeat(length)))
        ),
        'accepted'
      );
      time++;
      if (publicStoreRetention(store).payloadBytes === cap) break;
    }
    assert.equal(publicStoreRetention(store).stopped, false);
    // Fresh actual SDK requests remain pending while overflow arrives on wire.
    const nextRuns = views.map((v) => beginPublicViewRun(v));
    const nextResolvers = views.map((v, i) =>
      createPublicViewHeadResolver(v, nextRuns[i], { nowSeconds: () => 200 })
    );
    for (const resolver of nextResolvers) resolveHeads(resolver, [head]);
    await settle(() => requests.length === 8);
    const source = requests.slice(4).find((r) => r.kind === 30402)!;
    source.socket.send(
      JSON.stringify(['EVENT', source.id, signed(time, [['d', 'item']])])
    );
    await settle(() => publicStoreRetention(store).stopped);
    for (let i = 0; i < views.length; i++) {
      assert.equal(publicViewProjectionAvailable(views[i]), false);
      assert.deepEqual(headResolutionSnapshot(nextResolvers[i]), []);
      assert.deepEqual(headResolutionSnapshot(resolvers[i]), []);
      assert.deepEqual(headResolverRetentionState(nextResolvers[i]), {
        available: false,
        definitiveAbsence: false
      });
      assert.equal(publicRunSnapshot(nextRuns[i]).active, false);
      assert.equal(publicRunSnapshot(nextRuns[i]).activeRequests, 0);
    }
    await settle(() => requests.slice(4).every((r) => closes.includes(r.id)));
    source.socket.send(JSON.stringify(['EVENT', source.id, old]));
    assert.equal(insertPublicEnvelope(store, proof(old)), 'limit');
    assert.equal(publicStoreRetention(store).payloadBytes, cap);
    assert.deepEqual(privateWork, originalPrivate);
    assert.throws(() => beginPublicViewRun(views[0]));
    closePublicRuntime(context);
    assert.equal(headResolverRetentionState(nextResolvers[0]).available, false);
  } finally {
    closePublicRuntime(context);
    key.fill(0);
    globalThis.WebSocket = priorSocket;
    if (priorWindow) Object.defineProperty(globalThis, 'window', priorWindow);
    else delete (globalThis as { window?: unknown }).window;
    for (const socket of sockets) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((e) => (e ? reject(e) : resolve()))
    );
  }
});
