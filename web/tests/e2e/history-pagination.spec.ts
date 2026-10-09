import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/history-pagination.ts';
declare global {
  interface Window {
    hcp097: typeof Fixture;
    hcp097Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
    hcp097Pending: Promise<boolean>;
  }
}
const origins = ['wss://archive.example.org', 'wss://peer.example.org'];
const servers: WebSocketServer[] = [],
  endpoints: string[] = [];
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
let requests: { origin: string; frame: unknown[] }[] = [],
  closes: unknown[][] = [];
let frames: Record<string, unknown>[][] = [[], []],
  fault: number | undefined,
  hold: number | undefined,
  ignoreLimit = false;
test.beforeAll(async () => {
  server = await createStaticHarness();
  for (let i = 0; i < 2; i++) {
    const ws = new WebSocketServer({
      host: '127.0.0.1',
      port: 0,
      maxPayload: 65536
    });
    servers.push(ws);
    await once(ws, 'listening');
    const address = ws.address();
    if (!address || typeof address === 'string') throw Error('no loopback');
    endpoints.push('ws://127.0.0.1:' + address.port);
    ws.on('connection', (socket) =>
      socket.on('message', (bytes) => {
        const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes);
        const frame = JSON.parse(raw.toString()) as unknown[];
        if (frame[0] === 'CLOSE') {
          closes.push(frame);
          return;
        }
        if (frame[0] !== 'REQ') return;
        requests.push({ origin: origins[i], frame });
        const f = frame[2] as { limit: number; since?: number; until?: number };
        if (f.limit === 0) {
          socket.send(JSON.stringify(['EOSE', frame[1]]));
          return;
        }
        if (fault === i) {
          socket.send(
            JSON.stringify(['CLOSED', frame[1], 'controlled unavailable'])
          );
          return;
        }
        const rows = frames[i]
          .filter(
            (e) =>
              (f.since === undefined || Number(e.created_at) >= f.since) &&
              (f.until === undefined || Number(e.created_at) <= f.until)
          )
          .sort(
            (a, b) =>
              Number(b.created_at) - Number(a.created_at) ||
              String(a.id).localeCompare(String(b.id))
          );
        for (const event of ignoreLimit ? rows : rows.slice(0, f.limit))
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        if (hold !== i) socket.send(JSON.stringify(['EOSE', frame[1]]));
      })
    );
  }
  const output = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/history-pagination.ts', import.meta.url)
        ),
        name: 'hcp097',
        formats: ['iife']
      }
    }
  });
  const result = Array.isArray(output) ? output[0] : output;
  if (!('output' in result)) throw Error('no bundle');
  const chunks = result.output.filter((c) => c.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const ws of servers) {
    for (const socket of ws.clients) socket.terminate();
    await new Promise<void>((resolve) => ws.close(() => resolve()));
  }
  await server.close();
});
test.beforeEach(async ({ page }) => {
  requests = [];
  frames = [[], []];
  closes = [];
  fault = undefined;
  hold = undefined;
  ignoreLimit = false;
  await page.addInitScript(
    ({ origins, endpoints }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const i = origins.indexOf(new URL(String(url)).origin);
          if (i < 0) throw Error('unapproved fixture destination');
          super(endpoints[i], protocols);
        }
      };
    },
    { origins, endpoints }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp097Fixture = await window.hcp097.makeFixture();
    window.hcp097Fixture.capture();
    window.hcp097Fixture.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp097Fixture.snapshot()?.backfill))
    .toBe('complete');
  await expect.poll(() => requests.length).toBe(6);
  requests = [];
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp097Fixture?.close());
});
const event = (wire: string) => JSON.parse(wire) as Record<string, unknown>;
test('actual retained outer metadata sets inclusive until independently of original inner ordering', async ({
  page
}) => {
  const wire = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(100)
  );
  await page.evaluate((w) => window.hcp097Fixture.seed(w), wire);
  frames[0] = [event(wire)];
  expect(await page.evaluate(() => window.hcp097Fixture.captureOlder())).toBe(
    true
  );
  expect(await page.evaluate(() => window.hcp097Fixture.loadOlder())).toBe(
    true
  );
  expect(requests[0].frame[2]).toEqual({
    kinds: [1059],
    '#p': [await page.evaluate(() => window.hcp097Fixture.owner)],
    limit: 200,
    until: 100
  });
  expect(
    await page.evaluate(
      () => window.hcp097Fixture.olderSnapshot()?.historyComplete
    )
  ).toBe(false);
});
test('equal-time full bucket stays inclusive and repeated page saturates without automatic third request', async ({
  page
}) => {
  const seed = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(100)
  );
  await page.evaluate((w) => window.hcp097Fixture.seed(w), seed);
  frames[0] = (
    await page.evaluate(() =>
      Array.from({ length: 200 }, () =>
        window.hcp097Fixture.oldSignedOuter(100)
      )
    )
  ).map(event);
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect(requests).toHaveLength(2);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.sources[0])
  ).toMatchObject({ until: 100, state: 'capped' });
  await page.evaluate(() => window.hcp097Fixture.loadOlder());
  expect(requests).toHaveLength(4);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.sources[0])
  ).toMatchObject({ until: 100, state: 'saturated' });
  expect(
    requests
      .filter((r) => r.origin === origins[0])
      .map((r) => (r.frame[2] as { until: number }).until)
  ).toEqual([100, 100]);
});
test('actual older retained ID progresses source boundary without changing original outbox', async ({
  page
}) => {
  const original = await page.evaluate(
    async () => (await window.hcp097Fixture.row())?.wire
  );
  const seed = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(100)
  );
  await page.evaluate((w) => window.hcp097Fixture.seed(w), seed);
  const older = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(99)
  );
  frames[0] = [event(seed), event(older)];
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.sources[0])
  ).toMatchObject({ until: 99, state: 'older_available' });
  expect(
    await page.evaluate(async () => (await window.hcp097Fixture.row())?.wire)
  ).toBe(original);
  expect(
    await page.evaluate(
      (id) => window.hcp097Fixture.stored(id),
      String(frames[0][1].id)
    )
  ).toBeDefined();
  expect(
    await page.evaluate(
      (id) => window.hcp097Fixture.publicContains(id),
      String(frames[0][1].id)
    )
  ).toBe(false);
});
test('one actual relay CLOSED preserves successful peer and old confirmed metadata', async ({
  page
}) => {
  const seed = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(100)
  );
  await page.evaluate(async (w) => {
    await window.hcp097Fixture.seed(w);
    await window.hcp097Fixture.seed(w, 'wss://peer.example.org');
  }, seed);
  const older = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(99)
  );
  fault = 0;
  frames[1] = [event(older)];
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect(requests).toHaveLength(2);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.sources)
  ).toMatchObject([
    { state: 'partial', until: 100 },
    { state: 'older_available', until: 99 }
  ]);
  expect(
    await page.evaluate(
      (id) => window.hcp097Fixture.stored(id),
      String(event(seed).id)
    )
  ).toBeDefined();
  expect(
    await page.evaluate(
      (id) => window.hcp097Fixture.stored(id),
      String(event(older).id)
    )
  ).toBeDefined();
});
test('copied history token wrong review and closed original unlock cannot open older requests', async ({
  page
}) => {
  expect(
    await page.evaluate(() => window.hcp097Fixture.captureOlder('unreviewed'))
  ).toBe(false);
  await page.evaluate(() => window.hcp097Fixture.captureOlder());
  expect(
    await page.evaluate(() => window.hcp097Fixture.copiedOlderStart())
  ).toBe(false);
  await page.evaluate(() => window.hcp097Fixture.closeUnlock());
  expect(await page.evaluate(() => window.hcp097Fixture.loadOlder())).toBe(
    false
  );
  expect(requests).toEqual([]);
});
test('original stop during first source wait closes it and fences remaining source requests', async ({
  page
}) => {
  hold = 0;
  await page.evaluate(() => {
    window.hcp097Fixture.captureOlder();
    window.hcp097Pending = window.hcp097Fixture.loadOlder();
  });
  await expect.poll(() => requests.length).toBe(1);
  await page.evaluate(() => window.hcp097Fixture.stopOlder());
  await expect
    .poll(() => closes.some((frame) => frame[1] === requests[0].frame[1]))
    .toBe(true);
  await page.evaluate(() => window.hcp097Pending);
  expect(requests).toHaveLength(1);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.state)
  ).toBe('stopped');
});
test('fresh extension owner change after history capture prevents either source query', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp097Fixture.captureOlder();
    window.hcp097Fixture.changeKey();
  });
  await page.evaluate(() => window.hcp097Fixture.loadOlder());
  expect(requests).toEqual([]);
  expect(
    await page.evaluate(
      () => window.hcp097Fixture.olderSnapshot()?.historyComplete
    )
  ).toBe(false);
});
test('501 duplicate deliveries exhaust shared action cap before dedup with no automatic retry or eviction', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp097Fixture.outer);
  frames[0] = Array.from({ length: 501 }, () => event(wire));
  ignoreLimit = true;
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect(requests).toHaveLength(1);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot())
  ).toMatchObject({
    state: 'needs_action',
    reason: 'budget',
    historyComplete: false
  });
});

