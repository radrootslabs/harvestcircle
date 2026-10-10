import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { writeFile } from 'node:fs/promises';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/revise-listing.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
declare global {
  interface Window {
    hcp119: typeof Fixture;
    hcp119Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp119Second: Awaited<ReturnType<typeof Fixture.openSecondEditor>>;
    hcp119OriginalSign(input: Fixture.ControlledSignInput): Promise<NostrEvent>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let event: NostrEvent | undefined,
  partial = false,
  effects: NostrEvent[] = [];
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
        socket.send(JSON.stringify(['OK', event.id, true, '']));
        return;
      }
      if (frame[0] !== 'REQ') return;
      const filters = frame.slice(2) as {
        kinds?: number[];
        ids?: string[];
        authors?: string[];
        '#d'?: string[];
      }[];
      if (
        event &&
        filters.some(
          (filter) =>
            (!filter.kinds || filter.kinds.includes(event!.kind)) &&
            (!filter.ids || filter.ids.includes(event!.id)) &&
            (!filter.authors || filter.authors.includes(event!.pubkey)) &&
            (!filter['#d'] ||
              filter['#d'].includes(
                event!.tags.find((tag) => tag[0] === 'd')?.[1] ?? ''
              ))
        )
      )
        socket.send(JSON.stringify(['EVENT', frame[1], event]));
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
          new URL('./harness/revise-listing.ts', import.meta.url)
        ),
        name: 'hcp119',
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
    window.hcp119Fixture = await window.hcp119.createFixture();
    return window.hcp119Fixture.event;
  });
});
test.afterEach(async ({ page }) => {
  for (const tab of tabs) {
    if (!tab.isClosed()) {
      await tab.evaluate(async () => {
        await window.hcp119Second?.close();
      });
      await tab.close();
    }
  }
  await page.evaluate(async () => {
    await window.hcp119Fixture?.close();
  });
});
async function editor(page: import('@playwright/test').Page) {
  await page.evaluate(() => window.hcp119Fixture.renderEditor());
  await expect(page.locator('#listing-title')).not.toHaveAttribute(
    'readonly',
    ''
  );
}
async function review(page: import('@playwright/test').Page) {
  await editor(page);
  await page.locator('#listing-title').fill('Actual revised carrots');
  await page
    .locator('#listing-description')
    .fill('Complete exact revised public description.');
  await page
    .getByRole('button', { name: 'Review changes', exact: true })
    .click();
  await expect(
    page.getByRole('region', { name: 'Public listing review' })
  ).toBeVisible();
}
test('HCP119 exact review retains coordinate/published_at and performs no reservation sign or send', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp119Fixture.counts());
  await review(page);
  const region = page.getByRole('region', { name: 'Public listing review' });
  await expect(region).toContainText(
    'Complete exact revised public description.'
  );
  await expect(region).toContainText(
    await page.evaluate(() => window.hcp119Fixture.naddr)
  );
  expect(await page.evaluate(() => window.hcp119Fixture.counts())).toEqual(
    before
  );
  expect(
    await page.evaluate(() => window.hcp119Fixture.operations())
  ).toHaveLength(1);
  expect(effects).toHaveLength(0);
  await page.getByRole('button', { name: 'Edit details', exact: true }).click();
  await expect(page.locator('#listing-title')).toHaveValue(
    'Actual revised carrots'
  );
  await expect(page.locator('#listing-title')).toBeFocused();
});
test('HCP119 explicit Publish changes persists one genuine signed update with original coordinate and durable target facts', async ({
  page
}) => {
  const base = await page.evaluate(() => window.hcp119Fixture.event);
  const before = await page.evaluate(() => window.hcp119Fixture.counts().signs);
  await review(page);
  await page
    .getByRole('button', { name: 'Publish changes', exact: true })
    .click();
  await expect.poll(() => effects.length).toBeGreaterThan(0);
  await expect
    .poll(() =>
      page.evaluate(
        async () =>
          (await window.hcp119Fixture.operations())
            .find(
              (row) => row.artifact?.eventId !== window.hcp119Fixture.event.id
            )
            ?.receipts.filter((fact) => fact.readbackWire !== null).length ?? 0
      )
    )
    .toBe(1);
  const rows = await page.evaluate(() => window.hcp119Fixture.operations());
  expect(rows).toHaveLength(2);
  const revised = rows.find((row) => row.artifact?.eventId !== base.id);
  expect(revised?.capture.targets).toEqual(['wss://discovery.example.org']);
  expect(revised?.artifact).toBeTruthy();
  const wire = JSON.parse(revised!.artifact!.wire) as NostrEvent;
  expect(wire.kind).toBe(30402);
  expect(wire.pubkey).toBe(base.pubkey);
  expect(wire.tags.find((tag) => tag[0] === 'd')).toEqual(
    base.tags.find((tag) => tag[0] === 'd')
  );
  expect(wire.tags.find((tag) => tag[0] === 'published_at')).toEqual(
    base.tags.find((tag) => tag[0] === 'published_at')
  );
  expect(wire.created_at).toBeGreaterThan(base.created_at);
  expect(wire.created_at - Math.floor(Date.now() / 1000)).toBeLessThanOrEqual(
    300
  );
  expect(
    effects.every((effect) => effect.id === wire.id && effect.kind === 30402)
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp119Fixture.counts().signs)).toBe(
    before + 1
  );
  expect(
    rows.find((row) => row.artifact?.eventId === base.id)?.artifact?.wire
  ).toBe(JSON.stringify(base));
  await page.evaluate(() => window.hcp119Fixture.open());
  await expect
    .poll(() => page.evaluate(() => window.hcp119Fixture.snapshot()?.status))
    .toBe('ready');
  const reopened = await page.evaluate(() => window.hcp119Fixture.snapshot());
  expect(reopened?.sourceEventId).toBe(wire.id);
  expect(reopened?.form.title).toBe('Actual revised carrots');
  expect(reopened?.publishedAtSeconds).toBe(
    Number(base.tags.find((tag) => tag[0] === 'published_at')![1])
  );
});
test('HCP119 separate sold consequence review is not payment or stock reservation and emits only reviewed kind30402', async ({
  page
}) => {
  await editor(page);
  const before = await page.evaluate(() => window.hcp119Fixture.counts().signs);
  await page.getByText('Other listing actions', { exact: true }).click();
  await page.getByRole('button', { name: 'Mark as sold', exact: true }).click();
  await expect(
    page.getByRole('region', { name: 'Confirm sold update' })
  ).toContainText('not a payment or stock reservation receipt');
  expect(effects).toHaveLength(0);
  await page
    .getByRole('button', { name: 'Review sold update', exact: true })
    .click();
  await expect(
    page.getByRole('region', { name: 'Public listing review' })
  ).toContainText('Sold');
  expect(await page.evaluate(() => window.hcp119Fixture.counts().signs)).toBe(
    before
  );
  await page
    .getByRole('button', { name: 'Publish sold update', exact: true })
    .click();
  await expect.poll(() => effects.length).toBeGreaterThan(0);
  expect(
    effects.every(
      (effect) =>
        effect.kind === 30402 &&
        effect.tags.some((tag) => tag[0] === 'status' && tag[1] === 'sold')
    )
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp119Fixture.counts().signs)).toBe(
    before + 1
  );
});
test('HCP119 cancel sold confirmation preserves proposal and performs no effect', async ({
  page
}) => {
  await editor(page);
  await page.locator('#listing-title').fill('Keep my exact proposal');
  const before = await page.evaluate(() => window.hcp119Fixture.counts());
  await page.getByText('Other listing actions', { exact: true }).click();
  await page.getByRole('button', { name: 'Mark as sold', exact: true }).click();
  await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
  await expect(page.locator('#listing-title')).toHaveValue(
    'Keep my exact proposal'
  );
  expect(await page.evaluate(() => window.hcp119Fixture.counts())).toEqual(
    before
  );
  expect(effects).toHaveLength(0);
});
test('HCP119 independently changed head fences an already reviewed proposal before publication', async ({
  page
}) => {
  await review(page);
  event = await page.evaluate(() => window.hcp119Fixture.competing());
  const before = await page.evaluate(() => window.hcp119Fixture.counts().signs);
  await page.evaluate(() => window.hcp119Fixture.lookupAgain());
  await expect(
    page.getByRole('button', { name: 'Publish changes', exact: true })
  ).toHaveCount(0);
  await expect(page.locator('#listing-title')).toHaveValue(
    'Actual revised carrots'
  );
  await expect(page.locator('#listing-title')).toHaveAttribute('readonly', '');
  expect(await page.evaluate(() => window.hcp119Fixture.counts().signs)).toBe(
    before
  );
  expect(effects).toHaveLength(0);
});
test('HCP119 incomplete critical deletion coverage cannot expose revision or sold actions', async ({
  page
}) => {
  partial = true;
  await page.evaluate(() => window.hcp119Fixture.renderEditor());
  await expect(page.locator('#listing-title')).toHaveAttribute('readonly', '');
  await expect(
    page.getByRole('button', { name: /Review changes|Mark as sold/ })
  ).toHaveCount(0);
  expect(effects).toHaveLength(0);
});
test('HCP119 actual backward wall-clock change fences the captured review without inventing a later timestamp', async ({
  page
}) => {
  await review(page);
  const before = await page.evaluate(() => window.hcp119Fixture.counts().signs);
  await page.evaluate(() => {
    const actualNow = Date.now;
    Date.now = () => actualNow() - 301000;
  });
  await expect(
    page.getByRole('button', { name: 'Publish changes', exact: true })
  ).toHaveCount(0);
  await expect(page.locator('#listing-title')).toHaveValue(
    'Actual revised carrots'
  );
  expect(await page.evaluate(() => window.hcp119Fixture.counts().signs)).toBe(
    before
  );
  expect(
    await page.evaluate(() => window.hcp119Fixture.operations())
  ).toHaveLength(1);
  expect(effects).toHaveLength(0);
});
for (const width of [320, 1024])
  test(`HCP119 ${width}px exact revision review and sold confirmation remain readable`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await review(page);
    await expect(
      page.getByRole('region', { name: 'Public listing review' })
    ).toContainText('Actual revised carrots');
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth
      )
    ).toBe(true);
    await page
      .getByRole('button', { name: 'Edit details', exact: true })
      .click();
    await page.getByText('Other listing actions', { exact: true }).click();
    await page
      .getByRole('button', { name: 'Mark as sold', exact: true })
      .click();
    await expect(
      page.getByRole('region', { name: 'Confirm sold update' })
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth
      )
    ).toBe(true);
    expect(effects).toHaveLength(0);
  });

