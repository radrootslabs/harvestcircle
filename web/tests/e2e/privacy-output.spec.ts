import { expect, test } from '@playwright/test';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
import { createStaticHarness } from '../integration/harness/static.ts';

test('HCP128 actual Messages SSR describes the connection and unlock condition', async ({
  request
}) => {
  const server = await createStaticHarness();
  try {
    const response = await request.get(server.url + '/messages', {
      headers: { Accept: 'text/html' }
    });
    expect(response.status()).toBe(200);
    const html = await response.text();
    expect(html).toContain(
      'Messages stay hidden until you connect and explicitly unlock your inbox.'
    );
    expect(html).not.toContain('unavailable during development');
  } finally {
    await server.close();
  }
});

test('HCP128 real private routes keep generic noindex metadata through hydration and reload without extension effects', async ({
  page
}) => {
  const coordinate = encodeProductReference({
    kind: 30402,
    pubkey: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
    identifier: 'HCP128_TEST_ONLY_PUBLIC_COORDINATE'
  });
  expect(coordinate).toBeTruthy();
  const handle = '12345678-1234-4234-8234-123456789abc';
  // Generic means independent of private content or peer identity. The
  // approved editor uses finite route-mode labels, not a product-only title.
  const paths = [
    ['/messages', 'HarvestCircle'],
    ['/messages/' + handle, 'HarvestCircle'],
    ['/sell', 'New listing · HarvestCircle'],
    ['/selling', 'HarvestCircle'],
    ['/selling/drafts/' + handle, 'Continue draft · HarvestCircle'],
    ['/products/' + coordinate + '/edit', 'Edit listing · HarvestCircle']
  ] as const;
  const server = await createStaticHarness();
  const errors: string[] = [];
  const external: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    await page.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    await page.addInitScript(() => {
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        get() {
          throw Error('HCP128 unrequested extension access');
        }
      });
    });
    for (const [pathname, title] of paths) {
      for (const navigate of [
        () => page.goto(server.url + pathname),
        () => page.reload()
      ]) {
        expect((await navigate())?.status()).toBe(200);
        await expect(
          page
            .getByRole('navigation', { name: 'Primary' })
            .getByRole('button', { name: 'Connect extension', exact: true })
        ).toBeEnabled();
        await expect(page).toHaveTitle(title);
        await expect(page.locator('meta[name="robots"]')).toHaveAttribute(
          'content',
          'noindex'
        );
        expect(new URL(page.url()).pathname).toBe(pathname);
        expect(new URL(page.url()).search + new URL(page.url()).hash).toBe('');
      }
    }
    expect(errors).toEqual([]);
    expect(external).toEqual([]);
  } finally {
    await server.close();
  }
});
