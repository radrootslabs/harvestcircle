import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import type { NostrEvent } from 'applesauce-core/helpers';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/public-hides.ts';
declare global {
  interface Window {
    hcp122: typeof Fixture;
    hcp122Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string | undefined;
let events: NostrEvent[] = [],
  effects = 0;
let queries: { authors?: string[]; kinds?: number[] }[] = [];
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
      if (frame[0] === 'EVENT' || frame[0] === 'AUTH') {
        effects++;
        return;
      }
      if (frame[0] !== 'REQ') return;
      const filters = frame.slice(2) as {
        authors?: string[];
        kinds?: number[];
        '#d'?: string[];
      }[];
      queries.push(...filters);
      for (const event of events)
        if (
          filters.some(
            (filter) =>
              (!filter.kinds || filter.kinds.includes(event.kind)) &&
              (!filter.authors || filter.authors.includes(event.pubkey)) &&
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
  queries = [];
  await page.addInitScript((endpoint) => {
    const Native = window.WebSocket;
    window.WebSocket = class extends Native {
      constructor(url: string | URL, protocols?: string | string[]) {
        if (
          !['wss://discovery.example.org', 'wss://inbox.example.org'].includes(
            new URL(url.toString()).origin
          )
        )
          throw Error('no live relay permission');
        super(endpoint, protocols);
      }
    };
  }, endpoint);
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    await window.hcp122Fixture?.close();
  });
});
async function start(page: Page, denied = false, retained = false) {
  await page.goto(server.url + '/privacy');
  if (!bundle) {
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
            new URL('./harness/public-hides.ts', import.meta.url)
          ),
          name: 'hcp122',
          formats: ['iife']
        }
      }
    });
    const result = Array.isArray(built) ? built[0] : built;
    if (!('output' in result)) throw Error('fixture bundle missing');
    const chunks = result.output.filter((row) => row.type === 'chunk');
    expect(chunks).toHaveLength(1);
    bundle = chunks[0].code;
  }
  await page.addScriptTag({ content: bundle });
  events = await page.evaluate(
    async ({ denied, retained }) => {
      window.hcp122Fixture = await window.hcp122.createFixture(
        denied,
        retained
      );
      return window.hcp122Fixture.events;
    },
    { denied, retained }
  );
  await page.evaluate(() => window.hcp122Fixture.render());
}
test('HCP122 SSR privacy states local hide consequences without private inventory', async ({
  page
}) => {
  await page.goto(server.url + '/privacy');
  // Immediate body assertion is a real pre-producer failure, not a future-module collection failure.
  expect(await page.locator('main').innerText()).toContain(
    'Hiding a publisher is local'
  );
  await expect(page.getByRole('button', { name: /Undo hide/ })).toHaveCount(0);
  expect(queries).toEqual([]);
  expect(effects).toBe(0);
});
test('HCP122 acknowledged hide persists native owner data and Undo restores eligible source', async ({
  page
}) => {
  await start(page);
  await expect(page.locator('#hcp122-panel')).toContainText('Visible carrots');
  await page
    .locator('#hcp122-panel')
    .getByRole('button', { name: 'Hide publisher locally', exact: true })
    .click();
  await expect(page.locator('#hcp122-panel')).toContainText(
    'Publisher hidden in this browser'
  );
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toHaveLength(
    1
  );
  await page.evaluate(() => window.hcp122Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp122Fixture.publicSnapshot())
  ).toMatchObject({ outcome: 'unavailable', food: undefined });
  const before = queries.length;
  await page.evaluate(() => window.hcp122Fixture.read());
  expect(queries).toHaveLength(before);
  await page
    .locator('#hcp122-panel')
    .getByRole('button', { name: /Undo hide/ })
    .click();
  await expect(page.locator('#hcp122-panel')).toContainText('Hide undone');
  await page.evaluate(() => window.hcp122Fixture.read());
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp122Fixture.publicSnapshot()?.food?.title)
    )
    .toBe('Visible carrots');
  expect(effects).toBe(0);
});
test('HCP122 operator-denied canonical detail emits no head deletion or profile query', async ({
  page
}) => {
  await start(page, true);
  await page.evaluate(() => window.hcp122Fixture.read());
  expect(
    await page.evaluate(() => window.hcp122Fixture.publicSnapshot())
  ).toMatchObject({ outcome: 'unavailable', food: undefined });
  expect(queries).toEqual([]);
  expect(effects).toBe(0);
});
test('HCP122 broad search excludes verified denied publisher before auxiliary profile lookup', async ({
  page
}) => {
  await start(page, true);
  await page.evaluate(() => window.hcp122Fixture.search());
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp122Fixture.searchSnapshot()?.refresh)
    )
    .toBe('bounded-eose');
  expect(
    await page.evaluate(() => window.hcp122Fixture.searchSnapshot()?.listings)
  ).toEqual([]);
  expect(
    queries.some((query) => query.authors?.includes(events[0].pubkey))
  ).toBe(false);
  expect(effects).toBe(0);
});
test('HCP122 local hide updates an already settled search and Undo restores its eligible row', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() => window.hcp122Fixture.search());
  const results = page.locator('#hcp122-panel [data-public-search]');
  await expect(results).toContainText('Visible carrots');
  await page
    .locator('#hcp122-panel')
    .getByRole('button', { name: 'Hide publisher locally', exact: true })
    .click();
  await expect(results).not.toContainText('Visible carrots');
  const before = queries.length;
  await page
    .locator('#hcp122-panel')
    .getByRole('button', { name: /Undo hide/ })
    .click();
  await expect(results).toContainText('Visible carrots');
  expect(queries).toHaveLength(before);
  expect(effects).toBe(0);
});
test('HCP122 Undo of a retained local hide cannot override operator denial or fetch that publisher', async ({
  page
}) => {
  await start(page, true);
  expect(await page.evaluate(() => window.hcp122Fixture.hide())).toEqual({
    status: 'saved'
  });
  await page
    .locator('#hcp122-panel')
    .getByRole('button', { name: /Undo hide/ })
    .click();
  await page.evaluate(() => window.hcp122Fixture.read());
  expect(
    await page.evaluate(() => window.hcp122Fixture.publicSnapshot())
  ).toMatchObject({ outcome: 'unavailable', food: undefined });
  expect(queries).toEqual([]);
  expect(effects).toBe(0);
});
test('HCP122 detached render publisher cannot authorize local hide or automatic public report', async ({
  page
}) => {
  await start(page);
  expect(await page.evaluate(() => window.hcp122Fixture.fake())).toEqual({
    status: 'invalid'
  });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual([]);
  expect(effects).toBe(0);
});
test('HCP122 actual account switch retires the old owner inventory without removing stored facts', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() => window.hcp122Fixture.hide());
  const before = await page.evaluate(() => window.hcp122Fixture.rows());
  await page.evaluate(() => window.hcp122Fixture.switchOwner());
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual({ state: 'guest' });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual(
    before
  );
  await expect(
    page.locator('#hcp122-panel').getByRole('button', { name: /Undo hide/ })
  ).toHaveCount(0);
  expect(effects).toBe(0);
});
test('HCP122 hiding an own public source preserves original signed operation and delivery receipts', async ({
  page
}) => {
  await start(page, false, true);
  const before = await page.evaluate(() => window.hcp122Fixture.nativeRecord());
  expect(before).toBeDefined();
  expect((before as { artifact?: unknown }).artifact).toBeDefined();
  await page.evaluate(() => window.hcp122Fixture.hide());
  expect(
    await page.evaluate(() => window.hcp122Fixture.nativeRecord())
  ).toEqual(before);
  const own = await page.evaluate(() => window.hcp122Fixture.sellingSnapshot());
  expect(own?.attention).toHaveLength(1);
  expect(own?.attention[0].provenance).toContain(
    'Local publication evidence retained'
  );
  expect(own?.attention[0].href).toBeUndefined();
  expect(effects).toBe(0);
});
test('HCP122 corrupt local preference is retained and never replaced with an empty inventory', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() => window.hcp122Fixture.corrupt());
  await page.evaluate(() => window.hcp122Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual({ state: 'unavailable', reason: 'corrupt_record' });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toHaveLength(
    1
  );
  expect(effects).toBe(0);
});
test('HCP122 actual native transaction abort gives no Saved or empty-data replacement', async ({
  page
}) => {
  await start(page);
  expect(await page.evaluate(() => window.hcp122Fixture.abortWrite())).toEqual({
    status: 'aborted'
  });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual([]);
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toMatchObject({ state: 'ready', publishers: [] });
  expect(effects).toBe(0);
});
test('HCP122 failed readback after actual write completion stays unknown until explicit native reconciliation', async ({
  page
}) => {
  await start(page);
  expect(
    await page.evaluate(() => window.hcp122Fixture.failReadback())
  ).toEqual({ status: 'unknown_completion' });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toHaveLength(
    1
  );
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toMatchObject({ state: 'ready', publishers: [] });
  await page.evaluate(() => window.hcp122Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toMatchObject({ state: 'ready', publishers: [events[0].pubkey] });
  expect(effects).toBe(0);
});
test('HCP122 a concurrent full-wire native write causes conflict without overwriting another preference', async ({
  page
}) => {
  await start(page);
  expect(
    await page.evaluate(() => window.hcp122Fixture.conflictWrite())
  ).toEqual({ status: 'conflict' });
  const rows = await page.evaluate(() => window.hcp122Fixture.rows());
  expect(rows).toHaveLength(1);
  expect(
    (JSON.parse((rows[0] as { wire: string }).wire) as { publishers: string[] })
      .publishers
  ).not.toContain(events[0].pubkey);
  expect(effects).toBe(0);
});
test('HCP122 denied storage leaves browse-only with honest unconfirmed preferences', async ({
  page
}) => {
  await start(page);
  expect(await page.evaluate(() => window.hcp122Fixture.denyStorage())).toEqual(
    { state: 'unavailable', reason: 'unavailable' }
  );
  await expect(page.locator('#hcp122-panel')).toContainText(
    'previous hides are unconfirmed'
  );
  await expect(
    page
      .locator('#hcp122-panel')
      .getByRole('button', { name: 'Hide publisher locally', exact: true })
  ).toHaveCount(0);
  expect(effects).toBe(0);
});
test('HCP122 original generation is revoked after same-author reconnect and new custody reopens exact facts', async ({
  page
}) => {
  await start(page);
  await page.evaluate(() => window.hcp122Fixture.hide());
  const before = await page.evaluate(() => window.hcp122Fixture.rows());
  expect(await page.evaluate(() => window.hcp122Fixture.reconnect())).toEqual({
    status: 'stopped'
  });
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual(
    before
  );
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toMatchObject({ state: 'ready', publishers: [events[0].pubkey] });
  expect(effects).toBe(0);
});
for (const width of [320, 1024])
  test(`HCP122 ${width}px local hide and Undo are separately visible and operable`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await start(page);
    await page
      .locator('#hcp122-panel')
      .getByRole('button', { name: 'Hide publisher locally', exact: true })
      .click();
    await expect(
      page.locator('#hcp122-panel').getByRole('button', { name: /Undo hide/ })
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth
      )
    ).toBe(false);
    expect(effects).toBe(0);
  });

