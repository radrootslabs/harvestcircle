import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/listing-continuity.ts';
import type { PublicDraftRecord } from '../../src/lib/contracts/local-records.ts';
declare global {
  interface Window {
    hcp116: typeof Fixture;
    hcp116Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp116Observation: ReturnType<typeof Fixture.nativeObservation>;
    hcp116Abort: ReturnType<typeof Fixture.abortDraftWrites>;
    hcp116Unknown: ReturnType<typeof Fixture.unknownDraftAcknowledgment>;
    hcp116Input: HTMLInputElement;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
const form = {
  title: 'Exact incomplete draft',
  description: '  Raw description\n',
  location: '',
  amount: '2,50',
  currency: '',
  unit: 'kg',
  quantity: '03',
  contactType: '',
  contactValue: ''
} as const;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const built = await build({
    configFile: false,
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/listing-continuity.ts', import.meta.url)
        ),
        name: 'hcp116',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('actual bundle missing');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  await server?.close();
});
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.WebSocket = class extends WebSocket {
      constructor() {
        super(
          (() => {
            throw Error('HCP116 source tests authorize no relay traffic');
          })()
        );
      }
    };
  });
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp116Fixture = await window.hcp116.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    window.hcp116Observation?.restore();
    window.hcp116Abort?.restore();
    window.hcp116Unknown?.release();
    await window.hcp116Fixture?.close();
  });
});
async function navigate(page: Page, pathname: string) {
  await page.evaluate((pathname) => {
    const a = document.createElement('a');
    a.id = 'hcp116-route';
    a.href = pathname;
    a.textContent = 'HCP116 controlled local route';
    document.body.append(a);
  }, pathname);
  await page.locator('#hcp116-route').click();
  await expect(page).toHaveURL(server.url + pathname);
  await page.evaluate(() => document.querySelector('#hcp116-route')?.remove());
}
async function newRoute(page: Page) {
  await navigate(page, '/sell');
  await page
    .getByRole('button', { name: 'Connect extension', exact: true })
    .last()
    .click();
  await expect(page.locator('main form')).toBeVisible();
}
async function drafts(page: Page) {
  const rows = await page.evaluate(
    async () =>
      (await window.hcp116.inspectNative()).drafts as { wire: string }[]
  );
  return rows.map((row) => JSON.parse(row.wire) as PublicDraftRecord);
}
test('HCP116 genuine first acknowledged save replaces entry URL with same original DOM focus and one durable draft', async ({
  page
}) => {
  await newRoute(page);
  const field = page.getByLabel('Product name', { exact: true });
  await page.evaluate(() => {
    window.hcp116Input = document.querySelector(
      '#listing-title'
    ) as HTMLInputElement;
  });
  await field.fill(form.title);
  await expect(page).toHaveURL(/\/selling\/drafts\/[0-9a-f-]{36}$/);
  await expect(field).toBeFocused();
  expect(
    await page.evaluate(
      () => window.hcp116Input === document.querySelector('#listing-title')
    )
  ).toBe(true);
  const rows = await drafts(page);
  expect(rows).toHaveLength(1);
  expect(new URL(page.url()).pathname).toBe('/selling/drafts/' + rows[0].id);
  expect(rows[0].form.title).toBe(form.title);
  await field.fill('Later exact input');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const later = await drafts(page);
  expect(later).toHaveLength(1);
  expect(later[0].id).toBe(rows[0].id);
  expect(later[0].form.title).toBe('Later exact input');
});
test('HCP116 held first native acknowledgment preserves latest edits focus and original id without another draft', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Observation = window.hcp116.nativeObservation();
    window.hcp116Observation.holdSave();
    window.hcp116Input = document.querySelector(
      '#listing-title'
    ) as HTMLInputElement;
  });
  const title = page.getByLabel('Product name', { exact: true });
  await title.fill('First captured input');
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp116Observation.state().held)
    )
    .toBe(true);
  await title.fill('Latest input while original acknowledgment waits');
  await expect(page).toHaveURL(server.url + '/sell');
  await page.evaluate(() => window.hcp116Observation.release());
  await expect(page).toHaveURL(/\/selling\/drafts\/[0-9a-f-]{36}$/);
  await expect(title).toHaveValue(
    'Latest input while original acknowledgment waits'
  );
  await expect(title).toBeFocused();
  expect(
    await page.evaluate(
      () => window.hcp116Input === document.querySelector('#listing-title')
    )
  ).toBe(true);
  const rows = await drafts(page);
  expect(rows).toHaveLength(1);
  expect(rows[0].form.title).toBe(
    'Latest input while original acknowledgment waits'
  );
});
test('HCP116 durable draft URL reload and actual Search Back resume exact original raw values', async ({
  page
}) => {
  await newRoute(page);
  await page.getByLabel('Product name', { exact: true }).fill(form.title);
  await expect(page).toHaveURL(/\/selling\/drafts\/[0-9a-f-]{36}$/);
  await page.getByLabel('Price', { exact: true }).fill(form.amount);
  await page.getByLabel('Per', { exact: true }).selectOption('kg');
  await page
    .getByLabel('Advertised quantity (optional)', { exact: true })
    .fill('03');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const target = page.url(),
    before = await drafts(page);
  await page.getByRole('link', { name: 'Search', exact: true }).click();
  await expect(page.getByLabel('What are you looking for?')).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(target);
  await expect(page.getByLabel('Price', { exact: true })).toHaveValue('2,50');
  await expect(
    page.getByLabel('Advertised quantity (optional)', { exact: true })
  ).toHaveValue('03');
  expect(await drafts(page)).toEqual(before);
  // TEST_ONLY same observed author after real reload; this controlled provider
  // offers no signature, relay or extension qualification.
  await page.addInitScript(
    (owner) =>
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        value: {
          getPublicKey: () => Promise.resolve(owner),
          signEvent: () =>
            Promise.reject(
              Error('HCP116 reload asserts no signature capability')
            )
        }
      }),
    before[0].owner
  );
  await page.evaluate(async () => await window.hcp116Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page
    .getByRole('button', { name: 'Connect extension', exact: true })
    .last()
    .click();
  await expect(page).toHaveURL(target);
  await expect(page.getByLabel('Price', { exact: true })).toHaveValue('2,50');
  await expect(
    page.getByLabel('Advertised quantity (optional)', { exact: true })
  ).toHaveValue('03');
  expect(await drafts(page)).toEqual(before);
});
test('HCP116 Save and close waits for genuine original acknowledgment and flushes latest input before Selling', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Observation = window.hcp116.nativeObservation();
    window.hcp116Observation.holdSave();
  });
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Initial close input');
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp116Observation.state().held)
    )
    .toBe(true);
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await expect(page).toHaveURL(server.url + '/sell');
  await page
    .getByLabel('Description', { exact: true })
    .fill('Latest exact close description\n');
  await page.evaluate(() => window.hcp116Observation.release());
  await expect(page).toHaveURL(server.url + '/selling');
  const rows = await drafts(page);
  expect(rows).toHaveLength(1);
  expect(rows[0].form.description).toBe('Latest exact close description\n');
});
test('HCP116 genuine native abort keeps current form and route until explicit Save and close succeeds', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Abort = window.hcp116.abortDraftWrites();
  });
  await page.getByLabel('Product name', { exact: true }).fill(form.title);
  await expect(
    page.getByText('Not saved. Keep your current input here.', { exact: true })
  ).toBeVisible();
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await expect(
    page.getByText('Not saved. Keep your current input here.', { exact: true })
  ).toBeVisible();
  await expect(page).toHaveURL(server.url + '/sell');
  await expect(page.getByLabel('Product name', { exact: true })).toHaveValue(
    form.title
  );
  expect(await drafts(page)).toHaveLength(0);
  const attempted = await page.evaluate(() => window.hcp116Abort.state());
  expect(attempted.aborts).toBeGreaterThan(0);
  expect(new Set(attempted.ids).size).toBe(1);
  await page.evaluate(() => window.hcp116Abort.restore());
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await expect(page).toHaveURL(server.url + '/selling');
  const rows = await drafts(page);
  expect(rows).toHaveLength(1);
  expect(rows[0].id).toBe(attempted.ids[0]);
  expect(rows[0].form.title).toBe(form.title);
});
test('HCP116 actual lost put and readback acknowledgments preserve original attempt then reconcile before latest save-close', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Unknown = window.hcp116.unknownDraftAcknowledgment();
  });
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Original uncertain input');
  await expect(
    page.getByText('Not saved. Keep your current input here.', { exact: true })
  ).toBeVisible();
  await expect(page).toHaveURL(server.url + '/sell');
  const before = await drafts(page);
  expect(before).toHaveLength(1);
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Latest input after unknown receipt');
  await expect(page.getByLabel('Product name', { exact: true })).toHaveValue(
    'Latest input after unknown receipt'
  );
  const actual = await page.evaluate(() => window.hcp116Unknown.state());
  expect(actual.puts).toBe(1);
  expect(actual.held).toBeGreaterThanOrEqual(2);
  await page.evaluate(() => window.hcp116Unknown.release());
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await expect(page).toHaveURL(server.url + '/selling');
  const after = await drafts(page);
  expect(after).toHaveLength(1);
  expect(after[0].id).toBe(before[0].id);
  expect(after[0].form.title).toBe('Latest input after unknown receipt');
});
test('HCP116 copied save-close cannot close original saved controller or create another draft', async ({
  page
}) => {
  const actual = await page.evaluate(async (form) => {
    const f = window.hcp116Fixture;
    await f.open({ mode: 'new' });
    if (!(await f.edit(form))) throw Error('actual draft failed');
    const before = await f.inspect(),
      copied = await f.saveClose(true),
      originalAlive = !!f.snapshot(),
      original = await f.saveClose();
    return {
      before,
      copied,
      originalAlive,
      original,
      snapshot: f.snapshot(),
      after: await f.inspect()
    };
  }, form);
  expect(actual).toMatchObject({
    copied: false,
    originalAlive: true,
    original: true
  });
  expect(actual.snapshot).toBeUndefined();
  expect(actual.after).toEqual(actual.before);
});
test('HCP116 disconnected original editor cannot save-close through stale original ownership', async ({
  page
}) => {
  const actual = await page.evaluate(async (form) => {
    const f = window.hcp116Fixture;
    await f.open({ mode: 'new' });
    await f.edit(form);
    const before = await f.inspect();
    f.disconnect();
    return {
      closed: await f.saveClose(),
      before,
      after: await f.inspect(),
      snapshot: f.snapshot()
    };
  }, form);
  expect(actual.closed).toBe(false);
  expect(actual.snapshot).toBeUndefined();
  expect(actual.after).toEqual(actual.before);
});
test('HCP116 logout while original save-close waits prevents late Selling navigation and leaves genuine local fact', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Observation = window.hcp116.nativeObservation();
    window.hcp116Observation.holdSave();
  });
  await page.getByLabel('Product name', { exact: true }).fill(form.title);
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp116Observation.state().held)
    )
    .toBe(true);
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await page.getByText('Identity', { exact: true }).click();
  await page
    .getByRole('button', { name: 'Disconnect', exact: true })
    .first()
    .click();
  await page.evaluate(() => window.hcp116Observation.release());
  await expect(page.locator('main form')).toHaveCount(0);
  await expect(page).toHaveURL(server.url + '/sell');
  expect(await drafts(page)).toHaveLength(1);
});
test('HCP116 missing draft stays unavailable with safe Selling link and no empty substitute', async ({
  page
}) => {
  await navigate(page, '/selling/drafts/' + crypto.randomUUID());
  await page
    .getByRole('button', { name: 'Connect extension', exact: true })
    .last()
    .click();
  await expect(
    page.getByText('This draft is not available here.', { exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Back to Selling', exact: true })
  ).toHaveAttribute('href', '/selling');
  await expect(page.locator('main form')).toHaveCount(0);
  await expect(
    page.getByRole('button', { name: 'Save and close', exact: true })
  ).toHaveCount(0);
  expect(await drafts(page)).toHaveLength(0);
});
test('HCP116 leaving route during original save-close prevents late Selling navigation', async ({
  page
}) => {
  await newRoute(page);
  await page.evaluate(() => {
    window.hcp116Observation = window.hcp116.nativeObservation();
    window.hcp116Observation.holdSave();
  });
  await page.getByLabel('Product name', { exact: true }).fill(form.title);
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp116Observation.state().held)
    )
    .toBe(true);
  await page
    .getByRole('button', { name: 'Save and close', exact: true })
    .click();
  await navigate(page, '/search');
  await page.evaluate(() => window.hcp116Observation.release());
  await expect(page).toHaveURL(server.url + '/search');
  await expect(page.locator('#listing-title')).toHaveCount(0);
  expect(await drafts(page)).toHaveLength(1);
});
test('HCP116 overlapping original save-close calls acknowledge latest input only once', async ({
  page
}) => {
  await page.evaluate(async (form) => {
    const f = window.hcp116Fixture;
    await f.open({ mode: 'new' });
    window.hcp116Observation = window.hcp116.nativeObservation();
    window.hcp116Observation.holdSave();
    void f.edit(form);
  }, form);
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp116Observation.state().held)
    )
    .toBe(true);
  const actual = await page.evaluate(async (form) => {
    const f = window.hcp116Fixture,
      first = f.saveClose(),
      second = f.saveClose();
    void f.edit({ ...form, title: 'Latest overlapping input' });
    window.hcp116Observation.release();
    return {
      results: await Promise.all([first, second]),
      rows: await f.inspect(),
      snapshot: f.snapshot()
    };
  }, form);
  expect(actual.results.filter(Boolean)).toHaveLength(1);
  expect(actual.snapshot).toBeUndefined();
  expect(actual.rows.drafts).toHaveLength(1);
  expect(
    (
      JSON.parse(
        (actual.rows.drafts[0] as { wire: string }).wire
      ) as PublicDraftRecord
    ).form.title
  ).toBe('Latest overlapping input');
});
for (const unknown of [false, true])
  test(`HCP116 genuine ${unknown ? 'unknown signing' : 'signed retained'} operation state prevents an apparently fresh draft submission`, async ({
    page
  }) => {
    const original = await page.evaluate(
      async (unknown) => await window.hcp116Fixture.captured(unknown),
      unknown
    );
    const before = await page.evaluate(
      async () => await window.hcp116Fixture.inspect()
    );
    await navigate(page, '/selling/drafts/' + original.draftId);
    await page
      .getByRole('button', { name: 'Connect extension', exact: true })
      .last()
      .click();
    await expect(page.locator('main form')).toBeVisible();
    await expect(
      page.getByLabel('Product name', { exact: true })
    ).toHaveAttribute('readonly', '');
    await expect(
      page.getByText(
        unknown
          ? 'The outcome is unknown. Keep the original listing operation for reconciliation.'
          : 'Signed listing saved in this browser. No relay acceptance is recorded.',
        { exact: true }
      )
    ).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Save and close', exact: true })
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Review listing', exact: true })
    ).toHaveCount(0);
    expect(
      await page.evaluate(async () => await window.hcp116Fixture.inspect())
    ).toEqual(before);
  });

test('HCP116 foreign genuine draft exposes only unavailable state and safe Selling link, never resets data', async ({
  page
}) => {
  const original = await page.evaluate(async () => {
    const captured = await window.hcp116Fixture.captured();
    const rows = await window.hcp116Fixture.inspect();
    await window.hcp116Fixture.close();
    window.hcp116Fixture = await window.hcp116.createFixture();
    return { captured, rows };
  });
  await navigate(page, '/selling/drafts/' + original.captured.draftId);
  await page
    .getByRole('button', { name: 'Connect extension', exact: true })
    .last()
    .click();
  await expect(
    page.getByText('This draft is not available here.', { exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Back to Selling', exact: true })
  ).toHaveAttribute('href', '/selling');
  await expect(page.locator('main form')).toHaveCount(0);
  expect(
    await page.evaluate(async () => await window.hcp116Fixture.inspect())
  ).toEqual(original.rows);
});
