import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { writeFile } from 'node:fs/promises';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/withdraw-listing.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
declare global {
  interface Window {
    hcp120: typeof Fixture;
    hcp120Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp120Second: Awaited<ReturnType<typeof Fixture.openSecondEditor>>;
    hcp120OriginalSign(input: Fixture.ControlledSignInput): Promise<NostrEvent>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let event: NostrEvent | undefined,
  partial = false,
  effects: NostrEvent[] = [],
  retained: NostrEvent[] = [],
  holdAck = false;
let tabs: import('@playwright/test').Page[] = [];
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
        event = frame[1] as NostrEvent;
        effects.push(event);
        retained.push(event);
        if (!holdAck) socket.send(JSON.stringify(['OK', event.id, true, '']));
        return;
      }
      if (frame[0] !== 'REQ') return;
      const filters = frame.slice(2) as {
        kinds?: number[];
        ids?: string[];
        authors?: string[];
        '#d'?: string[];
        '#e'?: string[];
        '#a'?: string[];
      }[];
      for (const actual of retained)
        if (
          filters.some(
            (filter) =>
              (!filter.kinds || filter.kinds.includes(actual.kind)) &&
              (!filter.ids || filter.ids.includes(actual.id)) &&
              (!filter.authors || filter.authors.includes(actual.pubkey)) &&
              (['#d', '#e', '#a'] as const).every(
                (key) =>
                  !filter[key] ||
                  actual.tags.some(
                    (tag) =>
                      tag[0] === key.slice(1) && filter[key]!.includes(tag[1])
                  )
              )
          )
        )
          socket.send(JSON.stringify(['EVENT', frame[1], actual]));
      if (!(partial && filters.some((filter) => filter.kinds?.includes(5))))
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
          new URL('./harness/withdraw-listing.ts', import.meta.url)
        ),
        name: 'hcp120',
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
  tabs = [];
  event = undefined;
  partial = false;
  effects = [];
  retained = [];
  holdAck = false;
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
  event = await page.evaluate(async () => {
    window.hcp120Fixture = await window.hcp120.createFixture();
    return window.hcp120Fixture.event;
  });
  retained.push(event);
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    await window.hcp120Second?.close();
  });
  for (const tab of tabs) {
    if (!tab.isClosed()) {
      await tab.evaluate(async () => {
        await window.hcp120Second?.close();
      });
      await tab.close();
    }
  }
  await page.evaluate(async () => {
    await window.hcp120Fixture?.close();
  });
});
async function editor(page: import('@playwright/test').Page) {
  await page.evaluate(() => window.hcp120Fixture.renderEditor());
  await expect(page.locator('#listing-title')).not.toHaveAttribute(
    'readonly',
    ''
  );
}
async function secondEditorPage(
  page: import('@playwright/test').Page,
  naddr: string
) {
  const tab = await page.context().newPage();
  tabs.push(tab);
  const owner = await page.evaluate(() => window.hcp120Fixture.owner);
  await tab.exposeFunction(
    'hcp120OriginalSign',
    async (input: Fixture.ControlledSignInput) =>
      await page.evaluate(
        (input) => window.hcp120Fixture.signOriginal(input),
        input
      )
  );
  await tab.addInitScript(
    ({ endpoint, owner }) => {
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
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        value: {
          getPublicKey: () => Promise.resolve(owner),
          signEvent: (input: Fixture.ControlledSignInput) =>
            window.hcp120OriginalSign(input)
        }
      });
    },
    { endpoint, owner }
  );
  await tab.goto(server.url + '/search');
  await tab.addScriptTag({ content: bundle });
  await tab.evaluate(
    async ({ owner, naddr }) => {
      window.hcp120Second = await window.hcp120.openSecondEditor(owner, naddr);
    },
    { owner, naddr }
  );
  return tab;
}

