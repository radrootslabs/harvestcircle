import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import WebSocket, { WebSocketServer } from 'ws';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  beginPublicViewRun,
  subscribePublicView,
  publicViewRunCurrent,
  closePublicRuntime
} from '../../src/lib/runtime/public-runtime.ts';
import {
  publicRunSnapshot,
  publicRequestScopeSnapshot,
  publicRunObservations,
  type RequestClock
} from '../../src/lib/nostr/request-scope.ts';
await test('actual SDK request ownership suppresses superseded results and preserves another view through its absolute deadline', async () => {
  const corpus = JSON.parse(
    await readFile(
      new URL(
        '../../../contracts/interop/food_availability/corpus.v1.json',
        import.meta.url
      ),
      'utf8'
    )
  ) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
  const wires = corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires,
    current = JSON.parse(wires.current) as { id: string },
    previous = JSON.parse(wires.previous) as { id: string };
  const origin = 'wss://one.example.org',
    server = new WebSocketServer({
      host: '127.0.0.1',
      port: 0,
      maxPayload: 4096
    });
  const sockets: WebSocket[] = [];
  const ids: string[] = [];
  let socket: WebSocket | undefined;
  let firstClose = () => {};
  const closedFirst = new Promise<void>((resolve) => {
    firstClose = resolve;
  });
  server.on('connection', (connection) => {
    sockets.push(connection);
    socket = connection;
    connection.on('message', (bytes) => {
      const buffer = Array.isArray(bytes)
        ? Buffer.concat(bytes)
        : Buffer.isBuffer(bytes)
          ? bytes
          : Buffer.from(bytes);
      const frame = JSON.parse(buffer.toString('utf8')) as unknown[];
      if (frame[0] === 'REQ') {
        const id = String(frame[1]);
        ids.push(id);
        connection.send(
          JSON.stringify(['EVENT', id, ids.length === 1 ? current : previous])
        );
      } else if (frame[0] === 'CLOSE' && frame[1] === ids[0]) firstClose();
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
  let now = 0;
  const timers = new Map<() => void, { at: number; callback: () => void }>();
  const clock: RequestClock = {
    now: () => now,
    schedule(callback, delay) {
      const cancel = () => {
        timers.delete(cancel);
      };
      timers.set(cancel, { at: now + delay, callback });
      return cancel;
    }
  };
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
  const context = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(context, policy, clock)!;
  const a = createPublicView(runtime),
    b = createPublicView(runtime),
    ra = beginPublicViewRun(a),
    rb = beginPublicViewRun(b);
  let writesA = 0,
    writesB = 0;
  let firstA = () => {},
    firstB = () => {},
    secondB = () => {};
  const initialA = new Promise<void>((resolve) => {
      firstA = resolve;
    }),
    initialB = new Promise<void>((resolve) => {
      firstB = resolve;
    }),
    continuedB = new Promise<void>((resolve) => {
      secondB = resolve;
    });
  try {
    const ha = subscribePublicView(
        a,
        ra,
        'search',
        [{ kinds: [30402], limit: 1 }],
        () => {
          writesA++;
          firstA();
        }
      ),
      hb = subscribePublicView(
        b,
        rb,
        'profile',
        [{ kinds: [30402], limit: 2 }],
        () => {
          writesB++;
          if (writesB === 1) firstB();
          else secondB();
        }
      );
    await Promise.all([initialA, initialB]);
    assert.equal(ids.length, 2);
    const next = beginPublicViewRun(a);
    await closedFirst;
    assert.equal(publicViewRunCurrent(a, ra), false);
    assert.equal(publicViewRunCurrent(a, next), true);
    assert.equal(publicRequestScopeSnapshot(ha).state, 'cancelled');
    assert.equal(publicRequestScopeSnapshot(hb).state, 'active');
    assert.ok(socket);
    socket.send(JSON.stringify(['EVENT', ids[0], previous]));
    socket.send(JSON.stringify(['EVENT', ids[1], previous]));
    await continuedB;
    assert.equal(writesA, 1);
    assert.equal(writesB, 2);
    assert.equal(publicRunSnapshot(ra).ingress.deliveries, 1);
    assert.equal(publicRunSnapshot(rb).ingress.deliveries, 2);
    assert.equal(publicRunObservations(rb, hb).length, 1);
    now = 10000;
    for (const [cancel, row] of [...timers])
      if (row.at <= now) {
        cancel();
        row.callback();
      }
    assert.equal(publicRequestScopeSnapshot(hb).state, 'deadline');
    assert.equal(
      publicRequestScopeSnapshot(hb).result.definitiveAbsence,
      false
    );
    assert.equal(timers.size, 0);
  } finally {
    closePublicRuntime(context);
    for (const connection of sockets) connection.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    globalThis.WebSocket = oldSocket;
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});
