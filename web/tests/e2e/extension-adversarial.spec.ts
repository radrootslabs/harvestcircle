import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as PublicFixture from './harness/sign-listing.ts';
import type * as PrivateFixture from './harness/private-preparation.ts';
declare global {
  interface Window {
    hcp136Public: typeof PublicFixture;
    hcp136Private: typeof PrivateFixture;
    hcp136LegacyReads: number;
    hcp136SignAttempts: number;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
const bundles = new Map<'public' | 'private', string>();
test.beforeAll(async () => {
  server = await createStaticHarness();
  for (const [role, entry, name] of [
    ['public', './harness/sign-listing.ts', 'hcp136Public'],
    ['private', './harness/private-preparation.ts', 'hcp136Private']
  ] as const) {
    const result = await build({
      configFile: false,
      logLevel: 'silent',
      build: {
        write: false,
        minify: false,
        lib: {
          entry: fileURLToPath(new URL(entry, import.meta.url)),
          name,
          formats: ['iife']
        }
      }
    });
    const output = Array.isArray(result) ? result[0] : result;
    if (!('output' in output)) throw Error('missing fixture bundle');
    const chunks = output.output.filter((item) => item.type === 'chunk');
    expect(chunks).toHaveLength(1);
    bundles.set(role, chunks[0].code);
    console.log(
      JSON.stringify({
        fixture: 'HCP136_HOSTILE_PROVIDER_ORIGINAL_CONSUMER',
        role,
        bundleSha256: createHash('sha256').update(chunks[0].code).digest('hex'),
        moduleIds: Object.keys(chunks[0].modules),
        qualification:
          'Actual Chromium/native storage/original SDK controlled providers; no installed extension, external client or operator Q.'
      })
    );
  }
});
test.afterAll(async () => server.close());
async function load(page: Page, role: 'public' | 'private') {
  await page.goto(server.url + '/search');
  const bundle = bundles.get(role);
  if (!bundle) throw Error('missing actual bundle');
  await page.addScriptTag({ content: bundle });
}
test('hostile signature denial leaves original captured intent without artifact or automatic prompt retry', async ({
  page
}) => {
  await load(page, 'public');
  const result = await page.evaluate(async () => {
    const f = await window.hcp136Public.createFixture();
    window.hcp136SignAttempts = 0;
    const provider = window.nostr as
      { signEvent(): Promise<never> } | undefined;
    if (!provider) throw Error('missing controlled provider');
    provider.signEvent = () => {
      window.hcp136SignAttempts++;
      return Promise.reject(Error('HOSTILE_PROVIDER_PRIVATE_ERROR_SENTINEL'));
    };
    try {
      const r = await f.exact();
      return { r, attempts: window.hcp136SignAttempts };
    } finally {
      f.close();
    }
  });
  expect(result.r.status).not.toBe('prepared');
  expect(result.r.retry).not.toBe('prepared');
  expect(result.r.same).toBe(true);
  expect(result.r.revision).toBe(1);
  expect(result.r.artifact).toBeNull();
  expect(result.r.plan).toBeUndefined();
  expect(result.r.originalCapture).toBe(true);
  expect(result.attempts).toBe(1);
  expect(JSON.stringify(result)).not.toContain(
    'HOSTILE_PROVIDER_PRIVATE_ERROR_SENTINEL'
  );
});
for (const mode of ['declined', 'missing'] as const)
  test(`${mode} original self encryption never inspects legacy fallback or persists plaintext`, async ({
    page
  }) => {
    await load(page, 'private');
    const result = await page.evaluate(async (mode) => {
      const s = window.hcp136Private,
        f = await s.makeFixture();
      const provider = window.nostr;
      if (!provider) throw Error('missing controlled provider');
      window.hcp136LegacyReads = 0;
      Object.defineProperty(provider, 'nip04', {
        configurable: true,
        get() {
          window.hcp136LegacyReads++;
          throw Error('legacy fallback forbidden');
        }
      });
      const before = f.counts(),
        stored = await f.stored();
      f.mode(mode);
      try {
        const first = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
        const afterFirst = f.counts();
        const retry = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
        return {
          first: first.status,
          retry: retry.status,
          before,
          afterFirst,
          after: f.counts(),
          legacy: window.hcp136LegacyReads,
          acknowledged: !!s.preparedSelfRecovery(f.preparation),
          composer: s.privateComposerSnapshot(f.composer),
          unchanged: stored === (await f.stored()),
          plaintext: (await f.stored()).includes(f.marker)
        };
      } finally {
        f.close();
      }
    }, mode);
    expect(result.acknowledged).toBe(false);
    expect(result.composer?.state).not.toBe('saved_encrypted');
    if (mode === 'missing') expect(result.composer).toBeUndefined();
    else expect(result.composer?.dirty).toBe(true);
    expect(result.legacy).toBe(0);
    expect(result.unchanged).toBe(true);
    expect(result.plaintext).toBe(false);
    expect(result.after.signs).toBe(result.before.signs);
    expect(result.after.decrypts).toBe(result.before.decrypts);
    expect(result.afterFirst.encrypts - result.before.encrypts).toBe(
      mode === 'missing' ? 0 : 1
    );
    expect(result.after.encrypts - result.afterFirst.encrypts).toBe(
      mode === 'missing' ? 0 : 1
    );
  });
