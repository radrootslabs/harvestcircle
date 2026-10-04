import { test, expect } from '@playwright/test';
import { createRelayHarness } from '../integration/harness/relay.ts';
import { createStaticHarness } from '../integration/harness/static.ts';
import { installControlledProvider } from './harness/provider.ts';

test('copied nested navigation and refresh boot the real shell without fabricating a product', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  const withoutJavaScript = await browser.newContext({
    javaScriptEnabled: false
  });
  try {
    for (const owned of [context, withoutJavaScript]) {
      await owned.route('**/*', (route) =>
        new URL(route.request().url()).origin === server.url
          ? route.continue()
          : route.abort()
      );
    }
    const copiedPath = '/products/unsupported-coordinate';
    const staticPage = await withoutJavaScript.newPage();
    await staticPage.goto(server.url);
    await expect(
      staticPage.getByRole('heading', { name: 'HarvestCircle' })
    ).toBeVisible();
    await expect(staticPage).toHaveTitle('HarvestCircle');
    const fallback = await staticPage.goto(server.url + copiedPath);
    expect(fallback?.status()).toBe(200);
    // The separate SPA document has no prerendered product or browser state.
    await expect(
      staticPage.getByRole('heading', { name: 'HarvestCircle' })
    ).toHaveCount(0);

    const page = await context.newPage();
    const failures: string[] = [];
    page.on('pageerror', (error) => failures.push(error.message));
    for (const navigate of [
      () => page.goto(server.url + copiedPath),
      () => page.reload()
    ]) {
      const result = await navigate();
      expect(result?.status()).toBe(200);
      await expect(
        page.getByRole('heading', { name: 'HarvestCircle' })
      ).toBeVisible();
      // Future product route controllers remain unimplemented in this slice.
      await expect(
        page.getByRole('heading', { name: '404', exact: true })
      ).toBeVisible();
      await expect(page.getByText('Not Found', { exact: true })).toBeVisible();
      expect(new URL(page.url()).pathname).toBe(copiedPath);
      expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
    }
    expect(failures).toEqual([]);
    for (const asset of ['/missing.js', '/missing.css', '/_app/missing.png']) {
      const response = await context.request.get(server.url + asset, {
        headers: { accept: 'text/html' }
      });
      expect(response.status()).toBe(404);
      expect(await response.text()).toBe('');
    }
  } finally {
    await context.close();
    await withoutJavaScript.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(withoutJavaScript.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

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
