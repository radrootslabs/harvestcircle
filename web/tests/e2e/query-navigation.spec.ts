import { test, expect } from '@playwright/test';
import { createStaticHarness } from '../integration/harness/static';

test('real public query navigation normalizes copies and restores Back/Forward state', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  const external: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    const page = await context.newPage();
    await page.goto(server.url);
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(page).toHaveURL(server.url + '/search');
    const input = page.getByRole('textbox', {
      name: 'What are you looking for?'
    });
    await input.fill('  ＣＡＲＲＯＴＳ\nVICTORIA 菜  ');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(page).toHaveURL(
      server.url + '/search?q=carrots+victoria+%E8%8F%9C'
    );
    await expect(input).toHaveValue('carrots victoria 菜');
    await expect(
      page.getByText('Search data is unavailable during development.')
    ).toBeVisible();
    await input.fill('Turnips');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(page).toHaveURL(server.url + '/search?q=turnips');
    await page.goBack();
    await expect(input).toHaveValue('carrots victoria 菜');
    await page.goForward();
    await expect(input).toHaveValue('turnips');
    expect([...new URL(page.url()).searchParams.keys()]).toEqual(['q']);
    expect(external).toEqual([]);
    expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

test('real search preserves rejected input and does not navigate or execute excess queries', async ({
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
    const input = page.getByRole('textbox', {
      name: 'What are you looking for?'
    });
    for (const [value, message] of [
      ['菜'.repeat(171), 'Use at most 512 UTF-8 bytes'],
      [Array(13).fill('carrots').join(' '), 'Use at most 12 words']
    ] as const) {
      await input.fill(value);
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect(input).toHaveValue(value);
      await expect(input).toHaveAttribute('aria-invalid', 'true');
      await expect(page.getByText(message, { exact: false })).toBeVisible();
      expect(new URL(page.url()).pathname).toBe('/');
      expect(new URL(page.url()).search).toBe('');
    }
    await page.goto(server.url + '/search?q=carrots&body=private');
    await expect(
      page.getByText('Enter plain search words without private URL context.')
    ).toBeVisible();
    await expect(
      page.getByRole('textbox', { name: 'What are you looking for?' })
    ).toHaveValue('');
  } finally {
    await context.close();
    await server.close();
  }
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

test('search editing waits for real hydration and query restoration when scripts are delayed', async ({
  browser
}) => {
  const server = await createStaticHarness();
  const context = await browser.newContext();
  const external: string[] = [];
  let releaseScripts!: () => void;
  const scriptsReady = new Promise<void>((resolve) => {
    releaseScripts = resolve;
  });
  let delayedScripts = 0;
  try {
    await context.route('**/*', async (route) => {
      if (new URL(route.request().url()).origin !== server.url) {
        external.push(route.request().url());
        return route.abort();
      }
      if (new URL(route.request().url()).pathname.endsWith('.js')) {
        delayedScripts++;
        await scriptsReady;
      }
      return route.continue();
    });
    const page = await context.newPage();
    await page.goto(server.url + '/search?q=turnips', { waitUntil: 'commit' });
    const input = page.getByRole('textbox', {
      name: 'What are you looking for?'
    });
    const submit = page.getByRole('button', { name: 'Search', exact: true });
    await expect(
      page.getByRole('heading', { name: 'Search food' })
    ).toBeVisible();
    await expect.poll(() => delayedScripts).toBeGreaterThan(0);
    await expect(input).toBeDisabled();
    await expect(submit).toBeDisabled();
    releaseScripts();
    await expect(input).toBeEnabled();
    await expect(input).toHaveValue('turnips');
    await expect(submit).toBeEnabled();
    await input.fill('  ＣＡＲＲＯＴＳ\nVICTORIA 菜  ');
    await submit.click();
    await expect(page).toHaveURL(
      server.url + '/search?q=carrots+victoria+%E8%8F%9C'
    );
    await expect(input).toBeEnabled();
    await expect(input).toHaveValue('carrots victoria 菜');
    expect(external).toEqual([]);
  } finally {
    releaseScripts();
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});
