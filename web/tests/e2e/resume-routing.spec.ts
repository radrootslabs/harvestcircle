import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/resume-routing.ts';
declare global {
  interface Window {
    hcp088: typeof Fixture;
    hcp088Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: { origin: string; wire: string }[] = [];
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
      events.push({
        origin: new URL(request.url!, 'http://localhost').searchParams.get(
          'origin'
        )!,
        wire: JSON.stringify(event)
      });
      socket.send(JSON.stringify(['OK', event.id, true, '']));
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
          new URL('./harness/resume-routing.ts', import.meta.url)
        ),
        name: 'hcp088',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle missing');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((r) => sockets.close(() => r()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  events = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            ![
              'wss://peer.example.org',
              'wss://archive.example.org',
              'wss://new-peer.example.org'
            ].includes(origin)
          )
            throw Error('unapproved destination');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp088Fixture = await window.hcp088.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp088Fixture?.close());
});
test('removed inbox is previewed exactly and unreviewed resumption has no automatic replay', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    return {
      captured: f.capture(),
      snapshot: f.snapshot(),
      result: await f.approve('unreviewed'),
      before,
      after: await f.row(),
      retry: await f.retry(true)
    };
  });
  expect(r.captured).toBe(true);
  expect(r.snapshot?.changes.peer.removed).toEqual(['wss://peer.example.org']);
  expect(r.snapshot?.changes.peer.added).toEqual([
    'wss://new-peer.example.org'
  ]);
  expect(r.result.status).toBe('invalid');
  expect(r.after).toEqual(r.before);
  expect(r.retry.status).toBe('invalid');
  expect(events).toEqual([]);
});
test('same original ciphertext reaches only the explicitly approved newly advertised destination and old attempts remain', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture;
    await f.retry(true);
    const before = await f.row(),
      counts = f.counts();
    f.capture();
    const updated = await f.approve(),
      old = await f.retry(true),
      result = await f.retry();
    return {
      before,
      after: await f.row(),
      counts,
      afterCounts: f.counts(),
      updated,
      old,
      result
    };
  });
  expect(r.updated.status).toBe('updated');
  expect(r.old.status).toBe('invalid');
  expect(r.result.status).toBe('completed');
  expect(r.after?.record.self).toEqual(r.before?.record.self);
  expect(r.after?.record.peerArtifact).toEqual(r.before?.record.peerArtifact);
  expect(r.after?.record.receipts?.slice(0, 2)).toEqual(
    r.before?.record.receipts
  );
  expect(r.after?.record.receipts).toHaveLength(3);
  expect(r.afterCounts.encrypts).toBe(r.counts.encrypts);
  expect(r.afterCounts.signs).toBe(r.counts.signs);
  expect(events.map((e) => e.origin)).toEqual([
    'wss://peer.example.org',
    'wss://archive.example.org',
    'wss://new-peer.example.org'
  ]);
  expect(events[2].wire).toBe(events[0].wire);
});
test('new unapproved advertised URL has no route capability or network access', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    return {
      captured: f.capture('wss://unapproved.example.org'),
      approved: await f.approve(),
      before,
      after: await f.row()
    };
  });
  expect(r.captured).toBe(false);
  expect(r.approved.status).toBe('invalid');
  expect(r.after).toEqual(r.before);
  expect(events).toEqual([]);
});
test('peer switch requires a new message and cannot retarget existing ciphertext', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    return {
      captured: f.capture(
        'wss://new-peer.example.org',
        'wss://archive.example.org',
        true
      ),
      approved: await f.approve(),
      before,
      after: await f.row()
    };
  });
  expect(r.captured).toBe(false);
  expect(r.approved.status).toBe('invalid');
  expect(r.after).toEqual(r.before);
  expect(events).toEqual([]);
});
test('sender archive changes use its own advertised set while preserving the peer artifact', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    f.capture('wss://peer.example.org', 'wss://new-peer.example.org');
    const snapshot = f.snapshot(),
      updated = await f.approve(),
      result = await f.retry();
    return { before, after: await f.row(), snapshot, updated, result };
  });
  expect(r.snapshot?.changes.archive.removed).toEqual([
    'wss://archive.example.org'
  ]);
  expect(r.updated.status).toBe('updated');
  expect(r.result.status).toBe('completed');
  expect(events.map((e) => e.origin)).toEqual([
    'wss://peer.example.org',
    'wss://new-peer.example.org'
  ]);
  expect(events[1].wire).toBe(r.before?.record.self.wire);
  expect(r.after?.record.peerArtifact).toEqual(r.before?.record.peerArtifact);
});
test('incomplete current discovery cannot supply route consent', async ({
  page
}) => {
  const r = await page.evaluate(() => {
    const f = window.hcp088Fixture;
    return f.capture(
      'wss://new-peer.example.org',
      'wss://archive.example.org',
      false,
      false
    );
  });
  expect(r).toBe(false);
  expect(events).toEqual([]);
});
test('stale discovery after review blocks destination update', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    f.capture();
    f.stale();
    return { result: await f.approve(), before, after: await f.row() };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.after).toEqual(r.before);
  expect(events).toEqual([]);
});
test('competing whole-wire successor cannot renew route-updated custody', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture;
    f.capture();
    await f.write('revision');
    const before = await f.row();
    return {
      result: await f.approve(),
      before,
      after: await f.row(),
      retry: await f.retry()
    };
  });
  expect(r.result.status).toBe('conflict');
  expect(r.after).toEqual(r.before);
  expect(r.retry.status).toBe('invalid');
  expect(events).toEqual([]);
});
test('fresh changed owner refuses reviewed destination update without encryption or signing', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture;
    f.capture();
    const before = await f.row(),
      counts = f.counts();
    f.mode('changed_key');
    return {
      result: await f.approve(),
      before,
      after: await f.row(),
      counts,
      afterCounts: f.counts()
    };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.after).toEqual(r.before);
  expect(r.afterCounts.encrypts).toBe(r.counts.encrypts);
  expect(r.afterCounts.signs).toBe(r.counts.signs);
  expect(events).toEqual([]);
});
test('actual IDB abort and post-put readback loss do not acknowledge route permission', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture;
    f.capture();
    const before = await f.row();
    f.fault('abort');
    const aborted = await f.approve(),
      afterAbort = await f.row();
    f.capture();
    f.fault('readback');
    const unknown = await f.approve(),
      retry = await f.retry();
    return { before, afterAbort, aborted, unknown, retry };
  });
  expect(r.aborted.status).toBe('aborted');
  expect(r.afterAbort).toEqual(r.before);
  expect(r.unknown.status).toBe('unknown_completion');
  expect(r.retry.status).toBe('invalid');
  expect(events).toEqual([]);
});

