import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/resume-routing.ts';
declare global {
  interface Window {
    hcp138: typeof Fixture;
    hcp138Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: { origin: string; wire: string }[] = [];
let rerouted = false;
let silent = false;
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
      if (silent) return;
      const accepted =
        rerouted || events.at(-1)?.origin === 'wss://peer.example.org';
      socket.send(
        JSON.stringify([
          'OK',
          event.id,
          accepted,
          accepted ? '' : 'blocked: original archive refusal'
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
          new URL('./harness/resume-routing.ts', import.meta.url)
        ),
        name: 'hcp138',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle missing');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP138_ORIGINAL_ARCHIVE_REROUTE',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Actual original native IDB/SDK signed and encrypted fixed artifacts/loopback ACKs; controlled routing source port distinct from live host/relay/client qualification; write:false bundle bytes not separately retained.'
    })
  );
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((r) => sockets.close(() => r()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  events = [];
  rerouted = false;
  silent = false;
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
    window.hcp138Fixture = await window.hcp138.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp138Fixture?.close());
});
test('accepted peer copy survives explicit sender archive reroute with original self wire and immutable receipt history', async ({
  page
}) => {
  const initial = await page.evaluate(async () => {
    const f = window.hcp138Fixture;
    const result = await f.retry(true),
      before = await f.row(),
      counts = f.counts();
    const captured = f.capture(
      'wss://peer.example.org',
      'wss://new-peer.example.org'
    );
    const snapshot = f.snapshot(),
      unreviewed = await f.approve('unreviewed');
    return {
      result,
      before,
      counts,
      captured,
      snapshot,
      unreviewed,
      untouched: await f.row()
    };
  });
  expect(initial.result.status).toBe('needs_action');
  expect(
    initial.before?.record.receipts?.map((r) => ({
      role: r.role,
      status: r.status
    }))
  ).toEqual([
    { role: 'peer', status: 'accepted' },
    { role: 'self_archive', status: 'refused' }
  ]);
  expect(initial.captured).toBe(true);
  expect(initial.snapshot?.changes.archive.removed).toEqual([
    'wss://archive.example.org'
  ]);
  expect(initial.unreviewed.status).toBe('invalid');
  expect(initial.untouched).toEqual(initial.before);
  expect(events.map((e) => e.origin)).toEqual([
    'wss://peer.example.org',
    'wss://archive.example.org'
  ]);
  rerouted = true;
  const final = await page.evaluate(async () => {
    const f = window.hcp138Fixture;
    const approved = await f.approve(),
      stale = await f.retry(true),
      retried = await f.retry();
    return {
      approved,
      stale,
      retried,
      after: await f.row(),
      counts: f.counts()
    };
  });
  expect(final.approved.status).toBe('updated');
  expect(final.stale.status).toBe('invalid');
  expect(final.retried.status).toBe('completed');
  expect(final.after?.record.self).toEqual(initial.before?.record.self);
  expect(final.after?.record.peerArtifact).toEqual(
    initial.before?.record.peerArtifact
  );
  expect(final.after?.record.receipts?.slice(0, 2)).toEqual(
    initial.before?.record.receipts
  );
  expect(final.after?.record.receipts).toHaveLength(3);
  expect(final.counts.encrypts).toBe(initial.counts.encrypts);
  expect(final.counts.signs).toBe(initial.counts.signs);
  expect(events.map((e) => e.origin)).toEqual([
    'wss://peer.example.org',
    'wss://archive.example.org',
    'wss://new-peer.example.org'
  ]);
  expect(events[0].wire).toBe(initial.before?.record.peerArtifact?.wire);
  expect(events[1].wire).toBe(initial.before?.record.self.wire);
  expect(events[2].wire).toBe(events[1].wire);
});

test('three actual silent attempts stop before archive publication without repeating crypto', async ({
  page
}) => {
  silent = true;
  const before = await page.evaluate(async () => ({
    row: await window.hcp138Fixture.row(),
    counts: window.hcp138Fixture.counts()
  }));
  const started = process.hrtime.bigint();
  const result = await page.evaluate(() => window.hcp138Fixture.retry(true));
  const elapsedMilliseconds = Number(process.hrtime.bigint() - started) / 1e6;
  console.log(
    JSON.stringify({
      fixture: 'HCP138_REAL_SILENT_ATTEMPT_CAP',
      elapsedMilliseconds,
      eventCount: events.length,
      clock: 'native process monotonic; browser performance.now unchanged'
    })
  );
  expect(elapsedMilliseconds).toBeGreaterThanOrEqual(29_000);
  expect(result.status).toBe('needs_action');
  expect(events).toHaveLength(3);
  for (const event of events) {
    expect(event.origin).toBe('wss://peer.example.org');
    expect(event.wire).toBe(before.row?.record.peerArtifact?.wire);
  }
  const after = await page.evaluate(async () => ({
    row: await window.hcp138Fixture.row(),
    counts: window.hcp138Fixture.counts()
  }));
  expect(after.counts.encrypts).toBe(before.counts.encrypts);
  expect(after.counts.decrypts).toBe(before.counts.decrypts);
  expect(after.counts.signs).toBe(before.counts.signs);
  expect(after.counts.keys).toBe(before.counts.keys + 3);
  expect(after.row?.record.self).toEqual(before.row?.record.self);
  expect(after.row?.record.peerArtifact).toEqual(
    before.row?.record.peerArtifact
  );
  expect(
    after.row?.record.receipts?.some((receipt) => receipt.status === 'accepted')
  ).toBe(false);
  expect(
    after.row?.record.receipts?.map((receipt) => ({
      role: receipt.role,
      attempt: receipt.attempt,
      status: receipt.status
    }))
  ).toEqual([
    { role: 'peer', attempt: 1, status: 'timed_out' },
    { role: 'peer', attempt: 2, status: 'timed_out' },
    { role: 'peer', attempt: 3, status: 'timed_out' }
  ]);
});
