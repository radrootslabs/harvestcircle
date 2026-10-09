import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-list.ts';
declare global {
  interface Window {
    hcp102: typeof Fixture;
    hcp102Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
    hcp102Pending?: Promise<boolean>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let requests: unknown[][] = [],
  events: unknown[][] = [],
  closes: unknown[][] = [],
  olderEvent: unknown;
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
      if (frame[0] === 'EVENT') events.push(frame);
      if (frame[0] === 'CLOSE') closes.push(frame);
      if (frame[0] === 'REQ') {
        requests.push(frame);
        const filter = frame[2] as Record<string, unknown>;
        if (filter.limit !== 0) {
          if (filter.until !== undefined && olderEvent)
            socket.send(JSON.stringify(['EVENT', frame[1], olderEvent]));
          socket.send(JSON.stringify(['EOSE', frame[1]]));
        }
      }
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
          new URL('./harness/inbox-list.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/inbox-list.ts', import.meta.url)
        ),
        name: 'hcp102',
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
  requests = [];
  events = [];
  closes = [];
  olderEvent = undefined;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://archive.example.org',
              'wss://discovery.example.org'
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
    await page.evaluate(async () => {
      window.hcp102Fixture?.settle();
      await window.hcp102Pending;
      await window.hcp102Fixture?.close();
    });
});
async function ready(
  page: import('@playwright/test').Page,
  kind: 'inbound' | 'self_archive' | 'hostile_text' = 'inbound',
  qualified = false,
  variant: 'single' | 'duplicate' | 'mixed' = 'single'
) {
  await page.evaluate(
    async ({ kind, qualified, variant }) => {
      window.hcp102Fixture = await window.hcp102.renderFixture(
        kind,
        qualified,
        variant
      );
    },
    { kind, qualified, variant }
  );
  await page.getByRole('button', { name: 'Check current preference' }).click();
}
async function unlock(page: import('@playwright/test').Page) {
  await page
    .getByRole('button', { name: 'Unlock messages', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp102Fixture.list().status))
    .toBe('ready');
}
test('production guest never renders request previews or background notification fiction', async ({
  page
}) => {
  await page.goto(server.url + '/messages');
  await expect(
    page.getByRole('heading', { name: 'Connect or unlock' })
  ).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Message requests' })
  ).toHaveCount(0);
  await expect(page.getByText('Actual encrypted room fixture')).toHaveCount(0);
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('actual stored incoming ciphertext remains unpreviewed while original page is locked', async ({
  page
}) => {
  await ready(page);
  await expect(
    page.getByRole('button', { name: 'Unlock messages', exact: true })
  ).toBeVisible();
  await expect(page.getByText('Actual encrypted room fixture')).toHaveCount(0);
  await expect(
    page.getByRole('heading', { name: 'Message requests' })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(requests).toEqual([]);
});
test('genuine authenticated incoming peer is one locally New request after explicit SDK decrypt', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await expect(
    page.getByRole('heading', { name: 'Message requests', exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Conversations', exact: true })
  ).toHaveCount(0);
  await expect(
    page.getByText('Actual encrypted room fixture', { exact: true })
  ).toBeVisible();
  await expect(page.getByText('New', { exact: true })).toHaveCount(1);
  const view = await page.evaluate(() => window.hcp102Fixture.list());
  expect(view.requests).toHaveLength(1);
  expect(view.requests[0].peer).toBe(
    await page.evaluate(() => window.hcp102Fixture.peer)
  );
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toMatchObject(
    { decrypts: 2, signs: 0, encrypts: 0 }
  );
  expect(requests).toEqual([]);
});
test('genuine self archive is a local conversation without outgoing New or empty request clutter', async ({
  page
}) => {
  await ready(page, 'self_archive');
  await unlock(page);
  await expect(
    page.getByRole('heading', { name: 'Conversations', exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Message requests', exact: true })
  ).toHaveCount(0);
  await expect(page.getByText('New', { exact: true })).toHaveCount(0);
  await expect(
    page.getByText('You: Actual encrypted room fixture', { exact: true })
  ).toBeVisible();
  expect(
    (await page.evaluate(() => window.hcp102Fixture.list())).conversations
  ).toHaveLength(1);
  expect(events).toEqual([]);
});
test('one authenticated pair with incoming and self archive stays one local conversation', async ({
  page
}) => {
  await ready(page, 'inbound', false, 'mixed');
  await unlock(page);
  const view = await page.evaluate(() => window.hcp102Fixture.list());
  expect(view.requests).toEqual([]);
  expect(view.conversations).toHaveLength(1);
  expect(view.conversations[0]).toMatchObject({ count: 2, unread: 1 });
  await expect(page.getByText('New', { exact: true })).toHaveCount(1);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('two actual different outer wraps of one rumor produce one request and one local New', async ({
  page
}) => {
  await ready(page, 'inbound', false, 'duplicate');
  await unlock(page);
  const view = await page.evaluate(() => window.hcp102Fixture.list());
  expect(view.requests).toHaveLength(1);
  expect(view.requests[0]).toMatchObject({ count: 1, unread: 1 });
  expect(
    (await page.evaluate(() => window.hcp102Fixture.native())).received
  ).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toMatchObject(
    { decrypts: 4 }
  );
  await expect(page.getByText('New', { exact: true })).toHaveCount(1);
});
test('actual original local read flag suppresses New without changing request acceptance or notifying a peer', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await expect(page.getByText('New', { exact: true })).toHaveCount(1);
  const saved = await page.evaluate(() => window.hcp102Fixture.markRead());
  expect(saved?.status).toBe('saved');
  await page.evaluate(() => window.hcp102Fixture.next());
  await expect(page.getByText('New', { exact: true })).toHaveCount(0);
  expect(
    (await page.evaluate(() => window.hcp102Fixture.list())).requests
  ).toHaveLength(1);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('explicit original row mapping returns opaque local UUID and does not mark read or persist plaintext', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const before = await page.evaluate(() => window.hcp102Fixture.native());
  expect(before.pairs).toEqual([]);
  await page.getByRole('button', { name: /Open conversation/ }).click();
  await expect
    .poll(() => page.evaluate(() => window.hcp102Fixture.navigations().length))
    .toBe(1);
  const href = await page.evaluate(() => window.hcp102Fixture.navigations()[0]);
  expect(href).toMatch(
    /^\/messages\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
  );
  const after = await page.evaluate(() => window.hcp102Fixture.native());
  expect(after.pairs).toHaveLength(1);
  expect(after.received).toEqual(before.received);
  expect(JSON.stringify(after)).not.toContain('Actual encrypted room fixture');
  await expect(page.getByText('New', { exact: true })).toHaveCount(1);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('copied page and scalar peer arguments cannot import rows or native mapping/history effects', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const before = await page.evaluate(() => window.hcp102Fixture.native()),
    delta = await page.evaluate(() => window.hcp102Fixture.delta()),
    copied = await page.evaluate(() => window.hcp102Fixture.copied());
  expect(copied).toMatchObject({
    list: { status: 'unavailable', requests: [], conversations: [] },
    older: false
  });
  expect(copied.open).toBeUndefined();
  expect(await page.evaluate(() => window.hcp102Fixture.native())).toEqual(
    before
  );
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toEqual(
    delta
  );
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('Stop removes decrypted rows and denies stale original row action before any native change', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const before = await page.evaluate(() => window.hcp102Fixture.native());
  await page
    .getByRole('button', { name: 'Lock messages', exact: true })
    .click();
  await expect(page.getByText('Actual encrypted room fixture')).toHaveCount(0);
  await expect(page.getByText('New', { exact: true })).toHaveCount(0);
  expect(
    await page.evaluate(() => window.hcp102Fixture.open())
  ).toBeUndefined();
  expect(await page.evaluate(() => window.hcp102Fixture.native())).toEqual(
    before
  );
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('logout during actual held decrypt cannot render old-generation previews after SDK settlement', async ({
  page
}) => {
  await ready(page);
  await page.evaluate(() => {
    window.hcp102Fixture.mode('hold_decrypt');
    window.hcp102Pending = window.hcp102Fixture.unlock();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp102Fixture.pending()))
    .toBe(true);
  await page.evaluate(() => window.hcp102Fixture.logout());
  await expect(page.getByText('Actual encrypted room fixture')).toHaveCount(0);
  await page.evaluate(async () => {
    window.hcp102Fixture.settle();
    await window.hcp102Pending;
  });
  await expect(page.getByText('Actual encrypted room fixture')).toHaveCount(0);
  expect(
    (await page.evaluate(() => window.hcp102Fixture.list())).requests
  ).toEqual([]);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('partial unlocked local list has visible limitations and never remote Seen or a live badge', async ({
  page
}) => {
  await ready(page, 'hostile_text');
  await unlock(page);
  await expect(
    page.getByText(
      'Inbox access has not been qualified and exercised. Unlocked local copies do not prove a successful relay check.',
      { exact: true }
    )
  ).toBeVisible();
  await expect(page.getByText(/https:\/\/untrusted.example.org/)).toBeVisible();
  await expect(page.locator('a[href*="untrusted.example.org"]')).toHaveCount(0);
  await expect(
    page.getByText(/^(Live|Seen|Read by|Order accepted)$/)
  ).toHaveCount(0);
  await expect(
    page.getByRole('heading', { name: 'Conversations', exact: true })
  ).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('unqualified original Load older stays unavailable without private REQ or SDK work', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const before = await page.evaluate(() => window.hcp102Fixture.delta());
  expect(await page.evaluate(() => window.hcp102Fixture.older())).toBe(false);
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toEqual(
    before
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('qualified explicit Load older uses original outer cursor and retains a real relay-only cipher without automatic decrypt', async ({
  page
}) => {
  await ready(page, 'inbound', true);
  await unlock(page);
  await page
    .getByRole('button', { name: 'Check for messages', exact: true })
    .click();
  await expect.poll(() => requests.length).toBe(3);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp102Fixture.snapshot().lastCheckedAt)
    )
    .not.toBeNull();
  const before = await page.evaluate(() => window.hcp102Fixture.native()),
    delta = await page.evaluate(() => window.hcp102Fixture.delta());
  olderEvent = JSON.parse(
    await page.evaluate(() => window.hcp102Fixture.olderWire)
  );
  await page
    .getByRole('button', { name: 'Load older messages', exact: true })
    .click();
  await expect.poll(() => requests.length).toBe(4);
  const filter = requests[3][2] as Record<string, unknown>;
  expect(filter.limit).toBe(200);
  expect(filter.until).toBe(1699999800);
  await expect
    .poll(() =>
      page.evaluate(
        async () => (await window.hcp102Fixture.native()).received.length
      )
    )
    .toBe(before.received.length + 1);
  expect(await page.evaluate(() => window.hcp102Fixture.delta())).toEqual(
    delta
  );
  await expect(
    page.getByRole('button', { name: 'Read new messages (1)', exact: true })
  ).toBeVisible();
  expect(
    (await page.evaluate(() => window.hcp102Fixture.list())).requests
  ).toHaveLength(1);
  await page
    .getByRole('button', { name: 'Read new messages (1)', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp102Fixture.delta().decrypts))
    .toBe(delta.decrypts + 2);
  expect(
    (await page.evaluate(() => window.hcp102Fixture.list())).requests[0].count
  ).toBe(1);
  expect(events).toEqual([]);
  expect(
    requests.filter(
      (frame) => (frame[2] as Record<string, unknown>).limit === 0
    )
  ).toHaveLength(1);
});