test('unsupported current inbox resolution also fences previously genuine old-route replay', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp088Fixture,
      before = await f.row();
    const captured = f.capture('wss://unapproved.example.org');
    return {
      captured,
      before,
      result: await f.retry(true),
      after: await f.row()
    };
  });
  expect(r.captured).toBe(false);
  expect(r.result.status).toBe('invalid');
  expect(r.after).toEqual(r.before);
  expect(events).toEqual([]);
});

async function assertAlreadyHeldFenced(
  page: import('@playwright/test').Page,
  destination: string,
  reviewReady: boolean
) {
  const r = await page.evaluate(async (destination) => {
    const f = window.hcp088Fixture,
      held = f.captureHeld(),
      before = await f.row();
    const reviewed = f.capture(destination);
    return {
      held,
      reviewed,
      before,
      result: await f.runHeld(),
      after: await f.row()
    };
  }, destination);
  expect(r.held).toBe(true);
  expect(r.reviewed).toBe(reviewReady);
  expect(r.result.status).toBe('needs_action');
  expect(r.after).toEqual(r.before);
  expect(events).toEqual([]);
}
test('already captured genuine retry cannot publish while supported destination review is pending', async ({
  page
}) => {
  await assertAlreadyHeldFenced(page, 'wss://new-peer.example.org', true);
});
test('already captured genuine retry cannot replay after current inbox becomes unsupported', async ({
  page
}) => {
  await assertAlreadyHeldFenced(page, 'wss://unapproved.example.org', false);
});
