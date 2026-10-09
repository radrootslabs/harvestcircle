import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/stop-send.ts';
declare global {
  interface Window {
    hcp089: typeof Fixture;
    hcp089Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
    hcp089Pending: ReturnType<
      Awaited<ReturnType<typeof Fixture.makeFixture>>['run']
    >;
    hcp089Advance: () => void;
    hcp089Release: () => void;
    hcp089Waiting: boolean;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: { origin: string; wire: string }[] = [],
  response: 'accepted' | 'refused' | 'unknown' | 'silent' = 'accepted';
let archiveResponse: 'accepted' | 'refused' | 'unknown' | 'silent' | undefined;
let pendingAcks: { socket: import('ws').WebSocket; id: string }[] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string')
    throw Error('loopback unavailable');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] !== 'EVENT') return;
      const event = frame[1] as { id: string };
      pendingAcks.push({ socket, id: event.id });
      events.push({
        origin: new URL(request.url!, 'http://localhost').searchParams.get(
          'origin'
        )!,
        wire: JSON.stringify(event)
      });
      const outcome =
        events.at(-1)!.origin === 'wss://archive.example.org'
          ? (archiveResponse ?? response)
          : response;
      if (outcome !== 'silent')
        socket.send(
          JSON.stringify([
            'OK',
            event.id,
            outcome === 'accepted',
            outcome === 'unknown'
              ? 'Timeout'
              : outcome === 'refused'
                ? 'auth-required: fixture refusal'
                : ''
          ])
        );
    })
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/stop-send.ts', import.meta.url)
        ),
        name: 'hcp089',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle unavailable');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
