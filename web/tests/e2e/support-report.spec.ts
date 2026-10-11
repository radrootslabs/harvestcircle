import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/support-report.ts';
declare global {
  interface Window {
    hcp124: typeof Fixture;
    hcp124Private?: Awaited<ReturnType<typeof Fixture.createPrivateFixture>>;
    hcp124Public?: Awaited<ReturnType<typeof Fixture.createPublicFixture>>;
    hcp124General?: ReturnType<typeof Fixture.createGeneralFixture>;
    hcp124Support?: ReturnType<typeof Fixture.createSupportFixture>;
    hcp124Clipboard: string[];
    hcp124PendingCopy?: Promise<'copied' | 'unavailable' | 'stopped'>;
    hcp124FinishCopy?: () => void;
    hcp124CopyStarted?: boolean;
  }
}

// This case deliberately needs no future fixture/producer compilation. The
// first preproducer run uses exact accepted HCP123 clean static output.
test('HCP124 SSR practical About and honest missing support without private inventory', async ({
  page
}) => {
  const server = await createStaticHarness();
  try {
    await page.addInitScript(() => {
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        get() {
          throw Error('About must not acquire an extension');
        }
      });
    });
    await page.goto(server.url + '/about');
    expect(await page.locator('h1').innerText()).toBe('About HarvestCircle');
    await expect(
      page.getByText('Search without connecting.', { exact: false })
    ).toBeVisible();
    await expect(
      page.getByText('Messages do not reserve food or confirm an order.', {
        exact: false
      })
    ).toBeVisible();
    await expect(
      page.getByText('Payments are not handled here.', { exact: false })
    ).toBeVisible();
    await expect(page.locator('#report')).toBeVisible();
    await expect(
      page.getByText('Support has not been configured.', { exact: false })
    ).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'Contact support', exact: true })
    ).toHaveCount(0);
    await expect(page.locator('input[type=checkbox]')).toHaveCount(0);
    await expect(page).toHaveTitle('About — HarvestCircle');
  } finally {
    await server.close();
  }
});

