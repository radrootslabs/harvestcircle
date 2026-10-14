import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/local-data-review.ts';
declare global {
  interface Window {
    hcp125: typeof Fixture;
    hcp125Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}

test('HCP125 first-level SSR privacy discloses public encrypted local and remote limits to guests', async ({
  page
}) => {
  const server = await createStaticHarness();
  try {
    await page.addInitScript(() => {
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        get() {
          throw Error('guest extension effect');
        }
      });
    });
    await page.goto(server.url + '/privacy');
    await expect(
      page.getByRole('heading', { name: 'Privacy', exact: true })
    ).toBeVisible();
    expect(await page.getByRole('heading').allTextContents()).toEqual(
      expect.arrayContaining([
        'Public listings',
        'Private messages',
        'In this browser',
        'Removing information'
      ])
    );
    await expect(
      page.getByText('Unsent message text is not automatically saved.', {
        exact: false
      })
    ).toBeVisible();
    await expect(
      page.getByText('does not remove other people', { exact: false })
    ).toBeVisible();
    await expect(
      page.getByRole('button', {
        name: 'Review data in this browser',
        exact: true
      })
    ).toBeVisible();
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await expect(page.locator('[data-local-data-owner]')).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Confirm removal', exact: true })
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: /clear everything/i })
    ).toHaveCount(0);
  } finally {
    await server.close();
  }
});

