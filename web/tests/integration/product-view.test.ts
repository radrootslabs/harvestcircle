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
  createProductView,
  productViewSnapshot,
  closeProductView
} from '../../src/lib/catalog/product-view.ts';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
await test('actual SDK direct coordinate detail publishes genuine facts and asserted metadata with bounded teardown', async () => {
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
    const updates: string[] = [];
    const target = listings[0];
    const naddr = encodeProductReference({
      kind: 30402,
      pubkey: target.pubkey,
      identifier: target.tags.find((t) => t[0] === 'd')![1]
    })!;
    const owner = createProductView(
      view,
      naddr,
      { nowSeconds: () => base.created_at + 10 },
      (snapshot) => updates.push(snapshot.outcome)
    );
    for (
      let n = 0;
      n < 300 &&
      (!productViewSnapshot(owner).publisher.assertedName ||
        productViewSnapshot(owner).outcome !== 'active');
      n++
    )
      await new Promise((resolve) => setTimeout(resolve, 10));
    const first = productViewSnapshot(owner);
    assert.equal(first.outcome, 'active');
    assert.equal(first.eventId, target.id);
    assert.equal(
      first.food?.title,
      base.tags.find((t) => t[0] === 'title')![1]
    );
    assert.equal(first.publisher.pubkey, target.pubkey);
    assert.equal(first.publisher.assertedName, true);
    for (let n = 0; n < 300 && !updates.includes('active'); n++)
      await new Promise((resolve) => setTimeout(resolve, 10));
    assert.ok(updates.includes('active'));
    assert.ok(filters.length >= 3);
    assert.ok(
      filters.every(
        (f) =>
          Array.isArray(f.authors) &&
          (f.authors as string[]).length === 1 &&
          (f.authors as string[])[0] === target.pubkey
      )
    );
    closeProductView(owner);
    const settled = filters.length;
    await new Promise((resolve) => setTimeout(resolve, 300));
    assert.equal(filters.length, settled);
    assert.equal(productViewSnapshot(owner).outcome, 'unavailable');
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
