import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/new-listing-readiness.ts';
declare global {
  interface Window {
    hcp109: typeof Fixture;
    hcp109Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/new-listing-readiness.ts', import.meta.url)
        ),
        name: 'hcp109',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('fixture bundle missing');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => server.close());
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp109Fixture?.close());
});
for (const mode of ['guest', 'signing_only', 'compatible_unqualified'] as const)
  test(
    'HCP109 actual unfinished IndexedDB draft saves while new-listing review is blocked: ' +
      mode,
    async ({ page }) => {
      await page.evaluate(async (mode) => {
        window.hcp109Fixture = await window.hcp109.createFixture(mode);
      }, mode);
      const before = await page.evaluate(() => window.hcp109Fixture.snapshot());
      expect(before.readiness).toMatchObject({
        draft: true,
        readyToReview: false,
        preferenceWrite: 'not_requested'
      });
      expect(before.captured).toBe(false);
      expect(before.current).toBe(false);
      const actual = await page.evaluate(() => window.hcp109Fixture.save());
      expect(actual.saved).toBe(true);
      expect(actual.snapshot?.state).toBe('saved');
      expect(actual.snapshot?.draft?.form).toMatchObject({
        title: '',
        amount: '.',
        description: 'unfinished local public description'
      });
      expect(actual.rows.ok).toBe(true);
      if (!actual.rows.ok) throw Error(actual.rows.reason);
      expect(actual.rows.value).toHaveLength(1);
      expect(actual.rows.value[0]).toEqual(actual.snapshot?.draft);
      expect(actual.counts).toEqual(actual.baseline);
      expect(actual.counts.signs).toBe(0);
    }
  );
test('HCP109 qualified existing signed inbox admits original review with no redundant preference write or signature', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp109Fixture = await window.hcp109.createFixture('qualified');
  });
  const actual = await page.evaluate(() => window.hcp109Fixture.snapshot());
  expect(actual.readiness.readyToReview).toBe(true);
  expect(actual.captured).toBe(true);
  expect(actual.current).toBe(true);
  expect(actual.counts).toEqual(actual.baseline);
  expect(actual.counts.signs).toBe(0);
  expect(await page.evaluate(() => window.hcp109Fixture.copies())).toEqual({
    copiedSetup: false,
    copiedProof: false,
    genuine: true
  });
});
test('HCP109 original Stop revokes held new-listing proof while incomplete draft remains saveable', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp109Fixture = await window.hcp109.createFixture('qualified');
  });
  expect(
    (await page.evaluate(() => window.hcp109Fixture.snapshot())).current
  ).toBe(true);
  await page.evaluate(() => window.hcp109Fixture.stop());
  const stopped = await page.evaluate(() => window.hcp109Fixture.snapshot());
  expect(stopped.current).toBe(false);
  expect(stopped.readiness.readyToReview).toBe(false);
  const saved = await page.evaluate(() => window.hcp109Fixture.save());
  expect(saved.saved).toBe(true);
  expect(saved.rows.ok).toBe(true);
  expect(saved.counts.signs).toBe(0);
  expect(saved.counts).toEqual(saved.baseline);
});