async function load(page: import('@playwright/test').Page, initialise = true) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (initialise) => {
    window.hcp089Fixture = await window.hcp089.makeFixture(initialise);
  }, initialise);
}
test.beforeEach(async ({ page }) => {
  events = [];
  response = 'accepted';
  archiveResponse = undefined;
  pendingAcks = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            !['wss://peer.example.org', 'wss://archive.example.org'].includes(
              origin
            )
          )
            throw Error('unapproved destination');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
      };
    },
    { endpoint }
  );
  await load(page);
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp089Fixture?.close());
});
test('Stop after an actual silent EVENT preserves original uncertain target receipt', async ({
  page
}) => {
  response = 'silent';
  const before = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    const before = await f.row();
    window.hcp089Pending = f.run();
    return before;
  });
  await expect.poll(() => events.length).toBe(1);
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.stop();
    return {
      result: await window.hcp089Pending,
      row: await f.row(),
      snapshot: f.snapshot()
    };
  });
  expect(r.result.status).toBe('stopped');
  expect(events).toHaveLength(1);
  expect(r.row.record.self).toEqual(before.record.self);
  expect(r.row.record.peerArtifact).toEqual(before.record.peerArtifact);
  expect(r.row.record.receipts).toHaveLength(1);
  expect(r.row.record.receipts![0]).toMatchObject({
    role: 'peer',
    origin: 'wss://peer.example.org',
    eventId: before.record.peerArtifact!.eventId,
    status: 'stopped',
    attempt: 1
  });
});
test('disconnect during actual publish retains original-owner uncertainty without archive or auto resume', async ({
  page
}) => {
  response = 'silent';
  const before = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    const before = await f.row();
    window.hcp089Pending = f.run();
    return before;
  });
  await expect.poll(() => events.length).toBe(1);
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.disconnect();
    const result = await window.hcp089Pending;
    return {
      result,
      row: await f.row(),
      recapture: f.capture(),
      rerun: await f.run()
    };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.recapture).toBe(false);
  expect(r.rerun.status).toBe('invalid');
  expect(events).toHaveLength(1);
  expect(r.row.record.owner).toBe(before.record.owner);
  expect(r.row.record.self).toEqual(before.record.self);
  expect(r.row.record.peerArtifact).toEqual(before.record.peerArtifact);
  expect(r.row.record.receipts).toHaveLength(1);
  expect(r.row.record.receipts![0].status).toBe('stopped');
});
test('Stop before execution preserves exact bytes with no attempted fact or EVENT', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    const before = await f.row();
    f.stop();
    return { before, after: await f.row(), result: await f.run() };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.after.wire).toBe(r.before.wire);
  expect(events).toEqual([]);
});
test('late ACK after Stop cannot erase uncertainty or publish sender archive', async ({
  page
}) => {
  response = 'silent';
  await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    window.hcp089Pending = f.run();
  });
  await expect.poll(() => events.length).toBe(1);
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.stop();
    await window.hcp089Pending;
    return f.row();
  });
  for (const ack of pendingAcks)
    if (ack.socket.readyState === 1)
      ack.socket.send(JSON.stringify(['OK', ack.id, true, '']));
  const after = await page.evaluate(() => window.hcp089Fixture.row());
  expect(r.record.receipts).toHaveLength(1);
  expect(r.record.receipts![0].status).toBe('stopped');
  expect(after.wire).toBe(r.wire);
  expect(events).toHaveLength(1);
});
test('real accepted SDK result crossing Stop and logout during receipt read preserves acceptance', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    let acknowledged = false,
      stopped = false;
    // Observe the actual SDK socket's OK before the receipt read. The native
    // transport still receives and verifies the same frame through Applesauce.
    const cursor = Reflect.get<IDBIndex, 'openCursor'>(
      IDBIndex.prototype,
      'openCursor'
    );
    const Native = window.WebSocket;
    window.WebSocket = class extends Native {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        this.addEventListener('message', (event) => {
          const frame = JSON.parse(String(event.data)) as unknown[];
          if (frame[0] === 'OK' && frame[2] === true) acknowledged = true;
        });
      }
    };
    IDBIndex.prototype.openCursor = function (
      ...args: Parameters<IDBIndex['openCursor']>
    ) {
      if (
        acknowledged &&
        !stopped &&
        this.objectStore.name === 'private_sends'
      ) {
        stopped = true;
        f.stop();
        f.disconnect();
      }
      return cursor.apply(this, args);
    };
    try {
      window.hcp089Pending = f.run();
      await window.hcp089Pending;
    } finally {
      IDBIndex.prototype.openCursor = cursor;
      window.WebSocket = Native;
    }
    window.hcp089Waiting = stopped;
  });
  const r = await page.evaluate(async () => ({
    stopped: window.hcp089Waiting,
    result: await window.hcp089Pending,
    row: await window.hcp089Fixture.row(),
    status: await window.hcp089Fixture.sendStatus()
  }));
  expect(r.stopped).toBe(true);
  expect(r.result.status).toBe('stopped');
  expect(events).toHaveLength(1);
  expect(r.row.record.receipts).toHaveLength(1);
  expect(r.row.record.receipts![0].status).toBe('accepted');
  expect(r.status!.recipient.acceptedTargets).toEqual([
    'wss://peer.example.org'
  ]);
  expect(r.status!.archive.state).toBe('pending');
});
test('actual original-owner settlement survives logout, is idempotent and rejects a cloned result', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    const before = await f.row();
    const result = await f.publishObserved();
    f.disconnect();
    const counts = f.countsValue();
    const clone = await f.settleObserved(true),
      first = await f.settleObserved(),
      after = await f.row(),
      again = await f.settleObserved(),
      final = await f.row();
    return {
      result,
      before,
      clone,
      first: first.status,
      again: again.status,
      after,
      final,
      counts,
      countsAfter: f.countsValue()
    };
  });
  expect(r.result.status).toBe('accepted');
  expect(r.clone.status).toBe('invalid');
  expect(r.first).toBe('saved');
  expect(r.again).toBe('existing');
  expect(r.after.wire).toBe(r.final.wire);
  expect(r.after.record.receipts).toHaveLength(1);
  expect(r.after.record.owner).toBe(r.before.record.owner);
  expect(r.after.record.self).toEqual(r.before.record.self);
  expect(r.after.record.peerArtifact).toEqual(r.before.record.peerArtifact);
  expect(r.countsAfter).toEqual(r.counts);
  expect(events).toHaveLength(1);
});
test('late actual result cannot attach to a different immutable operation scope', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    await f.publishObserved();
    const wire = await f.replaceImmutableScope();
    return { wire, result: await f.settleObserved(), after: await f.row() };
  });
  expect(r.result.status).toBe('conflict');
  expect(r.after.wire).toBe(r.wire);
  expect(r.after.record.receipts).toBeUndefined();
  expect(events).toHaveLength(1);
});
test('real settlement IDB abort is not durable success and explicit metadata retry preserves the original result', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    await f.publishObserved();
    const before = await f.row();
    f.disconnect();
    f.faultSettlement('abort');
    const failed = await f.settleObserved();
    f.clearFault();
    const after = await f.row();
    const retried = await f.settleObserved();
    return {
      before,
      after,
      failed,
      retried: retried.status,
      row: await f.row()
    };
  });
  expect(r.failed.status).toBe('aborted');
  expect(r.after.wire).toBe(r.before.wire);
  expect(r.retried).toBe('saved');
  expect(r.row.record.receipts).toHaveLength(1);
  expect(events).toHaveLength(1);
});
test('real post-put settlement readback failure remains unknown despite committed bytes', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    await f.publishObserved();
    f.disconnect();
    f.faultSettlement('readback');
    const failed = await f.settleObserved();
    f.clearFault();
    const retried = await f.settleObserved();
    return { failed, retried: retried.status, row: await f.row() };
  });
  expect(r.failed.status).toBe('unknown_completion');
  expect(r.retried).toBe('existing');
  expect(r.row.record.receipts).toHaveLength(1);
  expect(events).toHaveLength(1);
});
test('stopped projection retains uncertain target and honest recipient/archive language', async ({
  page
}) => {
  response = 'silent';
  await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    window.hcp089Pending = f.run();
  });
  await expect.poll(() => events.length).toBe(1);
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.stop();
    await window.hcp089Pending;
    return { snapshot: f.snapshot(), status: await f.sendStatus() };
  });
  expect(r.snapshot!.labels).toContain('Stopped');
  expect(r.status!.recipient.uncertainTargets).toEqual([
    'wss://peer.example.org'
  ]);
  expect(r.status!.labels).toContain('Recipient delivery unknown');
  expect(r.status!.labels).toContain('Sender archive pending');
  expect([...r.snapshot!.labels, ...r.status!.labels].join(' ')).not.toMatch(
    /Order accepted|Seen|Read|Delivered to person|permanent retention|guaranteed notification/
  );
});
test('reload remembers ciphertext but only explicit matching-owner Resume publishes original artifacts', async ({
  page
}) => {
  response = 'silent';
  const before = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    const row = await f.row();
    window.hcp089Pending = f.run();
    return row;
  });
  await expect.poll(() => events.length).toBe(1);
  const stopped = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.stop();
    await window.hcp089Pending;
    return f.row();
  });
  await page.reload();
  await load(page, false);
  expect(events).toHaveLength(1);
  const captured = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    const prepared = await f.prepare();
    return { prepared, captured: f.capture(), row: await f.row() };
  });
  expect(captured.prepared.status).toBe('existing');
  expect(captured.captured).toBe(true);
  expect(events).toHaveLength(1);
  expect(captured.row.wire).toBe(stopped.wire);
  response = 'accepted';
  const resumed = await page.evaluate(async () => ({
    result: await window.hcp089Fixture.run(),
    row: await window.hcp089Fixture.row()
  }));
  expect(resumed.result.status).toBe('completed');
  expect(events).toHaveLength(3);
  expect(events.slice(1).map((e) => e.wire)).toEqual([
    before.record.peerArtifact!.wire,
    before.record.self.wire
  ]);
  expect(resumed.row.record.receipts).toHaveLength(3);
  expect(resumed.row.record.receipts![0]).toEqual(stopped.record.receipts![0]);
});
test('Stop in sender archive preserves actual prior recipient acceptance and partial archive uncertainty', async ({
  page
}) => {
  archiveResponse = 'silent';
  await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    await f.prepare();
    f.capture();
    window.hcp089Pending = f.run();
  });
  await expect.poll(() => events.length).toBe(2);
  const r = await page.evaluate(async () => {
    const f = window.hcp089Fixture;
    f.stop();
    const result = await window.hcp089Pending;
    return { result, row: await f.row(), status: await f.sendStatus() };
  });
  expect(r.result.status).toBe('stopped');
  expect(events).toHaveLength(2);
  expect(r.row.record.receipts!.map((f) => [f.role, f.status])).toEqual([
    ['peer', 'accepted'],
    ['self_archive', 'stopped']
  ]);
  expect(r.status!.recipient.state).toBe('accepted_by_inbox_relay');
  expect(r.status!.archive.state).toBe('pending');
  expect(r.status!.partial).toBe(true);
  expect(r.status!.labels).toContain('Accepted by recipient inbox relay');
  expect(r.status!.labels).toContain('Partial delivery');
  expect(r.status!.labels).toContain('Sender archive pending');
});
