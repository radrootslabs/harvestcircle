import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/retry-listing.ts';
declare global {
  interface Window {
    hcp113: typeof Fixture;
    hcp113Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp113Resume: Awaited<ReturnType<typeof Fixture.openResumeFixture>>;
  }
}
const first = 'wss://discovery.example.org',
  second = 'wss://second.example.org';
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  bundle: string,
  endpoint: string;
let frames: { origin: string; message: unknown[] }[] = [];
let stored: Record<string, unknown> | undefined;
let responses = new Map<
  string,
  'accepted' | 'refused' | 'literal-timeout' | 'silent' | 'held'
>();
let holdReadback = false;
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
  sockets.on('connection', (socket, request) => {
    const origin = request.url === '/b' ? second : first;
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes.toString()
        : Array.isArray(bytes)
          ? Buffer.concat(bytes).toString()
          : Buffer.from(bytes).toString();
      const message = JSON.parse(raw) as unknown[];
      frames.push({ origin, message });
      if (message[0] === 'EVENT') {
        stored = message[1] as Record<string, unknown>;
        const response = responses.get(origin) ?? 'accepted';
        if (response === 'held' || response === 'silent') return;
        socket.send(
          JSON.stringify([
            'OK',
            stored.id,
            response === 'accepted',
            response === 'accepted'
              ? ''
              : response === 'literal-timeout'
                ? 'Timeout'
                : 'blocked: controlled refusal'
          ])
        );
      } else if (message[0] === 'REQ' && !holdReadback) {
        if (stored && responses.get(origin) !== 'silent')
          socket.send(JSON.stringify(['EVENT', message[1], stored]));
        socket.send(JSON.stringify(['EOSE', message[1]]));
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
          new URL('./harness/retry-listing.ts', import.meta.url)
        ),
        name: 'hcp113',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('bundle unavailable');
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
  responses = new Map();
  holdReadback = false;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      class LoopbackSocket extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const role =
            String(url) === 'wss://discovery.example.org/'
              ? '/a'
              : String(url) === 'wss://second.example.org/'
                ? '/b'
                : undefined;
          if (!role) throw Error('unfrozen controlled target');
          super(endpoint + role, protocols);
        }
      }
      window.WebSocket = LoopbackSocket;
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp113Fixture = await window.hcp113.createFixture(true);
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp113Resume?.close();
      window.hcp113Fixture?.close();
    });
});
async function restart(page: import('@playwright/test').Page, changed = false) {
  const details = await page.evaluate(() => window.hcp113Fixture.details());
  await page.evaluate(() => window.hcp113Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(
    async ({ owner, command, changed }) => {
      window.hcp113Resume = await window.hcp113.openResumeFixture(
        owner,
        command,
        true,
        changed
      );
    },
    { owner: details.row.owner, command: details.row.id, changed }
  );
  return details;
}
// Additional post-producer regressions; these receive no initial RED credit.
test('HCP113 genuine original explicit retry uses one stored signature and a second reviewed action skips independently verified targets', async ({
  page
}) => {
  const original = await page.evaluate(() => window.hcp113Fixture.details());
  expect(await page.evaluate(() => window.hcp113Fixture.start())).toBe(true);
  expect(await page.evaluate(() => window.hcp113Fixture.result())).toEqual({
    status: 'completed'
  });
  expect(frames.filter(({ message }) => message[0] === 'EVENT')).toHaveLength(
    2
  );
  const delivered = await page.evaluate(() => window.hcp113Fixture.details());
  expect(delivered.row.artifact).toEqual(original.row.artifact);
  expect(delivered.row.capture).toEqual(original.row.capture);
  expect(delivered.signs).toBe(1);
  expect(await page.evaluate(() => window.hcp113Fixture.start())).toBe(true);
  expect(await page.evaluate(() => window.hcp113Fixture.result())).toEqual({
    status: 'completed'
  });
  expect(frames.filter(({ message }) => message[0] === 'EVENT')).toHaveLength(
    2
  );
});
test('HCP113 actual forty-five-second action window stops before another target without changing host verification limits', async ({
  page
}) => {
  const original = await restart(page);
  holdReadback = true;
  const started = performance.now();
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'needs_action'
  });
  expect(performance.now() - started).toBeGreaterThanOrEqual(44000);
  expect(frames.filter(({ message }) => message[0] === 'EVENT')).toHaveLength(
    1
  );
  expect(frames.some(({ origin }) => origin === second)).toBe(false);
  const retained = await page.evaluate(() => window.hcp113Resume.details());
  expect(retained.row.artifact).toEqual(original.row.artifact);
  expect(retained.row.capture).toEqual(original.row.capture);
  expect(retained.row.receipts.some((fact) => fact.status === 'accepted')).toBe(
    true
  );
  expect(retained.signs).toBe(0);
});
test('HCP113 Stop during the last target readback remains stopped while accepted remote evidence survives', async ({
  page
}) => {
  await page.evaluate(() => window.hcp113Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp113Fixture = await window.hcp113.createFixture(false);
  });
  const original = await page.evaluate(() => window.hcp113Fixture.details());
  await page.evaluate(() => window.hcp113Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(
    async ({ owner, command }) => {
      window.hcp113Resume = await window.hcp113.openResumeFixture(
        owner,
        command,
        false
      );
    },
    { owner: original.row.owner, command: original.row.id }
  );
  holdReadback = true;
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  await expect
    .poll(() => frames.filter(({ message }) => message[0] === 'REQ').length)
    .toBeGreaterThan(0);
  await page.evaluate(() => window.hcp113Resume.stop());
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'stopped'
  });
  const retained = await page.evaluate(() => window.hcp113Resume.details());
  expect(retained.row.receipts.some((fact) => fact.status === 'accepted')).toBe(
    true
  );
  expect(retained.signs).toBe(0);
  expect(frames.filter(({ message }) => message[0] === 'EVENT')).toHaveLength(
    1
  );
});
test('HCP113 actual page restart and explicit Resume deliver the same acknowledged signed artifact without any new signature', async ({
  page
}) => {
  const original = await restart(page);
  frames = [];
  expect(
    await page.evaluate(() => window.hcp113Resume.preview())
  ).toBeDefined();
  expect(frames).toHaveLength(0);
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'completed'
  });
  const current = await page.evaluate(() => window.hcp113Resume.details());
  expect(original.signs).toBe(1);
  expect(current.signs).toBe(0);
  expect(current.row.id).toBe(original.row.id);
  expect(current.row.capture).toEqual(original.row.capture);
  expect(current.row.artifact).toEqual(original.row.artifact);
  const events = frames.filter((f) => f.message[0] === 'EVENT');
  expect(events.map((f) => f.origin)).toEqual([first, second]);
  for (const event of events)
    expect(JSON.stringify(event.message[1])).toBe(original.row.artifact!.wire);
});
test('HCP113 independently retained accepted first target is never republished when explicit reopened resume retries refused second target', async ({
  page
}) => {
  responses.set(second, 'refused');
  expect(await page.evaluate(() => window.hcp113Fixture.start())).toBe(true);
  expect(await page.evaluate(() => window.hcp113Fixture.result())).toEqual({
    status: 'needs_action'
  });
  const original = await restart(page);
  expect(
    original.row.receipts.some(
      (f) => f.origin === first && f.status === 'accepted'
    )
  ).toBe(true);
  responses.set(second, 'accepted');
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'completed'
  });
  expect(
    frames.filter((f) => f.message[0] === 'EVENT').map((f) => f.origin)
  ).toEqual([second]);
  const now = await page.evaluate(() => window.hcp113Resume.details());
  expect(now.signs).toBe(0);
  expect(now.row.artifact).toEqual(original.row.artifact);
  expect(now.row.capture).toEqual(original.row.capture);
});
test('HCP113 changed qualified configuration never silently retargets original capture on explicit Resume', async ({
  page
}) => {
  const original = await restart(page, true);
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'needs_action'
  );
  expect(frames).toHaveLength(0);
  const now = await page.evaluate(() => window.hcp113Resume.details());
  expect(now.row).toEqual(original.row);
  expect(now.signs).toBe(0);
});
test('HCP113 fresh changed actual extension owner cannot resume a retained author command', async ({
  page
}) => {
  const original = await restart(page);
  await page.evaluate(() => window.hcp113Resume.changeKey());
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'needs_action'
  );
  expect(frames).toHaveLength(0);
  const now = await page.evaluate(() => window.hcp113Resume.details());
  expect(now.row).toEqual(original.row);
  expect(now.signs).toBe(0);
});
test('HCP113 reopening and review or wrong consent never automatically publishes', async ({
  page
}) => {
  const original = await restart(page);
  frames = [];
  await page.evaluate(() => window.hcp113Resume.preview());
  expect(
    await page.evaluate(() => window.hcp113Resume.start('wrong_consent'))
  ).toBe('invalid');
  expect(frames).toHaveLength(0);
  expect(
    (await page.evaluate(() => window.hcp113Resume.details())).row
  ).toEqual(original.row);
});
test('HCP113 operation-local Stop during actual held EVENT preserves uncertainty and prevents dependent readback or another target', async ({
  page
}) => {
  await restart(page);
  responses.set(first, 'held');
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  await expect
    .poll(() => frames.filter((f) => f.message[0] === 'EVENT').length)
    .toBe(1);
  await page.evaluate(() => window.hcp113Resume.stop());
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'stopped'
  });
  expect(frames.filter((f) => f.message[0] === 'REQ')).toHaveLength(0);
  expect(frames.filter((f) => f.origin === second)).toHaveLength(0);
  const d = await page.evaluate(() => window.hcp113Resume.details());
  expect(d.row.receipts.map((f) => f.status)).toEqual(['stopped']);
  expect(d.signs).toBe(0);
  expect(d.row.artifact).not.toBeNull();
});
test('HCP113 copied Stop control cannot cancel the genuine action or erase native accepted evidence', async ({
  page
}) => {
  await restart(page);
  holdReadback = true;
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  await expect
    .poll(() => frames.filter((f) => f.message[0] === 'REQ').length)
    .toBe(1);
  await page.evaluate(() => window.hcp113Resume.copiedStop());
  await page.waitForTimeout(100);
  expect(
    (await page.evaluate(() => window.hcp113Resume.details())).completed
  ).toBe(false);
  const before = await page.evaluate(() => window.hcp113Resume.details());
  expect(before.row.receipts.some((f) => f.status === 'accepted')).toBe(true);
  await page.evaluate(() => window.hcp113Resume.stop());
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'stopped'
  });
  const after = await page.evaluate(() => window.hcp113Resume.details());
  expect(after.row.receipts.some((f) => f.status === 'accepted')).toBe(true);
  expect(after.row.artifact).toEqual(before.row.artifact);
  expect(after.signs).toBe(0);
  expect(frames.filter((f) => f.message[0] === 'EVENT')).toHaveLength(1);
});
test('HCP113 original Stop while the final actual key query is held prevents any EVENT after the provider settles', async ({
  page
}) => {
  await restart(page);
  frames = [];
  expect(
    await page.evaluate(() =>
      window.hcp113Resume.start('resume_stored_listing', true)
    )
  ).toBe('started');
  await expect
    .poll(() =>
      page.evaluate(async () => (await window.hcp113Resume.details()).heldKeys)
    )
    .toBeGreaterThan(0);
  await page.evaluate(() => window.hcp113Resume.stop());
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'stopped'
  });
  await page.evaluate(() => window.hcp113Resume.releaseKey());
  expect(frames).toHaveLength(0);
  expect(
    (await page.evaluate(() => window.hcp113Resume.details())).row.receipts
  ).toHaveLength(0);
});
test('HCP113 Stop before a held real native readonly completion acknowledgement settles stays stopped without later effects', async ({
  page
}) => {
  const original = await restart(page);
  expect(
    await page.evaluate(() =>
      window.hcp113Resume.start('resume_stored_listing', false, true)
    )
  ).toBe('started');
  await expect
    .poll(() => page.evaluate(() => window.hcp113Resume.heldReadCount()))
    .toBe(1);
  expect(frames).toHaveLength(0);
  await page.evaluate(() => window.hcp113Resume.stop());
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'stopped'
  });
  await page.evaluate(() => window.hcp113Resume.releaseRead());
  const retained = await page.evaluate(() => window.hcp113Resume.details());
  expect(retained.row).toEqual(original.row);
  expect(retained.signs).toBe(0);
  expect(frames).toHaveLength(0);
});
for (const response of ['refused', 'literal-timeout'] as const)
  test(
    'HCP113 actual ' +
      response +
      ' requires another explicit action without spin or signing',
    async ({ page }) => {
      await restart(page);
      responses.set(first, response);
      frames = [];
      expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
        'started'
      );
      expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
        status: 'needs_action'
      });
      expect(frames.filter((f) => f.message[0] === 'EVENT')).toHaveLength(1);
      expect(frames.filter((f) => f.message[0] === 'REQ')).toHaveLength(0);
      const d = await page.evaluate(() => window.hcp113Resume.details());
      expect(d.row.receipts[0].status).toBe(
        response === 'refused' ? 'refused' : 'unknown'
      );
      expect(d.signs).toBe(0);
    }
  );
