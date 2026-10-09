import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-sync.ts';
declare global {
  interface Window {
    hcp095: typeof Fixture;
    hcp095Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
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
      const filter = frame[2] as { limit: number };
      if (filter.limit === 0) {
        live.set(socket, String(frame[1]));
        for (const event of frames)
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else {
        for (const event of frames)
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
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
          new URL('./harness/inbox-sync.ts', import.meta.url)
        ),
        name: 'hcp095',
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
  await page.evaluate(async () => {
    window.hcp095Fixture = await window.hcp095.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp095Fixture?.close());
});
const eventFrom = (wire: string) => JSON.parse(wire) as Record<string, unknown>;
function deliver(event: Record<string, unknown>) {
  for (const [socket, id] of live)
    socket.send(JSON.stringify(['EVENT', id, event]));
}
test('closing original unlock after live start sends actual CLOSE and prevents resurrection', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  await page.evaluate(() => window.hcp095Fixture.closeUnlock());
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      return { snapshot: f.snapshot(), start: f.start(), capture: f.capture() };
    })
  ).toMatchObject({
    snapshot: { state: 'stopped' },
    start: false,
    capture: false
  });
  expect(requests).toHaveLength(3);
});
test('second capture cannot replace the original admitted foreground owner', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      return [f.capture(), f.capture(), f.start()];
    })
  ).toEqual([true, false, true]);
  await expect.poll(() => requests.length).toBe(3);
  expect(
    requests.filter((r) => (r[2] as { limit: number }).limit === 0)
  ).toHaveLength(1);
});
test('logout during actual held fresh-key job fences its settled continuation and no REQ opens', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.holdNextKey();
    f.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.pendingKey()))
    .toBe(true);
  expect(await page.evaluate(() => window.hcp095Fixture.identityState())).toBe(
    'pending'
  );
  expect(requests).toEqual([]);
  expect(await page.evaluate(() => window.hcp095Fixture.keyJobOccupied())).toBe(
    true
  );
  await page.evaluate(() => window.hcp095Fixture.logoutIdentity());
  expect(
    await page.evaluate(() => window.hcp095Fixture.snapshot())
  ).toMatchObject({ state: 'stopped', retained: 0, candidates: 0 });
  expect(await page.evaluate(() => window.hcp095Fixture.pendingKey())).toBe(
    true
  );
  await page.evaluate(() => window.hcp095Fixture.settleKey());
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.keyJobOccupied()))
    .toBe(false);
  expect(await page.evaluate(() => window.hcp095Fixture.identityState())).toBe(
    'guest'
  );
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      return { snapshot: f.snapshot(), start: f.start(), capture: f.capture() };
    })
  ).toMatchObject({
    snapshot: { state: 'stopped', retained: 0, candidates: 0 },
    start: false,
    capture: false
  });
  expect(requests).toEqual([]);
});
test('reviewed capture is inert and wrong review or absent exercised access cannot start', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      return {
        wrong: f.capture(true, 'unreviewed'),
        without: f.capture(false),
        ready: f.capture(),
        snapshot: f.snapshot()
      };
    })
  ).toMatchObject({
    wrong: false,
    without: false,
    ready: true,
    snapshot: { state: 'ready' }
  });
  expect(requests).toEqual([]);
});
test('actual live REQ precedes bounded backfill and gap delivery is retained once across both', async ({
  page
}) => {
  frames = [eventFrom(await page.evaluate(() => window.hcp095Fixture.outer))];
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.start();
  });
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp095Fixture.snapshot()?.duplicates)
    )
    .toBe(2);
  const owner = await page.evaluate(() => window.hcp095Fixture.owner);
  expect(requests).toHaveLength(3);
  expect(requests.map((r) => (r[2] as { limit: number }).limit)).toEqual([
    0, 200, 200
  ]);
  expect(requests[0][2]).toEqual({ kinds: [1059], '#p': [owner], limit: 0 });
  expect(requests[1][2]).toEqual({ kinds: [1059], '#p': [owner], limit: 200 });
  expect(
    await page.evaluate(() => window.hcp095Fixture.snapshot())
  ).toMatchObject({
    state: 'live',
    retained: 1,
    duplicates: 2,
    backfill: 'complete',
    historyComplete: false
  });
  expect(await page.evaluate(() => window.hcp095Fixture.publicContains())).toBe(
    false
  );
});
test('single no-later-event live tail arrives after both EOSE without decrypt or throttle', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp095Fixture.countsValue());
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.snapshot()?.backfill))
    .toBe('complete');
  deliver(eventFrom(await page.evaluate(() => window.hcp095Fixture.outer)));
  await expect
    .poll(() =>
      page.evaluate(async () => !!(await window.hcp095Fixture.stored()))
    )
    .toBe(true);
  expect(
    await page.evaluate(() => window.hcp095Fixture.snapshot())
  ).toMatchObject({ state: 'live', retained: 1, historyComplete: false });
  const after = await page.evaluate(() => window.hcp095Fixture.countsValue());
  expect(after.decrypts).toBe(before.decrypts);
  expect(after.encrypts).toBe(before.encrypts);
  expect(after.signs).toBe(before.signs);
});
test('repeated start never allocates a second live subscription', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      f.capture();
      return [f.start(), f.start()];
    })
  ).toEqual([true, false]);
  await expect.poll(() => requests.length).toBe(3);
  expect(
    requests.filter((r) => (r[2] as { limit: number }).limit === 0)
  ).toHaveLength(1);
});
test('route disposal closes actual live request and suspends original Messages lifetime', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  await page.evaluate(() => window.hcp095Fixture.navigateAway());
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      return { snapshot: f.snapshot(), start: f.start(), capture: f.capture() };
    })
  ).toMatchObject({
    snapshot: {
      state: 'stopped',
      foregroundOnly: true,
      historyComplete: false
    },
    start: false,
    capture: false
  });
  expect(requests).toHaveLength(3);
});
test('closed original unlocked cache refuses foreground receive without sockets', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      f.closeUnlock();
      return f.capture();
    })
  ).toBe(false);
  expect(requests).toEqual([]);
});
test('revoked actual access after capture fences start', async ({ page }) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      f.capture();
      f.revokeAccess();
      return f.start();
    })
  ).toBe(false);
  expect(requests).toEqual([]);
});
test('fresh extension owner changed before transport prevents live or history REQ', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.changeKey();
    f.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.snapshot()?.state))
    .toBe('stopped');
  expect(requests).toEqual([]);
});
test('logout closes captured scope and never revives hidden retrieval', async ({
  page
}) => {
  expect(
    await page.evaluate(() => {
      const f = window.hcp095Fixture;
      f.capture();
      f.logout();
      return { start: f.start(), capture: f.capture(), snapshot: f.snapshot() };
    })
  ).toMatchObject({
    start: false,
    capture: false,
    snapshot: { state: 'stopped' }
  });
  expect(requests).toEqual([]);
});
test('121 duplicate live deliveries charge before dedup and pause the source', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.capture();
    f.start();
  });
  await expect.poll(() => requests.length).toBe(3);
  const e = eventFrom(await page.evaluate(() => window.hcp095Fixture.outer));
  for (let i = 0; i < 121; i++) deliver(e);
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.snapshot()?.state))
    .toBe('needs_action');
  expect(
    await page.evaluate(() => window.hcp095Fixture.snapshot())
  ).toMatchObject({ reason: 'budget', historyComplete: false });
  await expect.poll(() => closes.length).toBeGreaterThanOrEqual(2);
  expect(await page.evaluate(() => window.hcp095Fixture.publicContains())).toBe(
    false
  );
});
test('actual native retention abort pauses both subscriptions without durable credit', async ({
  page
}) => {
  frames = [eventFrom(await page.evaluate(() => window.hcp095Fixture.outer))];
  await page.evaluate(() => {
    const f = window.hcp095Fixture;
    f.faultRetention('abort');
    f.capture();
    f.start();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp095Fixture.snapshot()?.state))
    .toBe('needs_action');
  expect(
    await page.evaluate(async () => {
      const f = window.hcp095Fixture;
      f.restoreFault();
      return { snapshot: f.snapshot(), stored: await f.stored() };
    })
  ).toMatchObject({
    snapshot: { retained: 0, reason: 'aborted' },
    stored: undefined
  });
});
