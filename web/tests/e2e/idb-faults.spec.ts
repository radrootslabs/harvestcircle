import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
import type * as Drafts from '../../src/lib/persistence/drafts.ts';
declare global {
  interface Window {
    hcp134: typeof Database & typeof Drafts;
    hcp134Database: Database.BrowserDatabase;
    hcp134Repository: Drafts.PublicDraftRepository;
    hcp134ExtensionReads: number;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const source = (name: string) =>
    fileURLToPath(
      new URL('../../src/lib/persistence/' + name, import.meta.url)
    );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [
      {
        name: 'hcp134-real-idb-entry',
        resolveId(id) {
          if (id === 'virtual:hcp134') return '\0hcp134';
        },
        load(id) {
          if (id === '\0hcp134')
            return ['database.ts', 'drafts.ts']
              .map((name) => `export * from ${JSON.stringify(source(name))};`)
              .join('');
        }
      }
    ],
    build: {
      write: false,
      minify: false,
      rolldownOptions: { input: 'virtual:hcp134' },
      lib: { entry: 'virtual:hcp134', name: 'hcp134', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output))
    throw Error('missing real IDB qualification bundle');
  const chunks = output.output.filter((entry) => entry.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP134_ACTUAL_DRAFT_IDB_FAULT_AND_PAGE_RELOAD',
      bundle_sha256: createHash('sha256').update(bundle).digest('hex'),
      modules: Object.keys(chunks[0].modules),
      scope:
        'Actual original database/draft modules in a test-only virtual entry; injected native-write faults, not physical host quota exhaustion or pipeline publication proof'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page, navigate = true) {
  if (navigate) await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (owner) => {
    const api = window.hcp134,
      opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw Error(opened.reason);
    const repository = api.createPublicDraftRepository(opened.owner, owner);
    if (!repository) throw Error('missing original owner draft repository');
    window.hcp134Database = opened.owner;
    window.hcp134Repository = repository;
  }, owner);
}
for (const boundary of ['create', 'save'] as const)
  for (const fault of ['abort', 'quota', 'unknown'] as const)
    test(`HCP134 real draft ${boundary} ${fault} preserves one exact original write through explicit retry and actual page reload`, async ({
      page
    }) => {
      let sockets = 0;
      page.on('websocket', () => sockets++);
      await page.addInitScript(() => {
        window.hcp134ExtensionReads = 0;
        Object.defineProperty(window, 'nostr', {
          configurable: true,
          get() {
            window.hcp134ExtensionReads++;
            throw Error('HCP134 unsolicited extension access');
          }
        });
      });
      await load(page);
      const result = await page.evaluate(
        async ({ boundary, fault }) => {
          const api = window.hcp134,
            repository = window.hcp134Repository;
          const form = {
            title: 'Public partial draft',
            description: 'HCP134_PUBLIC_DRAFT_UNFINISHED_STRINGS',
            location: '',
            amount: '.',
            currency: '',
            unit: '',
            quantity: '',
            contactType: '' as const,
            contactValue: ''
          };
          const base =
            boundary === 'save'
              ? await api.createPublicDraft(repository, form)
              : undefined;
          if (base && !base.ok) throw Error(base.reason);
          const next = {
            ...form,
            title: 'Acknowledged public edit',
            description: 'HCP134_PUBLIC_EXACT_CAPTURE_é😀',
            amount: '3.5'
          };
          const captured = base?.ok
            ? api.capturePublicDraftSave(
                repository,
                base.value.id,
                base.value.revision,
                next
              )
            : api.capturePublicDraftCreate(repository, next);
          if (!captured.ok) throw Error(captured.reason);
          const expected = api.publicDraftWriteSnapshot(
            repository,
            captured.value
          );
          if (!expected) throw Error('missing immutable original write');
          const nativePut = Reflect.get<IDBObjectStore, 'put'>(
              IDBObjectStore.prototype,
              'put'
            ),
            nativeAbort = Reflect.get<IDBTransaction, 'abort'>(
              IDBTransaction.prototype,
              'abort'
            );
          let hits = 0;
          IDBObjectStore.prototype.put = function (
            value: unknown,
            key?: IDBValidKey
          ) {
            if (this.name !== 'public_drafts' || hits > 0)
              return key === undefined
                ? nativePut.call(this, value)
                : nativePut.call(this, value, key);
            hits++;
            if (fault === 'quota')
              throw new DOMException(
                'Controlled native-write quota refusal',
                'QuotaExceededError'
              );
            const request =
              key === undefined
                ? nativePut.call(this, value)
                : nativePut.call(this, value, key);
            if (fault === 'abort')
              request.addEventListener(
                'success',
                () => nativeAbort.call(this.transaction),
                { once: true }
              );
            else
              return new Proxy(request, {
                get(target, property) {
                  if (property === 'addEventListener')
                    return (
                      ...args: Parameters<IDBRequest['addEventListener']>
                    ) => {
                      // Lose only the producer request-success callback. The real
                      // queued write and native transaction still complete normally.
                      if (args[0] !== 'success')
                        target.addEventListener(...args);
                    };
                  return Reflect.get(target, property, target) as unknown;
                }
              });
            return request;
          };
          let first: Awaited<ReturnType<typeof api.commitPublicDraftWrite>>;
          try {
            first = await api.commitPublicDraftWrite(
              repository,
              captured.value
            );
          } finally {
            IDBObjectStore.prototype.put = nativePut;
          }
          const observed = await api.observePublicDraftWrite(
            repository,
            captured.value
          );
          const beforeRetry = await api.listPublicDrafts(repository);
          let retryPuts = 0;
          IDBObjectStore.prototype.put = function (
            value: unknown,
            key?: IDBValidKey
          ) {
            if (this.name === 'public_drafts') retryPuts++;
            return key === undefined
              ? nativePut.call(this, value)
              : nativePut.call(this, value, key);
          };
          let retry: Awaited<ReturnType<typeof api.commitPublicDraftWrite>>;
          try {
            retry = await api.commitPublicDraftWrite(
              repository,
              captured.value
            );
          } finally {
            IDBObjectStore.prototype.put = nativePut;
          }
          const retained = await api.readPublicDraft(repository, expected.id),
            rows = await api.listPublicDrafts(repository);
          api.closeBrowserDatabase(window.hcp134Database);
          return {
            first,
            hits,
            expected,
            base: base?.ok ? base.value : null,
            observed,
            beforeRetry,
            retry,
            retryPuts,
            retained,
            rows
          };
        },
        { boundary, fault }
      );
      expect(result.hits).toBe(1);
      expect(result.first).toEqual({
        ok: false,
        reason: fault === 'unknown' ? 'unknown_completion' : 'aborted'
      });
      if (fault === 'unknown') {
        expect(result.observed).toEqual({
          state: 'committed',
          value: result.expected
        });
        expect(result.beforeRetry).toEqual({
          ok: true,
          value: [result.expected]
        });
        expect(result.retryPuts).toBe(0);
      } else {
        expect(result.observed).toEqual({ state: 'base_observed' });
        expect(result.beforeRetry).toEqual({
          ok: true,
          value: result.base ? [result.base] : []
        });
        expect(result.retryPuts).toBe(1);
      }
      expect(result.retry).toEqual({ ok: true, value: result.expected });
      expect(result.retained).toEqual(result.retry);
      expect(result.rows).toEqual({ ok: true, value: [result.expected] });
      expect(await page.evaluate(() => window.hcp134ExtensionReads)).toBe(0);
      await page.reload();
      await load(page, false);
      const reopened = await page.evaluate(async (id) => {
        const api = window.hcp134,
          repository = window.hcp134Repository;
        try {
          return {
            row: await api.readPublicDraft(repository, id),
            rows: await api.listPublicDrafts(repository)
          };
        } finally {
          api.closeBrowserDatabase(window.hcp134Database);
        }
      }, result.expected.id);
      expect(reopened.row).toEqual({ ok: true, value: result.expected });
      expect(reopened.rows).toEqual({ ok: true, value: [result.expected] });
      expect(sockets).toBe(0);
      expect(await page.evaluate(() => window.hcp134ExtensionReads)).toBe(0);
    });
