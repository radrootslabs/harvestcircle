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
  test.setTimeout(1_800_000);
  directory = await mkdtemp(path.join(tmpdir(), 'hcp015-search-form-'));
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
      'web/package.json',
      'web/pnpm-lock.yaml',
      'web/src',
      'web/tools/source-boundaries.mjs',
      'web/src/lib/components/AppShell.svelte',
      'web/src/app.css',
      'web/src/routes/+layout.svelte',
      'web/src/lib/components/SearchForm.svelte',
      'web/src/routes/+page.svelte',
      'web/tests/unit/components/search-form.test.ts',
      'web/tests/e2e/search-form.spec.ts',
      'web/tests/e2e/harness/search-form.svelte',
      'web/tests/integration/harness/static.ts'
    ])
      await cp(path.join(capsule, relative), path.join(checkout, relative), {
        recursive: true
      });
    const fixture = await readFile(
      path.join(capsule, 'web/tests/e2e/harness/search-form.svelte'),
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
      { cwd: path.join(checkout, 'web'), stdio: 'pipe', timeout: 1800000 }
    );
    const sourceAudit = execFileSync(
      process.execPath,
      ['tools/check-source.mjs'],
      { cwd: path.join(checkout, 'web') }
    ).toString();
    const identities = [];
    for (const relative of [
      'web/src/lib/components/AppShell.svelte',
      'web/src/lib/components/SearchForm.svelte',
      'web/src/lib/components/primitives/FormField.svelte',
      'web/src/lib/components/primitives/Button.svelte',
      'web/src/app.css',
      'web/src/theme.css',
      'web/src/routes/+layout.svelte'
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
        timeout: 1800000
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
        fixture: 'HCP015_TEST_ONLY_COMPILE',
        identities,
        sourceAudit,
        productionRouteSha256: createHash('sha256')
          .update(
            await readFile(path.join(capsule, 'web/src/routes/+page.svelte'))
          )
          .digest('hex'),
        fixtureSourceSha256: createHash('sha256').update(fixture).digest('hex'),
        compiledRouteSha256: createHash('sha256')
          .update(
            await readFile(path.join(checkout, 'web/src/routes/+page.svelte'))
          )
          .digest('hex'),
        compiledLayoutSha256: createHash('sha256')
          .update(
            await readFile(path.join(checkout, 'web/src/routes/+layout.svelte'))
          )
          .digest('hex'),
        retention:
          'HASHLOGS_ONLY: disposable compiled fixture output is removed after tests',
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
  test(`actual shared form submits raw blank/multiline input and guards IME/unavailable at ${width}px`, async ({
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
      const query = page.getByRole('textbox', {
        name: 'What are you looking for?'
      });
      const search = page.getByRole('button', { name: 'Search', exact: true });
      const status = page.getByRole('main').getByRole('status');
      await expect(query).toHaveCount(1);
      await expect(query).toHaveAttribute('rows', '2');
      await expect(search).toBeEnabled();
      await search.click();
      await expect(status).toHaveText('[""]');
      await query.fill('  carrots & Victoria  ');
      await query.press('End');
      await query.press('Enter');
      await query.press('x');
      await expect(query).toHaveValue('  carrots & Victoria  \nx');
      await expect(status).toHaveText('[""]');
      await query.press('Control+Enter');
      await query.press('Meta+Enter');
      await expect(status).toHaveText('[""]');
      await query.fill('  carrots & Victoria  \nx');
      await search.click();
      await expect(status).toHaveText(
        JSON.stringify(['', '  carrots & Victoria  \nx'])
      );

      await query.dispatchEvent('compositionstart', { data: '菜' });
      await query.dispatchEvent('keydown', {
        key: 'Enter',
        ctrlKey: true,
        isComposing: true
      });
      await query.dispatchEvent('keydown', {
        key: 'Enter',
        metaKey: true,
        isComposing: true
      });
      await page.locator('form').evaluate((form) => {
        if (
          form.dispatchEvent(
            new SubmitEvent('submit', { bubbles: true, cancelable: true })
          )
        )
          throw new Error('Composing submit was not prevented');
      });
      await expect(status).toHaveText(
        JSON.stringify(['', '  carrots & Victoria  \nx'])
      );
      await query.dispatchEvent('compositionend', { data: '菜' });
      await query.fill('菜');
      await query.press('Tab');
      await expect(search).toBeFocused();
      await page.keyboard.press('Enter');
      const expected = JSON.stringify(['', '  carrots & Victoria  \nx', '菜']);
      await expect(status).toHaveText(expected);

      for (const toggle of ['Toggle disabled', 'Toggle caller']) {
        await page.getByRole('button', { name: toggle }).click();
        await expect(search).toBeDisabled();
        await expect(
          page.getByText('Search is unavailable during development.')
        ).toBeVisible();
        await query.fill('not submitted');
        await query.press('Control+Enter');
        await query.press('Meta+Enter');
        await search.evaluate((button) =>
          button.dispatchEvent(new MouseEvent('click', { bubbles: true }))
        );
        await page.locator('form').evaluate((form) => {
          if (
            form.dispatchEvent(
              new SubmitEvent('submit', { bubbles: true, cancelable: true })
            )
          )
            throw new Error('Unavailable submit was not prevented');
        });
        await expect(status).toHaveText(expected);
        await page.getByRole('button', { name: toggle }).click();
        await expect(search).toBeEnabled();
      }
      const measured = await page.evaluate(() => ({
        width: innerWidth,
        scroll: globalThis.document.documentElement.scrollWidth,
        textarea: (() => {
          const rect = globalThis.document
            .querySelector('textarea')!
            .getBoundingClientRect();
          return { width: rect.width, height: rect.height, x: rect.x };
        })(),
        controls: Array.from(
          globalThis.document.querySelectorAll('button')
        ).map((element) => element.getBoundingClientRect().height)
      }));
      expect(measured.scroll).toBeLessThanOrEqual(width);
      expect(measured.textarea.width).toBeGreaterThan(0);
      expect(measured.textarea.x + measured.textarea.width).toBeLessThanOrEqual(
        width
      );
      for (const height of measured.controls)
        expect(height).toBeGreaterThanOrEqual(44);
      console.log(
        JSON.stringify({
          fixture: 'HCP015_CONTROLLED_FORM_ONLY',
          measured,
          submitted: ['', '  carrots & Victoria  \nx', '菜'],
          shortcut: 'OMITTED',
          composition: 'forced submit prevented until compositionend'
        })
      );
      expect(new URL(page.url()).pathname).toBe('/');
      expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
      expect(external).toEqual([]);
    } finally {
      await context.close();
    }
  });
}
