import { test, expect, type BrowserContext } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { cp, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
declare global {
  interface Window {
    hcp056Counts: {
      keys: number;
      signs: number;
      encrypts: number;
      decrypts: number;
    };
    hcp056Release?: () => void;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
async function provider(
  context: BrowserContext,
  mode: 'exact' | 'refused' | 'pending' | 'unsupported' = 'exact'
) {
  await context.addInitScript(
    ({ owner, mode }) => {
      window.hcp056Counts = { keys: 0, signs: 0, encrypts: 0, decrypts: 0 };
      window.nostr = {
        getPublicKey: () => {
          window.hcp056Counts.keys++;
          if (mode === 'refused')
            return Promise.reject(new Error('HC_TEST_RAW_PROVIDER_DIAGNOSTIC'));
          if (mode === 'pending')
            return new Promise<string>((resolve) => {
              window.hcp056Release = () => resolve(owner);
            });
          return Promise.resolve(owner);
        },
        signEvent: () => {
          window.hcp056Counts.signs++;
          return Promise.reject(new Error('Unexpected signature'));
        },
        ...(mode === 'unsupported'
          ? {}
          : {
              nip44: {
                encrypt: (_key: string, text: string) => {
                  window.hcp056Counts.encrypts++;
                  return Promise.resolve(text);
                },
                decrypt: (_key: string, text: string) => {
                  window.hcp056Counts.decrypts++;
                  return Promise.resolve(text);
                }
              }
            })
      };
    },
    { owner, mode }
  );
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
test.beforeAll(async () => {
  server = await createStaticHarness();
});
test.afterAll(async () => {
  await server.close();
});
test('explicit Connect is enabled after hydration and never repeats Send', async ({
  browser
}) => {
  const context = await browser.newContext();
  await provider(context);
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/messages');
    const main = page.getByRole('main'),
      connect = main.getByRole('button', {
        name: 'Connect extension',
        exact: true
      });
    await expect(connect).toBeEnabled();
    await connect.focus();
    await page.keyboard.press('Enter');
    await expect(page.getByText('Identity', { exact: true })).toBeVisible();
    await expect(main.locator('[role="status"][tabindex="-1"]')).toBeFocused();
    await expect(main.locator('input, textarea, form')).toHaveCount(0);
    expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
      keys: 1,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
    expect(await main.textContent()).not.toContain(
      'HC_TEST_RAW_PROVIDER_DIAGNOSTIC'
    );
    await page.locator('summary').click();
    await page.getByRole('button', { name: 'Disconnect', exact: true }).click();
    await expect(connect).toBeEnabled();
    expect(await page.evaluate(() => window.hcp056Counts.keys)).toBe(1);
  } finally {
    await context.close();
  }
});
test('failed protected Connect cannot steal focus from a later navbar Connect', async ({
  browser
}) => {
  const context = await browser.newContext();
  await context.addInitScript((owner) => {
    let calls = 0;
    window.nostr = {
      getPublicKey: () =>
        ++calls === 1
          ? Promise.reject(new Error('denied'))
          : Promise.resolve(owner),
      signEvent: () => Promise.reject(new Error('Unexpected signature'))
    };
  }, owner);
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/messages');
    const main = page.getByRole('main');
    await main
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await expect(
      main.getByText('Connection was not approved.', { exact: true })
    ).toBeVisible();
    await page
      .getByRole('navigation', { name: 'Primary' })
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await expect(page.getByText('Identity', { exact: true })).toBeVisible();
    console.log(
      JSON.stringify({
        fixture: 'HCP056_COMPETING_FOCUS',
        activeId: await page.evaluate(() => document.activeElement?.id)
      })
    );
    await expect(page.locator('#identity-navbar-status')).toBeFocused();
    await expect(main.locator('#identity-protected-status')).not.toBeFocused();
  } finally {
    await context.close();
  }
});
test('Messages is discoverable to guests without acquiring identity or private data', async ({
  browser
}) => {
  const context = await browser.newContext();
  await provider(context);
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/about');
    const messages = page
      .getByRole('navigation', { name: 'Primary' })
      .getByRole('link', { name: 'Messages', exact: true });
    await expect(messages).toHaveAttribute('href', '/messages');
    await messages.click();
    await expect(
      page
        .getByRole('main')
        .getByRole('heading', { name: 'Connect or unlock', exact: true })
    ).toBeVisible();
    expect(await page.evaluate(() => window.hcp056Counts.keys)).toBe(0);
    await expect(
      page.locator('main input, main textarea, main form')
    ).toHaveCount(0);
  } finally {
    await context.close();
  }
});
test('denied connection has finite help and no raw provider diagnostic', async ({
  browser
}) => {
  const context = await browser.newContext();
  await provider(context, 'refused');
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/messages');
    const main = page.getByRole('main');
    await main
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await expect(
      main.getByText('Connection was not approved.', { exact: true })
    ).toBeVisible();
    await expect(
      main.getByRole('link', { name: 'Connection help' })
    ).toHaveAttribute('href', '/about#help');
    expect(await main.textContent()).not.toContain(
      'HC_TEST_RAW_PROVIDER_DIAGNOSTIC'
    );
    expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
      keys: 1,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
  } finally {
    await context.close();
  }
});
test('pending connection survives route change without restoring a protected form or scheduling again', async ({
  browser
}) => {
  const context = await browser.newContext();
  await provider(context, 'pending');
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/messages');
    await page
      .locator('main')
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await expect(
      page
        .locator('main')
        .getByText('Waiting for extension approval.', { exact: true })
    ).toBeVisible();
    await page.getByRole('link', { name: 'Search', exact: true }).click();
    const retained = page.getByLabel('What are you looking for?');
    await retained.focus();
    await page.evaluate(() => window.hcp056Release?.());
    await expect(page.getByText('Identity', { exact: true })).toBeVisible();
    await expect(retained).toBeFocused();
    await expect(page.getByLabel('What are you looking for?')).toBeVisible();
    expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
      keys: 1,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
  } finally {
    await context.close();
  }
});
test('Disconnect while approval is pending keeps the real slot and fences late connection', async ({
  browser
}) => {
  const context = await browser.newContext();
  await provider(context, 'pending');
  try {
    const page = await context.newPage();
    await page.goto(server.url + '/messages');
    const main = page.getByRole('main');
    await main
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await main.getByRole('button', { name: 'Disconnect', exact: true }).click();
    await main
      .getByRole('button', { name: 'Connect extension', exact: true })
      .click();
    await expect(
      main.getByText(
        'An extension request is still pending. Wait for it to finish before trying again.',
        { exact: true }
      )
    ).toBeVisible();
    expect(await page.evaluate(() => window.hcp056Counts.keys)).toBe(1);
    await page.evaluate(() => window.hcp056Release?.());
    await expect(page.locator('summary')).toHaveCount(0);
    await expect(main.locator('input, textarea, form')).toHaveCount(0);
    expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
      keys: 1,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
    // The old genuine SDK job must actually settle before the same author
    // can connect again. The later navbar action owns focus on this same URL.
    const navbarConnect = page
      .getByRole('navigation', { name: 'Primary', exact: true })
      .getByRole('button', { name: 'Connect extension', exact: true });
    await expect
      .poll(async () => {
        await navbarConnect.click();
        return page.evaluate(() => window.hcp056Counts.keys);
      })
      .toBe(2);
    await page.evaluate(() => window.hcp056Release?.());
    await expect(page.getByText('Identity', { exact: true })).toBeVisible();
    await expect(page.locator('#identity-navbar-status')).toBeFocused();
    await expect(main.locator('#identity-protected-status')).not.toBeFocused();
  } finally {
    await context.close();
  }
});
test.describe('actual shared inline capability component', () => {
  let fixtureServer: Awaited<ReturnType<typeof createStaticHarness>>,
    directory: string;
  test.beforeAll(async () => {
    test.setTimeout(120_000);
    directory = await mkdtemp(path.join(tmpdir(), 'hcp056-gates-'));
    const capsule = fileURLToPath(new URL('../../../', import.meta.url)),
      checkout = path.join(directory, 'checkout');
    execFileSync('git', [
      'clone',
      '--quiet',
      '--no-hardlinks',
      capsule,
      checkout
    ]);
    for (const name of [
      'src/lib/components/CapabilityGate.svelte',
      'src/lib/runtime/view-context.ts',
      'src/lib/components/AppShell.svelte',
      'src/lib/components/AccountGate.svelte',
      'src/lib/components/InboxSetup.svelte',
      'src/lib/messaging/inbox-setup-view.ts',
      'src/routes/+layout.svelte',
      'tests/e2e/harness/capability-gate.svelte'
    ])
      await cp(
        path.join(capsule, 'web', name),
        path.join(checkout, 'web', name)
      );
    const fixture = await readFile(
      path.join(capsule, 'web/tests/e2e/harness/capability-gate.svelte'),
      'utf8'
    );
    await writeFile(
      path.join(checkout, 'web/src/routes/+page.svelte'),
      fixture.replaceAll('../../../src/lib/', '../lib/')
    );
    const store = execFileSync('corepack', ['pnpm', 'store', 'path'], {
      cwd: path.join(capsule, 'web')
    })
      .toString()
      .trim();
    execFileSync(
      'corepack',
      [
        'pnpm',
        'install',
        '--offline',
        '--frozen-lockfile',
        '--store-dir',
        path.dirname(store)
      ],
      { cwd: path.join(checkout, 'web'), timeout: 90_000, stdio: 'pipe' }
    );
    const audit = execFileSync(process.execPath, ['tools/check-source.mjs'], {
      cwd: path.join(checkout, 'web')
    }).toString();
    execFileSync(process.execPath, ['tools/build-info.mjs'], {
      cwd: path.join(checkout, 'web')
    });
    execFileSync(process.execPath, ['node_modules/vite/bin/vite.js', 'build'], {
      cwd: path.join(checkout, 'web'),
      timeout: 90_000,
      stdio: 'pipe'
    });
    console.log(
      JSON.stringify({
        fixture: 'HCP056_ACTUAL_SHARED_GATE',
        sourceSha256: createHash('sha256').update(fixture).digest('hex'),
        audit,
        qualification:
          'controlled Chromium and actual shared view/component/SDK; no named extension/inbox/private publication qualification'
      })
    );
    fixtureServer = await createStaticHarness({
      buildRoot: path.join(checkout, 'web/build')
    });
  });
  test.afterAll(async () => {
    await fixtureServer?.close();
    if (directory) await rm(directory, { recursive: true, force: true });
  });
  test('Connect never repeats an earlier inline Send, and self-copy requires a separate reviewed action', async ({
    browser
  }) => {
    const context = await browser.newContext();
    await provider(context);
    try {
      const page = await context.newPage();
      await page.goto(fixtureServer.url);
      const main = page.getByRole('main');
      await main.getByRole('button', { name: 'Explicit test Send' }).click();
      await main
        .getByRole('button', { name: 'Connect extension', exact: true })
        .click();
      await expect(
        main.getByText('Earlier sends: 1', { exact: true })
      ).toBeVisible();
      expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
        keys: 1,
        signs: 0,
        encrypts: 0,
        decrypts: 0
      });
      await expect(
        main.locator('[role="status"][tabindex="-1"]')
      ).toBeFocused();
      await main
        .getByRole('button', { name: 'Check messaging support', exact: true })
        .click();
      await expect(
        main.getByText('Messaging support checked.', { exact: true })
      ).toBeVisible();
      expect(await page.evaluate(() => window.hcp056Counts)).toEqual({
        keys: 3,
        signs: 0,
        encrypts: 1,
        decrypts: 1
      });
      await expect(
        main.getByText('Earlier sends: 1', { exact: true })
      ).toBeVisible();
    } finally {
      await context.close();
    }
  });
  test('reviewed probe waits for actual completion before returning focus', async ({
    browser
  }) => {
    const context = await browser.newContext();
    await provider(context);
    try {
      const page = await context.newPage();
      await page.goto(fixtureServer.url);
      const main = page.getByRole('main');
      await main
        .getByRole('button', { name: 'Connect extension', exact: true })
        .click();
      await page.evaluate(() => {
        window.nostr = {
          getPublicKey: () =>
            Promise.resolve(
              '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
            ),
          signEvent: () => Promise.reject(new Error('Unexpected signature')),
          nip44: {
            encrypt: (_key: string, text: string) =>
              new Promise<string>((resolve) => {
                window.hcp056Release = () => resolve(text);
              }),
            decrypt: (_key: string, text: string) => Promise.resolve(text)
          }
        };
      });
      await main
        .getByRole('button', { name: 'Check messaging support', exact: true })
        .click();
      await expect(
        main.getByText('Waiting for extension approval.', { exact: true })
      ).toBeVisible();
      const retained = main.getByRole('button', { name: 'Explicit test Send' });
      await retained.focus();
      await expect(retained).toBeFocused();
      await expect(main.locator('#identity-inline-status')).not.toBeFocused();
      await page.evaluate(() => window.hcp056Release?.());
      await expect(
        main.getByText('Messaging support checked.', { exact: true })
      ).toBeVisible();
      await expect(main.locator('#identity-inline-status')).toBeFocused();
    } finally {
      await context.close();
    }
  });
  test('connected signing-only identity has an explicit unsupported message-encryption state', async ({
    browser
  }) => {
    const context = await browser.newContext();
    await provider(context, 'unsupported');
    try {
      const page = await context.newPage();
      await page.goto(fixtureServer.url);
      const main = page.getByRole('main');
      await main
        .getByRole('button', { name: 'Connect extension', exact: true })
        .click();
      await main
        .getByRole('button', { name: 'Check messaging support', exact: true })
        .click();
      await expect(
        main.getByText(
          'This extension does not support the required message encryption.',
          { exact: true }
        )
      ).toBeVisible();
      await expect(main.locator('textarea, form')).toHaveCount(0);
      expect(
        await page.evaluate(
          () => window.hcp056Counts.encrypts + window.hcp056Counts.signs
        )
      ).toBe(0);
    } finally {
      await context.close();
    }
  });
});
