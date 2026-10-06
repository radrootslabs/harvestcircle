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
  closePublicRuntime
} from '../../src/lib/runtime/public-runtime.ts';
import {
  createFoodSearchCoordinator,
  beginFoodSearch,
  chronologicalFoodSearch,
  foodSearchSnapshot,
  resolveFoodSearchPublishers,
  showMoreFoodSearch,
  cancelFoodSearch
} from '../../src/lib/catalog/search-run.ts';
await test('actual SDK listings request only twenty displayed publishers, keep asserted text separate and share lifecycle budgets', async () => {
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
  const keys: Uint8Array[] = [];
  const listings = Array.from({ length: 21 }, (_, n) => {
    const key = crypto.getRandomValues(new Uint8Array(32));
    keys.push(key);
    return finalizeEvent(
      {
        kind: 30402,
        created_at: base.created_at - n,
        tags: base.tags.map((t) =>
          t[0] === 'd' ? ['d', `listing_${n}`] : [...t]
        ),
        content: base.content
      },
      key
    );
  });
  const profiles = keys.map((key, n) =>
    finalizeEvent(
      {
        kind: 0,
        created_at: base.created_at,
        tags: [],
        content: JSON.stringify({
          display_name: `<script>Farmer ${n}</script>`,
          pubkey: 'forged',
          nip05: 'identity@example.org',
          picture: 'https://tracking.example.org'
        })
      },
      key
    )
  );
  for (const key of keys) key.fill(0);
  const origin = 'wss://one.example.org',
    server = new WebSocketServer({
      host: '127.0.0.1',
      port: 0,
      maxPayload: 65536
    });
  const sockets: WebSocket[] = [],
    filters: Record<string, unknown>[] = [];
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
        queries = frame.slice(2) as Record<string, unknown>[];
      filters.push(...queries);
      const kind = (queries[0].kinds as number[])[0];
      const matches = (pubkey: string, identifier?: string) =>
        queries.some((q) => {
          const authors = q.authors as string[] | undefined;
          return (
            !authors ||
            (authors.includes(pubkey) &&
              (identifier === undefined ||
                (q['#d'] as string[]).includes(identifier)))
          );
        });
      const values =
        kind === 0
          ? profiles.filter((v) => matches(v.pubkey))
          : kind === 30402
            ? listings.filter((v) =>
                matches(v.pubkey, v.tags.find((t) => t[0] === 'd')![1])
              )
            : [];
      for (const value of values)
        socket.send(JSON.stringify(['EVENT', id, value]));
      socket.send(JSON.stringify(['EOSE', id]));
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
    const view = createPublicView(mountPublicRuntime(context, policy)!);
    const coordinator = createFoodSearchCoordinator(view, {
        nowSeconds: () => base.created_at + 10
      }),
      token = beginFoodSearch(coordinator, '');
    chronologicalFoodSearch(token);
    for (
      let n = 0;
      n < 200 && foodSearchSnapshot(coordinator)?.refresh !== 'bounded-eose';
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    const before = foodSearchSnapshot(coordinator)!;
    assert.equal(before.listings.length, 20);
    assert.equal(before.hasMore, true);
    assert.deepEqual(
      before.listings.map((v) => v.eventId),
      listings.slice(0, 20).map((v) => v.id)
    );
    assert.ok(
      before.listings.every((v) => v.publisher.label === v.publisher.pubkey)
    );
    resolveFoodSearchPublishers(token);
    resolveFoodSearchPublishers(token);
    for (
      let n = 0;
      n < 200 &&
      !foodSearchSnapshot(coordinator)?.listings.every(
        (v) => v.publisher.assertedName
      );
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    const first = foodSearchSnapshot(coordinator)!;
    const profileQueries = filters.filter(
      (q) => (q.kinds as number[])[0] === 0
    );
    assert.equal(profileQueries.length, 1);
    assert.equal((profileQueries[0].authors as string[]).length, 20);
    assert.equal(profileQueries[0].limit, 200);
    assert.deepEqual(
      first.listings.map((v) => v.publisher.pubkey),
      listings.slice(0, 20).map((v) => v.pubkey)
    );
    assert.ok(
      first.listings.every(
        (v) =>
          v.publisher.assertedName && v.publisher.label.startsWith('<script>')
      )
    );
    assert.equal(first.run?.ingress.deliveries, 62);
    assert.equal(first.run?.activeRequests, 0);
    const count = filters.length;
    showMoreFoodSearch(token);
    assert.equal(filters.length, count);
    assert.equal(foodSearchSnapshot(coordinator)!.listings.length, 21);
    resolveFoodSearchPublishers(token);
    for (
      let n = 0;
      n < 200 &&
      !foodSearchSnapshot(coordinator)?.listings[20].publisher.assertedName;
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 5));
    const all = foodSearchSnapshot(coordinator)!;
    assert.equal(all.run?.ingress.deliveries, 63);
    const queries = filters.filter((q) => (q.kinds as number[])[0] === 0);
    assert.equal(queries.length, 2);
    assert.deepEqual(queries[1].authors, [listings[20].pubkey]);
    assert.equal(all.definitiveAbsence, false);
    assert.ok(
      all.listings.every(
        (v) => v.price.amount === before.listings[0].price.amount
      )
    );
    cancelFoodSearch(token);
    assert.throws(() => resolveFoodSearchPublishers(token));
  } finally {
    closePublicRuntime(context);
    for (const key of keys) key.fill(0);
    globalThis.WebSocket = oldSocket;
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow);
    else delete (globalThis as { window?: unknown }).window;
    for (const socket of sockets) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((e) => (e ? reject(e) : resolve()))
    );
  }
});
