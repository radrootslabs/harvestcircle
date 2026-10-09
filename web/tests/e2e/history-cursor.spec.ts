import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/history-cursor.ts';
declare global {
  interface Window {
    hcp096: typeof Fixture;
    hcp096Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let holdLatest = false,
  finiteOnly = false;
let requests: unknown[][] = [],
  closes: unknown[][] = [],
  live = new Map<WebSocket, string>(),
  frames: Record<string, unknown>[] = [];
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
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'CLOSE') {
        closes.push(frame);
        return;
      }
      if (frame[0] !== 'REQ') return;
      requests.push(frame);
      const filter = frame[2] as { limit: number; since?: number };
      if (filter.limit === 0) {
        live.set(socket, String(frame[1]));
        for (const event of (finiteOnly ? [] : frames).filter(
          (e) =>
            filter.since === undefined || Number(e.created_at) >= filter.since
        ))
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else {
        for (const event of frames.filter(
          (e) =>
            filter.since === undefined || Number(e.created_at) >= filter.since
        ))
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        if (!holdLatest) socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
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
          new URL('./harness/history-cursor.ts', import.meta.url)
        ),
        name: 'hcp096',
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
test.beforeEach(async ({ page }, testInfo) => {
  holdLatest = false;
  finiteOnly = false;
  requests = [];
  closes = [];
  frames = [];
  live = new Map();
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (new URL(String(url)).origin !== 'wss://archive.example.org')
            throw Error('unapproved receive destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  const age = testInfo.title.includes('much older delayed') ? 30 * 86400 : 0;
  await page.evaluate(async (age) => {
    window.hcp096Fixture = await window.hcp096.makeFixture(undefined, age);
  }, age);
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp096Fixture?.close());
});
const eventFrom = (wire: string) => JSON.parse(wire) as Record<string, unknown>;

test('actual SDK live precedes latest without since then48h5m outer overlap', async ({
  page
}) => {
  const now = Math.floor(Date.now() / 1000);
  await page.evaluate(() => {
    window.hcp096Fixture.capture();
    window.hcp096Fixture.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  const owner = await page.evaluate(() => window.hcp096Fixture.owner);
  expect(requests[0][2]).toEqual({ kinds: [1059], '#p': [owner], limit: 0 });
  expect(requests[1][2]).toEqual({ kinds: [1059], '#p': [owner], limit: 200 });
  const overlap = requests[2][2] as { since: number };
  expect(overlap.since).toBeGreaterThanOrEqual(now - 173100);
  expect(overlap.since).toBeLessThanOrEqual(
    Math.floor(Date.now() / 1000) - 173100
  );
  await expect
    .poll(() => page.evaluate(() => window.hcp096Fixture.snapshot()?.backfill))
    .toBe('complete');
  expect(
    await page.evaluate(() => window.hcp096Fixture.snapshot()?.historyComplete)
  ).toBe(false);
});
test('actual signed two-day backdate survives inner-free latest and overlap dedup', async ({
  page
}) => {
  const raw = await page.evaluate(() =>
    window.hcp096Fixture.oldSignedOuter(Math.floor(Date.now() / 1000) - 172800)
  );
  const event = eventFrom(raw);
  frames = [event];
  await page.evaluate(() => {
    window.hcp096Fixture.capture();
    window.hcp096Fixture.start();
  });
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp096Fixture.snapshot()?.duplicates)
    )
    .toBe(2);
  expect(
    await page.evaluate(
      (id) => window.hcp096Fixture.stored(id),
      String(event.id)
    )
  ).toMatchObject({ id: event.id });
  expect(
    await page.evaluate(() => window.hcp096Fixture.snapshot())
  ).toMatchObject({ retained: 1, historyComplete: false });
});
test('much older delayed persisted signed artifact is retained by unfiltered latest window', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp096Fixture.outer);
  frames = [eventFrom(wire)];
  expect(Number(frames[0].created_at)).toBeLessThan(
    Math.floor(Date.now() / 1000) - 29 * 86400
  );
  expect(
    await page.evaluate(
      async () => (await window.hcp096Fixture.row())?.record.self.wire
    )
  ).toBe(wire);
  await page.evaluate(() => {
    window.hcp096Fixture.capture();
    window.hcp096Fixture.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  await expect
    .poll(() => page.evaluate(() => window.hcp096Fixture.snapshot()?.pending))
    .toBe(0);
  expect(
    await page.evaluate(() => window.hcp096Fixture.snapshot())
  ).toMatchObject({ retained: 1, duplicates: 1, historyComplete: false });
  expect(
    await page.evaluate(
      (id) => window.hcp096Fixture.stored(id),
      String(frames[0].id)
    )
  ).toBeDefined();
  expect(
    await page.evaluate(
      async () => (await window.hcp096Fixture.row())?.record.self.wire
    )
  ).toBe(wire);
});
test('backward clock uses original earlier clock not future outer hint', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp096Fixture.capture(
      true,
      'reviewed_foreground_inbox',
      Math.floor(Date.now() / 1000) + 86400
    );
    window.hcp096Fixture.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  expect((requests[2][2] as { since: number }).since).toBeLessThanOrEqual(
    Math.floor(Date.now() / 1000) - 173100
  );
});
test('noninteger outer hint cannot allocate socket permission', async ({
  page
}) => {
  expect(
    await page.evaluate(() =>
      window.hcp096Fixture.capture(true, 'reviewed_foreground_inbox', 1.5)
    )
  ).toBe(false);
  expect(requests).toEqual([]);
});
test('stop at first finite request fences overlap continuation', async ({
  page
}) => {
  holdLatest = true;
  await page.evaluate(() => {
    window.hcp096Fixture.capture();
    window.hcp096Fixture.start();
  });
  await expect.poll(() => requests.length).toBe(2);
  await page.evaluate(() => window.hcp096Fixture.stop());
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  for (const socket of sockets.clients)
    socket.send(JSON.stringify(['EOSE', requests[1][1]]));
  expect(
    await page.evaluate(() => window.hcp096Fixture.snapshot()?.state)
  ).toBe('stopped');
  expect(requests).toHaveLength(2);
});

test('latest and overlap share500 deliveries instead of renewing duplicate budget', async ({
  page
}) => {
  finiteOnly = true;
  const outer = eventFrom(
    await page.evaluate(() => window.hcp096Fixture.outer)
  );
  frames = Array.from({ length: 251 }, () => outer);
  await page.evaluate(() => {
    window.hcp096Fixture.capture();
    window.hcp096Fixture.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp096Fixture.snapshot()?.state))
    .toBe('needs_action');
  await expect
    .poll(() => page.evaluate(() => window.hcp096Fixture.snapshot()?.pending))
    .toBe(0);
  expect(requests).toHaveLength(3);
  expect(
    await page.evaluate(() => window.hcp096Fixture.snapshot())
  ).toMatchObject({
    reason: 'budget',
    candidates: 500,
    historyComplete: false
  });
  expect(await page.evaluate(() => window.hcp096Fixture.start())).toBe(false);
});
