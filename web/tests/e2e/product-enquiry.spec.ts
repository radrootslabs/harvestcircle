import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/product-enquiry.ts';
declare global {
  interface Window {
    hcp106: typeof Fixture;
    hcp106Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
type Mode =
  | 'normal'
  | 'missing'
  | 'wrong_author'
  | 'wrong_coordinate'
  | 'invalid_signature'
  | 'sold'
  | 'deleted'
  | 'partial';
let mode: Mode = 'normal',
  requests: unknown[][] = [],
  events: unknown[][] = [];
const secret = new Uint8Array(32).fill(84),
  otherSecret = new Uint8Array(32).fill(90),
  peer = getPublicKey(secret);
function listing(createdAt = 1700000060) {
  const event = finalizeEvent(
    {
      kind: 30402,
      created_at: createdAt,
      content: 'Independently signed carrots',
      tags: [
        ['d', mode === 'wrong_coordinate' ? 'apples' : 'carrots'],
        ['title', 'Verified carrots'],
        ['summary', 'Actual signed listing'],
        ['published_at', '1700000000'],
        ['location', 'Victoria'],
        ['price', '3.5', 'CAD'],
        ['radroots:price_unit', 'lb'],
        ['status', mode === 'sold' ? 'sold' : 'active']
      ]
    },
    mode === 'wrong_author' ? otherSecret : secret
  );
  return mode === 'invalid_signature'
    ? { ...event, sig: '0'.repeat(128) }
    : event;
}
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string') throw Error('no loopback');
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
        events.push(frame);
        return;
      }
      if (frame[0] !== 'REQ') return;
      requests.push(frame);
      const filters = frame.slice(2) as { kinds?: number[]; ids?: string[] }[];
      const send = () => {
        if (mode !== 'missing') {
          if (filters.some((f) => f.kinds?.includes(0)))
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                finalizeEvent(
                  {
                    kind: 0,
                    created_at: 1700000060,
                    tags: [],
                    content: JSON.stringify({ name: 'Another display name' })
                  },
                  secret
                )
              ])
            );
          if (filters.some((f) => f.kinds?.includes(30402))) {
            const exact = filters.find((f) => f.ids)?.ids;
            const event = exact?.includes(listing(1700000050).id)
              ? listing(1700000050)
              : listing();
            socket.send(JSON.stringify(['EVENT', frame[1], event]));
          }
          if (mode === 'deleted' && filters.some((f) => f.kinds?.includes(5)))
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                finalizeEvent(
                  {
                    kind: 5,
                    created_at: 1700000070,
                    tags: [['a', '30402:' + peer + ':carrots']],
                    content: ''
                  },
                  secret
                )
              ])
            );
        }
        if (!(
          mode === 'partial' &&
          filters.some((filter) => filter.kinds?.includes(5))
        ))
          socket.send(JSON.stringify(['EOSE', frame[1]]));
      };
      send();
    })
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [svelte({ configFile: false })],
    resolve: {
      conditions: ['browser'],
      alias: {
        '$app/navigation': fileURLToPath(
          new URL('./harness/product-enquiry.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/product-enquiry.ts', import.meta.url)
        ),
        name: 'hcp106',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('no bundle');
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
  mode = 'normal';
  requests = [];
  events = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://discovery.example.org',
              'wss://archive.example.org',
              'wss://peer.example.org'
            ].includes(new URL(String(url)).origin)
          )
            throw Error('unapproved destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});

test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp106Fixture?.close());
});
async function ready(page: Page, existing = false) {
  await page.evaluate(async (existing) => {
    window.hcp106Fixture = await window.hcp106.renderFixture(existing);
  }, existing);
  await expect(page.locator('[data-enquiry-fixture]')).toBeVisible();
  if (existing)
    await expect
      .poll(() =>
        page.evaluate(async () =>
          (await window.hcp106Fixture.native()).received.every(
            (row) =>
              (JSON.parse((row as { wire: string }).wire) as { read: unknown })
                .read !== null
          )
        )
      )
      .toBe(true);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp106Fixture.viewSnapshot().outcome)
    )
    .not.toBe('checking');
}
const panel = (page: Page) => page.locator('[data-enquiry-fixture]');
const open = (page: Page) =>
  panel(page)
    .getByRole('button', { name: 'Message seller', exact: true })
    .click();
