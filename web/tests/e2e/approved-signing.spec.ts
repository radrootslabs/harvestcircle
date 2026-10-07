import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Signing from './harness/approved-signing.ts';
import type { ApprovedPublicSigning } from '../../src/lib/nostr/approved-signing.ts';
import type {
  ExtensionAdapter,
  ApprovedSignResult
} from '../../src/lib/nostr/extension.ts';
declare global {
  interface Window {
    hcp053: typeof Signing;
    hcp053Fixture: ReturnType<typeof Signing.makeFixture>;
    hcp053Adapter: ExtensionAdapter;
    hcp053Approval: ApprovedPublicSigning;
    hcp053Mode: Signing.Mode;
    hcp053Calls: string[];
    hcp053Release: () => void;
    hcp053Task: Promise<ApprovedSignResult>;
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
          new URL('./harness/approved-signing.ts', import.meta.url)
        ),
        name: 'hcp053',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  const moduleIds = Object.keys(chunks[0].modules);
  expect(moduleIds.some((id) => id.includes('extension-signer'))).toBe(true);
  expect(
    moduleIds.filter((id) =>
      /\/(?:private-key-signer|password-signer|nostr-connect-signer)\.[cm]?[jt]s(?:$|\?)/.test(
        id
      )
    )
  ).toEqual([]);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP053_CONTROLLED_PROVIDER_ACTUAL_SDK_CRYPTO',
      moduleIds,
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      claim:
        'controlled provider source evidence only, no real extension qualification'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url);
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    const s = window.hcp053,
      f = s.makeFixture();
    window.hcp053Fixture = f;
    window.hcp053Mode = 'exact';
    window.hcp053Calls = [];
    window.nostr = {
      getPublicKey: () => {
        window.hcp053Calls.push('key');
        return Promise.resolve(f.owner);
      },
      signEvent: (template: Parameters<typeof f.sign>[0]) => {
        window.hcp053Calls.push('sign');
        return Promise.resolve(f.sign(template, window.hcp053Mode));
      }
    };
    window.hcp053Adapter = s.createExtensionAdapter();
    await s.connectExtensionAdapter(window.hcp053Adapter);
    const a = s.approveCapturedPublicSigning(
      f.record,
      f.owner,
      f.id,
      'reviewed_captured_operation'
    );
    if (!a) throw new Error('missing fixture approval');
    window.hcp053Approval = a;
    window.hcp053Calls = [];
  });
}
test.afterEach(async ({ page }) => {
  await page.evaluate(() => window.hcp053Fixture?.close());
});
test('exact actual SDK response returns immutable artifact and never publishes/stores', async ({
  page
}) => {
  await load(page);
  const value = await page.evaluate(async () => {
    const s = window.hcp053,
      result = await s.signApprovedExtensionAdapter(
        window.hcp053Adapter,
        window.hcp053Approval
      );
    return {
      status: result.status,
      author:
        result.status === 'signed'
          ? s.capturedArtifactSnapshot(result.artifact)?.author
          : null,
      expected: window.hcp053Fixture.owner,
      calls: window.hcp053Calls
    };
  });
  expect(value.status).toBe('signed');
  expect(value.author).toBe(value.expected);
  expect(value.calls).toEqual(['key', 'sign', 'key']);
});
for (const mode of [
  'author',
  'kind',
  'time',
  'tags',
  'content',
  'cached',
  'mutate',
  'denied'
] as const)
  test(`actual SDK hostile ${mode} is finite and returns no artifact`, async ({
    page
  }) => {
    await load(page);
    await page.evaluate((mode) => {
      window.hcp053Mode = mode;
    }, mode);
    expect(
      await page.evaluate(
        async () =>
          (
            await window.hcp053.signApprovedExtensionAdapter(
              window.hcp053Adapter,
              window.hcp053Approval
            )
          ).status
      )
    ).toBe(mode === 'denied' ? 'refused' : 'mismatch');
  });
test('unsettled signing retains shared slot and disconnect fences late exact response', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp053Fixture;
    window.nostr = {
      getPublicKey: () => {
        window.hcp053Calls.push('key');
        return Promise.resolve(f.owner);
      },
      signEvent: (template: Parameters<typeof f.sign>[0]) => {
        window.hcp053Calls.push('sign');
        return new Promise((resolve) => {
          window.hcp053Release = () => resolve(f.sign(template));
        });
      }
    };
    window.hcp053Task = window.hcp053.signApprovedExtensionAdapter(
      window.hcp053Adapter,
      window.hcp053Approval
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp053Calls))
    .toEqual(['key', 'sign']);
  expect(
    await page.evaluate(
      async () =>
        (
          await window.hcp053.signApprovedExtensionAdapter(
            window.hcp053Adapter,
            window.hcp053Approval
          )
        ).status
    )
  ).toBe('busy');
  await page.evaluate(() =>
    window.hcp053.disconnectExtensionAdapter(window.hcp053Adapter)
  );
  expect(
    await page.evaluate(
      async () =>
        (
          await window.hcp053.connectExtensionAdapter(
            window.hcp053.createExtensionAdapter()
          )
        ).admission
    )
  ).toBe('busy');
  expect(
    await page.evaluate(async () => {
      window.hcp053Release();
      return (await window.hcp053Task).status;
    })
  ).toBe('unknown');
  expect(
    await page.evaluate(() =>
      window.hcp053.extensionSnapshot(window.hcp053Adapter)
    )
  ).toEqual({ state: 'guest', reason: 'disconnected' });
  expect(await page.evaluate(() => window.hcp053Calls)).toEqual([
    'key',
    'sign'
  ]);
});
for (const phase of ['before', 'after'] as const)
  test(`fresh ${phase} owner change prevents artifact admission`, async ({
    page
  }) => {
    await load(page);
    const outcome = await page.evaluate(async (phase) => {
      const s = window.hcp053,
        f = window.hcp053Fixture,
        other = s.makeFixture();
      let keys = 0;
      try {
        window.nostr = {
          getPublicKey: () => {
            window.hcp053Calls.push('key');
            keys++;
            return Promise.resolve(
              phase === 'before' || keys > 1 ? other.owner : f.owner
            );
          },
          signEvent: (template: Parameters<typeof f.sign>[0]) => {
            window.hcp053Calls.push('sign');
            return Promise.resolve(f.sign(template));
          }
        };
        const result = await s.signApprovedExtensionAdapter(
          window.hcp053Adapter,
          window.hcp053Approval
        );
        return {
          status: result.status,
          calls: window.hcp053Calls,
          snapshot: s.extensionSnapshot(window.hcp053Adapter)
        };
      } finally {
        other.close();
      }
    }, phase);
    expect(outcome.status).toBe(phase === 'before' ? 'stale' : 'unknown');
    expect(outcome.calls).toEqual(
      phase === 'before' ? ['key'] : ['key', 'sign', 'key']
    );
    expect(outcome.snapshot).toEqual({ state: 'guest', reason: 'changed_key' });
  });