async function removalReview(page: import('@playwright/test').Page) {
  await editor(page);
  await page.getByText('Other listing actions', { exact: true }).click();
  const removal = page.getByRole('button', {
    name: 'Request removal',
    exact: true
  });
  // Count is an immediate DOM observation: a missing future action fails this
  // body without making isEnabled wait for an element that does not exist.
  expect(await removal.count()).toBe(1);
  expect(await removal.isEnabled()).toBe(true);
  await removal.click();
  await expect(
    page.getByRole('region', { name: 'Confirm removal request' })
  ).toContainText('cannot erase all other copies');
  await page
    .getByRole('button', { name: 'Review removal request', exact: true })
    .click();
  await expect(
    page.getByRole('region', { name: 'Public listing review' })
  ).toContainText('Removal request');
}
test('HCP120 separate consequence confirmation and full removal review perform no sign capture or send', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp120Fixture.counts());
  await removalReview(page);
  const region = page.getByRole('region', { name: 'Public listing review' });
  await expect(region).toContainText(
    await page.evaluate(() => window.hcp120Fixture.naddr)
  );
  await expect(region).toContainText('cannot erase all other copies');
  expect(await page.evaluate(() => window.hcp120Fixture.counts())).toEqual(
    before
  );
  expect(
    await page.evaluate(() => window.hcp120Fixture.operations())
  ).toHaveLength(1);
  expect(effects).toHaveLength(0);
});
test('HCP120 explicit removal persists distinct same-author kind5 and delivery independently of suppressed listing', async ({
  page
}) => {
  const base = await page.evaluate(() => window.hcp120Fixture.event);
  const signs = await page.evaluate(() => window.hcp120Fixture.counts().signs);
  await removalReview(page);
  await page
    .getByRole('button', { name: 'Publish removal request', exact: true })
    .click();
  await expect.poll(() => effects.length).toBeGreaterThan(0);
  await expect
    .poll(
      async () =>
        (await page.evaluate(() => window.hcp120Fixture.operations()))
          .find((row) => row.capture.kind === 5)
          ?.receipts.filter((fact) => fact.readbackWire !== null).length ?? 0
    )
    .toBe(1);
  const rows = await page.evaluate(() => window.hcp120Fixture.operations());
  expect(rows).toHaveLength(2);
  const removal = rows.find((row) => row.capture.kind === 5)!;
  expect(removal.source).toEqual({
    type: 'public_head',
    wire: JSON.stringify(base)
  });
  expect(removal.capture.targets).toEqual(['wss://discovery.example.org']);
  const signed = JSON.parse(removal.artifact!.wire) as NostrEvent;
  expect(signed.kind).toBe(5);
  expect(signed.pubkey).toBe(base.pubkey);
  expect(signed.tags).toEqual([
    ['e', base.id],
    [
      'a',
      `30402:${base.pubkey}:${base.tags.find((tag) => tag[0] === 'd')![1]}`
    ],
    ['k', '30402']
  ]);
  expect(signed.created_at).toBeGreaterThanOrEqual(base.created_at);
  expect(signed.created_at - Math.floor(Date.now() / 1000)).toBeLessThanOrEqual(
    300
  );
  expect(
    effects.every((actual) => actual.id === signed.id && actual.kind === 5)
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp120Fixture.counts().signs)).toBe(
    signs + 1
  );
  expect(
    rows.find((row) => row.artifact?.eventId === base.id)?.artifact?.wire
  ).toBe(JSON.stringify(base));
  await expect(
    page.getByText('Exact read-back recorded for 1 of 1 targets.', {
      exact: true
    })
  ).toBeVisible();
  await page.evaluate(() => window.hcp120Fixture.open());
  await expect
    .poll(() => page.evaluate(() => window.hcp120Fixture.snapshot()?.status))
    .toBe('withdrawn');
  const after = await page.evaluate(() => window.hcp120Fixture.operations());
  expect(after.find((row) => row.capture.kind === 5)).toEqual(removal);
  await writeFile(
    test.info().outputPath('actual-retained-removal-operation.json'),
    JSON.stringify(removal)
  );
});
test('HCP120 cancel removal confirmation preserves exact proposal with no effect', async ({
  page
}) => {
  await editor(page);
  await page.locator('#listing-title').fill('Keep my local proposed carrots');
  const before = await page.evaluate(() => window.hcp120Fixture.counts());
  await page.getByText('Other listing actions', { exact: true }).click();
  expect(
    await page
      .getByRole('button', { name: 'Request removal', exact: true })
      .isEnabled()
  ).toBe(true);
  await page
    .getByRole('button', { name: 'Request removal', exact: true })
    .click();
  await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
  await expect(page.locator('#listing-title')).toHaveValue(
    'Keep my local proposed carrots'
  );
  expect(await page.evaluate(() => window.hcp120Fixture.counts())).toEqual(
    before
  );
  expect(effects).toHaveLength(0);
});
test('HCP120 wrong author source cannot expose a removal signing path', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp120Fixture.counts().signs);
  expect(
    await page.evaluate(() =>
      window.hcp120Fixture.open(false, window.hcp120Fixture.foreign())
    )
  ).toBeUndefined();
  const tab = await secondEditorPage(
    page,
    await page.evaluate(() => window.hcp120Fixture.foreign())
  );
  await expect(tab.locator('#hcp119-second')).toContainText(
    'Supported local listing values are not available for editing'
  );
  await expect(
    tab
      .locator('#hcp119-second')
      .getByRole('button', { name: 'Request removal', exact: true })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp120Fixture.counts().signs)).toBe(
    before
  );
  expect(effects).toHaveLength(0);
});
test('HCP120 incomplete deletion coverage cannot authorize withdrawal', async ({
  page
}) => {
  partial = true;
  await page.evaluate(() => window.hcp120Fixture.renderEditor());
  await expect(page.locator('#listing-title')).toHaveAttribute('readonly', '');
  await expect(
    page.getByRole('button', { name: 'Request removal', exact: true })
  ).toHaveCount(0);
  expect(effects).toHaveLength(0);
});
test('HCP120 independently competing head fences an already reviewed deletion without replacing proposal', async ({
  page
}) => {
  await removalReview(page);
  event = await page.evaluate(() => window.hcp120Fixture.competing());
  retained.push(event);
  const before = await page.evaluate(() => window.hcp120Fixture.counts().signs);
  await page.evaluate(() => window.hcp120Fixture.lookupAgain());
  await expect(
    page.getByRole('button', { name: 'Publish removal request', exact: true })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp120Fixture.counts().signs)).toBe(
    before
  );
  expect(effects).toHaveLength(0);
});
test('HCP120 Stop prevents more attempts and never retracts an already sent kind5', async ({
  page
}) => {
  holdAck = true;
  await removalReview(page);
  await page
    .getByRole('button', { name: 'Publish removal request', exact: true })
    .click();
  await expect.poll(() => effects.length).toBe(1);
  const sent = effects[0];
  expect(sent.kind).toBe(5);
  await page
    .getByRole('button', { name: 'Stop current action', exact: true })
    .click();
  await expect(
    page.getByRole('button', {
      name: 'Publishing removal request',
      exact: true
    })
  ).toHaveCount(0);
  expect(effects).toEqual([sent]);
  const rows = await page.evaluate(() => window.hcp120Fixture.operations());
  expect(rows.find((row) => row.capture.kind === 5)?.artifact?.wire).toBe(
    JSON.stringify(sent)
  );
  expect(
    rows
      .find((row) => row.capture.kind === 5)
      ?.receipts.some((fact) => fact.status === 'accepted')
  ).toBe(false);
});
for (const width of [320, 1024])
  test(`HCP120 ${width}px reviewed removal is readable without raw message deletion`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await removalReview(page);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      )
    ).toBe(true);
    await expect(
      page.getByRole('button', { name: /Delete message|Delete conversation/ })
    ).toHaveCount(0);
    expect(effects).toHaveLength(0);
  });

