import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/prepare-listing.ts';
declare global {
  interface Window {
    hcp110: typeof Fixture;
    hcp110Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
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
          new URL('./harness/prepare-listing.ts', import.meta.url)
        ),
        name: 'hcp110',
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
  await page.evaluate(async () => {
    window.hcp110Fixture = await window.hcp110.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp110Fixture?.close());
});

test('HCP110 double submit retains one immutable intent and future edits cannot replace it', async ({
  page
}) => {
  const result = await page.evaluate(() => window.hcp110Fixture.doubleSubmit());
  expect(result.same).toBe(true);
  expect(result.repeatReview).toBe('existing');
  expect(result.repeatCaptureUnchanged).toBe(true);
  expect(result.operations).toBe(1);
  expect(result.futureCaptureUnchanged).toBe(true);
  expect(result.retry).toBe('existing');
  expect(result.signatures).toBe(0);
});
test('HCP110 concurrent original reviews retain one command capture across asynchronous hashing', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.concurrentReviews()
  );
  expect(result.sameReview).toBe(true);
  expect(result.sameTemplate).toBe(true);
  expect(result.first).toBe('saved');
  expect(result.second).toBe('existing');
  expect(result.operations).toBe(1);
  expect(result.signatures).toBe(0);
});
test('HCP110 actual committed intent with lost acknowledgement reconciles exact original capture without another write', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.unknown('exact')
  );
  expect(result.status).toBe('existing');
  expect(result.nativePuts).toBe(1);
  expect(result.originalCapture).toBe(true);
  expect(result.operations).toBe(1);
  expect(result.retry).toBe('existing');
  expect(result.signatures).toBe(0);
});
test('HCP110 failed native readback preserves original unknown completion before explicit retry', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.unknown('unavailable')
  );
  expect(result.status).toBe('unknown_completion');
  expect(result.nativePuts).toBe(1);
  expect(result.originalCapture).toBe(true);
  expect(result.operations).toBe(1);
  expect(result.retry).toBe('existing');
  expect(result.signatures).toBe(0);
});
test('HCP110 actual mismatched readback refuses the original command without adopting another capture', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.unknown('mismatch')
  );
  expect(result.status).toBe('conflict');
  expect(result.originalCapture).toBe(false);
  expect(result.nativePuts).toBe(2);
  expect(result.operations).toBe(1);
  expect(result.retry).toBe('conflict');
  expect(result.signatures).toBe(0);
});
test('HCP110 native read request setup failure resolves typed unavailable without writes', async ({
  page
}) => {
  const result = await page.evaluate(() => window.hcp110Fixture.failedRead());
  expect(result.status).toBe('unavailable');
  expect(result.operations).toBe(0);
  expect(result.signatures).toBe(0);
});

test('HCP110 changing the same command conflicts and a random listing identifier is independent of title', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.commandConflict()
  );
  expect(result.conflict).toBe('conflict');
  expect(result.identifierRandom).toBe(true);
  expect(result.identifierStable).toBe(true);
  expect(result.operations).toBe(1);
  expect(result.signatures).toBe(0);
});

test('HCP110 exact acknowledged draft revision and original frozen destinations are reserved', async ({
  page
}) => {
  const result = await page.evaluate(() =>
    window.hcp110Fixture.exactRevision()
  );
  expect(result.exact).toBe(true);
  expect(result.description).toBe('Second complete description');
  expect(result.targetsFrozen).toBe(true);
  expect(result.signatures).toBe(0);
});

test('HCP110 stale draft revision or original readiness retirement cannot reserve a new intent', async ({
  page
}) => {
  const result = await page.evaluate(() => window.hcp110Fixture.stale());
  expect(result.changedDraft).toBe('conflict');
  expect(result.retired).toBe('unavailable');
  expect(result.operations).toBe(0);
  expect(result.signatures).toBe(0);
});

test('HCP110 actual transaction abort creates no operation and original explicit retry reconciles the same capture', async ({
  page
}) => {
  const result = await page.evaluate(() => window.hcp110Fixture.abort());
  expect(result.aborted).toBe('aborted');
  expect(result.beforeRetry).toBe(0);
  expect(result.retry).toBe('saved');
  expect(result.operations).toBe(1);
  expect(result.originalCapture).toBe(true);
  expect(result.signatures).toBe(0);
});