test.describe('HCP124 actual production selected-report controls', () => {
  let server: Awaited<ReturnType<typeof createStaticHarness>>,
    sockets: WebSocketServer,
    endpoint: string,
    bundle: string | undefined;
  let publicEvents: unknown[] = [],
    events: unknown[][] = [],
    queries: unknown[][] = [];
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
      throw Error('controlled loopback unavailable');
    endpoint = 'ws://127.0.0.1:' + address.port;
    sockets.on('connection', (socket) =>
      socket.on('message', (bytes) => {
        const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes);
        const frame = JSON.parse(raw.toString()) as unknown[];
        if (frame[0] === 'EVENT' || frame[0] === 'AUTH') events.push(frame);
        if (frame[0] === 'REQ') {
          queries.push(frame);
          const filters = frame.slice(2) as {
            kinds?: number[];
            ids?: string[];
          }[];
          if (filters.some((filter) => filter.kinds?.includes(30402)))
            for (const event of publicEvents)
              socket.send(JSON.stringify(['EVENT', frame[1], event]));
          socket.send(JSON.stringify(['EOSE', frame[1]]));
        }
      })
    );
  });
  test.afterAll(async () => {
    for (const socket of sockets?.clients ?? []) socket.terminate();
    if (sockets)
      await new Promise<void>((resolve) => sockets.close(() => resolve()));
    await server?.close();
  });
  test.beforeEach(async ({ page }) => {
    publicEvents = [];
    events = [];
    queries = [];
    await page.addInitScript((endpoint) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://peer.example.org',
              'wss://archive.example.org',
              'wss://discovery.example.org'
            ].includes(new URL(String(url)).origin)
          )
            throw Error('no live relay authorization');
          super(endpoint, protocols);
        }
      };
      window.hcp124Clipboard = [];
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: {
          writeText: (text: string) => {
            window.hcp124Clipboard.push(text);
            return Promise.resolve();
          }
        }
      });
    }, endpoint);
  });
  test.afterEach(async ({ page }) => {
    if (!page.isClosed())
      await page.evaluate(async () => {
        await window.hcp124Private?.close();
        await window.hcp124Public?.close();
        await window.hcp124General?.close();
        await window.hcp124Support?.close();
      });
  });
  async function load(page: Page) {
    await page.goto(server.url + '/about');
    if (!bundle) {
      const output = await build({
        configFile: false,
        root: process.cwd(),
        logLevel: 'silent',
        plugins: [
          svelte({ configFile: false, compilerOptions: { dev: false } })
        ],
        resolve: {
          conditions: ['browser'],
          alias: {
            '$app/navigation': fileURLToPath(
              new URL('./harness/conversation-thread.ts', import.meta.url)
            )
          }
        },
        build: {
          write: false,
          emptyOutDir: false,
          minify: false,
          lib: {
            entry: fileURLToPath(
              new URL('./harness/support-report.ts', import.meta.url)
            ),
            name: 'hcp124',
            formats: ['iife']
          }
        }
      });
      const result = Array.isArray(output) ? output[0] : output;
      if (!result || !('output' in result))
        throw Error('fixture output unavailable');
      const chunks = result.output.filter((row) => row.type === 'chunk');
      expect(chunks).toHaveLength(1);
      const chunk = chunks[0];
      bundle = chunk.code;
    }
    await page.addScriptTag({ content: bundle });
  }
  async function privateReport(page: Page) {
    await load(page);
    await page.evaluate(async () => {
      window.hcp124Private = await window.hcp124.createPrivateFixture();
    });
    await page.evaluate(() => window.hcp124Private!.checkSetup());
    expect(await page.evaluate(() => window.hcp124Private!.unlock())).toBe(
      true
    );
    await expect
      .poll(() =>
        page.evaluate(() => window.hcp124Private!.thread().messages.length)
      )
      .toBe(3);
    await expect
      .poll(() =>
        page.evaluate(
          async () =>
            (await window.hcp124Private!.native()).received.filter((row) => {
              const parsed = JSON.parse((row as { wire: string }).wire) as {
                read: unknown;
              };
              return parsed.read !== null;
            }).length
        )
      )
      .toBe(3);
    await page.evaluate(async () => {
      await window.hcp124Private!.waitOwner();
      window.hcp124Private!.report();
    });
    await expect(page.locator('#hcp124-report-fixture')).toBeVisible();
  }
  const panel = (page: Page) => page.locator('#hcp124-report-fixture');

  test('actual support link has the reviewed destination without transcript carrier or automatic navigation', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => {
      window.hcp124Support = window.hcp124.createSupportFixture();
    });
    const link = page
      .locator('#hcp124-support-fixture')
      .getByRole('link', { name: 'Contact support', exact: true });
    await expect(link).toHaveAttribute(
      'href',
      'https://support.operator.org/report'
    );
    await expect(link).toHaveAttribute('rel', /noopener/);
    await expect(link).toHaveAttribute('rel', /noreferrer/);
    await expect(link).toHaveAttribute('referrerpolicy', 'no-referrer');
    expect(page.url()).toBe(server.url + '/about');
    expect(events).toEqual([]);
    expect(queries).toEqual([]);
  });
  test('voluntary plain notes require explicit review and optional copy without support transport', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => {
      window.hcp124General = window.hcp124.createGeneralFixture();
    });
    await panel(page)
      .getByLabel('Problem description')
      .fill('<img src=x> Deliberately selected description');
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([]);
    await panel(page)
      .getByRole('button', { name: 'Review report', exact: true })
      .click();
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toContainText('<img src=x> Deliberately selected description');
    await expect(panel(page).locator('img')).toHaveCount(0);
    await panel(page)
      .getByRole('button', { name: 'Copy reviewed report', exact: true })
      .click();
    expect(await page.evaluate(() => window.hcp124Clipboard.length)).toBe(1);
    expect(events).toEqual([]);
    expect(queries).toEqual([]);
    expect(page.url()).toBe(server.url + '/about');
  });
  test('private message and peer choices start unselected and no transcript is copied automatically', async ({
    page
  }) => {
    await privateReport(page);
    expect(
      await panel(page).locator('input[type=checkbox]:checked').count()
    ).toBe(0);
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toHaveCount(0);
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([]);
    await expect(
      panel(page).getByText(
        'Support cannot automatically read encrypted messages.',
        { exact: false }
      )
    ).toBeVisible();
    await expect(
      panel(page).getByText(
        'Copied text can remain in your clipboard or other applications.',
        { exact: false }
      )
    ).toBeVisible();
  });
  test('selected private text needs extra acknowledgment and excludes every unselected message and owner', async ({
    page
  }) => {
    await privateReport(page);
    const before = await page.evaluate(async () => ({
      native: await window.hcp124Private!.native(),
      delta: window.hcp124Private!.delta(),
      owner: window.hcp124Private!.owner,
      peer: window.hcp124Private!.peer
    }));
    await panel(page)
      .getByLabel('Share message text and ID 1', { exact: true })
      .check();
    await panel(page)
      .getByRole('button', { name: 'Review report', exact: true })
      .click();
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toHaveCount(0);
    await panel(page)
      .getByLabel(
        'I understand selected private information will leave HarvestCircle if I share it.',
        { exact: true }
      )
      .check();
    await panel(page)
      .getByRole('button', { name: 'Review report', exact: true })
      .click();
    const text = await panel(page)
      .getByRole('region', { name: 'Selected report preview' })
      .innerText();
    expect(text).toContain('Actual encrypted thread fixture');
    expect(text).not.toContain('About carrots');
    expect(text).not.toContain('About apples');
    expect(text).not.toContain(before.owner);
    expect(text).not.toContain(before.peer);
    await panel(page)
      .getByRole('button', { name: 'Copy reviewed report', exact: true })
      .click();
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([text]);
    expect(await page.evaluate(() => window.hcp124Private!.delta())).toEqual(
      before.delta
    );
    expect(await page.evaluate(() => window.hcp124Private!.native())).toEqual(
      before.native
    );
    expect(events).toEqual([]);
    const support = panel(page).getByRole('link', {
      name: 'Contact support',
      exact: true
    });
    await expect(support).toHaveAttribute(
      'href',
      'https://support.operator.org/report'
    );
  });
  test('genuine logout retires an already captured selected private preview before copy', async ({
    page
  }) => {
    await privateReport(page);
    expect(
      await page.evaluate(() => window.hcp124Private!.captureSelected())
    ).toContain('Actual encrypted thread fixture');
    await page.evaluate(() => window.hcp124Private!.logout());
    expect(
      await page.evaluate(() => window.hcp124Private!.captured())
    ).toBeUndefined();
    expect(
      await page.evaluate(() => window.hcp124Private!.copyCaptured())
    ).toBe('stopped');
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([]);
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toHaveCount(0);
  });
  test('an already requested clipboard copy can finish after logout without claiming current success', async ({
    page
  }) => {
    await privateReport(page);
    const selected = await page.evaluate(() =>
      window.hcp124Private!.captureSelected()
    );
    await page.evaluate(() => {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: {
          writeText: (text: string) =>
            new Promise<void>((resolve) => {
              window.hcp124CopyStarted = true;
              window.hcp124FinishCopy = () => {
                window.hcp124Clipboard.push(text);
                resolve();
              };
            })
        }
      });
      window.hcp124PendingCopy = window.hcp124Private!.copyCaptured();
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp124CopyStarted))
      .toBe(true);
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([]);
    await page.evaluate(() => window.hcp124Private!.logout());
    expect(
      await page.evaluate(() => window.hcp124Private!.captured())
    ).toBeUndefined();
    const outcome = await page.evaluate(async () => {
      window.hcp124FinishCopy!();
      return await window.hcp124PendingCopy;
    });
    expect(outcome).toBe('stopped');
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([
      selected
    ]);
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toHaveCount(0);
    expect(events).toEqual([]);
  });
  test('acknowledged real native block retires captured private report with encrypted evidence retained', async ({
    page
  }) => {
    await privateReport(page);
    await page.evaluate(() => window.hcp124Private!.captureSelected());
    const before = await page.evaluate(() => window.hcp124Private!.native());
    expect(await page.evaluate(() => window.hcp124Private!.block())).toEqual({
      status: 'saved'
    });
    expect(
      await page.evaluate(() => window.hcp124Private!.captured())
    ).toBeUndefined();
    expect(
      await page.evaluate(() => window.hcp124Private!.copyCaptured())
    ).toBe('stopped');
    expect(await page.evaluate(() => window.hcp124Clipboard)).toEqual([]);
    expect(await page.evaluate(() => window.hcp124Private!.native())).toEqual(
      before
    );
  });
  test('actual signed public listing copies only separately selected version and coordinate', async ({
    page
  }) => {
    await load(page);
    publicEvents = await page.evaluate(async () => {
      window.hcp124Public = await window.hcp124.createPublicFixture();
      return window.hcp124Public.events;
    });
    await page.evaluate(() => window.hcp124Public!.read());
    await expect
      .poll(() =>
        page.evaluate(() => window.hcp124Public!.publicSnapshot()?.outcome)
      )
      .toBe('active');
    const selected = await page.evaluate(() => window.hcp124Public!.report());
    expect(selected?.kind).toBe('listing');
    expect(
      await panel(page).locator('input[type=checkbox]:checked').count()
    ).toBe(0);
    await panel(page)
      .getByLabel('Share listing version', { exact: true })
      .check();
    await panel(page)
      .getByRole('button', { name: 'Review report', exact: true })
      .click();
    const text = await panel(page)
      .getByRole('region', { name: 'Selected report preview' })
      .innerText();
    expect(text).toContain((publicEvents[0] as { id: string }).id);
    expect(text).not.toContain('Coordinate:');
    await panel(page).getByLabel('Share coordinate', { exact: true }).check();
    await panel(page)
      .getByRole('button', { name: 'Review report', exact: true })
      .click();
    await expect(
      panel(page).getByRole('region', { name: 'Selected report preview' })
    ).toContainText('30402:');
    expect(events).toEqual([]);
  });
  for (const width of [320, 1024])
    test(
      'HCP124 ' +
        width +
        ' private report selection and disclosure reflow without hidden confirmation',
      async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await privateReport(page);
        await panel(page)
          .getByLabel('Share message text and ID 1', { exact: true })
          .check();
        await panel(page)
          .getByLabel(
            'I understand selected private information will leave HarvestCircle if I share it.',
            { exact: true }
          )
          .check();
        await panel(page)
          .getByRole('button', { name: 'Review report', exact: true })
          .click();
        await expect(
          panel(page).getByRole('region', { name: 'Selected report preview' })
        ).toBeVisible();
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= window.innerWidth
          )
        ).toBe(true);
        await expect(
          panel(page).getByRole('button', {
            name: 'Copy reviewed report',
            exact: true
          })
        ).toBeVisible();
      }
    );
});
