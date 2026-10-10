import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/edit-source.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
declare global {
  interface Window {
    hcp118: typeof Fixture;
    hcp118Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let event: NostrEvent | undefined,
  partial = false,
  requests: unknown[][] = [],
  effects: unknown[][] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string') throw Error('loopback missing');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'EVENT') {
        effects.push(frame);
        return;
      }
      if (frame[0] !== 'REQ') return;
      requests.push(frame);
      const filters = frame.slice(2) as { kinds?: number[] }[];
      if (event && filters.some((f) => f.kinds?.includes(30402)))
        socket.send(JSON.stringify(['EVENT', frame[1], event]));
      if (!(partial && filters.some((f) => f.kinds?.includes(5))))
        socket.send(JSON.stringify(['EOSE', frame[1]]));
    })
  );
  const built = await build({
    configFile: false,
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/edit-source.ts', import.meta.url)
        ),
        name: 'hcp118',
        formats: ['iife']
      }
    }
  });
  const result = Array.isArray(built) ? built[0] : built;
  if (!('output' in result)) throw Error('actual fixture bundle missing');
  const chunks = result.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets?.clients ?? []) socket.terminate();
  if (sockets)
    await new Promise<void>((resolve, reject) =>
      sockets.close((error) => (error ? reject(error) : resolve()))
    );
  await server?.close();
});
test.beforeEach(async ({ page }) => {
  event = undefined;
  partial = false;
  requests = [];
  effects = [];
  await page.addInitScript((endpoint) => {
    const Native = window.WebSocket;
    window.WebSocket = class extends Native {
      constructor(url: string | URL, protocols?: string | string[]) {
        const origin = new URL(url.toString()).origin;
        if (
          ![
            'wss://discovery.example.org',
            'wss://inbox.example.org',
            'wss://second.example.org'
          ].includes(origin)
        )
          throw Error('no live relay permission');
        super(endpoint, protocols);
      }
    };
  }, endpoint);
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  event = await page.evaluate(async () => {
    window.hcp118Fixture = await window.hcp118.createFixture();
    return window.hcp118Fixture.event;
  });
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    await window.hcp118Fixture?.close();
  });
});
test('HCP118 exact retained source performs independent head/deletion lookup and genuine current-head authorization', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp118Fixture.counts());
  await page.evaluate(() => window.hcp118Fixture.open());
  await expect
    .poll(() => page.evaluate(() => window.hcp118Fixture.snapshot()?.status))
    .toBe('ready');
  const result = await page.evaluate(() => ({
    snapshot: window.hcp118Fixture.snapshot(),
    authorized: window.hcp118Fixture.authorized(),
    copied: window.hcp118Fixture.authorized(true),
    counts: window.hcp118Fixture.counts()
  }));
  expect(result.authorized).toBe(true);
  expect(result.copied).toBe(false);
  expect(result.snapshot?.sourceEventId).toBe(event!.id);
  expect(result.snapshot?.publishedAtSeconds).toBe(
    Number(event!.tags.find((t) => t[0] === 'published_at')![1])
  );
  expect(result.counts).toEqual(before);
  expect(requests.some((r) => JSON.stringify(r).includes('"#d"'))).toBe(true);
  expect(requests.some((r) => JSON.stringify(r).includes('"kinds":[5]'))).toBe(
    true
  );
  expect(requests.some((r) => JSON.stringify(r).includes('"search"'))).toBe(
    false
  );
  expect(effects).toHaveLength(0);
});
test('HCP118 retained values without critical lookup remain read-only and cannot infer absence', async ({
  page
}) => {
  const value = await page.evaluate(() => window.hcp118Fixture.open(false));
  expect(value?.status).toBe('read-only');
  expect(await page.evaluate(() => window.hcp118Fixture.authorized())).toBe(
    false
  );
  expect(requests).toHaveLength(0);
});
test('HCP118 partial deletion coverage cannot authorize an unchanged known source', async ({
  page
}) => {
  partial = true;
  await page.evaluate(() => window.hcp118Fixture.open());
  await expect.poll(() => requests.length).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.hcp118Fixture.authorized())).toBe(
    false
  );
  expect(
    await page.evaluate(
      () => window.hcp118Fixture.snapshot()?.definitiveAbsence
    )
  ).toBe(false);
});
test('HCP118 Tera/external retained-head source is not adopted', async ({
  page
}) => {
  await page.evaluate(() => window.hcp118Fixture.external());
  expect(
    await page.evaluate(() => window.hcp118Fixture.open())
  ).toBeUndefined();
  expect(requests).toHaveLength(0);
});
test('HCP118 foreign author is refused before any current-head query', async ({
  page
}) => {
  expect(
    await page.evaluate(() =>
      window.hcp118Fixture.open(true, window.hcp118Fixture.foreign())
    )
  ).toBeUndefined();
  expect(requests).toHaveLength(0);
});
test('HCP118 newer independently signed head stops the original retained base', async ({
  page
}) => {
  event = await page.evaluate(() => window.hcp118Fixture.competing());
  await page.evaluate(() => window.hcp118Fixture.open());
  await expect
    .poll(() => page.evaluate(() => window.hcp118Fixture.snapshot()?.status))
    .toBe('conflict');
  expect(await page.evaluate(() => window.hcp118Fixture.authorized())).toBe(
    false
  );
  expect(effects).toHaveLength(0);
});
test('HCP118 original source closes on cancellation; copied close cannot revoke original custody', async ({
  page
}) => {
  await page.evaluate(() => window.hcp118Fixture.open());
  await expect
    .poll(() => page.evaluate(() => window.hcp118Fixture.authorized()))
    .toBe(true);
  await page.evaluate(() => window.hcp118Fixture.copiedClose());
  expect(await page.evaluate(() => window.hcp118Fixture.authorized())).toBe(
    true
  );
  await page.evaluate(() => window.hcp118Fixture.cancel());
  expect(
    await page.evaluate(() => window.hcp118Fixture.snapshot())
  ).toBeUndefined();
  expect(await page.evaluate(() => window.hcp118Fixture.authorized())).toBe(
    false
  );
});
for (const width of [320, 1024])
  test(`HCP118 ${width}px conflict displays actual current and exact proposed values without merge or publish`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(() => window.hcp118Fixture.open());
    await expect
      .poll(() => page.evaluate(() => window.hcp118Fixture.snapshot()?.status))
      .toBe('ready');
    await page.evaluate(() => window.hcp118Fixture.renderEditor());
    const title = page.locator('#listing-title');
    await expect(title).not.toHaveAttribute('readonly', '');
    await title.fill('Exact proposed carrots');
    await page
      .locator('#listing-description')
      .fill('Raw proposed description\n' + 'long '.repeat(100));
    await page.locator('#listing-location').fill('Proposed town');
    await page.locator('#listing-amount').fill('9');
    await page.locator('#listing-quantity').fill('4');
    await expect
      .poll(() =>
        page.evaluate(
          async () =>
            (await window.hcp118Fixture.drafts()).find(
              (row) => row.form.title === 'Exact proposed carrots'
            )?.form
        )
      )
      .toEqual({
        title: 'Exact proposed carrots',
        description: 'Raw proposed description\n' + 'long '.repeat(100),
        location: 'Proposed town',
        amount: '9',
        currency: 'USD',
        unit: 'kg',
        quantity: '4',
        contactType: '',
        contactValue: ''
      });
    const beforeExternalFixture = await page.evaluate(
      () => window.hcp118Fixture.counts().signs
    );
    event = await page.evaluate(() => window.hcp118Fixture.competing());
    const before = await page.evaluate(
      () => window.hcp118Fixture.counts().signs
    );
    expect(before).toBe(beforeExternalFixture + 1);
    await page.evaluate(() => window.hcp118Fixture.lookupAgain());
    await expect
      .poll(() => page.evaluate(() => window.hcp118Fixture.snapshot()?.status))
      .toBe('conflict');
    await expect(page.locator('#hcp118-conflict')).toContainText(
      'This listing changed elsewhere. Review the latest version before publishing.'
    );
    await expect(page.locator('#hcp118-conflict')).toContainText(
      'Actual current carrots'
    );
    await expect(page.locator('#hcp118-conflict')).toContainText(
      'Exact proposed carrots'
    );
    await expect(page.locator('#hcp118-conflict')).toContainText(
      'Proposed town'
    );
    await expect(title).toHaveValue('Exact proposed carrots');
    await expect(title).toHaveAttribute('readonly', '');
    const retained = await page.evaluate(() => window.hcp118Fixture.drafts());
    expect(retained).toHaveLength(2);
    expect(retained.find((row) => row.form.title === 'Carrots')).toBeDefined();
    expect(
      retained.find((row) => row.form.title === 'Exact proposed carrots')?.form
        .location
    ).toBe('Proposed town');
    expect(await page.evaluate(() => window.hcp118Fixture.counts().signs)).toBe(
      before
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth
      )
    ).toBe(true);
    await expect(
      page.getByRole('button', { name: /publish|merge/i })
    ).toHaveCount(0);
    expect(effects).toHaveLength(0);
  });
