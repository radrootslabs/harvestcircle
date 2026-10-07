import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-setup-view.ts';
declare global {
  interface Window {
    hcp070: typeof Fixture;
    hcp070Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
    hcp070Release?: () => void;
    hcp070Provider?: Awaited<ReturnType<typeof Fixture.makeSetupFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
let socketServer: WebSocketServer, bundle: string, endpoint: string;
let frames: string[] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  socketServer = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(socketServer, 'listening');
  const address = socketServer.address();
  if (!address || typeof address === 'string')
    throw new Error('missing loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  socketServer.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const frame = JSON.parse(
        Buffer.from(bytes as ArrayBuffer).toString()
      ) as [string, { id: string }];
      if (frame[0] === 'EVENT') {
        frames.push(JSON.stringify(frame));
        socket.send(JSON.stringify(['OK', frame[1].id, true, '']));
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
          new URL('./harness/inbox-setup-view.ts', import.meta.url)
        ),
        name: 'hcp070',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing fixture output');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of socketServer.clients) socket.terminate();
  await new Promise<void>((resolve) => socketServer.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  frames = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (String(url) !== 'wss://discovery.example.org/')
            throw new Error('unexpected destination');
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
      window.hcp070Release?.();
      await window.hcp070Fixture?.close();
      window.hcp070Provider?.close();
    });
});
test('production shared gate reacquires its genuine controller after disconnect and reconnect', async ({
  page
}) => {
  await page.goto(server.url + '/messages');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp070Provider = await window.hcp070.makeSetupFixture();
  });
  const main = page.locator('#main-content');
  const panel = main.getByRole('region', { name: 'Inbox setup' });
  await main.getByRole('button', { name: 'Connect extension' }).click();
  await main.getByRole('button', { name: 'Check messaging support' }).click();
  await expect(panel.getByText(/^Public key:/)).toBeVisible();
  await page.locator('header summary').click();
  await page
    .locator('header')
    .getByRole('button', { name: 'Disconnect', exact: true })
    .click();
  await expect(panel.getByText(/^Public key:/)).toHaveCount(0);
  await main.getByRole('button', { name: 'Connect extension' }).click();
  await main.getByRole('button', { name: 'Check messaging support' }).click();
  await expect(panel.getByText(/^Public key:/)).toBeVisible();
  expect(frames).toEqual([]);
});
test('incomplete actual lookup shows uncertainty without an empty-success claim', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp070Fixture = await window.hcp070.renderFixture('incomplete');
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await expect(
    page.getByText(
      'The lookup is incomplete. Your current inbox preference is unknown.'
    )
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Enable inbox' })).toHaveCount(
    0
  );
  expect(frames).toEqual([]);
});
test('inline review and Not now retain parent memory text and never send', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp070Fixture = await window.hcp070.renderFixture();
  });
  await page.getByLabel('Memory-only enquiry').fill('Keep this unsent request');
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await page.getByLabel('wss://inbox.example.org', { exact: true }).check();
  await page.getByRole('button', { name: 'Review inbox change' }).click();
  await expect(
    page.getByText(
      /This replaces your public inbox preference for other clients/
    )
  ).toBeVisible();
  await expect(
    page
      .getByRole('list', { name: 'Current observed inbox destinations' })
      .getByText('wss://old.example.org', { exact: true })
  ).toBeVisible();
  await expect(
    page
      .getByRole('list', { name: 'Proposed public inbox destinations' })
      .getByText('wss://old.example.org', { exact: true })
  ).toBeVisible();
  await expect(
    page
      .getByRole('list', { name: 'Proposed public inbox destinations' })
      .getByText('wss://inbox.example.org', { exact: true })
  ).toBeVisible();
  await page.getByRole('button', { name: 'Not now' }).click();
  await expect(page.getByLabel('Memory-only enquiry')).toHaveValue(
    'Keep this unsent request'
  );
  await expect(page.getByText('Explicit enquiry sends: 0')).toBeVisible();
  expect(frames).toEqual([]);
});
test('compatible observed preference offers Unlock and never a redundant replacement', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp070Fixture = await window.hcp070.renderFixture('compatible');
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await expect(
    page.getByRole('button', { name: 'Unlock inbox' })
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Enable inbox' })).toHaveCount(
    0
  );
  await page.getByRole('button', { name: 'Unlock inbox' }).click();
  await expect(
    page.getByText('Inbox access has not been qualified and exercised.')
  ).toBeVisible();
  expect(frames).toEqual([]);
});
test('Enable displays real approval wait, records named preference acceptance and never sends an enquiry', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = await window.hcp070.renderFixture('publish');
    window.hcp070Fixture = f;
    const wait = new Promise<void>((resolve) => {
      window.hcp070Release = resolve;
    });
    f.beforeSign(() => wait);
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await page.getByLabel('wss://inbox.example.org', { exact: true }).check();
  await page.getByRole('button', { name: 'Review inbox change' }).click();
  await page.getByRole('button', { name: 'Enable inbox' }).click();
  await expect(
    page.getByText(
      'Preparing your inbox setup. Extension approvals or named relay acceptance may be pending. Another approval may be needed.'
    )
  ).toBeVisible();
  expect(frames).toEqual([]);
  await page.evaluate(() => window.hcp070Release?.());
  await expect(
    page.getByText('Relay acceptance and exact readback are separate facts.')
  ).toBeVisible();
  await expect(
    page.getByText('Accepted by wss://discovery.example.org')
  ).toBeVisible();
  await expect(page.getByText('Explicit enquiry sends: 0')).toBeVisible();
  expect(frames).toHaveLength(1);
  expect((JSON.parse(frames[0]) as [string, { kind: number }])[1].kind).toBe(
    10050
  );
});
test('Not now during real approval wait prevents a late signature from publishing', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = await window.hcp070.renderFixture('publish');
    window.hcp070Fixture = f;
    const wait = new Promise<void>((resolve) => {
      window.hcp070Release = resolve;
    });
    f.beforeSign(() => wait);
  });
  await page.getByRole('button', { name: 'Check current preference' }).click();
  await page.getByLabel('wss://inbox.example.org', { exact: true }).check();
  await page.getByRole('button', { name: 'Review inbox change' }).click();
  await page.getByRole('button', { name: 'Enable inbox' }).click();
  await expect(
    page.getByText(
      'Preparing your inbox setup. Extension approvals or named relay acceptance may be pending. Another approval may be needed.'
    )
  ).toBeVisible();
  await page.getByRole('button', { name: 'Not now' }).click();
  await page.evaluate(() => window.hcp070Release?.());
  await expect(
    page.getByText(
      'Inbox setup is paused. Your unsent text stays in this page.'
    )
  ).toBeVisible();
  await expect(page.getByText('Explicit enquiry sends: 0')).toBeVisible();
  expect(frames).toEqual([]);
});
