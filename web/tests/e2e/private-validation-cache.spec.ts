import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/private-validation-cache.ts';
declare global {
  interface Window {
    cr005: typeof Fixture;
    cr005Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
const baseline = process.env.HCP_CR005_PHASE === 'baseline';
test.beforeAll(async () => {
  server = await createStaticHarness();
  const entry = fileURLToPath(
      new URL('./harness/private-validation-cache.ts', import.meta.url)
    ),
    result = await build({
      configFile: false,
      logLevel: 'silent',
      plugins: [
        {
          name: 'cr005-count-real-validator',
          enforce: 'pre',
          transform(code, id) {
            if (!id.endsWith('/src/lib/nostr/verified-envelope.ts')) return;
            const marker = '  if (\n    typeof raw';
            expect(code.split(marker)).toHaveLength(2);
            return {
              code: code.replace(
                marker,
                '  globalThis.__cr005VerifyCalls = (globalThis.__cr005VerifyCalls ?? 0) + 1;\n' +
                  marker
              ),
              map: null
            };
          }
        }
      ],
      build: {
        write: false,
        minify: false,
        lib: { entry, name: 'cr005', formats: ['iife'] }
      }
    });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('fixture bundle absent');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'CR005_TEST_ONLY_ACTUAL_IDB_REAL_VALIDATOR_COUNT',
      phase: baseline ? 'baseline' : 'candidate',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Counter before unchanged real validator, no fabricated proofs/operator/fullQ'
    })
  );
});
test.afterAll(async () => server?.close());
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/about');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.cr005Fixture = await window.cr005.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed()) await page.evaluate(() => window.cr005Fixture?.close());
});
test('CR005 actual capacity sample retains complete cursor checks while reusing unchanged validation', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    return {
      first: await f.sample(),
      second: await f.sample(),
      inventory: await f.inventory(),
      metadata: await f.metadata(),
      outboxFirst: await f.sample('private_sends'),
      outboxSecond: await f.sample('private_sends'),
      bytes: f.bytes()
    };
  });
  console.log(
    JSON.stringify({
      fixture: 'CR005_ACTUAL_CAPACITY_COUNTS',
      phase: baseline ? 'baseline' : 'candidate',
      ...result
    })
  );
  for (const row of [
    result.first,
    result.second,
    result.inventory,
    result.metadata
  ])
    expect(row).toMatchObject({ ok: true, cursorVisits: 2000 });
  expect(result.inventory.count).toBe(2000);
  expect(result.metadata.count).toBe(2000);
  expect(result.outboxFirst).toMatchObject({ ok: true, cursorVisits: 100 });
  expect(result.outboxSecond).toMatchObject({ ok: true, cursorVisits: 100 });
  expect(result.bytes.received).toBeLessThanOrEqual(50331648);
  expect(result.bytes.sends).toBeLessThanOrEqual(8388608);
  if (baseline) {
    expect(result.first.verificationCalls).toBe(2001);
    expect(result.second.verificationCalls).toBe(2001);
    expect(result.inventory.verificationCalls).toBe(4000);
    expect(result.metadata.verificationCalls).toBe(6000);
  } else {
    expect(result.first.verificationCalls).toBe(2000);
    expect(result.second.verificationCalls).toBe(0);
    expect(result.inventory.verificationCalls).toBe(0);
    expect(result.metadata.verificationCalls).toBe(0);
  }
});
for (const change of ['valid', 'corrupt', 'summary', 'id'] as const)
  test(
    'CR005 warm complete namespace rejects or revalidates changed ' +
      change +
      ' tail through a second connection',
    async ({ page }) => {
      const result = await page.evaluate(async (change) => {
        const f = window.cr005Fixture;
        await f.sample();
        await f.changeTail(change, true);
        return await f.sample();
      }, change);
      if (change === 'valid')
        expect(result).toMatchObject({
          ok: true,
          cursorVisits: 2000,
          verificationCalls: 1
        });
      else
        expect(result).toMatchObject({
          ok: false,
          reason: 'corrupt_record',
          cursorVisits: 2000
        });
    }
  );
test('CR005 wrong namespace cannot borrow warm validation or claim empty owner history', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    await f.sample();
    return { read: await f.foreignRead(), metadata: await f.metadata(true) };
  });
  expect(result.read).toMatchObject({ ok: false });
  expect(result.metadata).toMatchObject({ ok: false, reason: 'invalid_scope' });
});
test('CR005 warm outbox still rejects actual101rows without eviction', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    await f.sample('private_sends');
    return await f.overfillSends();
  });
  expect(result).toMatchObject({ ok: false, reason: 'capacity' });
});
test('CR005 encoded byte accounting rejects changed oversized received namespace', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    await f.sample();
    return await f.oversizeReceived();
  });
  expect(result.bytes).toBeGreaterThan(50331648);
  expect(result).toMatchObject({ ok: false, reason: 'capacity' });
});

test('CR005 warm received namespace rejects actual2001rows without eviction', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    await f.sample();
    return await f.overfillReceived();
  });
  expect(result).toMatchObject({ ok: false, reason: 'capacity' });
});
test('CR005 changed current read/source metadata is revalidated and reflected in original history', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.cr005Fixture;
    await f.sample();
    await f.changeTail('metadata', true);
    return { read: await f.sample(), metadata: await f.metadata() };
  });
  expect(result.read).toMatchObject({
    ok: true,
    cursorVisits: 2000,
    verificationCalls: 1
  });
  expect(result.metadata).toMatchObject({
    ok: true,
    cursorVisits: 2000,
    verificationCalls: 0,
    count: 2001
  });
});
