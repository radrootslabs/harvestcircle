import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { writeFile } from 'node:fs/promises';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/selling-index.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
declare global {
  interface Window {
    hcp121: typeof Fixture;
    hcp121Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: NostrEvent[] = [],
  effects = 0;
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 262144
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
        effects++;
        return;
      }
      if (frame[0] !== 'REQ') return;
      const filters = frame.slice(2) as {
        kinds?: number[];
        authors?: string[];
        ids?: string[];
        '#d'?: string[];
      }[];
      for (const event of events)
        if (
          filters.some(
            (filter) =>
              (!filter.kinds || filter.kinds.includes(event.kind)) &&
              (!filter.authors || filter.authors.includes(event.pubkey)) &&
              (!filter.ids || filter.ids.includes(event.id)) &&
              (!filter['#d'] ||
                event.tags.some(
                  (tag) => tag[0] === 'd' && filter['#d']!.includes(tag[1])
                ))
          )
        )
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
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
          new URL('./harness/selling-index.ts', import.meta.url)
        ),
        name: 'hcp121',
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
  events = [];
  effects = 0;
  await page.addInitScript((endpoint) => {
    const Native = window.WebSocket;
    window.WebSocket = class extends Native {
      constructor(url: string | URL, protocols?: string | string[]) {
        if (
          ![
            'wss://discovery.example.org',
            'wss://inbox.example.org',
            'wss://second.example.org'
          ].includes(new URL(url.toString()).origin)
        )
          throw Error('no live relay permission');
        super(endpoint, protocols);
      }
    };
  }, endpoint);
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    await window.hcp121Fixture?.close();
  });
});
async function start(
  page: import('@playwright/test').Page,
  mode: 'empty' | 'retained' = 'retained'
) {
  const event = await page.evaluate(async (mode) => {
    window.hcp121Fixture = await window.hcp121.createFixture(mode);
    return window.hcp121Fixture.event;
  }, mode);
  if (event) events.push(event);
}
async function render(page: import('@playwright/test').Page) {
  await page.evaluate(() => window.hcp121Fixture.render());
}
test('HCP121 genuine empty workspace has a real List food link and omits empty panels', async ({
  page
}) => {
  await start(page, 'empty');
  await render(page);
  const section = page.locator('#hcp121-selling');
  // Immediate DOM count supplies meaningful pre-producer RED without waiting
  // for a future link that the original AccountGate does not render.
  expect(
    await section.getByRole('link', { name: 'List food', exact: true }).count()
  ).toBe(1);
  await expect(section).toContainText('No listings yet.');
  for (const name of ['Needs attention', 'Drafts', 'Listings'])
    await expect(
      section.getByRole('heading', { name, exact: true })
    ).toHaveCount(0);
  await expect(
    section.getByRole('link', { name: 'List food', exact: true })
  ).toHaveAttribute('href', '/sell');
  expect(effects).toBe(0);
});
test('HCP121 unresolved original listing is one attention row without duplicate draft/listing and retains exact target detail', async ({
  page
}) => {
  await start(page);
  const before = await page.evaluate(() => window.hcp121Fixture.records());
  const counts = await page.evaluate(() => window.hcp121Fixture.counts());
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(
    section.getByRole('heading', { name: 'Needs attention', exact: true })
  ).toBeVisible();
  await expect(section.locator('article')).toHaveCount(1);
  await expect(
    section.getByRole('heading', { name: 'Drafts', exact: true })
  ).toHaveCount(0);
  await expect(
    section.getByRole('heading', { name: 'Listings', exact: true })
  ).toHaveCount(0);
  await section.getByText('Delivery details', { exact: true }).click();
  await expect(section).toContainText(
    'Relay acceptance recorded for 0 of 2 targets.'
  );
  await expect(section).toContainText(
    'Exact read-back recorded for 0 of 2 targets.'
  );
  expect(await page.evaluate(() => window.hcp121Fixture.records())).toEqual(
    before
  );
  expect((await page.evaluate(() => window.hcp121Fixture.counts())).signs).toBe(
    counts.signs
  );
  expect(effects).toBe(0);
  await writeFile(
    test.info().outputPath('hcp121-original-native-records.json'),
    JSON.stringify(before, null, 2)
  );
});
test('HCP121 explicit same-author reconnect reacquires original seller work without native mutation or publication', async ({
  page
}) => {
  await start(page);
  const before = await page.evaluate(() => window.hcp121Fixture.records());
  const counts = await page.evaluate(() => window.hcp121Fixture.counts());
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(section.locator('article')).toHaveCount(1);
  await page.evaluate(() => window.hcp121Fixture.reconnect());
  await expect(section.locator('article')).toHaveCount(1);
  await expect(
    section.getByRole('heading', { name: 'Needs attention', exact: true })
  ).toBeVisible();
  expect(await page.evaluate(() => window.hcp121Fixture.records())).toEqual(
    before
  );
  expect((await page.evaluate(() => window.hcp121Fixture.counts())).signs).toBe(
    counts.signs
  );
  expect(effects).toBe(0);
});
test('HCP121 changed proposed draft stays on its original attention row with exact saved values', async ({
  page
}) => {
  await start(page);
  const draft = await page.evaluate(() =>
    window.hcp121Fixture.changeCapturedDraft()
  );
  const before = await page.evaluate(() => window.hcp121Fixture.records());
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(
    section.getByRole('heading', { name: 'Needs attention', exact: true })
  ).toBeVisible();
  await expect(section.locator('article')).toHaveCount(1);
  await expect(
    section.getByRole('heading', { name: 'Drafts', exact: true })
  ).toHaveCount(0);
  await expect(
    section.getByRole('link', { name: 'Continue editing', exact: true })
  ).toHaveAttribute('href', '/selling/drafts/' + draft.id);
  expect(await page.evaluate(() => window.hcp121Fixture.records())).toEqual(
    before
  );
  expect(draft.form.title).toBe('Unfinished proposed carrots');
  expect(draft.form.description).toBe('');
  expect(effects).toBe(0);
});

