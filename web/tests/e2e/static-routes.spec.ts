import { test, expect } from '@playwright/test';
import { naddrEncode } from 'applesauce-core/helpers/pointers';
import {
  encodeProductReference,
  decodeProductReference
} from '../../src/lib/nostr/references.ts';
import { createStaticHarness } from '../integration/harness/static.ts';

test('built fallback preserves exact raw identifier through hint-free public links and reload without hinted egress', async ({
  browser
}) => {
  const coordinate = {
    kind: 30402,
    pubkey: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
    identifier: '\ufeffRAW_D e\u0301/菜'
  };
  const canonical = encodeProductReference(coordinate);
  if (!canonical) throw Error('qualified coordinate unavailable');
  const hinted = naddrEncode({
    ...coordinate,
    relays: ['wss://unapproved.invalid/']
  }).toUpperCase();
  expect(decodeProductReference(hinted)?.identifier).toBe(
    coordinate.identifier
  );
  const server = await createStaticHarness();
  const context = await browser.newContext();
  const external: string[] = [],
    errors: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    await context.addInitScript(() => {
      for (const name of ['nostr', 'WebSocket'])
        Object.defineProperty(window, name, {
          configurable: true,
          get() {
            throw Error('guest acquired ' + name);
          }
        });
    });
    const page = await context.newPage();
    page.on('pageerror', (error) => errors.push(error.message));
    expect(
      (await page.goto(server.url + '/products/' + hinted))?.status()
    ).toBe(200);
    const link = page.getByRole('link', {
      name: 'Public food link',
      exact: true
    });
    await expect(link).toHaveAttribute('href', '/products/' + canonical);
    await expect(page).toHaveTitle('Food — HarvestCircle');
    await expect(
      page.getByText('Food sources are unavailable.', { exact: true })
    ).toBeVisible();
    await link.click();
    await expect(page).toHaveURL(server.url + '/products/' + canonical);
    expect((await page.reload())?.status()).toBe(200);
    await expect(
      page.getByRole('link', { name: 'Public food link', exact: true })
    ).toHaveAttribute('href', '/products/' + canonical);
    expect(
      decodeProductReference(
        new URL(page.url()).pathname.slice('/products/'.length)
      )?.identifier
    ).toBe(coordinate.identifier);
    for (const target of [
      '/products/' + 'naddr1' + 'q'.repeat(2049),
      '/_app/missing.js',
      '/missing.css'
    ]) {
      const response = await context.request.get(server.url + target, {
        headers: { accept: 'text/html' },
        maxRedirects: 0
      });
      expect(response.status()).toBe(404);
      expect(response.headers().location).toBeUndefined();
      expect(await response.text()).toBe('');
    }
    expect(external).toEqual([]);
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});
