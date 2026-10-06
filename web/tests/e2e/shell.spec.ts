import { expect, test } from '@playwright/test';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rm,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';

// Isolated qualification clone: actual components compile into disposable output.
// The primary static payload and product route source are never overwritten.
const capsule = fileURLToPath(new URL('../../../', import.meta.url));
let directory: string;
let server: Awaited<ReturnType<typeof createStaticHarness>>;
test.beforeAll(async () => {
  test.setTimeout(120_000);
  directory = await mkdtemp(path.join(tmpdir(), 'hcp014-shell-'));
  try {
    const checkout = path.join(directory, 'checkout');
    execFileSync('git', [
      'clone',
      '--quiet',
      '--no-hardlinks',
      capsule,
      checkout
    ]);
    for (const relative of [
      'web/src/lib/components/AppShell.svelte',
      'web/src/app.css',
      'web/src/routes/+layout.svelte',
      'web/tests/unit/components/shell.test.ts',
      'web/tests/e2e/shell.spec.ts',
      'web/tests/e2e/harness/shell.svelte',
      'web/tests/integration/harness/static.ts'
    ])
      await cp(path.join(capsule, relative), path.join(checkout, relative), {
        recursive: true
      });
    const fixture = await readFile(
      path.join(capsule, 'web/tests/e2e/harness/shell.svelte'),
      'utf8'
    );
    await writeFile(
      path.join(checkout, 'web/src/routes/+page.svelte'),
      fixture.replaceAll('../../../src/lib/', '../lib/')
    );
    // Runtime store metadata is volume-specific; install the exact locked graph
    // into this disposable clone without a network or lock-repair fallback.
    const installed = execFileSync(
      'corepack',
      ['pnpm', 'install', '--offline', '--frozen-lockfile'],
      { cwd: path.join(checkout, 'web'), stdio: 'pipe', timeout: 90_000 }
    );
    const fixtureLayout =
      '<script lang="ts">import "../theme.css"; import "../app.css"; import type { Snippet } from "svelte"; let { children }: { children: Snippet } = $props();</script>{@render children()}';
    await writeFile(
      path.join(checkout, 'web/src/routes/+layout.svelte'),
      fixtureLayout
    );
    // Labelled test destinations only: no product feature/controller qualification.
    const derivedTargets = [];
    for (const route of ['search', 'sell', 'selling', 'about', 'privacy']) {
      const relative = 'web/src/routes/' + route + '/+page.svelte';
      const source =
        '<h1>HC_TEST_ONLY_NAVIGATION_TARGET</h1><p id="help">Test destination: ' +
        route +
        '</p>';
      await mkdir(path.dirname(path.join(checkout, relative)), {
        recursive: true
      });
      await writeFile(path.join(checkout, relative), source);
      derivedTargets.push({
        path: relative,
        source,
        sha256: createHash('sha256').update(source).digest('hex')
      });
    }
    const sourceAudit = execFileSync(
      process.execPath,
      ['tools/check-source.mjs'],
      { cwd: path.join(checkout, 'web') }
    ).toString();
    const identities = [];
    for (const relative of [
      'web/src/lib/components/AppShell.svelte',
      'web/src/app.css'
    ]) {
      identities.push({
        name: relative,
        sha256: createHash('sha256')
          .update(await readFile(path.join(checkout, relative)))
          .digest('hex')
      });
    }
    execFileSync(process.execPath, ['tools/build-info.mjs'], {
      cwd: path.join(checkout, 'web')
    });
    const compiled = execFileSync(
      process.execPath,
      ['node_modules/vite/bin/vite.js', 'build'],
      {
        cwd: path.join(checkout, 'web'),
        stdio: 'pipe',
        timeout: 90_000
      }
    );
    async function payload(
      directory: string,
      prefix = ''
    ): Promise<{ path: string; sha256: string }[]> {
      const files = [];
      for (const entry of await readdir(directory, { withFileTypes: true })) {
        const relative = prefix + entry.name;
        if (entry.isDirectory())
          files.push(
            ...(await payload(path.join(directory, entry.name), relative + '/'))
          );
        else
          files.push({
            path: relative,
            sha256: createHash('sha256')
              .update(await readFile(path.join(directory, entry.name)))
              .digest('hex')
          });
      }
      return files.sort((left, right) => left.path.localeCompare(right.path));
    }
    console.log(
      JSON.stringify({
        fixture: 'HCP014_TEST_ONLY_COMPILE',
        identities,
        sourceAudit,
        fixtureSourceSha256: createHash('sha256').update(fixture).digest('hex'),
        compiledRouteSha256: createHash('sha256')
          .update(
            await readFile(path.join(checkout, 'web/src/routes/+page.svelte'))
          )
          .digest('hex'),
        compiledLayoutSha256: createHash('sha256')
          .update(fixtureLayout)
          .digest('hex'),
        compiledLayout: fixtureLayout,
        derivedTargets,
        payload: await payload(path.join(checkout, 'web/build')),
        installed: installed.toString(),
        compiled: compiled.toString(),
        metadata: JSON.parse(
          await readFile(
            path.join(checkout, 'web/build/build-info.json'),
            'utf8'
          )
        ) as unknown
      })
    );
    server = await createStaticHarness({
      buildRoot: path.join(checkout, 'web/build')
    });
  } catch (error) {
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
});
test.afterAll(async () => {
  await server?.close();
  if (directory) await rm(directory, { recursive: true, force: true });
  if (server)
    expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

async function measure(page: import('@playwright/test').Page, stage: string) {
  const measured = await page.evaluate(() => ({
    width: innerWidth,
    scroll: globalThis.document.documentElement.scrollWidth,
    navigation: Array.from(
      globalThis.document.querySelectorAll(
        'nav[aria-label="Primary"] > .cluster > *'
      )
    ).map((element) => {
      const rect = element.getBoundingClientRect();
      return {
        label: element.textContent?.trim().replace(/\s+/g, ' '),
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height
      };
    }),
    heights: Array.from(
      globalThis.document.querySelectorAll(
        'nav a,nav span[aria-disabled],button,summary'
      )
    )
      .filter((element) => element.getBoundingClientRect().height > 0)
      .map((element) => element.getBoundingClientRect().height)
  }));
  expect(measured.scroll).toBeLessThanOrEqual(measured.width);
  for (const height of measured.heights)
    expect(height).toBeGreaterThanOrEqual(44);
  for (let index = 1; index < measured.navigation.length; index++) {
    const previous = measured.navigation[index - 1];
    const current = measured.navigation[index];
    expect(current.y + current.height / 2).toBeGreaterThanOrEqual(
      previous.y + previous.height / 2
    );
    if (current.y < previous.y + previous.height)
      expect(current.x).toBeGreaterThanOrEqual(previous.x + previous.width);
  }
  if (measured.width === 320)
    expect(measured.navigation.at(-1)!.y).toBeGreaterThan(
      measured.navigation[0].y
    );
  console.log(
    JSON.stringify({
      fixture: 'HCP014_CONTROLLED_PRESENTATION_ONLY',
      stage,
      measured
    })
  );
  return measured;
}

for (const width of [320, 1024]) {
  test(`actual shell keyboard, guest/connected order and unavailable callbacks at ${width}px`, async ({
    browser
  }) => {
    const context = await browser.newContext({
      viewport: { width, height: 900 }
    });
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
      const skip = page.getByRole('link', { name: 'Skip to main content' });
      await expect(
        page.getByRole('button', { name: 'Connect extension' })
      ).toBeEnabled();
      await page.keyboard.press('Tab');
      await expect(skip).toBeFocused();
      await expect(skip).toBeVisible();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('main')).toBeFocused();
      await expect(page.locator('main')).toHaveCount(1);
      await expect(page.locator('[id="main-content"]')).toHaveCount(1);
      const primary = page.getByRole('navigation', {
        name: 'Primary',
        exact: true
      });
      await expect(primary).toHaveText(
        'HarvestCircle Search List food Connect extension'
      );
      await expect(
        page.getByRole('link', { name: 'Search', exact: true })
      ).toHaveAttribute('aria-current', 'page');
      expect(await page.locator('a[aria-current="page"]').count()).toBe(1);
      const guest = await measure(page, 'guest');
      expect(guest.navigation.map((item) => item.label)).toEqual([
        'HarvestCircle',
        'Search',
        'List food',
        'Connect extension'
      ]);
      const guestConnect = page.getByRole('button', {
        name: 'Connect extension'
      });
      await guestConnect.focus();
      await expect(guestConnect).toBeFocused();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('status')).toHaveText('Commands: 1');
      await expect(primary).toContainText(
        'HarvestCircle Search Messages Selling Identity'
      );
      await expect(page.getByText('Messages', { exact: true })).toHaveAttribute(
        'aria-disabled',
        'true'
      );
      await expect(page.getByRole('link', { name: 'Messages' })).toHaveCount(0);
      await expect(page.getByRole('link', { name: 'Selling' })).toHaveAttribute(
        'href',
        '/selling'
      );
      const connectedClosed = await measure(page, 'connected-closed');
      expect(
        connectedClosed.navigation.map((item) => item.label?.split(' ')[0])
      ).toEqual(['HarvestCircle', 'Search', 'Messages', 'Selling', 'Identity']);
      const summary = page.locator('summary');
      await summary.focus();
      await page.keyboard.press('Enter');
      await expect(page.locator('details')).toHaveAttribute('open', '');
      await page.keyboard.press('Space');
      await expect(page.locator('details')).not.toHaveAttribute('open', '');
      await page.keyboard.press('Enter');
      await expect(page.locator('details')).toHaveAttribute('open', '');
      await expect(page.locator('.key')).toHaveText(
        'HC_TEST_ONLY_PUBLIC_KEY_' + 'a'.repeat(64)
      );
      await page.keyboard.press('Tab');
      const disconnect = page.getByRole('button', {
        name: 'Disconnect',
        exact: true
      });
      await expect(disconnect).toBeFocused();
      await page
        .getByRole('button', { name: 'Toggle command availability' })
        .click();
      await expect(disconnect).toBeDisabled();
      await disconnect.evaluate((element) =>
        (element as HTMLButtonElement).click()
      );
      await disconnect.evaluate((element) =>
        element.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      );
      await expect(page.getByRole('status')).toHaveText('Commands: 1');
      await measure(page, 'connected-open-long-key');
      await expect(page.locator('header')).not.toContainText(/unread/i);
      await page
        .getByRole('button', { name: 'Toggle command availability' })
        .click();
      await disconnect.focus();
      await expect(disconnect).toBeFocused();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('status')).toHaveText('Commands: 2');
      await page
        .getByRole('button', { name: 'Toggle command availability' })
        .click();
      const connect = page.getByRole('button', { name: 'Connect extension' });
      await expect(connect).toBeDisabled();
      await connect.evaluate((element) =>
        (element as HTMLButtonElement).click()
      );
      await connect.evaluate((element) =>
        element.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      );
      await expect(page.getByRole('status')).toHaveText('Commands: 2');
      expect(external).toEqual([]);
    } finally {
      await context.close();
    }
    expect(context.pages()).toHaveLength(0);
  });
}

