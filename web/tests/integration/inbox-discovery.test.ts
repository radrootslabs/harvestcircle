import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import WebSocket, { WebSocketServer } from 'ws';
import { finalizeEvent } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  beginPublicViewRun,
  subscribePublicView,
  closePublicRuntime
} from '../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  inboxResolutionSnapshot
} from '../../src/lib/messaging/resolve-inbox.ts';
import { inboxPreferenceWire } from '../../src/lib/nostr/inbox-preferences.ts';
import { publicRunSnapshot } from '../../src/lib/nostr/request-scope.ts';

void test('actual anonymous SDK discovers only fixed recipient10050 sources and preserves decoded extras independently of public catalog', async () => {
  const origins = ['wss://one.example.org', 'wss://two.example.org'];
  const key = crypto.getRandomValues(new Uint8Array(32));
  let old, newer, profile;
  try {
    old = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [
          ['relay', origins[0]],
          ['unknown', 'keep']
        ],
        content: 'original'
      },
      key
    );
    newer = finalizeEvent(
      { kind: 10050, created_at: 200, tags: [['relay']], content: '' },
      key
    );
    profile = finalizeEvent(
      { kind: 0, created_at: 100, tags: [], content: '{}' },
      key
    );
  } finally {
    key.fill(0);
  }
  const servers = origins.map(
    () => new WebSocketServer({ host: '127.0.0.1', port: 0, maxPayload: 8192 })
  );
  const sockets: WebSocket[] = [],
    requests: { origin: string; filter: unknown }[] = [];
  let closed = 0,
    catalogSeen = () => {},
    allClosed = () => {},
    nextClosed = () => {};
  const seen = [0, 0];
  const catalog = new Promise<void>((resolve) => {
      catalogSeen = resolve;
    }),
    finished = new Promise<void>((resolve) => {
      allClosed = resolve;
    }),
    nextFinished = new Promise<void>((resolve) => {
      nextClosed = resolve;
    });
  for (let index = 0; index < servers.length; index++)
    servers[index].on('connection', (socket) => {
      sockets.push(socket);
      socket.on('message', (bytes) => {
        const buffer = Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.isBuffer(bytes)
            ? bytes
            : Buffer.from(bytes);
        const frame = JSON.parse(buffer.toString('utf8')) as unknown[];
        if (frame[0] === 'REQ') {
          const filter = frame[2] as {
            kinds: number[];
            authors?: string[];
            limit: number;
          };
          requests.push({ origin: origins[index], filter });
          const id = String(frame[1]);
          if (filter.kinds[0] === 10050) {
            seen[index]++;
            assert.deepEqual(filter, {
              kinds: [10050],
              authors: [old.pubkey],
              limit: 200
            });
            socket.send(
              JSON.stringify([
                'EVENT',
                id,
                seen[index] > 1 || index === 0
                  ? { ...old, extra: { items: ['keep', null, true] } }
                  : newer
              ])
            );
            socket.send(JSON.stringify(['EOSE', id]));
          } else {
            socket.send(JSON.stringify(['EVENT', id, profile]));
          }
        } else if (frame[0] === 'CLOSE') {
          closed++;
          if (closed === 2) allClosed();
          if (closed === 4) nextClosed();
        }
        assert.notEqual(frame[0], 'AUTH');
        assert.notEqual(frame[0], 'EVENT');
      });
    });
  await Promise.all(servers.map((server) => once(server, 'listening')));
  const ports = servers.map((server) => {
    const address = server.address();
    assert.ok(address && typeof address !== 'string');
    return address.port;
  });
  const oldWindow = Object.getOwnPropertyDescriptor(globalThis, 'window'),
    oldSocket = globalThis.WebSocket;
  class FixtureSocket extends WebSocket {
    constructor(url: string | URL) {
      const index = origins.indexOf(String(url).replace(/\/$/, ''));
      assert.ok(index >= 0);
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
      inbox: [{ origin: 'wss://archive.example.org', read: true, write: true }],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  const context = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(context, policy)!,
    view = createPublicView(runtime),
    run = beginPublicViewRun(view),
    catalogView = createPublicView(runtime),
    catalogRun = beginPublicViewRun(catalogView);
  try {
    subscribePublicView(
      catalogView,
      catalogRun,
      'profile',
      [{ kinds: [0], authors: [old.pubkey], limit: 200 }],
      () => catalogSeen()
    );
    const resolver = resolveInboxPreference(view, run, old.pubkey);
    await Promise.all([finished, catalog]);
    const result = inboxResolutionSnapshot(resolver);
    assert.equal(result.status, 'unsupported');
    assert.equal(result.knownBase?.id, newer.id);
    assert.equal(result.coverage, 'bounded-eose');
    assert.equal(result.sources.length, 2);
    assert.equal(publicRunSnapshot(run).ingress.deliveries, 2);
    assert.ok(publicRunSnapshot(catalogRun).active);
    const another = beginPublicViewRun(view);
    assert.equal(inboxResolutionSnapshot(resolver).status, 'inconclusive');
    assert.ok(publicRunSnapshot(another).active);
    const compatible = resolveInboxPreference(view, another, old.pubkey);
    assert.equal(resolveInboxPreference(view, another, old.pubkey), compatible);
    await nextFinished;
    const compatibleResult = inboxResolutionSnapshot(compatible);
    assert.equal(compatibleResult.status, 'ready');
    assert.equal(compatibleResult.knownBase?.id, old.id);
    const preserved: unknown = JSON.parse(
      inboxPreferenceWire(compatibleResult.head!)!
    );
    assert.deepEqual(preserved, {
      ...(JSON.parse(JSON.stringify(old)) as object),
      extra: { items: ['keep', null, true] }
    });
    assert.equal(
      requests.filter(
        (row) => (row.filter as { kinds: number[] }).kinds[0] === 10050
      ).length,
      4
    );
    assert.equal(result.wireProvenance, 'decoded-sdk-json');
    assert.ok(inboxPreferenceWire(result.head!)?.includes('"relay"'));
  } finally {
    closePublicRuntime(context);
    for (const socket of sockets) socket.terminate();
    await Promise.all(
      servers.map(
        (server) =>
          new Promise<void>((resolve, reject) =>
            server.close((error) => (error ? reject(error) : resolve()))
          )
      )
    );
    globalThis.WebSocket = oldSocket;
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});
