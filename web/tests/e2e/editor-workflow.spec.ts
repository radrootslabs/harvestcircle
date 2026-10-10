import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/editor-workflow.ts';
import type { PublicDraftForm } from '../../src/lib/contracts/local-records.ts';
declare global {
  interface Window {
    hcp117: typeof Fixture;
    hcp117Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
const form: PublicDraftForm = {
  title: 'Exact 菜 listing',
  description:
    '  ' + 'Complete public description 菜\r\n'.repeat(40) + ' final line  ',
  location: 'Town',
  amount: '2.5',
  currency: 'GBP',
  unit: 'kg',
  quantity: '3',
  contactType: 'email',
  contactValue: 'public@example.org'
};
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
          new URL('./harness/editor-workflow.ts', import.meta.url)
        ),
        name: 'hcp117',
        formats: ['iife']
      }
    }
  });
  const result = Array.isArray(built) ? built[0] : built;
  if (!('output' in result)) throw Error('actual review bundle missing');
  const chunks = result.output.filter((row) => row.type === 'chunk');
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
            throw Error('HCP117 no live relay permission');
          })()
        );
      }
    };
  });
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    await window.hcp117Fixture?.close();
  });
});
test('HCP117 exact immutable full outgoing template and destinations review does not publish', async ({
  page
}) => {
  const result = await page.evaluate(async (form) => {
    const f = (window.hcp117Fixture = await window.hcp117.createFixture());
    await f.open();
    await f.edit(form);
    const before = f.counts(),
      a = await f.review(form),
      b = await f.review(form);
    return {
      a,
      b,
      before,
      after: f.counts(),
      rows: await f.inspect(),
      copied: f.snapshot(true)
    };
  }, form);
  expect(result.a.status).toBe('review');
  expect(result.b).toEqual(result.a);
  if (result.a.status !== 'review' || !result.a.snapshot)
    throw Error('genuine exact preview missing');
  const wire = JSON.parse(result.a.snapshot.templateWire) as {
    content: string;
    tags: string[][];
    pubkey: string;
  };
  expect(wire.content).toBe(
    form.description.replaceAll('\r\n', '\n').trim() +
      '\n\nPublic contact: mailto:public@example.org'
  );
  expect(result.a.snapshot.content).toBe(wire.content);
  expect(result.a.snapshot.summary.length).toBeLessThanOrEqual(240);
  expect(result.a.snapshot.content.length).toBeGreaterThan(1000);
  expect(result.a.snapshot.owner).toBe(wire.pubkey);
  expect(result.a.snapshot.targets).toEqual(['wss://discovery.example.org']);
  expect(wire.tags.find((tag) => tag[0] === 'summary')?.[1]).toBe(
    result.a.snapshot.summary
  );
  expect(result.rows.operations).toHaveLength(0);
  expect(result.rows.drafts).toHaveLength(1);
  expect(result.after.signs).toBe(result.before.signs);
  expect(result.copied).toBeUndefined();
});
test('HCP117 copy stale form changed draft and revoked readiness never retain review custody', async ({
  page
}) => {
  const result = await page.evaluate(async (form) => {
    const f = (window.hcp117Fixture = await window.hcp117.createFixture());
    await f.open();
    await f.edit(form);
    const copied = await f.review(form, true),
      stale = await f.review({ ...form, title: 'not saved' });
    const original = await f.review(form);
    await f.edit({ ...form, title: 'new saved values' });
    const afterEdit = f.snapshot();
    const next = await f.review({ ...form, title: 'new saved values' });
    f.stop();
    return {
      copied,
      stale,
      original,
      afterEdit,
      next,
      afterStop: f.snapshot(),
      rows: await f.inspect()
    };
  }, form);
  expect(result.copied.status).toBe('blocked');
  expect(result.stale.status).toBe('blocked');
  expect(result.original.status).toBe('review');
  expect(result.next.status).toBe('review');
  expect(result.afterEdit).toBeUndefined();
  expect(result.afterStop).toBeUndefined();
  expect(result.rows.operations).toHaveLength(0);
});
test('HCP117 unqualified inbox blocks disclosure while saving the complete draft remains available', async ({
  page
}) => {
  const result = await page.evaluate(async (form) => {
    const f = (window.hcp117Fixture = await window.hcp117.createFixture(false));
    await f.open();
    const saved = await f.edit(form),
      reviewed = await f.review(form);
    return {
      saved,
      reviewed,
      rows: await f.inspect(),
      signs: f.counts().signs
    };
  }, form);
  expect(result.saved).toBe(true);
  expect(result.reviewed.status).toBe('blocked');
  expect(result.rows.drafts).toHaveLength(1);
  expect(result.rows.operations).toHaveLength(0);
  expect(result.signs).toBe(0);
});
for (const width of [320, 1024]) {
  test(`HCP117 ${width}px same-route full review and Edit details preserve exact raw values and focus`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(async () => {
      window.hcp117Fixture = await window.hcp117.createFixture();
      window.hcp117Fixture.render();
    });
    await expect(page.locator('#listing-title')).toBeVisible();
    for (const [field, value] of Object.entries(form)) {
      const id =
        '#listing-' +
        (
          {
            contactType: 'contact-type',
            contactValue: 'contact-value'
          } as Record<string, string>
        )[field];
      const selector = field.startsWith('contact') ? id : '#listing-' + field;
      if (field === 'contactType') {
        await page
          .getByText('Add public contact details (optional)', { exact: true })
          .click();
        await page.locator(selector).selectOption(value);
      } else if (field === 'contactValue')
        await page.locator(selector).fill(value);
      else if (field === 'unit')
        await page.locator(selector).selectOption(value);
      else await page.locator(selector).fill(value);
    }
    const originalURL = page.url();
    await page
      .getByRole('button', { name: 'Review listing', exact: true })
      .click();
    await expect(
      page.getByRole('heading', { name: 'Public review', exact: true })
    ).toBeVisible();
    await expect(page.locator('#listing-review-content')).toHaveText(
      form.description.replaceAll('\r\n', '\n').trim() +
        '\n\nPublic contact: mailto:public@example.org',
      { useInnerText: false }
    );
    expect(await page.locator('#listing-review-content').textContent()).toBe(
      form.description.replaceAll('\r\n', '\n').trim() +
        '\n\nPublic contact: mailto:public@example.org'
    );
    await expect(page.locator('#listing-title')).toHaveCount(0);
    expect(page.url()).toBe(originalURL);
    await expect(
      page.getByRole('button', { name: 'Publish', exact: true })
    ).toHaveCount(0);
    await page
      .getByRole('button', { name: 'Edit details', exact: true })
      .click();
    await expect(page.locator('#listing-title')).toBeFocused();
    await expect(page.locator('#listing-title')).toHaveValue(form.title);
    await expect(page.locator('#listing-description')).toHaveValue(
      form.description.replaceAll('\r\n', '\n')
    );
    await expect(page.locator('#listing-amount')).toHaveValue(form.amount);
    await expect(page.locator('#listing-contact-value')).toHaveValue(
      form.contactValue
    );
    for (const [field, value] of Object.entries(form)) {
      const id =
        { contactType: 'contact-type', contactValue: 'contact-value' }[
          field as 'contactType' | 'contactValue'
        ] ?? field;
      await expect(page.locator('#listing-' + id)).toHaveValue(
        value.replaceAll('\r\n', '\n')
      );
    }
    expect(
      (await page.evaluate(() => window.hcp117Fixture.inspect())).operations
    ).toHaveLength(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      )
    ).toBe(true);
  });
  test(`HCP117 ${width}px linked inline errors preserve input and refuse stale saved content`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(async () => {
      window.hcp117Fixture = await window.hcp117.createFixture();
      window.hcp117Fixture.render();
    });
    const title = page.locator('#listing-title');
    await expect(title).toBeVisible();
    await title.fill('raw retained title');
    await page.locator('#listing-description').fill(' raw\n菜 description ');
    await page.locator('#listing-location').fill(form.location);
    await page.locator('#listing-currency').fill(form.currency);
    await page.locator('#listing-unit').selectOption(form.unit);
    await page.locator('#listing-amount').fill('2,50');
    await page
      .getByRole('button', { name: 'Review listing', exact: true })
      .click();
    await expect(page.locator('#listing-review-errors')).toBeVisible();
    await expect(page.locator('#listing-amount')).toHaveAttribute(
      'aria-invalid',
      'true'
    );
    await expect(page.locator('#listing-review-errors a')).toHaveAttribute(
      'href',
      '#listing-amount'
    );
    await expect(title).toHaveValue('raw retained title');
    await expect(page.locator('#listing-description')).toHaveValue(
      ' raw\n菜 description '
    );
    await expect(page.locator('#listing-amount')).toHaveValue('2,50');
    await expect(
      page.getByRole('heading', { name: 'Public review', exact: true })
    ).toHaveCount(0);
    expect(
      (await page.evaluate(() => window.hcp117Fixture.inspect())).operations
    ).toHaveLength(0);
  });
}
