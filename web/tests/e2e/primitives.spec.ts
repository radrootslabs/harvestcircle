import { expect, test } from '@playwright/test';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  mkdtemp,
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
  directory = await mkdtemp(path.join(tmpdir(), 'hcp013-components-'));
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
      'web/src/lib/components/primitives',
      'web/tests/unit/components/primitives.test.ts',
      'web/tests/e2e/primitives.spec.ts',
      'web/tests/e2e/harness/primitives.svelte',
      'web/tests/integration/harness/static.ts'
    ])
      await cp(path.join(capsule, relative), path.join(checkout, relative), {
        recursive: true
      });
    const fixture = await readFile(
      path.join(capsule, 'web/tests/e2e/harness/primitives.svelte'),
      'utf8'
    );
    await writeFile(
      path.join(checkout, 'web/src/routes/+page.svelte'),
      fixture.replaceAll('../../../src/lib/', '../lib/')
    );
    // Runtime store metadata is volume-specific; install the exact locked graph
    // into this disposable clone without a network or lock-repair fallback.
    const sourceStore = execFileSync('corepack', ['pnpm', 'store', 'path'], {
      cwd: path.join(capsule, 'web')
    })
      .toString()
      .trim();
    const installed = execFileSync(
      'corepack',
      [
        'pnpm',
        'install',
        '--offline',
        '--frozen-lockfile',
        '--store-dir',
        path.dirname(sourceStore)
      ],
      { cwd: path.join(checkout, 'web'), stdio: 'pipe', timeout: 90_000 }
    );
    const sourceAudit = execFileSync(
      process.execPath,
      ['tools/check-source.mjs'],
      { cwd: path.join(checkout, 'web') }
    ).toString();
    const identities = [];
    for (const name of [
      'FormField',
      'Button',
      'Disclosure',
      'PageHeading',
      'ActionGroup',
      'EmptyState',
      'ConfirmPanel'
    ]) {
      const source = await readFile(
        path.join(
          checkout,
          'web/src/lib/components/primitives',
          name + '.svelte'
        )
      );
      identities.push({
        name,
        sha256: createHash('sha256').update(source).digest('hex')
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
        fixture: 'HCP013_TEST_ONLY_COMPILE',
        identities,
        sourceAudit,
        fixtureSourceSha256: createHash('sha256').update(fixture).digest('hex'),
        compiledRouteSha256: createHash('sha256')
          .update(
            await readFile(path.join(checkout, 'web/src/routes/+page.svelte'))
          )
          .digest('hex'),
        payload: await payload(path.join(checkout, 'web/build')),
        sourceStore,
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

for (const width of [320, 1024]) {
  test(`actual primitives preserve labels, keyboard and safe return at ${width}px`, async ({
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
      const input = page.getByLabel('Public terms', { exact: true });
      await expect(input).toHaveAttribute(
        'aria-describedby',
        'terms-hint terms-error'
      );
      await expect(input).toHaveAttribute('aria-invalid', 'true');
      await expect(page.locator('#terms-error')).toHaveText(
        'Enter public terms'
      );
      const errorBox = await page.locator('#terms-error').boundingBox();
      const inputBox = await input.boundingBox();
      expect(errorBox!.y + errorBox!.height).toBeLessThanOrEqual(inputBox!.y);
      await input.focus();
      await page.keyboard.press('Tab');
      await expect(
        page.getByRole('button', { name: 'Fixture command', exact: true })
      ).toBeFocused();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('status')).toHaveText('Commands: 1');
      await expect(
        page.getByRole('button', { name: 'Disabled command' })
      ).toBeDisabled();
      await page
        .getByRole('button', { name: 'Disabled command' })
        .evaluate((element) => (element as HTMLButtonElement).click());
      await expect(page.getByRole('status')).toHaveText('Commands: 1');
      await page
        .getByRole('button', { name: 'Disabled command' })
        .evaluate((element) =>
          element.dispatchEvent(new MouseEvent('click', { bubbles: true }))
        );
      await expect(page.getByRole('status')).toHaveText('Commands: 1');
      await page.keyboard.press('Tab');
      await expect(
        page.getByRole('link', { name: 'Fixture navigation' })
      ).toBeFocused();
      await page.keyboard.press('Tab');
      await expect(page.locator('summary')).toBeFocused();
      await page.keyboard.press('Enter');
      await expect(page.locator('details')).toHaveAttribute('open', '');
      await page.keyboard.press('Space');
      await expect(page.locator('details')).not.toHaveAttribute('open', '');
      await page.getByRole('button', { name: 'Review discard' }).click();
      await page.keyboard.press('Tab');
      await expect(
        page.getByRole('button', { name: 'Keep editing' })
      ).toBeFocused();
      await expect(
        page.getByRole('region', { name: 'Discard this draft?' })
      ).toContainText('Remote copies remain.');
      await page.keyboard.press('Enter');
      await expect(
        page.getByRole('button', { name: 'Review discard' })
      ).toBeFocused();
      await expect(
        page.getByRole('button', { name: 'Keep editing' })
      ).toHaveCount(0);
      await page.keyboard.press('Enter');
      await page
        .getByRole('button', { name: 'Discard draft', exact: true })
        .click();
      await expect(page.getByRole('status')).toHaveText('Commands: 2');
      await expect(
        page.getByRole('button', { name: 'Discard draft', exact: true })
      ).toBeDisabled();
      await page.getByRole('button', { name: 'Keep editing' }).click();
      await expect(
        page.getByRole('button', { name: 'Review discard' })
      ).toBeFocused();
      const measured = await page.evaluate(() => ({
        width: innerWidth,
        scroll: document.documentElement.scrollWidth,
        heights: Array.from(
          document.querySelectorAll('input,button,a.button,summary')
        ).map((element) => element.getBoundingClientRect().height)
      }));
      expect(measured.scroll).toBeLessThanOrEqual(measured.width);
      for (const height of measured.heights)
        expect(height).toBeGreaterThanOrEqual(44);
      expect(external).toEqual([]);
      console.log(
        JSON.stringify({
          fixture: 'HCP013_ACTUAL_PRESENTATION_COMPONENTS_ONLY',
          measured
        })
      );
    } finally {
      await context.close();
    }
    expect(context.pages()).toHaveLength(0);
  });
}
