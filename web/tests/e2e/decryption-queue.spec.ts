import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/decryption-queue.ts';
declare global {
  interface Window {
    hcp098: typeof Fixture;
    hcp098Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
    hcp098Pending: Promise<unknown>;
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
          new URL('./harness/decryption-queue.ts', import.meta.url)
        ),
        name: 'HCP098',
        formats: ['iife']
      }
    }
  });
  const outputs = Array.isArray(result) ? result : [result];
  const chunk = outputs
    .flatMap((output) => ('output' in output ? output.output : []))
    .find((x) => x.type === 'chunk');
  if (!chunk || chunk.type !== 'chunk') throw Error('no actual JS');
  bundle = chunk.code + '\nwindow.hcp098=HCP098;';
});
test.beforeEach(async ({ page }, testInfo) => {
  await page.goto(server.url + '/messages');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (unknown) => {
    window.hcp098Fixture = await window.hcp098.makeFixture(21, unknown);
  }, testInfo.title.startsWith('unknown authenticated blocked'));
});
test.afterEach(async ({ page }) => {
  await page.evaluate(() => window.hcp098Fixture?.close());
});
test.afterAll(async () => {
  await server?.close();
});
test('21st envelope waits new explicit action after20 actual serialized nested decryptions', async ({
  page
}) => {
  expect(await page.evaluate(() => window.hcp098Fixture.capture())).toBe(true);
  await page.evaluate(() => window.hcp098Fixture.refresh());
  expect(
    (await page.evaluate(() => window.hcp098Fixture.snapshot()))?.queued
  ).toBe(21);
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(true);
  expect(await page.evaluate(() => window.hcp098Fixture.delta())).toMatchObject(
    { decrypts: 40, signs: 0, encrypts: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ state: 'needs_action', queued: 1, attempted: 20 });
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 1 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.batch('not_reviewed'))
  ).toBe(false);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(40);
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(true);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(42);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ state: 'idle', queued: 0, attempted: 1 });
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp098Fixture.publicContains())).toBe(
    false
  );
});
test('connect capture refresh and unreviewed batch never automatically decrypt', async ({
  page
}) => {
  expect(await page.evaluate(() => window.hcp098Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(await page.evaluate(() => window.hcp098Fixture.capture('wrong'))).toBe(
    false
  );
  expect(await page.evaluate(() => window.hcp098Fixture.capture())).toBe(true);
  await page.evaluate(() => window.hcp098Fixture.refresh());
  expect(await page.evaluate(() => window.hcp098Fixture.batch('wrong'))).toBe(
    false
  );
  expect(await page.evaluate(() => window.hcp098Fixture.delta())).toMatchObject(
    { decrypts: 0, signs: 0, encrypts: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ queued: 21, attempted: 0 });
});
test('actual provider refusal pauses scheduling and preserves every original ciphertext', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp098Fixture.capture();
    window.hcp098Fixture.mode('declined_decrypt');
  });
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(false);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({
    state: 'paused',
    reason: 'refused',
    queued: 21,
    attempted: 1
  });
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(1);
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
  await page.evaluate(() => window.hcp098Fixture.mode('normal'));
  expect(await page.evaluate(() => window.hcp098Fixture.batch('wrong'))).toBe(
    false
  );
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(1);
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(true);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ queued: 1, attempted: 20 });
});
test('unknown authenticated blocked inner peer still consumes20 envelope costs before hiding bodies', async ({
  page
}) => {
  await page.evaluate(() => window.hcp098Fixture.capture());
  expect(
    await page.evaluate(() =>
      window.hcp098Fixture.batch('reviewed_decrypt_batch', [
        window.hcp098Fixture.stranger
      ])
    )
  ).toBe(true);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(40);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ queued: 1, attempted: 20, blocked: 20 });
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
test('blocking disposable outer author does not replace actual authenticated inner peer', async ({
  page
}) => {
  await page.evaluate(() => window.hcp098Fixture.capture());
  expect(
    await page.evaluate(() =>
      window.hcp098Fixture.batch('reviewed_decrypt_batch', [
        window.hcp098Fixture.disposable
      ])
    )
  ).toBe(true);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ queued: 1, attempted: 20, blocked: 0 });
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 1 }
  );
});
test('expired UI wait preserves actual occupied SDK slot until original provider settles', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp098Fixture.capture();
    window.hcp098Fixture.mode('hold_decrypt');
    window.hcp098Pending = window.hcp098Fixture.batch();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp098Fixture.scheduler()))
    .toMatchObject({ state: 'active', pending: 'decrypt' });
  await page.evaluate(() => window.hcp098Fixture.expire());
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ state: 'paused', reason: 'wait_expired', queued: 21 });
  expect(
    await page.evaluate(() => window.hcp098Fixture.scheduler())
  ).toMatchObject({
    state: 'active',
    pending: 'decrypt',
    phase: 'wait_expired'
  });
  expect(
    await page.evaluate(() => window.hcp098Fixture.competing())
  ).toMatchObject({ status: 'busy' });
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(false);
  await page.evaluate(() => window.hcp098Fixture.settle());
  await page.evaluate(() => window.hcp098Pending);
  expect(
    await page.evaluate(() => window.hcp098Fixture.scheduler())
  ).toMatchObject({ state: 'idle' });
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(1);
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
test('logout during actual pending decrypt cannot cache a late result or schedule a second envelope', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp098Fixture.capture();
    window.hcp098Fixture.mode('hold_decrypt');
    window.hcp098Pending = window.hcp098Fixture.batch();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp098Fixture.scheduler()))
    .toMatchObject({ state: 'active', pending: 'decrypt' });
  await page.evaluate(() => window.hcp098Fixture.disconnect());
  expect(
    await page.evaluate(() => window.hcp098Fixture.scheduler())
  ).toMatchObject({ state: 'active', pending: 'decrypt' });
  await page.evaluate(() => window.hcp098Fixture.settle());
  await page.evaluate(() => window.hcp098Pending);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(1);
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { closed: true, count: 0 }
  );
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(false);
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
test('forged copied closed-unlock and invalid block metadata cannot gain SDK admission', async ({
  page
}) => {
  await page.evaluate(() => window.hcp098Fixture.capture());
  expect(await page.evaluate(() => window.hcp098Fixture.forged())).toBe(false);
  expect(await page.evaluate(() => window.hcp098Fixture.copied())).toBe(false);
  expect(
    await page.evaluate(() =>
      window.hcp098Fixture.batch('reviewed_decrypt_batch', ['wrong'])
    )
  ).toBe(false);
  await page.evaluate(() => window.hcp098Fixture.closeUnlock());
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(false);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(0);
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});