test('HCP122 existing hide reconciles the complete native row changed by another controller', async ({
  page
}) => {
  await start(page);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp122Fixture.publicSnapshot()?.food?.title)
    )
    .toBe('Visible carrots');
  await page.evaluate(() => window.hcp122Fixture.search());
  await expect(page.locator('[data-public-search]')).toContainText(
    'Visible carrots'
  );
  const before = await page.evaluate(() =>
    window.hcp122Fixture.hidesSnapshot()
  );
  expect(before).toMatchObject({ state: 'ready', revision: 0, publishers: [] });
  const concurrent = await page.evaluate(() =>
    window.hcp122Fixture.concurrentPreference(true)
  );
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual(before);
  expect(await page.evaluate(() => window.hcp122Fixture.hide())).toEqual({
    status: 'existing'
  });
  // Immediate actual-state assertion gives a real regression, without waiting for a UI deadline.
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual(concurrent.snapshot);
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual(
    concurrent.rows
  );
  expect(concurrent.snapshot).toMatchObject({ state: 'ready', revision: 2 });
  await expect(
    page.locator('#hcp122-panel').getByRole('button', { name: /Undo hide/ })
  ).toHaveCount(2);
  await expect(page.locator('[data-public-search]')).not.toContainText(
    'Visible carrots'
  );
  expect(
    await page.evaluate(() => window.hcp122Fixture.publicSnapshot())
  ).toMatchObject({ outcome: 'unavailable', food: undefined });
  expect(effects).toBe(0);
});
test('HCP122 existing Undo reconciles the full native row while preserving an unrelated hide', async ({
  page
}) => {
  await start(page);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp122Fixture.publicSnapshot()?.food?.title)
    )
    .toBe('Visible carrots');
  expect(await page.evaluate(() => window.hcp122Fixture.hide())).toEqual({
    status: 'saved'
  });
  const before = await page.evaluate(() =>
    window.hcp122Fixture.hidesSnapshot()
  );
  const concurrent = await page.evaluate(() =>
    window.hcp122Fixture.concurrentPreference(false)
  );
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual(before);
  expect(await page.evaluate(() => window.hcp122Fixture.undo())).toEqual({
    status: 'existing'
  });
  expect(
    await page.evaluate(() => window.hcp122Fixture.hidesSnapshot())
  ).toEqual(concurrent.snapshot);
  expect(await page.evaluate(() => window.hcp122Fixture.rows())).toEqual(
    concurrent.rows
  );
  expect(concurrent.snapshot).toMatchObject({ state: 'ready', revision: 3 });
  await expect(
    page.locator('#hcp122-panel').getByRole('button', { name: /Undo hide/ })
  ).toHaveCount(1);
  await page.evaluate(() => window.hcp122Fixture.read());
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp122Fixture.publicSnapshot()?.food?.title)
    )
    .toBe('Visible carrots');
  expect(effects).toBe(0);
});
