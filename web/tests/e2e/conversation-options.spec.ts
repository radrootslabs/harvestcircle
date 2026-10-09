import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/conversation-options.ts';
declare global {
  interface Window {
    hcp105: typeof Fixture;
    hcp105Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
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
  | 'partial'
  | 'hold';
let mode: Mode = 'normal',
  requests: unknown[][] = [],
  events: unknown[][] = [],
  release = () => {};
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
      if (mode === 'hold') release = send;
      else send();
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
          new URL('./harness/conversation-options.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/conversation-options.ts', import.meta.url)
        ),
        name: 'hcp105',
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
  release = () => {};
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
    await page.evaluate(() => window.hcp105Fixture?.close());
});
async function ready(
  page: Page,
  scenario: Fixture.OptionsScenario = 'contexts'
) {
  await page.evaluate(async (scenario) => {
    window.hcp105Fixture = await window.hcp105.renderFixture(scenario);
  }, scenario);
  await expect(page.locator('[data-options-fixture]')).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await window.hcp105Fixture.native()).received.every(
          (row) =>
            (JSON.parse((row as { wire: string }).wire) as { read: unknown })
              .read !== null
        )
      )
    )
    .toBe(true);
}
const panel = (page: Page) => page.locator('[data-options-fixture]');
const verify = (page: Page) =>
  panel(page)
    .getByRole('button', { name: 'Verify product citation', exact: true })
    .click();

test('mount and conversation disclosure show the original authenticated peer without discovery native writes or profile routes', async ({
  page
}) => {
  await ready(page);
  const before = await page.evaluate(async () => ({
    native: await window.hcp105Fixture.native(),
    delta: window.hcp105Fixture.delta()
  }));
  await panel(page)
    .getByText('Identity and conversation options', { exact: true })
    .click();
  await expect(panel(page)).toContainText(peer);
  expect(await page.evaluate(() => window.hcp105Fixture.options()?.peer)).toBe(
    peer
  );
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
  expect(
    await panel(page).locator('a[href*="/profiles"],a[href*="/shops"]').count()
  ).toBe(0);
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp105Fixture.native(),
      delta: window.hcp105Fixture.delta()
    }))
  ).toEqual(before);
});
test('local block and report hooks receive the authenticated inner peer without implementing private disclosure or public reports', async ({
  page
}) => {
  await ready(page);
  await panel(page)
    .getByText('Identity and conversation options', { exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Block person', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Report conversation', exact: true })
    .click();
  expect(await page.evaluate(() => window.hcp105Fixture.hooks())).toEqual([
    { action: 'block', peer },
    { action: 'report', peer }
  ]);
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('explicit citation verification uses actual independently signed pair publisher and fixed bounded public queries', async ({
  page
}) => {
  await ready(page);
  const before = await page.evaluate(async () => ({
    native: await window.hcp105Fixture.native(),
    delta: window.hcp105Fixture.delta()
  }));
  await verify(page);
  await expect(panel(page)).toContainText('Verified carrots');
  await expect(panel(page)).toContainText('Signed listing observed');
  expect(requests.length).toBeGreaterThan(0);
  for (const frame of requests)
    for (const filter of frame.slice(2) as {
      kinds: number[];
      authors: string[];
      limit: number;
    }[]) {
      expect(filter.kinds.every((kind) => [30402, 5].includes(kind))).toBe(
        true
      );
      expect(filter.authors).toEqual([peer]);
      expect(filter.limit).toBe(200);
    }
  expect(events).toEqual([]);
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp105Fixture.native(),
      delta: window.hcp105Fixture.delta()
    }))
  ).toEqual(before);
  await expect(panel(page)).toContainText('not a current or locked quote');
});
for (const rejection of [
  'missing',
  'wrong_author',
  'wrong_coordinate',
  'invalid_signature'
] as const)
  test('untrusted listing stays unresolved: ' + rejection, async ({ page }) => {
    mode = rejection;
    await ready(page);
    await verify(page);
    await expect(panel(page)).toContainText(
      'Listing citation remains unresolved'
    );
    await expect(panel(page)).not.toContainText('Signed listing observed');
    expect(events).toEqual([]);
  });