test('controlled shell callbacks become available only after actual hydration', async ({
  browser
}) => {
  const context = await browser.newContext({
    viewport: { width: 320, height: 900 }
  });
  let releaseScripts!: () => void;
  const scriptsReady = new Promise<void>((resolve) => {
    releaseScripts = resolve;
  });
  let delayedScripts = 0;
  const external: string[] = [];
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
    await page.goto(server.url, { waitUntil: 'commit' });
    const connect = page.getByRole('button', { name: 'Connect extension' });
    await expect(
      page.getByRole('heading', { name: 'HC_TEST_ONLY_SHELL' })
    ).toBeVisible();
    await expect.poll(() => delayedScripts).toBeGreaterThan(0);
    await expect(connect).toBeDisabled();
    await expect(page.getByRole('status')).toHaveText('Commands: 0');
    releaseScripts();
    await expect(connect).toBeEnabled();
    await connect.focus();
    await expect(connect).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('status')).toHaveText('Commands: 1');
    const identity = page.locator('summary');
    await identity.focus();
    await expect(identity).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.locator('details')).toHaveAttribute('open', '');
    await expect(
      page.getByRole('button', { name: 'Disconnect', exact: true })
    ).toBeEnabled();
    expect(external).toEqual([]);
  } finally {
    releaseScripts();
    await context.close();
  }
  expect(context.pages()).toHaveLength(0);
});
