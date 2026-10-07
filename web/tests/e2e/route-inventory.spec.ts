import { test, expect } from '@playwright/test';
import { encodeProductReference } from '../../src/lib/nostr/references';
import { createStaticHarness } from '../integration/harness/static';

// Canonical coordinate uses the already qualified public generator point.
const naddr = encodeProductReference({
  kind: 30402,
  pubkey: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  identifier: 'HCP020_TEST_ONLY'
});
if (!naddr) throw new Error('Qualified coordinate could not be encoded');
const handle = '12345678-1234-4234-8234-123456789abc';
// Current-source applicability only: public shipment history remains unknown.
test('current source has no revision-one aliases or query-driven redirects', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  const external: string[] = [];
  const errors: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    await context.addInitScript(() => {
      for (const name of ['indexedDB', 'nostr', 'WebSocket'])
        Object.defineProperty(window, name, {
          configurable: true,
          get() {
            throw new Error('Guest attempted ' + name);
          }
        });
    });
    for (const pathname of [
      '/products',
      '/products?q=carrots',
      '/my-listings'
    ]) {
      const response = await context.request.get(server.url + pathname, {
        headers: { accept: 'text/html' },
        maxRedirects: 0
      });
      expect(response.status()).toBe(404);
      expect(response.headers().location).toBeUndefined();
      expect(await response.text()).toBe('');
    }
    const page = await context.newPage();
    page.on('pageerror', (error) => errors.push(error.message));
    for (const query of [
      new URLSearchParams({ draft: handle }),
      new URLSearchParams({ edit: naddr }),
      new URLSearchParams({
        draft: handle,
        edit: naddr,
        returnTo: 'https://outside.invalid/'
      })
    ]) {
      const target = server.url + '/sell?' + query.toString();
      expect((await page.goto(target))?.status()).toBe(200);
      await expect(
        page.getByRole('heading', { name: 'Connect or unlock', exact: true })
      ).toBeVisible();
      expect(page.url()).toBe(target);
      // Exercise the real initialized client and Back, rather than SSR alone.
      await page.getByRole('link', { name: 'Search', exact: true }).click();
      await expect(page.getByLabel('What are you looking for?')).toBeEnabled();
      await page.goBack();
      await expect(
        page.getByRole('heading', { name: 'Connect or unlock', exact: true })
      ).toBeVisible();
      expect(page.url()).toBe(target);
      await expect(
        page.locator('main input, main textarea, main form')
      ).toHaveCount(0);
      expect(await page.locator('main').textContent()).not.toContain(handle);
      expect(await page.locator('main').textContent()).not.toContain(naddr);
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
const cases = [
  { path: '/', title: 'HarvestCircle', private: false },
  { path: '/search', title: 'Search food — HarvestCircle', private: false },
  { path: '/products/' + naddr, title: 'Food — HarvestCircle', private: false },
  { path: '/sell', title: 'HarvestCircle', private: true },
  { path: '/selling', title: 'HarvestCircle', private: true },
  { path: '/selling/drafts/' + handle, title: 'HarvestCircle', private: true },
  {
    path: '/products/' + naddr + '/edit',
    title: 'HarvestCircle',
    private: true
  },
  { path: '/messages', title: 'HarvestCircle', private: true },
  { path: '/messages/' + handle, title: 'HarvestCircle', private: true },
  { path: '/about', title: 'About — HarvestCircle', private: false },
  { path: '/privacy', title: 'Privacy — HarvestCircle', private: false }
];
for (const routeCase of cases) {
  test(
    'direct navigation and reload preserve actual route shell ' +
      routeCase.path,
    async ({ browser }) => {
      const server = await createStaticHarness();
      const context = await browser.newContext();
      try {
        await context.route('**/*', (route) =>
          new URL(route.request().url()).origin === server.url
            ? route.continue()
            : route.abort()
        );
        // Tripwires enforce no guest account/storage/signing/relay acquisition.
        await context.addInitScript(() => {
          for (const name of ['indexedDB', 'nostr', 'WebSocket'])
            Object.defineProperty(window, name, {
              configurable: true,
              get() {
                throw new Error('Guest attempted ' + name);
              }
            });
        });
        const page = await context.newPage();
        const errors: string[] = [];
        page.on('pageerror', (error) => errors.push(error.message));
        for (const navigate of [
          () => page.goto(server.url + routeCase.path),
          () => page.reload()
        ]) {
          expect((await navigate())?.status()).toBe(200);
          await expect(page).toHaveTitle(routeCase.title);
          await expect(page.getByRole('main')).toHaveCount(1);
          expect(new URL(page.url()).pathname).toBe(routeCase.path);
          await expect(
            page.getByRole('link', { name: 'Search', exact: true })
          ).toHaveCount(1);
          if (routeCase.private) {
            await expect(
              page.getByRole('heading', {
                name: 'Connect or unlock',
                exact: true
              })
            ).toBeVisible();
            await expect(page.locator('meta[name="robots"]')).toHaveAttribute(
              'content',
              'noindex'
            );
            await expect(
              page.getByRole('button', { name: 'Unlock', exact: true })
            ).toBeDisabled();
            await expect(page.locator('main button')).toHaveCount(2);
            await expect(
              page
                .locator('main')
                .getByRole('button', { name: 'Connect extension', exact: true })
            ).toBeEnabled();
            await expect(
              page
                .locator('main')
                .getByRole('link', { name: 'Connection help' })
            ).toHaveAttribute('href', '/about#help');
            await expect(
              page.locator('main input, main textarea, main form')
            ).toHaveCount(0);
            expect(await page.locator('main').textContent()).not.toContain(
              handle
            );
            expect(await page.locator('main').textContent()).not.toContain(
              naddr
            );
          }
        }
        expect(errors).toEqual([]);
      } finally {
        await context.close();
        await server.close();
      }
      expect(context.pages()).toHaveLength(0);
      expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
    }
  );
}

test('unknown routes and invalid opaque syntax never substitute data or return missing assets as HTML', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    for (const pathname of [
      '/orders',
      '/checkout',
      '/login',
      '/api/login',
      '/profiles'
    ]) {
      const response = await context.request.get(server.url + pathname, {
        headers: { accept: 'text/html' }
      });
      expect(response.status()).toBe(404);
      expect(await response.text()).toBe('');
    }
    for (const pathname of [
      '/products/unsupported-coordinate',
      '/selling/drafts/unknown-draft',
      '/messages/unknown-conversation'
    ]) {
      await page.goto(server.url + pathname);
      await expect(
        page.getByRole('heading', { name: '404', exact: true })
      ).toBeVisible();
      await expect(page).toHaveTitle('HarvestCircle');
      await expect(
        page.locator('main input, main textarea, main form')
      ).toHaveCount(0);
    }
    for (const pathname of [
      '/missing.js',
      '/missing.css',
      '/_app/missing.js'
    ]) {
      const response = await context.request.get(server.url + pathname, {
        headers: { accept: 'text/html' }
      });
      expect(response.status()).toBe(404);
      expect(await response.text()).toBe('');
    }
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});