test('explicit guest enquiry captures the signed publisher and readable viewed context without connection or Send', async ({
  page
}) => {
  await ready(page);
  const before = requests.length;
  await expect(
    panel(page).getByRole('textbox', { name: 'Your message', exact: true })
  ).toHaveCount(0);
  await open(page);
  await expect(
    panel(page).getByRole('textbox', { name: 'Your message', exact: true })
  ).toBeVisible();
  await expect(panel(page)).toContainText('Verified carrots');
  await expect(panel(page)).toContainText(peer);
  await expect(panel(page)).toContainText('not a confirmed order');
  expect(await page.evaluate(() => window.hcp106Fixture.snapshot()?.peer)).toBe(
    peer
  );
  expect(requests).toHaveLength(before);
  expect(events).toEqual([]);
  expect(await page.evaluate(() => window.hcp106Fixture.delta())).toEqual({
    decrypts: 0,
    signs: 0,
    encrypts: 0
  });
});
test('exact canonical coordinate and signed listing version survive capture and the first message has no parent', async ({
  page
}) => {
  await ready(page);
  await open(page);
  const actual = await page.evaluate(() => ({
    snapshot: window.hcp106Fixture.snapshot(),
    message: window.hcp106Fixture.message()
  }));
  expect(actual.snapshot?.eventId).toBe(listing().id);
  expect(actual.message?.tags).toEqual([['p', peer]]);
  expect(actual.message?.content).toContain('nostr:' + actual.snapshot?.naddr);
  expect(actual.message?.content).toContain('nostr:' + actual.snapshot?.nevent);
  expect(actual.message?.content).toContain('3.5 CAD/lb');
  expect(actual.message?.content).toContain('not a confirmed order');
  expect(
    actual.message?.tags.some((tag) =>
      ['e', 'a', 'q', 'order', 'thread'].includes(tag[0])
    )
  ).toBe(false);
  expect(events).toEqual([]);
});
test('editable amount timing and fake recipient assertions never change the publisher references or inventory', async ({
  page
}) => {
  await ready(page, true);
  const before = await page.evaluate(() => window.hcp106Fixture.native());
  await open(page);
  const text = '2 kg Thursday. Send to ' + getPublicKey(otherSecret);
  await panel(page)
    .getByRole('textbox', { name: 'Your message', exact: true })
    .fill(text);
  const message = await page.evaluate(() => window.hcp106Fixture.message());
  expect(message?.tags).toEqual([['p', peer]]);
  expect(message?.content).toContain(text);
  expect(message?.content).toContain('nostr:');
  expect(await page.evaluate(() => window.hcp106Fixture.native())).toEqual(
    before
  );
  expect(await page.evaluate(() => window.hcp106Fixture.delta())).toEqual({
    decrypts: 0,
    signs: 0,
    encrypts: 0
  });
  expect(events).toEqual([]);
});
test('a display name never becomes recipient authority or creates a shop route', async ({
  page
}) => {
  await ready(page);
  await open(page);
  expect(
    await page.evaluate(() => window.hcp106Fixture.message()?.tags)
  ).toEqual([['p', peer]]);
  expect(
    await panel(page)
      .locator('a[href*="/shops/"],a[href*="/profiles/"]')
      .count()
  ).toBe(0);
  expect(events).toEqual([]);
});
test('only an actual admitted original pair can be explicitly reused without a new UUID or message parent', async ({
  page
}) => {
  await ready(page, true);
  const before = await page.evaluate(() => window.hcp106Fixture.native());
  await open(page);
  expect(await page.evaluate(() => window.hcp106Fixture.reuse())).toBe(
    await page.evaluate(() => window.hcp106Fixture.knownPath())
  );
  expect(await page.evaluate(() => window.hcp106Fixture.native())).toEqual(
    before
  );
  expect(
    await page.evaluate(() => window.hcp106Fixture.message()?.tags)
  ).toEqual([['p', peer]]);
  expect(events).toEqual([]);
});
test('copied view enquiry and inbox tokens cannot borrow publisher private draft or conversation authority', async ({
  page
}) => {
  await ready(page, true);
  await open(page);
  const before = await page.evaluate(() => window.hcp106Fixture.native());
  expect(await page.evaluate(() => window.hcp106Fixture.copied())).toEqual({
    view: false,
    snapshot: undefined,
    message: undefined,
    update: false,
    reuse: undefined
  });
  expect(await page.evaluate(() => window.hcp106Fixture.native())).toEqual(
    before
  );
  expect(events).toEqual([]);
});
test('unreviewed capture editing and reuse do not change the original memory-only draft', async ({
  page
}) => {
  await ready(page, true);
  await open(page);
  const before = await page.evaluate(() => window.hcp106Fixture.snapshot());
  expect(await page.evaluate(() => window.hcp106Fixture.unreviewed())).toEqual({
    capture: false,
    update: false,
    reuse: undefined
  });
  expect(await page.evaluate(() => window.hcp106Fixture.snapshot())).toEqual(
    before
  );
  expect(events).toEqual([]);
});
test('private draft text is absent from URLs local storage and actual native persistence', async ({
  page
}) => {
  await ready(page, true);
  await open(page);
  const text = 'PRIVATE HCP106 guest enquiry zebra';
  await panel(page)
    .getByRole('textbox', { name: 'Your message', exact: true })
    .fill(text);
  expect(
    await page.evaluate(() => window.hcp106Fixture.persistentText())
  ).not.toContain(text);
  expect(events).toEqual([]);
});
test('closing the original product view invalidates its draft and cannot produce a later message', async ({
  page
}) => {
  await ready(page);
  await open(page);
  await page.evaluate(() => window.hcp106Fixture.closeView());
  expect(
    await page.evaluate(() => window.hcp106Fixture.snapshot())
  ).toBeUndefined();
  expect(
    await page.evaluate(() => window.hcp106Fixture.message())
  ).toBeUndefined();
  expect(await page.evaluate(() => window.hcp106Fixture.update('late'))).toBe(
    false
  );
  expect(events).toEqual([]);
});
for (const rejection of [
  'missing',
  'wrong_author',
  'wrong_coordinate',
  'invalid_signature',
  'sold',
  'deleted'
] as const)
  test(
    'unavailable or untrusted listing cannot allocate an enquiry: ' + rejection,
    async ({ page }) => {
      mode = rejection;
      await ready(page);
      expect(await page.evaluate(() => window.hcp106Fixture.capture())).toBe(
        false
      );
      expect(
        await page.evaluate(() => window.hcp106Fixture.message())
      ).toBeUndefined();
      await expect(
        panel(page).getByRole('textbox', { name: 'Your message', exact: true })
      ).toHaveCount(0);
      expect(events).toEqual([]);
    }
  );
