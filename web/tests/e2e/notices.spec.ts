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
  directory = await mkdtemp(path.join(tmpdir(), 'hcp016-notices-'));
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
      'web/src/lib/components/Notice.svelte',
      'web/src/lib/components/OperationStatus.svelte',
      'web/src/lib/components/ErrorSummary.svelte',
      'web/src/lib/presentation/status-copy.ts',
      'web/tests/unit/status-copy.test.ts',
      'web/tests/unit/components/notices.test.ts',
      'web/tests/unit/components/NoticeHarness.svelte',
      'web/tests/e2e/notices.spec.ts',
      'web/tests/e2e/harness/notices.svelte'
    ]) {
      await mkdir(path.dirname(path.join(checkout, relative)), {
        recursive: true
      });
      await cp(path.join(capsule, relative), path.join(checkout, relative));
    }
    const fixture = await readFile(
      path.join(capsule, 'web/tests/e2e/harness/notices.svelte'),
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
    for (const relative of [
      'web/src/lib/components/AppShell.svelte',
      'web/src/lib/components/Notice.svelte',
      'web/src/lib/components/OperationStatus.svelte',
      'web/src/lib/components/ErrorSummary.svelte',
      'web/src/lib/presentation/status-copy.ts',
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
        fixture: 'HCP016_TEST_ONLY_COMPILE',
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
  test(
    'persistent notices preserve field values and focus at ' + width + 'px',
    async ({ browser }) => {
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
        const field = page.getByRole('textbox', { name: 'Food', exact: true });
        await field.fill('unchanged multiline\n菜');
        await expect(
          page.getByRole('heading', { name: 'Check the following fields' })
        ).toHaveCount(0);
        await page.getByRole('button', { name: 'Review', exact: true }).click();
        await expect(field).toHaveValue('unchanged multiline\n菜');
        await expect(field).toHaveAttribute('aria-invalid', 'true');
        const link = page.getByRole('link', {
          name: 'Enter a supported food description.',
          exact: true
        });
        await expect(link).toHaveAttribute('href', '#food');
        await link.focus();
        await page.keyboard.press('Enter');
        await expect(field).toBeFocused();
        await expect(
          page.getByRole('link', { name: 'Unsafe target remains plain text.' })
        ).toHaveCount(0);
        const update = page.getByRole('button', { name: 'Update information' });
        await update.focus();
        await page.keyboard.press('Enter');
        await expect(update).toBeFocused();
        await expect(
          page.getByText(
            'One source did not respond. These results may be incomplete.'
          )
        ).toBeVisible();
        await expect(field).toHaveValue('unchanged multiline\n菜');
        await expect(link).toBeVisible();
        await expect(
          page.getByText(
            "We could not confirm whether the recipient's inbox accepted this message."
          )
        ).toBeVisible();
        await expect(
          page.getByText('The sender archive relay accepted the event.')
        ).toBeVisible();
        await expect(
          page.getByText(
            'Exact read-back is confirmed for this event and target.'
          )
        ).toBeVisible();
        const measured = await page.evaluate(() => ({
          width: innerWidth,
          scroll: globalThis.document.documentElement.scrollWidth,
          controls: Array.from(
            globalThis.document.querySelectorAll('button,a.shell-link')
          ).map((el) => el.getBoundingClientRect().height)
        }));
        expect(measured.scroll).toBeLessThanOrEqual(width);
        for (const height of measured.controls)
          expect(height).toBeGreaterThanOrEqual(44);
        console.log(
          JSON.stringify({
            fixture: 'HCP016_CONTROLLED_NOTICES_ONLY',
            measured,
            inputPreserved: true,
            informationalFocusPreserved: true,
            keyboardFieldFocus: true
          })
        );
        expect(external).toEqual([]);
        expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
      } finally {
        await context.close();
      }
    }
  );
}