test.describe('HCP125 original local data workflow and native storage', () => {
  let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
  test.beforeAll(async () => {
    server = await createStaticHarness();
    const entry = fileURLToPath(
      new URL('./harness/local-data-review.ts', import.meta.url)
    );
    const result = await build({
      configFile: false,
      logLevel: 'silent',
      plugins: [svelte()],
      build: {
        write: false,
        minify: false,
        rolldownOptions: { input: entry },
        lib: { entry, name: 'hcp125', formats: ['iife'] }
      }
    });
    const output = Array.isArray(result) ? result[0] : result;
    if (!('output' in output)) throw Error('actual fixture output missing');
    const chunks = output.output.filter((item) => item.type === 'chunk');
    expect(chunks).toHaveLength(1);
    bundle = chunks[0].code;
  });
  test.afterAll(async () => server?.close());
  async function load(page: Page, mode = 'normal') {
    await page.goto(server.url + '/about');
    await page.addScriptTag({ content: bundle });
    await page.evaluate(async (mode) => {
      window.hcp125Fixture = await window.hcp125.createFixture(mode);
    }, mode);
  }
  test.afterEach(async ({ page }) => {
    if (!page.isClosed())
      await page.evaluate(async () => window.hcp125Fixture?.close());
  });
  test('explicit review is owner scoped, starts with no selection, shows protected work and does not leak draft bodies', async ({
    page
  }) => {
    await load(page);
    expect(await page.evaluate(() => window.hcp125Fixture.effects())).toEqual({
      reads: 0,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await expect(page.locator('[data-local-data-owner]')).toHaveText(
      await page.evaluate(() => window.hcp125Fixture.owner)
    );
    await expect(
      page.getByText('Pending or uncertain work is protected.', {
        exact: false
      })
    ).toBeVisible();
    await expect(page.locator('#hcp125-panel input:checked')).toHaveCount(0);
    await expect(page.locator('#hcp125-panel')).not.toContainText(
      'PRIVATE OWNER B SENTINEL'
    );
    await expect(page.locator('#hcp125-panel')).not.toContainText(
      'PUBLIC DRAFT BODY SENTINEL'
    );
    expect(await page.evaluate(() => window.hcp125Fixture.effects())).toEqual({
      reads: 0,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
  });
  test('selection preview is reversible; confirmation removes only the selected public draft', async ({
    page
  }) => {
    await load(page);
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await page
      .getByRole('checkbox', { name: /Listing draft/ })
      .first()
      .check();
    await page
      .getByRole('button', { name: 'Review public removal', exact: true })
      .click();
    await expect(
      page.getByText('does not delete remote copies', { exact: false })
    ).toBeVisible();
    await page.getByRole('button', { name: 'Keep data', exact: true }).click();
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
    await page
      .getByRole('checkbox', { name: /Listing draft/ })
      .first()
      .check();
    await page
      .getByRole('button', { name: 'Review public removal', exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Confirm removal', exact: true })
      .click();
    await expect(page.getByRole('status').last()).toContainText(
      'Removed 1 selected public record'
    );
    const rows = await page.evaluate(() => window.hcp125Fixture.rows());
    expect(rows.publicDrafts).toBe(1);
    expect(rows.foreignDrafts).toBe(1);
    expect(rows.publicOperations).toBe(1);
  });
  test('changed inventory refuses selected removal without resetting local data', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreview());
    await page.evaluate(() => window.hcp125Fixture.changeDraft());
    expect(await page.evaluate(() => window.hcp125Fixture.confirm())).toBe(
      false
    );
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
    expect(
      await page.evaluate(() => window.hcp125Fixture.snapshot())
    ).toMatchObject({ notice: expect.stringContaining('not confirmed') });
  });
  test('logout retires inventory and preview and prevents any subsequent deletion', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreview());
    await page.evaluate(() => window.hcp125Fixture.logout());
    expect(await page.evaluate(() => window.hcp125Fixture.snapshot())).toEqual({
      state: 'guest'
    });
    expect(await page.evaluate(() => window.hcp125Fixture.confirm())).toBe(
      false
    );
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
    await expect(page.locator('[data-local-data-owner]')).toHaveCount(0);
  });
  test('owner invalidation after a real delete request aborts the native transaction and preserves the row', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreview());
    expect(
      await page.evaluate(() => window.hcp125Fixture.abortDuringDelete())
    ).toMatchObject({
      confirmed: false,
      deleteSuccesses: 1,
      nativeAborts: 1,
      nativeTerminal: 'abort'
    });
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
  });
  test('busy real owner Web Lock refuses removal without queueing or deleting', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreview());
    expect(
      await page.evaluate(() => window.hcp125Fixture.confirmWhileLocked())
    ).toBe(false);
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
  });
  test('corrupt records refuse inventory without reset and preserve the raw sentinel', async ({
    page
  }) => {
    await load(page, 'corrupt');
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await expect(page.getByRole('status').last()).toContainText(
      'Existing local data has not been reset'
    );
    expect(await page.evaluate(() => window.hcp125Fixture.rawCorrupt())).toBe(
      '{"schema":99}'
    );
    await expect(
      page.getByRole('button', { name: 'Confirm removal', exact: true })
    ).toHaveCount(0);
  });
  test('disposal retires original selection and rejects confirmation', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreview());
    await page.evaluate(() => window.hcp125Fixture.dispose());
    expect(await page.evaluate(() => window.hcp125Fixture.snapshot())).toEqual({
      state: 'closed'
    });
    expect(await page.evaluate(() => window.hcp125Fixture.confirm())).toBe(
      false
    );
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
  });
  test('malformed duplicate protected or cross-account selections cannot mint a cleanup preview', async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => window.hcp125Fixture.review());
    expect(
      await page.evaluate(() => window.hcp125Fixture.hostileSelections())
    ).toEqual([false, false, false, false, false]);
    expect(
      (await page.evaluate(() => window.hcp125Fixture.rows())).publicDrafts
    ).toBe(2);
  });
  test('selected received encrypted copy removal preserves outbox other copies and other accounts without decrypting', async ({
    page
  }) => {
    await load(page, 'messaging');
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await page
      .getByRole('checkbox', { name: /Encrypted received copy/ })
      .first()
      .check();
    await page
      .getByRole('button', { name: 'Review received removal', exact: true })
      .click();
    await expect(
      page.getByText('Relay recovery is not guaranteed.', { exact: false })
    ).toBeVisible();
    await page
      .getByRole('button', { name: 'Confirm removal', exact: true })
      .click();
    await expect(page.getByRole('status').last()).toContainText(
      'Removed 1 selected received encrypted copy'
    );
    expect(
      await page.evaluate(() => window.hcp125Fixture.rows())
    ).toMatchObject({
      received: 1,
      foreignReceived: 1,
      outbox: 1,
      publicDrafts: 2
    });
    expect(
      await page.evaluate(() => window.hcp125Fixture.effects())
    ).toMatchObject({ signs: 0, encrypts: 0, decrypts: 0 });
  });
  test('signing-only metadata review does not probe messaging or enable received removal', async ({
    page
  }) => {
    await load(page);
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await expect(
      page.getByText(
        'Enable messaging explicitly before reviewing received-copy removal.',
        { exact: false }
      )
    ).toBeVisible();
    await expect(
      page.getByRole('checkbox', { name: /Encrypted received copy/ }).first()
    ).toBeDisabled();
    expect(await page.evaluate(() => window.hcp125Fixture.effects())).toEqual({
      reads: 0,
      signs: 0,
      encrypts: 0,
      decrypts: 0
    });
  });
  test('stale selected received full wire refuses deletion and preserves the changed copy', async ({
    page
  }) => {
    await load(page, 'messaging');
    await page.evaluate(() => window.hcp125Fixture.reviewAndPreviewReceived());
    await page.evaluate(() => window.hcp125Fixture.changeReceived());
    expect(await page.evaluate(() => window.hcp125Fixture.confirm())).toBe(
      false
    );
    expect(
      await page.evaluate(() => window.hcp125Fixture.rows())
    ).toMatchObject({ received: 2, foreignReceived: 1, outbox: 1 });
  });
  test('held old confirmation cannot retire a new original-generation received preview after reconnect', async ({
    page
  }) => {
    await load(page, 'messaging');
    const result = await page.evaluate(() =>
      window.hcp125Fixture.oldConfirmationThenNewPreview()
    );
    expect(result).toMatchObject({
      held: true,
      newer: true,
      oldConfirmed: false,
      newConfirmed: true,
      currentPreview: {
        state: 'ready',
        preview: { category: 'received', selected: 1 }
      }
    });
    expect(
      await page.evaluate(() => window.hcp125Fixture.rows())
    ).toMatchObject({
      received: 0,
      foreignReceived: 1,
      outbox: 1,
      publicDrafts: 2
    });
  });
  for (const width of [320, 1024])
    test('local review fits viewport ' + width, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await load(page);
      await page
        .getByRole('button', {
          name: 'Review data in this browser',
          exact: true
        })
        .click();
      await expect(page.locator('[data-local-data-owner]')).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth
        )
      ).toBe(true);
    });

  test('CR004 reviewed settled removal frees actual capacity while preserving other namespaces', async ({
    page
  }) => {
    await load(page, 'outbox');
    const before = await page.evaluate(() => window.hcp125Fixture.rows());
    expect(before).toMatchObject({
      outbox: 100,
      publicDrafts: 2,
      publicOperations: 1,
      received: 2,
      foreignReceived: 1
    });
    expect(await page.evaluate(() => window.hcp125Fixture.reserveNext())).toBe(
      'capacity'
    );
    await page
      .getByRole('button', { name: 'Review data in this browser', exact: true })
      .click();
    await expect(
      page.getByRole('checkbox', { name: /Eligible settled send/ })
    ).toHaveCount(100);
    await page
      .getByRole('checkbox', { name: /Eligible settled send/ })
      .first()
      .check();
    await page
      .getByRole('button', { name: 'Review settled send removal', exact: true })
      .click();
    await expect(
      page.getByText('duplicate-prevention evidence', { exact: false })
    ).toBeVisible();
    await page
      .getByRole('button', { name: 'Confirm removal', exact: true })
      .click();
    await expect(page.getByRole('status').last()).toContainText(
      'Removed 1 selected settled send record'
    );
    const removed = await page.evaluate(() => window.hcp125Fixture.rows());
    expect(removed).toEqual({ ...before, outbox: 99 });
    expect(
      await page.evaluate(() => window.hcp125Fixture.effects())
    ).toMatchObject({ signs: 0, encrypts: 0, decrypts: 0 });
    const fresh = await page.evaluate(() => window.hcp125Fixture.freshSend());
    expect(fresh).toMatchObject({
      reservation: 'reserved',
      self: 'saved',
      pair: 'prepared',
      paired: true
    });
    expect(
      await page.evaluate(() => window.hcp125Fixture.settledIds())
    ).not.toContain(fresh.id);
    expect(
      await page.evaluate(() => window.hcp125Fixture.effects())
    ).toMatchObject({ signs: 2, encrypts: 2 });
    expect(await page.evaluate(() => window.hcp125Fixture.rows())).toEqual(
      before
    );
    expect(await page.evaluate(() => window.hcp125Fixture.reserveNext())).toBe(
      'capacity'
    );
  });

  test('CR004 a second same-origin tab holding the actual owner lock prevents cleanup', async ({
    page,
    context
  }) => {
    await load(page, 'outbox');
    expect(
      await page.evaluate(() => window.hcp125Fixture.reviewAndPreviewOutbox())
    ).toBe(true);
    const owner = await page.evaluate(() => window.hcp125Fixture.owner),
      other = await context.newPage();
    try {
      await other.goto(server.url + '/about');
      await other.evaluate(async (owner) => {
        const state = window as typeof window & {
          cr004Release?: () => void;
          cr004Held?: Promise<void>;
        };
        let entered = () => {};
        const ready = new Promise<void>((resolve) => {
          entered = resolve;
        });
        state.cr004Held = navigator.locks.request(
          'harvestcircle:owner:' + owner,
          async () => {
            entered();
            await new Promise<void>((resolve) => {
              state.cr004Release = resolve;
            });
          }
        );
        await ready;
      }, owner);
      expect(await page.evaluate(() => window.hcp125Fixture.confirm())).toBe(
        false
      );
      expect(
        (await page.evaluate(() => window.hcp125Fixture.rows())).outbox
      ).toBe(100);
      expect(
        await page.evaluate(() => window.hcp125Fixture.effects())
      ).toMatchObject({ signs: 0, encrypts: 0, decrypts: 0 });
    } finally {
      if (!other.isClosed()) {
        await other.evaluate(async () => {
          const state = window as typeof window & {
            cr004Release?: () => void;
            cr004Held?: Promise<void>;
          };
          state.cr004Release?.();
          await state.cr004Held;
        });
        await other.close();
      }
    }
  });

  for (const outcome of [
    'changed',
    'lock_busy',
    'sdk_busy',
    'key_changed',
    'aborted',
    'unknown'
  ] as const)
    test(
      'CR004 actual ' +
        outcome +
        ' cannot certify removal of the reviewed original',
      async ({ page }) => {
        await load(page, 'outbox');
        expect(
          await page.evaluate(() =>
            window.hcp125Fixture.reviewAndPreviewOutbox()
          )
        ).toBe(true);
        const result = await page.evaluate(async (outcome) => {
          const f = window.hcp125Fixture;
          if (outcome === 'changed') await f.changeOutbox();
          if (outcome === 'key_changed') f.switchKey();
          const fault =
            outcome === 'aborted' || outcome === 'unknown'
              ? await f.abortOutboxDelete(outcome === 'unknown')
              : undefined;
          const confirmed =
            fault?.confirmed ??
            (outcome === 'lock_busy'
              ? await f.confirmWhileLocked()
              : outcome === 'sdk_busy'
                ? await f.confirmWhileSdkBusy()
                : await f.confirm());
          return {
            confirmed,
            fault,
            rows: await f.rows(),
            effects: f.effects()
          };
        }, outcome);
        expect(result.confirmed).toBe(false);
        expect(result.rows.outbox).toBe(outcome === 'unknown' ? 99 : 100);
        expect(result.rows).toMatchObject({
          publicDrafts: 2,
          publicOperations: 1,
          received: 2,
          foreignReceived: 1
        });
        expect(result.effects).toMatchObject({
          signs: 0,
          encrypts: 0,
          decrypts: 0
        });
        if (outcome === 'aborted')
          expect(result.fault).toMatchObject({
            deletes: 1,
            aborts: 1,
            nativeTerminal: 'abort'
          });
        if (outcome === 'unknown')
          expect(result.fault).toMatchObject({
            aborts: 0,
            nativeTerminal: 'complete'
          });
      }
    );
});