test('HCP119 two original editors cannot reserve against an unresolved retained coordinate capture', async ({
  page
}) => {
  await review(page);
  const first = page.locator('#hcp118-conflict');
  const tab = await page.context().newPage();
  tabs.push(tab);
  const errors: string[] = [];
  tab.on('pageerror', (error) => errors.push(error.message));
  const source = await page.evaluate(() => ({
    owner: window.hcp119Fixture.owner,
    naddr: window.hcp119Fixture.naddr
  }));
  await tab.exposeFunction(
    'hcp119OriginalSign',
    async (input: Fixture.ControlledSignInput) =>
      await page.evaluate(
        (input) => window.hcp119Fixture.signOriginal(input),
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
            window.hcp119OriginalSign(input)
        }
      });
    },
    { endpoint, owner: source.owner }
  );
  await tab.goto(server.url + '/search');
  await tab.addScriptTag({ content: bundle });
  await tab.evaluate(async ({ owner, naddr }) => {
    window.hcp119Second = await window.hcp119.openSecondEditor(owner, naddr);
  }, source);
  const second = tab.locator('#hcp119-second');
  await expect(second.locator('#listing-title')).not.toHaveAttribute(
    'readonly',
    ''
  );
  await second.locator('#listing-title').fill('Second tab proposed carrots');
  await second
    .getByRole('button', { name: 'Review changes', exact: true })
    .click();
  await expect(
    second.getByRole('region', { name: 'Public listing review' })
  ).toBeVisible();
  await page.evaluate(() => window.hcp119Fixture.refuseNextSignature());
  await first
    .getByRole('button', { name: 'Publish changes', exact: true })
    .click();
  await expect(
    first.getByText(
      'The update did not complete. Your proposed draft remains here. Check any retained operation before another attempt.',
      { exact: true }
    )
  ).toBeVisible();
  expect(await page.evaluate(() => window.hcp119Fixture.signRequests())).toBe(
    1
  );
  expect(
    await page.evaluate(() => window.hcp119Fixture.operations())
  ).toHaveLength(2);
  expect(effects).toHaveLength(0);
  await second
    .getByRole('button', { name: 'Publish changes', exact: true })
    .click();
  // Wait for either genuine settled outcome, then assert the exact native rows.
  // The assertion never equates a still-pending request with successful fencing.
  await writeFile(
    test.info().outputPath('actual-second-tab-early-public-state.json'),
    JSON.stringify({
      phase: 'EARLY_AFTER_ACTION_NO_SETTLED_OR_ASSERTION_CREDIT',
      errors,
      secondText: await second.innerText(),
      operations: await page.evaluate(() => window.hcp119Fixture.operations()),
      signRequests: await page.evaluate(() =>
        window.hcp119Fixture.signRequests()
      ),
      effects
    })
  );
  try {
    await expect
      .poll(
        async () =>
          errors.length +
          (await second
            .getByText(
              /Relay acceptance recorded for|The update did not complete\.|Supported local listing values are not available for editing/
            )
            .count())
      )
      .toBeGreaterThan(0);
  } finally {
    const actualState = JSON.stringify({
      errors,
      secondText: await second.innerText(),
      publishingButtons: await second
        .getByRole('button', { name: 'Publishing update', exact: true })
        .count(),
      operations: await page.evaluate(() => window.hcp119Fixture.operations()),
      signRequests: await page.evaluate(() =>
        window.hcp119Fixture.signRequests()
      ),
      effects
    });
    await writeFile(
      test.info().outputPath('actual-second-tab-public-state.json'),
      actualState
    );
    await test.info().attach('actual-second-tab-public-state', {
      path: test.info().outputPath('actual-second-tab-public-state.json'),
      contentType: 'application/json'
    });
  }
  expect(errors).toEqual([]);
  expect(
    await page.evaluate(() => window.hcp119Fixture.operations())
  ).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp119Fixture.signRequests())).toBe(
    1
  );
  expect(effects).toHaveLength(0);
  await expect(
    second.getByRole('region', { name: 'Public listing review' })
  ).toContainText('Second tab proposed carrots');
});

test('HCP119 a retained pending signature fences a fresh editor without adopting another local capture', async ({
  page
}) => {
  await review(page);
  await page.evaluate(() => window.hcp119Fixture.holdNextSignature());
  await page
    .getByRole('button', { name: 'Publish changes', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp119Fixture.signRequests()))
    .toBe(1);
  expect(
    await page.evaluate(() => window.hcp119Fixture.open())
  ).toBeUndefined();
  expect(effects).toHaveLength(0);
  await page.evaluate(() => window.hcp119Fixture.releaseSignature());
  await expect.poll(() => effects.length).toBeGreaterThan(0);
});
