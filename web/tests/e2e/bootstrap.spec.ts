import { test, expect } from '@playwright/test';
import { createRelayHarness } from '../integration/harness/relay.ts';
import { createStaticHarness } from '../integration/harness/static.ts';
import { installControlledProvider } from './harness/provider.ts';

test('actual browser loads guest shell without an extension', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  try {
    await context.route('**/*', (route) =>
      new URL(route.request().url()).origin === server.url
        ? route.continue()
        : route.abort()
    );
    const page = await context.newPage();
    await page.goto(server.url);
    await expect(
      page.getByRole('heading', { name: 'HarvestCircle' })
    ).toBeVisible();
    await expect(page).toHaveTitle('HarvestCircle');
    expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
    console.log(`Controlled browser version: ${browser.version()}`);
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

test('controlled provider and actual loopback socket are isolated and torn down', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const relay = await createRelayHarness();
  const context = await browser.newContext();
  try {
    await installControlledProvider(context);
    await context.route('**/*', (route) =>
      new URL(route.request().url()).origin === server.url
        ? route.continue()
        : route.abort()
    );
    const page = await context.newPage();
    await page.goto(server.url);
    await expect(
      page.getByRole('heading', { name: 'HarvestCircle' })
    ).toBeVisible();
    const result = await page.evaluate(async (url) => {
      const provider = (
        window as unknown as {
          nostr: {
            fixture: string;
            getPublicKey(): Promise<string>;
            signEvent(): Promise<never>;
          };
        }
      ).nostr;
      let signingRejected = false;
      try {
        await provider.signEvent();
      } catch {
        signingRejected = true;
      }
      const reply = await new Promise<string>((resolve, reject) => {
        const socket = new WebSocket(url);
        socket.onopen = () =>
          socket.send(JSON.stringify(['REQ', 'browser', {}]));
        socket.onmessage = (event: MessageEvent<string>) => resolve(event.data);
        socket.onerror = () => reject(new Error('Loopback socket failed'));
      });
      return {
        fixture: provider.fixture,
        publicKey: await provider.getPublicKey(),
        signingRejected,
        reply
      };
    }, relay.url);
    expect(result).toEqual({
      fixture: 'HC_TEST_ONLY_PROVIDER',
      publicKey: '11'.repeat(32),
      signingRejected: true,
      reply: '["EOSE","browser"]'
    });
    expect(relay.state().subscriptions).toBe(1);
  } finally {
    await context.close();
    await relay.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(relay.state()).toEqual({
    listening: false,
    connections: 0,
    subscriptions: 0,
    childProcesses: 0
  });
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});
