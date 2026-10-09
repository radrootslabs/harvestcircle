let finiteSilent = false;
import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-retention.ts';
declare global {
  interface Window {
    hcp099: typeof Fixture;
    hcp099Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let requests: unknown[][] = [],
  closes: unknown[][] = [],
  live = new Map<WebSocket, string>(),
  finite: Record<string, unknown>[] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 262144
  });
  await once(sockets, 'listening');
  const a = sockets.address();
  if (!a || typeof a === 'string') throw Error('loopback absent');
  endpoint = 'ws://127.0.0.1:' + a.port;
  sockets.on('connection', (socket) =>
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
      requests.push(frame);
      const filter = frame[2] as { limit: number };
      if (filter.limit === 0) live.set(socket, String(frame[1]));
      else if (finiteSilent) return;
      else
        for (const event of finite)
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
          new URL('./harness/inbox-retention.ts', import.meta.url)
        ),
        name: 'HCP099',
        formats: ['iife']
      }
    }
  });
  const output = (Array.isArray(result) ? result : [result])
    .flatMap((r) => ('output' in r ? r.output : []))
    .find((x) => x.type === 'chunk');
  if (!output || output.type !== 'chunk') throw Error('bundle absent');
  bundle = output.code + '\nwindow.hcp099=HCP099;';
});
test.beforeEach(async ({ page }) => {
  requests = [];
  finiteSilent = false;
  closes = [];
  live = new Map();
  finite = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (new URL(String(url)).origin !== 'wss://archive.example.org')
            throw Error('egress refused');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/messages');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp099Fixture = await window.hcp099.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp099Fixture?.close());
});
test.afterAll(async () => {
  for (const s of sockets.clients) s.terminate();
  await new Promise<void>((r) => sockets.close(() => r()));
  await server?.close();
});
function deliver(event: unknown, count: number) {
  for (const [socket, id] of live)
    for (let i = 0; i < count; i++)
      socket.send(JSON.stringify(['EVENT', id, event]));
}
test('explicit Stop during actual queued native cursor aborts selected-copy deletion', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.fault('stop_cursor'));
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'stopped' }
  );
  await page.evaluate(() => window.hcp099Fixture.restore());
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('only two reviewed local received copies delete; third foreign owner and unresolved outbox remain', async ({
  page
}) => {
  expect(await page.evaluate(() => window.hcp099Fixture.select())).toBe(true);
  expect(
    await page.evaluate(() => window.hcp099Fixture.snapshot())
  ).toMatchObject({
    state: 'review',
    selected: 2,
    localCopyLoss: true,
    relayRecovery: 'not_guaranteed'
  });
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'removed', removed: 2 }
  );
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    1
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
  expect(await page.evaluate(() => window.hcp099Fixture.delta())).toEqual({
    decrypts: 0,
    encrypts: 0,
    signs: 0
  });
  expect(requests).toEqual([]);
});
test('capture is inert and wrong review or copied token cannot delete', async ({
  page
}) => {
  expect(
    await page.evaluate(() => window.hcp099Fixture.select(undefined, 'wrong'))
  ).toBe(false);
  expect(await page.evaluate(() => window.hcp099Fixture.select())).toBe(true);
  expect(
    await page.evaluate(() => window.hcp099Fixture.purge('wrong'))
  ).toEqual({ status: 'invalid' });
  expect(await page.evaluate(() => window.hcp099Fixture.copied())).toEqual({
    status: 'invalid'
  });
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('native whole-wire revision conflict aborts all selected copies', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.revise());
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'conflict' }
  );
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('actual native delete abort rolls back every selected row without success', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.fault('abort'));
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'aborted' }
  );
  await page.evaluate(() => window.hcp099Fixture.restore());
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('actual post-delete readback failure is unknown completion and not definite no deletion', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.fault('readback'));
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'unknown_completion' }
  );
  await page.evaluate(() => window.hcp099Fixture.restore());
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    1
  );
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('logout after selected review refuses local deletion in original namespace', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.logout());
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'stopped' }
  );
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
});
test('owner loss during actual queued native cursor prevents deletion before finish', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.select());
  await page.evaluate(() => window.hcp099Fixture.fault('logout_cursor'));
  expect(await page.evaluate(() => window.hcp099Fixture.purge())).toMatchObject(
    { status: 'stopped' }
  );
  await page.evaluate(() => window.hcp099Fixture.restore());
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
});
test('501 repeated finite ciphertext deliveries consume budget before dedup and pause', async ({
  page
}) => {
  const event = JSON.parse(
    await page.evaluate(() => window.hcp099Fixture.outer)
  ) as Record<string, unknown>;
  finite = Array.from({ length: 501 }, () => event);
  expect(await page.evaluate(() => window.hcp099Fixture.start())).toBe(true);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.state))
    .toBe('needs_action');
  expect(await page.evaluate(() => window.hcp099Fixture.sync())).toMatchObject({
    reason: 'budget',
    historyComplete: false,
    recoveryChoices: [
      'wait_then_review_refresh',
      'review_history_scope',
      'review_local_received_copies'
    ]
  });
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
});
test('121 malformed live deliveries count before rejection and pause instead of spinning', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.start());
  await expect.poll(() => requests.length).toBe(3);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.backfill))
    .toBe('complete');
  deliver(await page.evaluate(() => window.hcp099Fixture.storm()), 121);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.state))
    .toBe('needs_action');
  expect(await page.evaluate(() => window.hcp099Fixture.sync())).toMatchObject({
    reason: 'budget',
    historyComplete: false
  });
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
  expect(requests).toHaveLength(3);
});
test('oversized malformed live payloads consume two MiB before count cap and preserve outbox', async ({
  page
}) => {
  await page.evaluate(() => window.hcp099Fixture.start());
  await expect.poll(() => requests.length).toBe(3);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.backfill))
    .toBe('complete');
  deliver(await page.evaluate(() => window.hcp099Fixture.storm(34000)), 63);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.state))
    .toBe('needs_action');
  expect(await page.evaluate(() => window.hcp099Fixture.sync())).toMatchObject({
    reason: 'budget',
    retained: 0,
    historyComplete: false
  });
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
  expect(requests).toHaveLength(3);
});

test('actual fifteen-second finite elapsed pause exposes reviewed recovery and no automatic second window', async ({
  page
}) => {
  finiteSilent = true;
  expect(await page.evaluate(() => window.hcp099Fixture.start())).toBe(true);
  await expect.poll(() => requests.length).toBe(2);
  await expect
    .poll(() => page.evaluate(() => window.hcp099Fixture.sync()?.state))
    .toBe('needs_action');
  expect(await page.evaluate(() => window.hcp099Fixture.sync())).toMatchObject({
    reason: 'elapsed',
    historyComplete: false,
    recoveryChoices: [
      'wait_then_review_refresh',
      'review_history_scope',
      'review_local_received_copies'
    ]
  });
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(requests).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp099Fixture.preserved())).toBe(
    true
  );
  expect(await page.evaluate(() => window.hcp099Fixture.rows())).toHaveLength(
    3
  );
});
