import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/deliver-listing.ts';
declare global {
  interface Window {
    hcp112: typeof Fixture;
    hcp112Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  bundle: string,
  endpoint: string;
let frames: unknown[][] = [],
  stored: Record<string, unknown> | undefined,
  response: 'accepted' | 'refused' | 'timeout-refusal' | 'silent' = 'accepted',
  readback: 'exact' | 'different' | 'invalid' | 'missing' = 'exact';
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 262144
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string')
    throw Error('loopback unavailable');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) => {
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes.toString()
        : Array.isArray(bytes)
          ? Buffer.concat(bytes).toString()
          : Buffer.from(bytes).toString();
      const frame = JSON.parse(raw) as unknown[];
      frames.push(frame);
      if (frame[0] === 'EVENT') {
        stored = frame[1] as Record<string, unknown>;
        if (response === 'silent') return;
        socket.send(
          JSON.stringify([
            'OK',
            stored.id,
            response === 'accepted',
            response === 'accepted'
              ? ''
              : response === 'timeout-refusal'
                ? 'Timeout'
                : 'blocked: controlled refusal'
          ])
        );
      } else if (frame[0] === 'REQ') {
        if (stored && readback !== 'missing') {
          const event =
            readback === 'exact'
              ? stored
              : {
                  ...stored,
                  ...(readback === 'different'
                    ? { id: '0'.repeat(64) }
                    : { sig: '0'.repeat(128) })
                };
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
    });
  });
  const built = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/deliver-listing.ts', import.meta.url)
        ),
        name: 'hcp112',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('fixture bundle unavailable');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  frames = [];
  stored = undefined;
  response = 'accepted';
  readback = 'exact';
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      class LoopbackSocket extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (String(url) !== 'wss://discovery.example.org/')
            throw Error('unfrozen destination');
          super(endpoint, protocols);
        }
      }
      window.WebSocket = LoopbackSocket;
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp112Fixture = await window.hcp112.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp112Fixture?.close());
});
test('HCP112 actual durable signed listing precedes SDK EVENT and acceptance stays separate from exact loopback readback', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp112Fixture.run());
  expect(r.result.status).toBe('completed');
  expect(r.same).toBe(true);
  expect(r.signs).toBe(1);
  expect(r.trace.indexOf('ack:2')).toBeGreaterThan(-1);
  expect(r.trace.indexOf('EVENT')).toBeGreaterThan(r.trace.indexOf('ack:2'));
  expect(r.originalCapture).toBe(true);
  expect(r.row.receipts).toHaveLength(2);
  expect(r.row.receipts[0].status).toBe('accepted');
  expect(r.row.receipts[0].readbackWire).toBeNull();
  expect(r.row.receipts[1].readbackWire).toBe(r.row.artifact?.wire);
  const events = frames.filter((frame) => frame[0] === 'EVENT');
  expect(events).toHaveLength(1);
  expect(events[0][1]).toEqual(JSON.parse(r.row.artifact!.wire));
  const requests = frames.filter((frame) => frame[0] === 'REQ');
  expect(requests).toHaveLength(1);
  expect(requests[0][2]).toEqual({
    ids: [r.row.artifact!.eventId],
    authors: [r.owner],
    kinds: [30402],
    limit: 1
  });
});
for (const mode of ['different', 'invalid', 'missing'] as const)
  test(`HCP112 ${mode} loopback event cannot replace exact-ID signed readback`, async ({
    page
  }) => {
    readback = mode;
    const r = await page.evaluate(() => window.hcp112Fixture.run());
    expect(r.row.receipts).toHaveLength(1);
    expect(r.row.receipts[0].status).toBe('accepted');
    expect(r.row.receipts[0].readbackWire).toBeNull();
    expect(frames.filter((frame) => frame[0] === 'EVENT')).toHaveLength(1);
    expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(1);
  });
for (const mode of ['refused', 'timeout-refusal'] as const)
  test(`HCP112 actual ${mode} is retained without spinning or fabricating a retryable deadline`, async ({
    page
  }) => {
    response = mode;
    const r = await page.evaluate(() => window.hcp112Fixture.run());
    expect(r.result.status).toBe('needs_action');
    expect(r.row.receipts).toHaveLength(1);
    expect(r.row.receipts[0].status).toBe(
      mode === 'refused' ? 'refused' : 'unknown'
    );
    expect(r.row.receipts[0].readbackWire).toBeNull();
    expect(frames.filter((frame) => frame[0] === 'EVENT')).toHaveLength(1);
    expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(0);
  });
