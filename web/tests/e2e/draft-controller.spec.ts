import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
import type * as Controller from '../../src/lib/publishing/draft-controller.ts';
import type * as Drafts from '../../src/lib/persistence/drafts.ts';
declare global {
  interface Window {
    hcp048: typeof Database & typeof Drafts & typeof Controller;
    hcp048Database: Database.BrowserDatabase;
    hcp048Repository: Drafts.PublicDraftRepository;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
const form = {
  title: '',
  description: 'unfinished',
  location: '',
  amount: '.',
  currency: '',
  unit: '',
  quantity: '',
  contactType: '' as const,
  contactValue: ''
};
let server: Awaited<ReturnType<typeof createStaticHarness>>;
let bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const source = (name: string) =>
    fileURLToPath(
      new URL('../../src/lib/persistence/' + name, import.meta.url)
    );
  // Compile the exact current production modules together so both use the same
  // genuine WeakMap database ownership. This virtual entry is test-only.
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [
      {
        name: 'hcp048-test-entry',
        resolveId(id) {
          if (id === 'virtual:hcp048') return '\0hcp048';
        },
        load(id) {
          if (id === '\0hcp048')
            return `export * from ${JSON.stringify(source('database.ts'))};export * from ${JSON.stringify(source('drafts.ts'))};export * from ${JSON.stringify(source('../publishing/draft-controller.ts'))};`;
        }
      }
    ],
    build: {
      write: false,
      minify: false,
      rolldownOptions: { input: 'virtual:hcp048' },
      lib: { entry: 'virtual:hcp048', name: 'hcp048', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing test bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP048_ACTUAL_MODULES_REAL_IDB',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      exposure: 'test-only virtual entry, no production route or dependency'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page, selectedOwner = owner) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (expectedOwner) => {
    const api = window.hcp048;
    const opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    const repository = api.createPublicDraftRepository(
      opened.owner,
      expectedOwner
    );
    if (!repository) throw new Error('missing repository');
    window.hcp048Database = opened.owner;
    window.hcp048Repository = repository;
  }, selectedOwner);
}
test('immutable capture replays the exact committed ID/time/form once and denies foreign handles', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(
    async ({ form, peer }) => {
      const api = window.hcp048,
        repository = window.hcp048Repository;
      const captured = api.capturePublicDraftCreate(repository, form);
      if (!captured.ok) throw new Error(captured.reason);
      const before = api.publicDraftWriteSnapshot(repository, captured.value);
      if (!before) throw new Error('missing snapshot');
      const immutable = JSON.stringify(before);
      const mutable = before.form as { title: string };
      mutable.title = 'foreign';
      const absent = await api.observePublicDraftWrite(
        repository,
        captured.value
      );
      const first = await api.commitPublicDraftWrite(
        repository,
        captured.value
      );
      const replay = await api.commitPublicDraftWrite(
        repository,
        captured.value
      );
      const observed = await api.observePublicDraftWrite(
        repository,
        captured.value
      );
      const other = api.createPublicDraftRepository(
        window.hcp048Database,
        peer
      );
      if (!other) throw new Error('missing foreign namespace');
      const foreign = await api.commitPublicDraftWrite(other, captured.value);
      const rows = await api.listPublicDrafts(repository);
      return { immutable, absent, first, replay, observed, foreign, rows };
    },
    { form, peer }
  );
  expect(result.absent).toEqual({ state: 'base_observed' });
  expect(result.first).toEqual({
    ok: true,
    value: JSON.parse(
      result.immutable
    ) as import('../../src/lib/contracts/local-records.ts').PublicDraftRecord
  });
  expect(result.replay).toEqual(result.first);
  expect(result.observed).toEqual({
    state: 'committed',
    value: JSON.parse(
      result.immutable
    ) as import('../../src/lib/contracts/local-records.ts').PublicDraftRecord
  });
  expect(result.foreign).toEqual({ ok: false, reason: 'invalid_scope' });
  expect(result.rows).toEqual({
    ok: true,
    value: [
      JSON.parse(
        result.immutable
      ) as import('../../src/lib/contracts/local-records.ts').PublicDraftRecord
    ]
  });
});
test('genuine autosave coalesces pending edits, resumes durable input, and flushes before save-close', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp048,
      repository = window.hcp048Repository;
    const controller = await api.createPublicDraftController(repository, form);
    if (!controller) throw new Error('missing controller');
    api.editPublicDraft(controller, { ...form, title: 'first' });
    api.editPublicDraft(controller, { ...form, title: 'latest', amount: '1.' });
    const flushed = await api.flushPublicDraft(controller);
    const saved = api.publicDraftControllerSnapshot(controller);
    if (!saved?.draft) throw new Error('missing saved draft');
    const resumed = await api.createPublicDraftController(
      repository,
      form,
      saved.draft.id
    );
    if (!resumed) throw new Error('missing resumed controller');
    api.editPublicDraft(resumed, {
      ...saved.form,
      description: 'newer partial'
    });
    const closed = await api.saveAndClosePublicDraft(resumed);
    const final = api.publicDraftControllerSnapshot(resumed);
    const retained = await api.readPublicDraft(repository, saved.draft.id);
    const late = api.editPublicDraft(resumed, form);
    return { flushed, saved, closed, final, retained, late };
  }, form);
  expect(result.flushed).toBe(true);
  expect(result.saved).toMatchObject({
    state: 'saved',
    editVersion: 2,
    savedEditVersion: 2,
    draft: { revision: 0 },
    form: { title: 'latest', amount: '1.' }
  });
  expect(result.closed).toBe(true);
  expect(result.final).toMatchObject({
    state: 'closed',
    draft: { revision: 1, form: { description: 'newer partial' } }
  });
  expect(result.retained).toEqual({ ok: true, value: result.final?.draft });
  expect(result.late).toBe(false);
});
test('owner capacity refusal retains dirty input and prevents save-close without deleting drafts', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp048,
      repository = window.hcp048Repository;
    for (let i = 0; i < 20; i++) {
      const created = await api.createPublicDraft(repository, form);
      if (!created.ok) throw new Error(created.reason);
    }
    const controller = await api.createPublicDraftController(repository, form);
    if (!controller) throw new Error('missing controller');
    api.editPublicDraft(controller, { ...form, title: 'retain dirty input' });
    const closed = await api.saveAndClosePublicDraft(controller);
    const snapshot = api.publicDraftControllerSnapshot(controller);
    const rows = await api.listPublicDrafts(repository);
    return { closed, snapshot, count: rows.ok ? rows.value.length : -1 };
  }, form);
  expect(result.closed).toBe(false);
  expect(result.snapshot).toMatchObject({
    state: 'failed',
    failure: 'cap_reached',
    form: { title: 'retain dirty input' },
    savedEditVersion: -1,
    draft: null
  });
  expect(result.count).toBe(20);
});
test('an advanced competing revision conflicts with the original capture and keeps the current input', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp048,
      repository = window.hcp048Repository;
    const created = await api.createPublicDraft(repository, form);
    if (!created.ok) throw new Error(created.reason);
    const capture = api.capturePublicDraftSave(
      repository,
      created.value.id,
      0,
      { ...form, title: 'attempted' }
    );
    if (!capture.ok) throw new Error(capture.reason);
    const competitor = await api.savePublicDraft(
      repository,
      created.value.id,
      0,
      { ...form, title: 'competitor' }
    );
    const observed = await api.observePublicDraftWrite(
      repository,
      capture.value
    );
    const commit = await api.commitPublicDraftWrite(repository, capture.value);
    const retained = await api.readPublicDraft(repository, created.value.id);
    return { competitor, observed, commit, retained };
  }, form);
  expect(result.competitor.ok).toBe(true);
  expect(result.observed).toEqual({ state: 'conflict' });
  expect(result.commit).toEqual({ ok: false, reason: 'conflict' });
  expect(result.retained).toEqual(result.competitor);
});
test('private-shaped input is denied and closing the database invalidates genuine captures and workers', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp048,
      repository = window.hcp048Repository;
    const denied = await api.createPublicDraftController(repository, {
      body: 'private',
      subject: 'private'
    });
    const controller = await api.createPublicDraftController(repository, form);
    const captured = api.capturePublicDraftCreate(repository, form);
    if (!controller || !captured.ok) throw new Error('missing genuine handles');
    api.closeBrowserDatabase(window.hcp048Database);
    const commit = await api.commitPublicDraftWrite(repository, captured.value);
    const flush = await api.flushPublicDraft(controller);
    const snapshot = api.publicDraftControllerSnapshot(controller);
    return { denied: denied === undefined, commit, flush, snapshot };
  }, form);
  expect(result.denied).toBe(true);
  expect(result.commit).toEqual({ ok: false, reason: 'invalid_scope' });
  expect(result.flush).toBe(false);
  expect(result.snapshot).toMatchObject({
    state: 'failed',
    failure: 'invalid_scope',
    draft: null
  });
});

test('real repository clock reentry cannot acknowledge an older payload as the latest input', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp048,
      repository = window.hcp048Repository;
    const controller = await api.createPublicDraftController(repository, form);
    if (!controller) throw new Error('missing controller');
    const original = Date.now;
    let calls = 0;
    Date.now = () => {
      calls++;
      if (calls === 1)
        api.editPublicDraft(controller, {
          ...form,
          title: 'clock reentrant latest'
        });
      return original();
    };
    try {
      api.editPublicDraft(controller, {
        ...form,
        title: 'older captured form'
      });
      const flushed = await api.flushPublicDraft(controller);
      const snapshot = api.publicDraftControllerSnapshot(controller);
      const rows = await api.listPublicDrafts(repository);
      return { flushed, snapshot, rows, calls };
    } finally {
      Date.now = original;
    }
  }, form);
  expect(result.flushed).toBe(true);
  expect(result.calls).toBe(2);
  expect(result.snapshot).toMatchObject({
    state: 'saved',
    editVersion: 2,
    savedEditVersion: 2,
    form: { title: 'clock reentrant latest' },
    draft: { revision: 1, form: { title: 'clock reentrant latest' } }
  });
  expect(result.rows).toEqual({ ok: true, value: [result.snapshot?.draft] });
});
