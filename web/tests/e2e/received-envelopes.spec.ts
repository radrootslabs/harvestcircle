import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/received-envelopes.ts';
declare global {
  interface Window {
    hcp091: typeof Fixture;
    hcp091Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
    hcp091Run?: ReturnType<
      Awaited<ReturnType<typeof Fixture.makeFixture>>['run']
    >;
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
          new URL('./harness/received-envelopes.ts', import.meta.url)
        ),
        name: 'hcp091',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle missing');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  await server.close();
});
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp091Fixture = await window.hcp091.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp091Fixture?.close());
});
test('stored actual ciphertext is explicitly unlocked with two stock SDK decryptions and no plaintext persistence', async ({
  page
}) => {
  const before = await page.evaluate(() => window.hcp091Fixture.countsValue());
  const result = await page.evaluate(async () => {
    const f = window.hcp091Fixture;
    f.capture();
    const result = await f.run();
    return {
      result,
      counts: f.countsValue(),
      stored: await f.stored(),
      forged: f.forged()
    };
  });
  expect(result.result.status).toBe('authenticated');
  expect(result.result.snapshot).toBeDefined();
  expect(result.counts.decrypts).toBe(before.decrypts + 2);
  expect(result.counts.keys).toBeGreaterThan(before.keys + 1);
  expect(result.counts.encrypts).toBe(before.encrypts);
  expect(result.counts.signs).toBe(before.signs);
  expect(result.stored).toMatchObject({
    family: 'received_envelope',
    read: null
  });
  expect(result.stored).not.toHaveProperty('body');
  expect(result.forged).toBeUndefined();
});
test('absent explicit unlock cannot capture or decrypt', async ({ page }) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp091Fixture,
      before = f.countsValue();
    return {
      capture: f.capture('unreviewed'),
      result: await f.run(),
      before,
      after: f.countsValue()
    };
  });
  expect(r.capture).toBe(false);
  expect(r.result.status).toBe('invalid');
  expect(r.after).toEqual(r.before);
});
test('one invalid decrypted seal is isolated and a later explicit valid candidate remains usable', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp091Fixture;
    f.mode('wrong_plaintext');
    f.capture();
    const bad = await f.run();
    f.mode('normal');
    f.capture();
    const good = await f.run();
    return { bad, good };
  });
  expect(r.bad.status).toBe('mismatch');
  expect(r.bad.snapshot).toBeUndefined();
  expect(r.good.status).toBe('authenticated');
});
test('changed fresh extension owner fences the first decrypt', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp091Fixture,
      before = f.countsValue();
    f.mode('changed_key');
    f.capture();
    const result = await f.run();
    return { result, before, after: f.countsValue() };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.after.decrypts).toBe(r.before.decrypts);
  expect(r.result.snapshot).toBeUndefined();
});
test('actual decryption refusal has no nested proof or automatic follow-up', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp091Fixture,
      before = f.countsValue();
    f.mode('declined_decrypt');
    f.capture();
    const result = await f.run();
    return { result, before, after: f.countsValue() };
  });
  expect(r.result.status).toBe('refused');
  expect(r.after.decrypts).toBe(r.before.decrypts + 1);
  expect(r.result.snapshot).toBeUndefined();
});
test('Stop during actual provider wait cannot mint a proof or schedule inner decryption', async ({
  page
}) => {
  const before = await page.evaluate(() => {
    const f = window.hcp091Fixture;
    f.mode('hold_decrypt');
    f.capture();
    window.hcp091Run = f.run();
    return f.countsValue();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp091Fixture.pending()))
    .toBe(true);
  const r = await page.evaluate(async () => {
    const f = window.hcp091Fixture;
    f.stop();
    f.settle();
    return {
      result: await window.hcp091Run,
      counts: f.countsValue(),
      snapshot: f.snapshot()
    };
  });
  expect(r.result?.status).toBe('stopped');
  expect(r.counts.decrypts).toBe(before.decrypts + 1);
  expect(r.snapshot).toBeUndefined();
});
test('logout during actual provider wait destroys session-owned nested evidence', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp091Fixture;
    f.mode('hold_decrypt');
    f.capture();
    window.hcp091Run = f.run();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp091Fixture.pending()))
    .toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.hcp091Fixture;
    f.disconnect();
    f.settle();
    return await window.hcp091Run;
  });
  expect(result?.status).toBe('stopped');
  expect(result?.snapshot).toBeUndefined();
});
test('changed actual stored full-wire metadata during decrypt invalidates original custody', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp091Fixture;
    f.mode('hold_decrypt');
    f.capture();
    window.hcp091Run = f.run();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp091Fixture.pending()))
    .toBe(true);
  expect(
    await page.evaluate(async () => {
      const f = window.hcp091Fixture;
      const result = await f.changeObservedSource();
      f.mode('normal');
      f.settle();
      return result.ok;
    })
  ).toBe(true);
  const result = await page.evaluate(async () => await window.hcp091Run);
  expect(result?.status).toBe('conflict');
  expect(result?.snapshot).toBeUndefined();
});

test('actual stored-wire change during held final SDK key check cannot mint current nested custody', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.hcp091Fixture;
    f.holdFinalKey();
    f.capture();
    window.hcp091Run = f.run();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp091Fixture.pendingFinalKey()))
    .toBe(true);
  expect(
    await page.evaluate(async () => {
      const f = window.hcp091Fixture;
      const changed = await f.changeObservedSource();
      f.settleFinalKey();
      return changed.ok;
    })
  ).toBe(true);
  const result = await page.evaluate(async () => await window.hcp091Run);
  expect(result?.status).toBe('conflict');
  expect(result?.snapshot).toBeUndefined();
});