test('actual page reload recovers owner-scoped oldest metadata from existing IndexedDB without resaving outbox', async ({
  page
}) => {
  const wire = await page.evaluate(() =>
    window.hcp097Fixture.oldSignedOuter(100)
  );
  await page.evaluate((w) => window.hcp097Fixture.seed(w), wire);
  const original = await page.evaluate(
    async () => (await window.hcp097Fixture.row())?.wire
  );
  await page.evaluate(() => window.hcp097Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp097Fixture = await window.hcp097.makeFixture(undefined, 0, false);
    window.hcp097Fixture.capture();
    window.hcp097Fixture.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp097Fixture.snapshot()?.backfill))
    .toBe('complete');
  requests = [];
  frames[0] = [event(wire)];
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect((requests[0].frame[2] as { until: number }).until).toBe(100);
  expect(
    await page.evaluate(
      (id) => window.hcp097Fixture.stored(id),
      String(event(wire).id)
    )
  ).toBeDefined();
  expect(
    await page.evaluate(async () => (await window.hcp097Fixture.row())?.wire)
  ).toBe(original);
  expect(
    await page.evaluate(
      () => window.hcp097Fixture.olderSnapshot()?.historyComplete
    )
  ).toBe(false);
});

test('budget pause requires a new explicit review and starts a fresh bounded action without automatic retry', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp097Fixture.outer);
  frames[0] = Array.from({ length: 501 }, () => event(wire));
  ignoreLimit = true;
  await page.evaluate(async () => {
    window.hcp097Fixture.captureOlder();
    await window.hcp097Fixture.loadOlder();
  });
  expect(requests).toHaveLength(1);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot()?.reason)
  ).toBe('budget');
  expect(
    await page.evaluate(() => window.hcp097Fixture.loadOlder('unreviewed'))
  ).toBe(false);
  expect(requests).toHaveLength(1);
  frames = [[], []];
  ignoreLimit = false;
  expect(await page.evaluate(() => window.hcp097Fixture.loadOlder())).toBe(
    true
  );
  expect(requests).toHaveLength(3);
  expect(
    await page.evaluate(() => window.hcp097Fixture.olderSnapshot())
  ).toMatchObject({
    state: 'older_available',
    reason: null,
    historyComplete: false
  });
});
