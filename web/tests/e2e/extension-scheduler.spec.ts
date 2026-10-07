import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Scheduler from '../../src/lib/nostr/extension-scheduler.ts';
declare global {
  interface Window {
    hcp052: typeof Scheduler;
    hcp052Scheduler: Scheduler.ExtensionScheduler;
    hcp052Action: Scheduler.ExtensionAction;
    hcp052Task: Promise<Scheduler.ExtensionActionResult<unknown>>;
    hcp052Release: (value: string) => void;
    hcp052Calls: string[];
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
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
          new URL('../../src/lib/nostr/extension-scheduler.ts', import.meta.url)
        ),
        name: 'hcp052',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
  const chunks = output.output.filter((v) => v.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP052_ACTUAL_SCHEDULER_CONTROLLED_CALLS',
      moduleIds: Object.keys(chunks[0].modules),
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      claim:
        'actual Chromium and current scheduler; no real extension qualification'
    })
  );
});
test.afterAll(async () => await Promise.resolve(server.close()));
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(() => {
    window.hcp052Scheduler = window.hcp052.browserExtensionScheduler()!;
    window.hcp052Calls = [];
  });
});
test('load and shared accessor request no extension permission', async ({
  page
}) => {
  expect(
    await page.evaluate(() => ({
      calls: window.hcp052Calls,
      same:
        window.hcp052.browserExtensionScheduler() === window.hcp052Scheduler,
      snapshot: window.hcp052.extensionSchedulerSnapshot(window.hcp052Scheduler)
    }))
  ).toEqual({ calls: [], same: true, snapshot: { state: 'idle' } });
});
test('one explicit batch serializes all five call kinds and blocks competing actions', async ({
  page
}) => {
  await page.evaluate((owner) => {
    window.hcp052Task = window.hcp052.runExtensionAction(
      window.hcp052Scheduler,
      { owner, session: Symbol(), operation: Symbol() },
      () => true,
      async (a) => {
        const key = await window.hcp052.callExtension(a, 'key', () => {
          window.hcp052Calls.push('key');
          return new Promise<string>((resolve) => {
            window.hcp052Release = resolve;
          });
        });
        for (const kind of ['sign', 'encrypt', 'decrypt', 'auth'] as const)
          await window.hcp052.callExtension(a, kind, async () => {
            window.hcp052Calls.push(kind);
            return await Promise.resolve(kind);
          });
        return key;
      }
    );
  }, owner);
  expect(
    await page.evaluate(
      (owner) =>
        window.hcp052.runExtensionAction(
          window.hcp052Scheduler,
          { owner, session: Symbol(), operation: Symbol() },
          () => true,
          async () => {
            window.hcp052Calls.push('competing');

            await Promise.resolve();
          }
        ),
      owner
    )
  ).toEqual({ status: 'busy' });
  await page.evaluate((owner) => window.hcp052Release(owner), owner);
  expect(await page.evaluate(() => window.hcp052Task)).toMatchObject({
    status: 'completed',
    value: { status: 'settled', current: true, value: owner }
  });
  expect(await page.evaluate(() => window.hcp052Calls)).toEqual([
    'key',
    'sign',
    'encrypt',
    'decrypt',
    'auth'
  ]);
});
test('UI timeout retains slot, original capture and late signature without next AUTH', async ({
  page
}) => {
  await page.evaluate((owner) => {
    window.hcp052Task = window.hcp052.runExtensionAction(
      window.hcp052Scheduler,
      { owner, session: Symbol(), operation: Symbol() },
      () => true,
      async (a) => {
        window.hcp052Action = a;
        const signed = await window.hcp052.callExtension(
          a,
          'sign',
          () =>
            new Promise<string>((resolve) => {
              window.hcp052Calls.push('sign');
              window.hcp052Release = resolve;
            })
        );
        const auth = await window.hcp052.callExtension(a, 'auth', async () => {
          window.hcp052Calls.push('auth');
          return await Promise.resolve('unused');
        });
        return { signed, auth };
      }
    );
    window.hcp052.markExtensionWaitExpired(window.hcp052Action);
  }, owner);
  expect(
    await page.evaluate(() =>
      window.hcp052.extensionSchedulerSnapshot(window.hcp052Scheduler)
    )
  ).toEqual({ state: 'active', phase: 'wait_expired', pending: 'sign' });
  expect(
    await page.evaluate(
      (owner) =>
        window.hcp052.runExtensionAction(
          window.hcp052Scheduler,
          { owner, session: Symbol(), operation: Symbol() },
          () => true,
          async () => {
            window.hcp052Calls.push('unexpected');

            await Promise.resolve();
          }
        ),
      owner
    )
  ).toEqual({ status: 'busy' });
  await page.evaluate(() => window.hcp052Release('controlled late signature'));
  expect(await page.evaluate(() => window.hcp052Task)).toMatchObject({
    status: 'wait_expired',
    capture: { owner },
    value: {
      signed: {
        status: 'settled',
        current: false,
        value: 'controlled late signature',
        capture: { owner }
      },
      auth: { status: 'wait_expired' }
    }
  });
  expect(await page.evaluate(() => window.hcp052Calls)).toEqual(['sign']);
});
test('decrypt denial pauses batch with safe finite outcome and no automatic resume', async ({
  page
}) => {
  const outcome = await page.evaluate(
    (owner) =>
      window.hcp052.runExtensionAction(
        window.hcp052Scheduler,
        { owner, session: Symbol(), operation: Symbol() },
        () => true,
        async (a) => {
          const denied = await window.hcp052.callExtension(
            a,
            'decrypt',
            async () => {
              window.hcp052Calls.push('decrypt');
              await Promise.resolve();
              throw new Error('controlled private body');
            }
          );
          const next = await window.hcp052.callExtension(
            a,
            'sign',
            async () => {
              window.hcp052Calls.push('sign');
              return await Promise.resolve('unused');
            }
          );
          return { denied, next };
        }
      ),
    owner
  );
  expect(outcome).toMatchObject({
    status: 'denied',
    value: { denied: { status: 'denied' }, next: { status: 'denied' } }
  });
  expect(JSON.stringify(outcome)).not.toContain('controlled private body');
  expect(await page.evaluate(() => window.hcp052Calls)).toEqual(['decrypt']);
});
test('local stop and forgotten await retain shared browser scheduler until settlement', async ({
  page
}) => {
  await page.evaluate((owner) => {
    window.hcp052Task = window.hcp052.runExtensionAction(
      window.hcp052Scheduler,
      { owner, session: Symbol(), operation: Symbol() },
      () => true,
      async (a) => {
        window.hcp052Action = a;
        void window.hcp052.callExtension(
          a,
          'encrypt',
          () =>
            new Promise<string>((resolve) => {
              window.hcp052Calls.push('encrypt');
              window.hcp052Release = resolve;
            })
        );
        window.hcp052.stopExtensionAction(a);

        await Promise.resolve();
      }
    );
  }, owner);
  expect(
    await page.evaluate(
      (owner) =>
        window.hcp052.runExtensionAction(
          window.hcp052.browserExtensionScheduler()!,
          { owner, session: Symbol(), operation: Symbol() },
          () => true,
          async () => {
            window.hcp052Calls.push('unexpected');

            await Promise.resolve();
          }
        ),
      owner
    )
  ).toEqual({ status: 'busy' });
  await page.evaluate(() => window.hcp052Release('late cipher'));
  expect(await page.evaluate(() => window.hcp052Task)).toMatchObject({
    status: 'stopped'
  });
  expect(
    await page.evaluate(() =>
      window.hcp052.extensionSchedulerSnapshot(window.hcp052Scheduler)
    )
  ).toEqual({ state: 'idle' });
  expect(await page.evaluate(() => window.hcp052Calls)).toEqual(['encrypt']);
});