test('HCP113 actual resumed silent relay action retains at most three original-wire attempts and never creates a new listing', async ({
  page
}) => {
  const original = await restart(page);
  responses.set(first, 'silent');
  frames = [];
  expect(await page.evaluate(() => window.hcp113Resume.start())).toBe(
    'started'
  );
  expect(await page.evaluate(() => window.hcp113Resume.result())).toEqual({
    status: 'needs_action'
  });
  const d = await page.evaluate(() => window.hcp113Resume.details());
  expect(
    d.row.receipts.filter((f) => f.status === 'timed_out').map((f) => f.attempt)
  ).toEqual([1, 2, 3]);
  expect(new Set(d.row.receipts.map((f) => f.actionId)).size).toBe(1);
  expect(frames.filter((f) => f.message[0] === 'EVENT')).toHaveLength(3);
  expect(d.row.artifact).toEqual(original.row.artifact);
  expect(d.signs).toBe(0);
});
test('HCP113 actual local receipt abort requires action and preserves the signed artifact without an automatic second EVENT', async ({
  page
}) => {
  const original = await restart(page);
  frames = [];
  const d = await page.evaluate(async () => {
    const descriptor = Object.getOwnPropertyDescriptor(
      IDBObjectStore.prototype,
      'put'
    );
    if (!descriptor) throw Error('native put missing');
    const native = descriptor.value as IDBObjectStore['put'];
    Object.defineProperty(IDBObjectStore.prototype, 'put', {
      ...descriptor,
      value: function (
        this: IDBObjectStore,
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        if (this.name === 'public_operations') {
          this.transaction.abort();
          throw new DOMException(
            'HC_TEST_ONLY controlled put refusal followed real native abort',
            'QuotaExceededError'
          );
        }
        return native.apply(this, args);
      }
    });
    try {
      const started = await window.hcp113Resume.start(),
        result = await window.hcp113Resume.result(),
        details = await window.hcp113Resume.details();
      return { started, result, details };
    } finally {
      Object.defineProperty(IDBObjectStore.prototype, 'put', descriptor);
    }
  });
  expect(d.started).toBe('started');
  expect(d.result).toEqual({ status: 'needs_action' });
  expect(d.details.row.artifact).toEqual(original.row.artifact);
  expect(d.details.row.receipts).toHaveLength(0);
  expect(d.details.signs).toBe(0);
  expect(frames.filter((f) => f.message[0] === 'EVENT')).toHaveLength(1);
  expect(frames.filter((f) => f.message[0] === 'REQ')).toHaveLength(0);
});