test('HCP121 external own source stays visible and read-only with safe source-aware links', async ({
  page
}) => {
  await start(page);
  const event = await page.evaluate(() => window.hcp121Fixture.external());
  events = event ? [event] : [];
  await page.evaluate(() => window.hcp121Fixture.settled());
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(
    section.getByRole('heading', { name: 'Listings', exact: true })
  ).toBeVisible();
  await expect(section).toContainText('Read-only');
  await expect(
    section.getByRole('link', { name: 'Edit', exact: true })
  ).toHaveCount(0);
  await expect(
    section.getByRole('link', { name: 'View listing', exact: true })
  ).toHaveAttribute('href', /^\/products\/naddr1/);
  expect(effects).toBe(0);
});
test('HCP121 actual bounded remote-only own listing never becomes an adopted local source', async ({
  page
}) => {
  await start(page, 'empty');
  events = [await page.evaluate(() => window.hcp121Fixture.remoteListing())];
  const before = await page.evaluate(() => window.hcp121Fixture.records());
  expect(before).toEqual({ drafts: [], operations: [] });
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(
    section.getByRole('heading', { name: 'Listings', exact: true })
  ).toBeVisible();
  await expect(section).toContainText('Remote own eggs');
  await expect(section).toContainText('Bounded remote discovery');
  await expect(section).toContainText('Read-only');
  await expect(
    section.getByRole('link', { name: 'Edit', exact: true })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp121Fixture.records())).toEqual(
    before
  );
  expect(effects).toBe(0);
});

test('HCP121 actual installed account switch retires original local rows without adopting foreign records', async ({
  page
}) => {
  await start(page);
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(section.locator('article')).toHaveCount(1);
  const before = await page.evaluate(() => window.hcp121Fixture.records());
  const next = await page.evaluate(() => window.hcp121Fixture.switchOwner());
  await expect(section.locator('article')).toHaveCount(0);
  await expect(section).toContainText(
    'The extension identity changed. Connect again to continue.'
  );
  await page.evaluate(() => window.hcp121Fixture.reconnect());
  await expect(section).toContainText('No listings yet.');
  await expect(section.getByText('Carrots', { exact: true })).toHaveCount(0);
  expect(next).not.toBe(events[0].pubkey);
  expect(await page.evaluate(() => window.hcp121Fixture.records())).toEqual(
    before
  );
  expect(effects).toBe(0);
});

test('HCP121 settled retained listing and unfinished draft preserve ordering values links and minimal controls', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() => window.hcp121Fixture.settled());
  const draft = await page.evaluate(() => window.hcp121Fixture.draft());
  await render(page);
  const section = page.locator('#hcp121-selling');
  await expect(
    section.getByRole('heading', { name: 'Drafts', exact: true })
  ).toBeVisible();
  await expect(
    section.getByRole('heading', { name: 'Listings', exact: true })
  ).toBeVisible();
  await expect(
    section.getByRole('link', { name: 'Continue editing', exact: true })
  ).toHaveAttribute('href', '/selling/drafts/' + draft.id);
  await expect(
    section.getByRole('link', { name: 'Edit', exact: true })
  ).toHaveAttribute('href', /^\/products\/naddr1.+\/edit$/);
  await expect(section).toContainText('Unfinished spinach');
  await expect(
    section.getByRole('heading', { name: 'Needs attention', exact: true })
  ).toHaveCount(0);
  const headings = await section
    .locator(':scope > .page > section > h2')
    .allTextContents();
  expect(headings).toEqual(['Drafts', 'Listings']);
  expect(await section.getByRole('checkbox').count()).toBe(0);
  expect(
    await section
      .getByText(/revenue|orders|inventory total|bulk|performance graph/i)
      .count()
  ).toBe(0);
  expect(effects).toBe(0);
});
test('HCP121 original partial delivery keeps acceptance distinct from readback and disconnect retires account rows', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() =>
    window.hcp121Fixture.observed('accepted', 0, false)
  );
  await render(page);
  const section = page.locator('#hcp121-selling');
  await section.getByText('Delivery details', { exact: true }).click();
  await expect(section).toContainText(
    'Relay acceptance recorded for 1 of 2 targets.'
  );
  await expect(section).toContainText(
    'Exact read-back recorded for 0 of 2 targets.'
  );
  await page.evaluate(() => window.hcp121Fixture.disconnect());
  await expect(section.locator('article')).toHaveCount(0);
  await expect(
    section.getByRole('link', { name: 'Edit', exact: true })
  ).toHaveCount(0);
  expect(effects).toBe(0);
});
for (const width of [320, 1024])
  test(`HCP121 ${width}px index has readable rows and separately operable visible links`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await start(page);
    await render(page);
    const section = page.locator('#hcp121-selling');
    await expect(section.locator('article')).toHaveCount(1);
    await section.getByText('Delivery details', { exact: true }).click();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth
    );
    expect(overflow).toBe(false);
    expect(effects).toBe(0);
  });