for (const mode of ['logout', 'stop', 'changeKey'] as const)
  test(`HCP112 original ${mode} revokes the final effect without changing the artifact`, async ({
    page
  }) => {
    const r = await page.evaluate(async (mode) => {
      const f = window.hcp112Fixture;
      f[mode]();
      return await f.run();
    }, mode);
    expect(r.result.status).not.toBe('completed');
    expect(r.row.receipts).toHaveLength(0);
    expect(r.signs).toBe(1);
    expect(frames.filter((frame) => frame[0] === 'EVENT')).toHaveLength(0);
  });
test('HCP112 detached publication snapshot cannot borrow final delivery ownership', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp112Fixture.copied());
  expect(r.status).toBe('invalid');
  expect(frames).toHaveLength(0);
});
test('HCP112 actual original lease refuses a foreign target and changed policy without a relay effect', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp112Fixture;
    return { foreign: await f.foreignTarget(), changed: await f.retargeted() };
  });
  expect(r.foreign).toEqual({ status: 'completed', value: false });
  expect(r.changed).toEqual({ status: 'invalid' });
  expect(frames).toHaveLength(0);
});
test('HCP112 actual cached signed event is not an exact relay readback receipt', async ({
  page
}) => {
  readback = 'missing';
  const r = await page.evaluate(async () => {
    const f = window.hcp112Fixture,
      cachedId = await f.cacheArtifact();
    return { cachedId, ...(await f.run()) };
  });
  expect(r.cachedId.id).toBe(r.row.artifact?.eventId);
  expect(r.cachedId.seen).toEqual([
    'wss://discovery.example.org',
    'wss://other.example.org'
  ]);
  expect(r.row.receipts).toHaveLength(1);
  expect(r.row.receipts[0].status).toBe('accepted');
  expect(r.row.receipts[0].readbackWire).toBeNull();
  expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(1);
});
test('HCP112 actual silent relay deadlines retain three bounded timed-out attempts without a new signature', async ({
  page
}) => {
  response = 'silent';
  readback = 'missing';
  const r = await page.evaluate(() => window.hcp112Fixture.run());
  expect(r.result.status).toBe('needs_action');
  expect(r.signs).toBe(1);
  expect(r.row.receipts.map((fact) => fact.status)).toEqual([
    'timed_out',
    'timed_out',
    'timed_out'
  ]);
  expect(r.row.receipts.map((fact) => fact.attempt)).toEqual([1, 2, 3]);
  expect(new Set(r.row.receipts.map((fact) => fact.actionId)).size).toBe(1);
  expect(frames.filter((frame) => frame[0] === 'EVENT')).toHaveLength(3);
  expect(
    new Set(
      frames
        .filter((frame) => frame[0] === 'EVENT')
        .map((frame) => JSON.stringify(frame[1]))
    ).size
  ).toBe(1);
});
test('HCP112 logout after the actual EVENT retains a stopped observation and schedules no dependent readback', async ({
  page
}) => {
  response = 'silent';
  const run = page.evaluate(() => window.hcp112Fixture.run());
  await expect
    .poll(() => frames.filter((frame) => frame[0] === 'EVENT').length)
    .toBe(1);
  await page.evaluate(() => window.hcp112Fixture.logout());
  const r = await run;
  expect(r.result.status).toBe('stopped');
  expect(r.row.receipts).toHaveLength(1);
  expect(r.row.receipts[0].status).toBe('stopped');
  expect(r.row.receipts[0].readbackWire).toBeNull();
  expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(0);
});
for (const unknown of [false, true])
  test(`HCP112 actual receipt write unknown=${unknown} is reconciled before dependent readback`, async ({
    page
  }) => {
    const r = await page.evaluate(
      (unknown) => window.hcp112Fixture.receiptFault(unknown),
      unknown
    );
    expect(r.signs).toBe(1);
    expect(r.faultPuts).toBe(1);
    expect(frames.filter((frame) => frame[0] === 'EVENT')).toHaveLength(1);
    if (unknown) {
      expect(r.result.status).toBe('completed');
      expect(r.row.receipts).toHaveLength(2);
      expect(r.row.receipts[1].readbackWire).toBe(r.row.artifact?.wire);
      expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(1);
    } else {
      expect(r.result.status).toBe('storage_failed');
      expect(r.row.receipts).toHaveLength(0);
      expect(frames.filter((frame) => frame[0] === 'REQ')).toHaveLength(0);
    }
  });