test('incomplete deletion coverage is not silently admitted as current enquiry context', async ({
  page
}) => {
  mode = 'partial';
  await ready(page);
  await page.evaluate(() => window.hcp106Fixture.advance());
  expect(
    await page.evaluate(() => window.hcp106Fixture.viewSnapshot().lastKnown)
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp106Fixture.capture())).toBe(false);
  expect(events).toEqual([]);
});

test('original Close revokes mounted private guest text and request controls without side effects', async ({
  page
}) => {
  await ready(page, true);
  await open(page);
  const text = 'PRIVATE HCP106 terminal guest text';
  await panel(page)
    .getByRole('textbox', { name: 'Your message', exact: true })
    .fill(text);
  const before = await page.evaluate(async () => ({
    native: await window.hcp106Fixture.native(),
    delta: window.hcp106Fixture.delta()
  }));
  const queryCount = requests.length;
  await page.evaluate(() => window.hcp106Fixture.closeView());
  expect(
    await page.evaluate(() => window.hcp106Fixture.message())
  ).toBeUndefined();
  expect(
    await panel(page)
      .getByRole('textbox', { name: 'Your message', exact: true })
      .count()
  ).toBe(0);
  expect(
    await panel(page)
      .getByRole('button', { name: 'Send request', exact: true })
      .count()
  ).toBe(0);
  expect(await panel(page).textContent()).not.toContain(text);
  expect(await panel(page).textContent()).not.toContain('Viewed listing:');
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp106Fixture.native(),
      delta: window.hcp106Fixture.delta()
    }))
  ).toEqual(before);
  expect(requests).toHaveLength(queryCount);
  expect(events).toEqual([]);
});

test('ordinary original finite run expiry preserves an already captured unsent guest draft', async ({
  page
}) => {
  await ready(page);
  await open(page);
  const text = 'PRIVATE HCP106 unsent after finite run';
  await panel(page)
    .getByRole('textbox', { name: 'Your message', exact: true })
    .fill(text);
  const queryCount = requests.length;
  await page.evaluate(() => window.hcp106Fixture.advance());
  expect(await page.evaluate(() => window.hcp106Fixture.snapshot()?.text)).toBe(
    text
  );
  expect(
    await page.evaluate(() => window.hcp106Fixture.snapshot()?.listingCurrent)
  ).toBe(false);
  expect(
    await page.evaluate(() => window.hcp106Fixture.message())
  ).toBeUndefined();
  await expect(
    panel(page).getByRole('textbox', { name: 'Your message', exact: true })
  ).toHaveValue(text);
  await expect(panel(page)).toContainText(
    'Listing status needs review. Your unsent text is kept in memory.'
  );
  expect(
    await page.evaluate(() =>
      window.hcp106Fixture.update('Still editing after expiry')
    )
  ).toBe(true);
  await expect(
    panel(page).getByRole('textbox', { name: 'Your message', exact: true })
  ).toHaveValue('Still editing after expiry');
  expect(
    await page.evaluate(() => window.hcp106Fixture.message())
  ).toBeUndefined();
  expect(requests).toHaveLength(queryCount);
  expect(events).toEqual([]);
});