test('sold listing is independently observed without reserving inventory or accepting an order', async ({
  page
}) => {
  mode = 'sold';
  await ready(page);
  const native = await page.evaluate(() => window.hcp105Fixture.native());
  await verify(page);
  await expect(panel(page)).toContainText('Listing is sold');
  expect(await page.evaluate(() => window.hcp105Fixture.native())).toEqual(
    native
  );
  expect(events).toEqual([]);
});
test('actual signed public deletion keeps the citation withdrawn rather than trusted', async ({
  page
}) => {
  mode = 'deleted';
  await ready(page);
  await verify(page);
  await expect(panel(page)).toContainText('Listing is withdrawn');
  await expect(panel(page)).not.toContainText('Signed listing observed');
  expect(events).toEqual([]);
});
test('unknown private parent and arbitrary body link never spawn a public hash query or external navigation', async ({
  page
}) => {
  await ready(page, 'parent_reference');
  await expect(panel(page)).toContainText('Earlier message unavailable');
  await expect(panel(page)).toContainText('https://untrusted.example.org');
  expect(
    await page.evaluate(() => window.hcp105Fixture.capture('f'.repeat(64)))
  ).toBe(false);
  expect(requests).toEqual([]);
  expect(await panel(page).locator('a[href^="https://"]').count()).toBe(0);
  expect(events).toEqual([]);
  await verify(page);
  await expect(panel(page)).toContainText('Verified carrots');
  expect(JSON.stringify(requests)).not.toContain('f'.repeat(64));
});
test('included exact public version is independently verified from the genuine original message', async ({
  page
}) => {
  await ready(page, 'exact');
  await verify(page);
  await expect(panel(page)).toContainText('Included version verified');
  const id = await page.evaluate(() => window.hcp105Fixture.citedVersion);
  expect(
    requests.some((frame) =>
      (frame.slice(2) as { ids?: string[] }[]).some((filter) =>
        filter.ids?.includes(id)
      )
    )
  ).toBe(true);
  expect(events).toEqual([]);
});
test('a genuinely signed older included version stays distinct from the observed current listing and cannot lock a quote', async ({
  page
}) => {
  await ready(page, 'stale');
  await verify(page);
  await expect(panel(page)).toContainText(
    'Included version differs from the observed listing'
  );
  await expect(panel(page)).not.toContainText('Included version verified');
  await expect(panel(page)).toContainText('not a current or locked quote');
  expect(events).toEqual([]);
});
test('copied view runtime and citation tokens cannot borrow independent verification or option authority', async ({
  page
}) => {
  await ready(page);
  expect(
    await page.evaluate(() =>
      window.hcp105Fixture.capture(undefined, 'unreviewed')
    )
  ).toBe(false);
  expect(requests).toEqual([]);
  expect(await page.evaluate(() => window.hcp105Fixture.copied())).toEqual({
    view: false,
    runtime: false,
    snapshot: undefined,
    options: undefined,
    hook: false
  });
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('logout during actual held public response cannot revive private citation context or invoke local hooks', async ({
  page
}) => {
  mode = 'hold';
  await ready(page);
  await verify(page);
  await expect.poll(() => requests.length).toBe(1);
  await page.evaluate(() => window.hcp105Fixture.logout());
  mode = 'normal';
  release();
  await expect(panel(page)).not.toContainText('Verified carrots');
  await expect(panel(page)).not.toContainText(peer);
  expect(events).toEqual([]);
  expect(await page.evaluate(() => window.hcp105Fixture.hooks())).toEqual([]);
});
test('unmount closes the original held query without private writes or late context rendering', async ({
  page
}) => {
  mode = 'hold';
  await ready(page);
  const native = await page.evaluate(() => window.hcp105Fixture.native());
  await verify(page);
  await expect.poll(() => requests.length).toBe(1);
  await page.evaluate(() => window.hcp105Fixture.closePanel());
  mode = 'normal';
  release();
  await expect(panel(page)).toHaveCount(0);
  expect(events).toEqual([]);
  expect(await page.evaluate(() => window.hcp105Fixture.native())).toEqual(
    native
  );
});

test('expired signed listing with incomplete deletion coverage remains visibly unresolved', async ({
  page
}) => {
  mode = 'partial';
  await ready(page, 'exact');
  const before = await page.evaluate(async () => ({
    native: await window.hcp105Fixture.native(),
    delta: window.hcp105Fixture.delta()
  }));
  await verify(page);
  await expect
    .poll(() =>
      requests.some((frame) =>
        (frame.slice(2) as { kinds: number[] }[]).some((filter) =>
          filter.kinds.includes(5)
        )
      )
    )
    .toBe(true);
  await page.evaluate(() => window.hcp105Fixture.advance());
  await expect(panel(page)).toContainText(
    'Listing citation remains unresolved'
  );
  await expect(panel(page)).not.toContainText('Included version verified');
  await expect(panel(page)).not.toContainText('Signed listing observed');
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp105Fixture.native(),
      delta: window.hcp105Fixture.delta()
    }))
  ).toEqual(before);
  expect(events).toEqual([]);
});

for (const action of ['logout', 'selection', 'close'] as const)
  test(
    'standalone original message revocation clears private rendering and stale Reply: ' +
      action,
    async ({ page }) => {
      await ready(page, 'exact');
      await expect(panel(page).locator('article[data-message-id]')).toHaveCount(
        1
      );
      await expect(panel(page)).toContainText(
        'Quoted price: 4 GBP https://untrusted.example.org'
      );
      await expect(
        panel(page).getByRole('button', {
          name: 'Reply to this message',
          exact: true
        })
      ).toBeVisible();
      const before = await page.evaluate(async () => ({
        native: await window.hcp105Fixture.native(),
        delta: window.hcp105Fixture.delta()
      }));
      await page.evaluate(
        (action) => window.hcp105Fixture.revokeAndClick(action),
        action
      );
      expect(
        await page.evaluate(() => window.hcp105Fixture.currentMessage())
      ).toBe(false);
      if (action === 'close')
        expect(
          await page.evaluate(() => window.hcp105Fixture.terminal())
        ).toEqual(['reentered', 'survived']);
      expect(await page.evaluate(() => window.hcp105Fixture.hooks())).toEqual(
        []
      );
      await expect(panel(page).locator('article[data-message-id]')).toHaveCount(
        0
      );
      await expect(panel(page)).not.toContainText(
        'Quoted price: 4 GBP https://untrusted.example.org'
      );
      await expect(
        panel(page).getByRole('button', {
          name: 'Reply to this message',
          exact: true
        })
      ).toHaveCount(0);
      await expect(panel(page).locator('a[href*="/products/"]')).toHaveCount(0);
      await expect(panel(page)).not.toContainText(peer);
      expect(
        await page.evaluate(async () => ({
          native: await window.hcp105Fixture.native(),
          delta: window.hcp105Fixture.delta()
        }))
      ).toEqual(before);
      expect(requests).toEqual([]);
      expect(events).toEqual([]);
    }
  );