for (const first of ['revision', 'withdrawal'] as const)
  test(`HCP120 native unresolved ${first} capture fences the other original review`, async ({
    page
  }) => {
    await editor(page);
    const tab = await secondEditorPage(
      page,
      await page.evaluate(() => window.hcp120Fixture.naddr)
    );
    const second = tab.locator('#hcp119-second');
    await expect(second.locator('#listing-title')).not.toHaveAttribute(
      'readonly',
      ''
    );
    const primary = page.locator('#hcp118-conflict');
    const removal = first === 'withdrawal' ? primary : second;
    const revision = first === 'revision' ? primary : second;
    await removal.getByText('Other listing actions', { exact: true }).click();
    await removal
      .getByRole('button', { name: 'Request removal', exact: true })
      .click();
    await removal
      .getByRole('button', { name: 'Review removal request', exact: true })
      .click();
    await expect(
      removal.getByRole('region', { name: 'Public listing review' })
    ).toContainText('Removal request');
    await revision
      .locator('#listing-title')
      .fill('Concurrent proposed carrots');
    await revision
      .getByRole('button', { name: 'Review changes', exact: true })
      .click();
    await expect(
      revision.getByRole('region', { name: 'Public listing review' })
    ).toContainText('Concurrent proposed carrots');
    await page.evaluate(() => window.hcp120Fixture.refuseNextSignature());
    const firstScope = first === 'withdrawal' ? removal : revision;
    const firstLabel =
      first === 'withdrawal' ? 'Publish removal request' : 'Publish changes';
    await firstScope
      .getByRole('button', { name: firstLabel, exact: true })
      .click();
    await expect
      .poll(() => page.evaluate(() => window.hcp120Fixture.signRequests()))
      .toBe(1);
    await expect(
      firstScope.getByRole('button', { name: /^Publishing/ })
    ).toHaveCount(0);
    const before = await page.evaluate(() => window.hcp120Fixture.operations());
    expect(before).toHaveLength(2);
    expect(before.find((row) => row.artifact === null)?.capture.kind).toBe(
      first === 'withdrawal' ? 5 : 30402
    );
    const otherScope = first === 'withdrawal' ? revision : removal;
    await otherScope
      .getByRole('button', {
        name:
          first === 'withdrawal'
            ? 'Publish changes'
            : 'Publish removal request',
        exact: true
      })
      .click();
    await expect(
      otherScope.getByText(
        'The update did not complete. Your proposed draft remains here. Check any retained operation before another attempt.',
        { exact: true }
      )
    ).toBeVisible();
    await expect(
      otherScope.getByRole('button', { name: /^Publishing/ })
    ).toHaveCount(0);
    expect(
      await page.evaluate(() => window.hcp120Fixture.operations())
    ).toEqual(before);
    expect(await page.evaluate(() => window.hcp120Fixture.signRequests())).toBe(
      1
    );
    expect(effects).toHaveLength(0);
    await writeFile(
      test.info().outputPath('actual-native-competing-capture.json'),
      JSON.stringify(before)
    );
  });
