import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-ingress.ts';
declare global {
  interface Window {
    hcp090: typeof Fixture;
    hcp090Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let requests: unknown[][] = [],
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
      if (frame[0] !== 'REQ') return;
      requests.push(frame);
      for (const event of frames)
        socket.send(JSON.stringify(['EVENT', frame[1], event]));
      socket.send(JSON.stringify(['EOSE', frame[1]]));
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
          new URL('./harness/inbox-ingress.ts', import.meta.url)
        ),
        name: 'hcp090',
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
test.beforeEach(async ({ page }) => {
  requests = [];
  frames = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (origin !== 'wss://archive.example.org')
            throw Error('unapproved receive destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp090Fixture = await window.hcp090.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp090Fixture?.close());
});
test('explicit own inbox receive persists signed outer before decrypt and keeps it out of catalog', async ({
  page
}) => {
  const before = await page.evaluate(() => ({
    wire: window.hcp090Fixture.outer,
    owner: window.hcp090Fixture.owner,
    counts: window.hcp090Fixture.countsValue()
  }));
  frames = [JSON.parse(before.wire) as Record<string, unknown>];
  const capture = await page.evaluate(() => window.hcp090Fixture.capture());
  expect(capture).toBe(true);
  expect(requests).toEqual([]);
  expect(await page.evaluate(() => window.hcp090Fixture.start())).toBe(true);
  await expect
    .poll(() =>
      page.evaluate(async () => !!(await window.hcp090Fixture.stored()))
    )
    .toBe(true);
  const r = await page.evaluate(async () => ({
    row: await window.hcp090Fixture.stored(),
    counts: window.hcp090Fixture.countsValue(),
    catalog: window.hcp090Fixture.publicContains(),
    snapshot: window.hcp090Fixture.snapshot()
  }));
  expect(r.row).toMatchObject({
    family: 'received_envelope',
    owner: before.owner,
    outer: before.wire,
    read: null
  });
  expect(r.catalog).toBe(false);
  expect(r.counts.encrypts).toBe(before.counts.encrypts);
  expect(r.counts.decrypts).toBe(before.counts.decrypts);
  expect(r.counts.signs).toBe(before.counts.signs);
  expect(requests).toHaveLength(1);
  expect(requests[0][2]).toMatchObject({
    kinds: [1059],
    '#p': [before.owner],
    limit: 200
  });
});
test('a bad signature is isolated and the following valid outer remains retainable', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp090Fixture.outer);
  const valid = JSON.parse(wire) as Record<string, unknown>;
  frames = [{ ...valid, sig: '0'.repeat(128) }, valid];
  expect(await page.evaluate(() => window.hcp090Fixture.capture())).toBe(true);
  await page.evaluate(() => window.hcp090Fixture.start());
  await expect
    .poll(() =>
      page.evaluate(async () => !!(await window.hcp090Fixture.stored()))
    )
    .toBe(true);
  expect(await page.evaluate(() => window.hcp090Fixture.publicContains())).toBe(
    false
  );
});
test('duplicate candidates charge ingress before ciphertext retention dedup', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp090Fixture.outer);
  const event = JSON.parse(wire) as Record<string, unknown>;
  frames = [event, event, event];
  await page.evaluate(() => {
    window.hcp090Fixture.capture();
    window.hcp090Fixture.start();
  });
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp090Fixture.snapshot()?.candidates)
    )
    .toBe(3);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp090Fixture.snapshot()?.duplicates)
    )
    .toBe(2);
  expect(await page.evaluate(() => window.hcp090Fixture.publicContains())).toBe(
    false
  );
});
test('absent exercised access or wrong review cannot start private receive', async ({
  page
}) => {
  const r = await page.evaluate(() => ({
    without: window.hcp090Fixture.capture(false),
    wrong: window.hcp090Fixture.capture(true, 'unreviewed'),
    started: window.hcp090Fixture.start()
  }));
  expect(r).toEqual({ without: false, wrong: false, started: false });
  expect(requests).toEqual([]);
});
test('Stop before start prevents REQ and retention', async ({ page }) => {
  const result = await page.evaluate(() => {
    const f = window.hcp090Fixture;
    f.capture();
    f.stop();
    return { start: f.start(), snapshot: f.snapshot() };
  });
  expect(result.start).toBe(false);
  expect(result.snapshot).toMatchObject({
    state: 'stopped',
    retained: 0,
    candidates: 0
  });
  expect(requests).toEqual([]);
});
test('revoked actual access between capture and start prevents REQ', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp090Fixture;
      f.capture();
      f.revokeAccess();
      return f.start();
    })
  ).toBe(false);
  expect(requests).toEqual([]);
});
test('logout after capture fences start and closes the admitted private lifecycle', async ({
  page
}) => {
  const result = await page.evaluate(() => {
    const f = window.hcp090Fixture;
    f.capture();
    f.logout();
    return { start: f.start(), snapshot: f.snapshot() };
  });
  expect(result.start).toBe(false);
  expect(result.snapshot?.state).toBe('stopped');
  expect(requests).toEqual([]);
});
test('duplicate flood is charged before dedup and stops at the fixed 500-candidate budget', async ({
  page
}) => {
  const wire = await page.evaluate(() => window.hcp090Fixture.outer);
  frames = Array.from(
    { length: 501 },
    () => JSON.parse(wire) as Record<string, unknown>
  );
  await page.evaluate(() => {
    window.hcp090Fixture.capture();
    window.hcp090Fixture.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp090Fixture.snapshot()?.pending))
    .toBe(0);
  await expect
    .poll(() => page.evaluate(() => window.hcp090Fixture.snapshot()?.reason))
    .toBe('budget');
  const snapshot = await page.evaluate(() => window.hcp090Fixture.snapshot());
  expect(snapshot?.candidates).toBe(500);
  expect(requests).toHaveLength(1);
  expect(await page.evaluate(() => window.hcp090Fixture.publicContains())).toBe(
    false
  );
});
test('actual received-envelope transaction abort cannot report retained ciphertext', async ({
  page
}) => {
  frames = [
    JSON.parse(await page.evaluate(() => window.hcp090Fixture.outer)) as Record<
      string,
      unknown
    >
  ];
  await page.evaluate(() => {
    const f = window.hcp090Fixture;
    f.faultRetention('abort');
    f.capture();
    f.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp090Fixture.snapshot()?.state))
    .toBe('needs_action');
  const result = await page.evaluate(async () => {
    const f = window.hcp090Fixture;
    f.restoreFault();
    return { snapshot: f.snapshot(), row: await f.stored() };
  });
  expect(result.snapshot).toMatchObject({ retained: 0, reason: 'aborted' });
  expect(result.row).toBeUndefined();
});
test('actual post-put readback failure stays uncertain despite durable ciphertext', async ({
  page
}) => {
  frames = [
    JSON.parse(await page.evaluate(() => window.hcp090Fixture.outer)) as Record<
      string,
      unknown
    >
  ];
  await page.evaluate(() => {
    const f = window.hcp090Fixture;
    f.faultRetention('readback');
    f.capture();
    f.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp090Fixture.snapshot()?.state))
    .toBe('needs_action');
  const result = await page.evaluate(async () => {
    const f = window.hcp090Fixture;
    f.restoreFault();
    return { snapshot: f.snapshot(), row: await f.stored() };
  });
  expect(result.snapshot).toMatchObject({
    retained: 0,
    reason: 'unknown_completion'
  });
  expect(result.row).toMatchObject({ family: 'received_envelope', read: null });
});