test('malformed decrypted layer is isolated but consumes the explicit envelope cap', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp098Fixture.capture();
    window.hcp098Fixture.mode('wrong_plaintext');
  });
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(true);
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(20);
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({
    state: 'needs_action',
    queued: 1,
    attempted: 20,
    invalid: 20
  });
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
test('explicit Stop during actual provider wait keeps SDK occupied and prevents the next envelope', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp098Fixture.capture();
    window.hcp098Fixture.mode('hold_decrypt');
    window.hcp098Pending = window.hcp098Fixture.batch();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp098Fixture.scheduler()))
    .toMatchObject({ state: 'active', pending: 'decrypt' });
  await page.evaluate(() => window.hcp098Fixture.stop());
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ state: 'stopped', busy: true });
  expect(
    await page.evaluate(() => window.hcp098Fixture.competing())
  ).toMatchObject({ status: 'busy' });
  expect(await page.evaluate(() => window.hcp098Fixture.batch())).toBe(false);
  await page.evaluate(() => window.hcp098Fixture.settle());
  await page.evaluate(() => window.hcp098Pending);
  expect(
    await page.evaluate(() => window.hcp098Fixture.scheduler())
  ).toMatchObject({ state: 'idle' });
  expect(
    (await page.evaluate(() => window.hcp098Fixture.delta())).decrypts
  ).toBe(1);
  expect(await page.evaluate(() => window.hcp098Fixture.cache())).toMatchObject(
    { count: 0 }
  );
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
test('same-key genuine foreign identity generation cannot import original unlock authority', async ({
  page
}) => {
  await page.evaluate(() => window.hcp098Fixture.capture());
  await page.evaluate(() => window.hcp098Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp098Fixture.foreignCapture())
  ).toEqual({ captured: false, keys: 0, decrypts: 0, signs: 0 });
  expect(
    await page.evaluate(() => window.hcp098Fixture.snapshot())
  ).toMatchObject({ queued: 21, attempted: 0 });
  expect(
    await page.evaluate(() => window.hcp098Fixture.encryptedUnchanged())
  ).toBe(true);
});
