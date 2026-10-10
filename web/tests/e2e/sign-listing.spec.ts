import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/sign-listing.ts';
declare global {
  interface Window {
    hcp111: typeof Fixture;
    hcp111Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
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
          new URL('./harness/sign-listing.ts', import.meta.url)
        ),
        name: 'hcp111',
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
    window.hcp111Fixture = await window.hcp111.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp111Fixture?.close());
});
test('HCP111 original verified signed artifact commits before a typed plan and repeated clicks never re-sign', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp111Fixture.exact());
  console.log('HCP111_DIAGNOSTIC_ACTUAL_ORIGINAL_RESULT', JSON.stringify(r));
  expect(r.status).toBe('prepared');
  expect(r.same).toBe(true);
  expect(r.signatures).toBe(1);
  expect(r.keys).toBeGreaterThan(1);
  expect(r.revision).toBe(2);
  expect(r.artifact?.eventId).toBe(r.expectedHash);
  expect(r.plan?.wire).toBe(r.artifact?.wire);
  expect(r.originalCapture).toBe(true);
  expect(r.retry).toBe('prepared');
  expect(r.sameArtifact).toBe(true);
});
for (const mode of [
  'author',
  'kind',
  'time',
  'tags',
  'content',
  'cached',
  'mutate'
] as const)
  test(`HCP111 actual signed changed ${mode} response never commits or exposes a plan`, async ({
    page
  }) => {
    const r = await page.evaluate(
      (mode) => window.hcp111Fixture.wrong(mode),
      mode
    );
    expect(r.exposed).toBe(false);
    expect(r.signatures).toBe(1);
    expect(r.revision).toBe(1);
    expect(r.artifact).toBeNull();
    expect(r.retry).not.toBe('prepared');
  });
test('HCP111 changed actual fresh extension key stops signature and plan', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp111Fixture.changedKey());
  expect(r.exposed).toBe(false);
  expect(r.signatures).toBe(0);
  expect(r.artifact).toBeNull();
});
for (const logout of [false, true])
  test(`HCP111 held actual signature after Stop logout=${logout} remains original without artifact or plan`, async ({
    page
  }) => {
    const r = await page.evaluate(
      (logout) => window.hcp111Fixture.late(logout),
      logout
    );
    expect(r.exposed).toBe(false);
    expect(r.signatures).toBe(1);
    expect(r.revision).toBe(1);
    expect(r.artifact).toBeNull();
    expect(r.snapshot?.owner).toBe(r.originalOwner);
    expect(r.snapshot?.command).toBe(r.command);
    expect(r.snapshot?.hash).toBe(r.expectedHash);
    expect(r.snapshot?.lateAvailable).toBe(true);
  });
for (const revision of [1, 2] as const)
  test(`HCP111 actual revision${revision} transaction abort stops the dependent effect`, async ({
    page
  }) => {
    const r = await page.evaluate(
      (revision) => window.hcp111Fixture.fault(revision, false),
      revision
    );
    expect(r.exposed).toBe(false);
    expect(r.faultPuts).toBe(1);
    expect(r.revision).toBe(revision - 1);
    expect(r.artifact).toBeNull();
    expect(r.signatures).toBe(revision - 1);
    expect(r.originalCapture).toBe(true);
  });
test('HCP111 actual artifact commit lost acknowledgement reconciles exact original signed bytes before plan', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp111Fixture.fault(2, true));
  expect(r.status).toBe('prepared');
  expect(r.exposed).toBe(true);
  expect(r.faultPuts).toBe(1);
  expect(r.revision).toBe(2);
  expect(r.signatures).toBe(1);
  expect(r.sameArtifact).toBe(true);
  expect(r.originalCapture).toBe(true);
});
test('HCP111 original inbox retirement during the actual prompt revokes continuity without another effect', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp111Fixture.late(false, true));
  expect(r.exposed).toBe(false);
  expect(r.signatures).toBe(1);
  expect(r.revision).toBe(1);
  expect(r.artifact).toBeNull();
  expect(r.snapshot?.owner).toBe(r.originalOwner);
  expect(r.snapshot?.hash).toBe(r.expectedHash);
  expect(r.snapshot?.lateAvailable).toBe(true);
});
test('HCP111 unavailable artifact readback preserves uncertainty until exact original explicit retry without a new signature or put', async ({
  page
}) => {
  const r = await page.evaluate(() =>
    window.hcp111Fixture.fault(2, true, true)
  );
  expect(r.status).toBe('unknown');
  expect(r.exposed).toBe(false);
  expect(r.faultPuts).toBe(1);
  expect(r.revision).toBe(2);
  expect(r.signatures).toBe(1);
  expect(r.originalCapture).toBe(true);
  expect(r.retryStatus).toBe('prepared');
  expect(r.retryExact).toBe(true);
  expect(r.retryPuts).toBe(0);
});
test('HCP111 actual valid mismatched signed artifact readback conflicts without adoption or another effect', async ({
  page
}) => {
  const r = await page.evaluate(() =>
    window.hcp111Fixture.fault(2, true, false, 'valid')
  );
  expect(r.status).toBe('conflict');
  expect(r.exposed).toBe(false);
  expect(r.readStatus).toBe('found');
  expect(r.faultPuts).toBe(2);
  expect(r.nativePuts).toBe(3);
  expect(r.revision).toBe(2);
  expect(r.signatures).toBe(1);
  expect(r.originalCapture).toBe(false);
  expect(r.retryStatus).toBe('conflict');
  expect(r.retryExact).toBe(false);
  expect(r.retryPuts).toBe(0);
});
test('HCP111 actual corrupt artifact readback preserves stored evidence and refuses an explicit retry', async ({
  page
}) => {
  const r = await page.evaluate(() =>
    window.hcp111Fixture.fault(2, true, false, 'corrupt')
  );
  expect(r.status).toBe('unknown');
  expect(r.exposed).toBe(false);
  expect(r.readStatus).toBe('corrupt_record');
  expect(r.corruptWire).toBe('{"schema":999}');
  expect(r.faultPuts).toBe(2);
  expect(r.nativePuts).toBe(3);
  expect(r.signatures).toBe(1);
  expect(r.retryStatus).toBe('unavailable');
  expect(r.retryExact).toBe(false);
  expect(r.retryPuts).toBe(0);
});
