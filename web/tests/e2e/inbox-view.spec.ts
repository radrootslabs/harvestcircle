import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-view.ts';
declare global {
  interface Window {
    hcp101: typeof Fixture;
    hcp101Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
    hcp101SetupFixture: Awaited<ReturnType<typeof Fixture.renderSetupFixture>>;
    hcp101Provider?: Awaited<ReturnType<typeof Fixture.makeSetupFixture>>;
    hcp101Pending?: Promise<boolean>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let requests: unknown[][] = [],
  closes: unknown[][] = [],
  events: unknown[][] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string')
    throw Error('loopback unavailable');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'CLOSE') closes.push(frame);
      if (frame[0] === 'EVENT') events.push(frame);
      if (frame[0] === 'REQ') {
        requests.push(frame);
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
    })
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/inbox-view.ts', import.meta.url)
        ),
        name: 'hcp101',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test('original setup cleanup survives actual identity invalidation before page closure', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0);
  });
  const observed = await page.evaluate(() =>
    window.hcp101Fixture.closeInvalidatedPage()
  );
  console.log('actual original setup cleanup', JSON.stringify(observed));
  expect(observed.closedSetups).toBe(1);
  expect(observed.setupStatus).toBe('closed');
  expect(observed.closed).toBe(true);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  requests = [];
  closes = [];
  events = [];
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
      window.hcp101Fixture?.settle();
      await window.hcp101Pending;
      await window.hcp101Fixture?.close();
      await window.hcp101SetupFixture?.close();
      window.hcp101Provider?.close();
    });
});
test('failed original setup disposal remains incomplete until actual retained cleanup is retried', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0);
  });
  const observed = await page.evaluate(() =>
    window.hcp101Fixture.retryFailedSetupClose()
  );
  console.log(
    'actual failed original setup disposal',
    JSON.stringify(observed)
  );
  expect(observed.first).toBe(false);
  expect(observed.incomplete).toBe(true);
  expect(observed.second).toBe(true);
  expect(observed.closedSetups).toBe(2);
  expect(observed.cleanupRequired).toBe(false);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('revoked original exercised access becomes partial while preserving the actual earlier checked time', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0, true);
    await window.hcp101Fixture.checkSetup();
    await window.hcp101Fixture.unlock();
    await window.hcp101Fixture.check();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.snapshot().state))
    .toBe('empty');
  const before = await page.evaluate(
    () => window.hcp101Fixture.snapshot().lastCheckedAt
  );
  const observed = await page.evaluate(() => {
    window.hcp101Fixture.revokeAccess();
    return window.hcp101Fixture.snapshot();
  });
  console.log(
    'actual revoked original exercised access',
    JSON.stringify(observed)
  );
  expect(observed.state).toBe('partial');
  expect(observed.reason).toBe('access_unavailable');
  expect(observed.lastCheckedAt).toBe(before);
  expect(events).toEqual([]);
});
test('actual production guest connection publishes no event and cannot claim an empty inbox', async ({
  page
}) => {
  await page.goto(server.url + '/messages');
  await page.addScriptTag({ content: bundle });
  await expect(
    page
      .locator('#main-content')
      .getByRole('button', { name: 'Connect extension' })
  ).toBeVisible();
  await expect(
    page
      .locator('#main-content')
      .getByText('No messages found in the checked inbox history.')
  ).toHaveCount(0);
  await expect(
    page.locator('#main-content').getByText(/^Last checked/)
  ).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
  await expect(page).toHaveTitle('HarvestCircle');
  await expect(page.locator('meta[name="robots"]')).toHaveCount(1);
});
test('actual missing NIP44 encrypt capability is explanatory and never an empty inbox', async ({
  page
}) => {
  await page.goto(server.url + '/messages');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp101Provider = await window.hcp101.makeSetupFixture();
    const provider = window.nostr as
      { nip44?: { encrypt?: unknown } } | undefined;
    if (provider?.nip44) Reflect.deleteProperty(provider.nip44, 'encrypt');
  });
  const main = page.locator('#main-content');
  await main.getByRole('button', { name: 'Connect extension' }).click();
  await main.getByRole('button', { name: 'Check messaging support' }).click();
  await expect(
    main.getByText(
      'This extension does not support the required message encryption.'
    )
  ).toBeVisible();
  await expect(
    main.getByText('No messages found in the checked inbox history.')
  ).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('existing genuine signed preference offers an explicit disclosed unlock without replacement or send', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(21);
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await expect(
    page.getByRole('button', { name: 'Unlock messages', exact: true })
  ).toBeVisible();
  await expect(
    page.getByText(/Your extension will be asked to decrypt/)
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Enable inbox', exact: true })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('actual missing preference is setup and completed absence never becomes an empty message list', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101SetupFixture =
      await window.hcp101.renderSetupFixture('missing');
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await expect(
    page.getByText(
      'No preference was observed in this completed bounded lookup. This does not prove global absence.'
    )
  ).toBeVisible();
  expect(
    await page.evaluate(() => window.hcp101SetupFixture.snapshot().state)
  ).toBe('setup');
  await expect(
    page.getByText('No messages found in the checked inbox history.')
  ).toHaveCount(0);
  await expect(page.getByText(/^Last checked/)).toHaveCount(0);
  expect(events).toEqual([]);
});
test('incomplete actual preference lookup stays unknown without unlock, checked time or fabricated absence', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101SetupFixture =
      await window.hcp101.renderSetupFixture('incomplete');
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await expect(
    page.getByText(
      'The lookup is incomplete. Your current inbox preference is unknown.'
    )
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Unlock messages', exact: true })
  ).toHaveCount(0);
  await expect(page.getByText(/^Last checked/)).toHaveCount(0);
  expect(
    await page.evaluate(() => window.hcp101SetupFixture.counts().signs)
  ).toBe(0);
  expect(events).toEqual([]);
});
test('explicit original Unlock processes20 actual encrypted envelopes and waits for a second review for21st', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(21);
    await window.hcp101Fixture.checkSetup();
  });
  await page
    .getByRole('button', { name: 'Unlock messages', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.snapshot().queued))
    .toBe(1);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 40, signs: 0, encrypts: 0 }
  );
  await expect(
    page.getByRole('button', { name: 'Read new messages (1)', exact: true })
  ).toBeVisible();
  expect(
    await page.evaluate(() => window.hcp101Fixture.encryptedUnchanged())
  ).toBe(true);
  await expect(page.getByText(/^Last checked/)).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('a further explicit decrypt batch consumes only the remaining envelope and cannot send or rewrite', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(21);
    await window.hcp101Fixture.checkSetup();
    await window.hcp101Fixture.unlock();
  });
  await page
    .getByRole('button', { name: 'Read new messages (1)', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.snapshot().queued))
    .toBe(0);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 42, signs: 0, encrypts: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp101Fixture.encryptedUnchanged())
  ).toBe(true);
  await expect(
    page.getByRole('button', { name: /^Read new messages/ })
  ).toHaveCount(0);
  expect(events).toEqual([]);
});
test('unqualified real access remains partial and Check cannot mint checked time, empty state or private REQ', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0, false);
    await window.hcp101Fixture.checkSetup();
    await window.hcp101Fixture.unlock();
  });
  await page
    .getByRole('button', { name: 'Check for messages', exact: true })
    .click();
  expect(
    await page.evaluate(() => window.hcp101Fixture.snapshot())
  ).toMatchObject({
    state: 'partial',
    lastCheckedAt: null,
    count: 0,
    queued: 0
  });
  await expect(
    page.getByText('No messages found in the checked inbox history.')
  ).toHaveCount(0);
  await expect(page.getByText(/^Last checked/)).toHaveCount(0);
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('only actual controlled qualified live-before-finite completion supplies a bounded empty state and checked time', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0, true);
    await window.hcp101Fixture.checkSetup();
    await window.hcp101Fixture.unlock();
  });
  expect(
    await page.evaluate(() => window.hcp101Fixture.snapshot().lastCheckedAt)
  ).toBeNull();
  await page
    .getByRole('button', { name: 'Check for messages', exact: true })
    .click();
  await expect(
    page.getByText('No messages found in the checked inbox history.')
  ).toBeVisible();
  await expect(page.getByText(/^Last checked/)).toBeVisible();
  expect(requests.length).toBe(3);
  expect((requests[0][2] as { limit: number }).limit).toBe(0);
  expect((requests[1][2] as { limit: number }).limit).toBe(200);
  expect((requests[2][2] as { limit: number }).limit).toBe(200);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(events).toEqual([]);
});
test('copied page tokens and mutable detached setup observations cannot acquire or change original custody', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0);
  });
  expect(await page.evaluate(() => window.hcp101Fixture.copied())).toEqual({
    unlock: false,
    check: false,
    next: false
  });
  expect(
    await page.evaluate(() => window.hcp101Fixture.mutateOwnership())
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('logout during held actual extension decryption clears presentation and prevents late cached admission', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(1);
    await window.hcp101Fixture.checkSetup();
    window.hcp101Fixture.mode('hold_decrypt');
    window.hcp101Pending = window.hcp101Fixture.unlock();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.pending()))
    .toBe(true);
  await page.evaluate(() => window.hcp101Fixture.logout());
  expect(
    await page.evaluate(() => window.hcp101Fixture.snapshot())
  ).toMatchObject({
    state: 'unavailable',
    count: 0,
    queued: 0,
    lastCheckedAt: null
  });
  expect(
    await page.evaluate(() => window.hcp101Fixture.scheduler()?.state)
  ).toBe('active');
  await page.evaluate(async () => {
    window.hcp101Fixture.settle();
    await window.hcp101Pending;
  });
  expect(await page.evaluate(() => window.hcp101Fixture.snapshot().count)).toBe(
    0
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('a second explicit qualified Check performs fresh finite requests while preserving original live ownership', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp101Fixture = await window.hcp101.renderFixture(0, true);
    await window.hcp101Fixture.checkSetup();
    await window.hcp101Fixture.unlock();
    await window.hcp101Fixture.check();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.snapshot().state))
    .toBe('empty');
  expect(requests.length).toBe(3);
  const before = await page.evaluate(
    () => window.hcp101Fixture.snapshot().lastCheckedAt
  );
  await page
    .getByRole('button', { name: 'Check for messages', exact: true })
    .click();
  await expect.poll(() => requests.length).toBe(5);
  await expect
    .poll(() => page.evaluate(() => window.hcp101Fixture.snapshot().state))
    .toBe('empty');
  expect(closes.length).toBeGreaterThanOrEqual(4);
  expect(
    await page.evaluate(() => window.hcp101Fixture.snapshot().lastCheckedAt)
  ).toBeGreaterThanOrEqual(before!);
  expect((requests[3][2] as { limit: number }).limit).toBe(200);
  expect((requests[4][2] as { limit: number }).limit).toBe(200);
  expect(await page.evaluate(() => window.hcp101Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(events).toEqual([]);
});
