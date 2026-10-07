import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Setup from './harness/inbox-setup.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';
import type { InboxSetupReview } from '../../src/lib/messaging/inbox-setup.ts';
declare global {
  interface Window {
    hcp065: typeof Setup;
    hcp065Fixture: Awaited<ReturnType<typeof Setup.makeSetupFixture>>;
    hcp065Db: BrowserDatabase;
    hcp065Repo: PublicQuotaRepository;
    hcp065Review: InboxSetupReview;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const entry = fileURLToPath(
    new URL('./harness/inbox-setup.ts', import.meta.url)
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: { entry, name: 'hcp065', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing fixture output');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP065_ACTUAL_SDK_OWNERS_AND_REAL_IDB_WEB_LOCKS',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'controlled local Chromium/source only; no real extension/relay/client/retention qualification'
    })
  );
});
test.afterAll(async () => server.close());
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    const s = window.hcp065;
    window.hcp065Fixture = await s.makeSetupFixture();
    const opened = await s.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    window.hcp065Db = opened.owner;
    const repo = s.createPublicQuotaRepository(
      opened.owner,
      window.hcp065Fixture.owner
    );
    if (!repo) throw new Error('missing owner repository');
    window.hcp065Repo = repo;
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp065Fixture?.close();
      if (window.hcp065Db) window.hcp065.closeBrowserDatabase(window.hcp065Db);
    });
});
test('Connect and review alone write nothing; explicit durable capture equals the exact preview', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const s = window.hcp065,
      f = window.hcp065Fixture,
      review = await s.reviewInboxSetup(
        f.identity,
        f.own.resolver,
        s.fixturePolicy(),
        s.setupInput(),
        () => 101
      );
    if (review.status !== 'review') throw new Error(review.reason);
    const preview = s.inboxSetupPreview(review.review)!,
      before = await s.inspectPublicStorage(window.hcp065Repo);
    const count = before.ok
      ? s.publicInventorySnapshot(window.hcp065Repo, before.value)?.rows.length
      : -1;
    const id = crypto.randomUUID(),
      saved = await s.saveInboxPreferenceOperation(
        window.hcp065Repo,
        review.review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      );
    return {
      count,
      preview,
      status: saved.status,
      row:
        saved.status === 'saved'
          ? s.publicRecordSnapshot(saved.record, f.owner, id)
          : null,
      counts: f.counts()
    };
  });
  expect(r.count).toBe(0);
  expect(r.status).toBe('saved');
  expect(r.row?.family).toBe('preference_operation');
  if (r.row?.family === 'preference_operation') {
    expect(r.row.capture.wire).toBe(r.preview.wire);
    expect(r.row.source.wire).toBe(r.preview.originalWire);
    expect(r.row.capture.targets).toEqual(r.preview.destinations);
    expect(r.row.artifact).toBeNull();
    expect(r.row.receipts).toEqual([]);
  }
  expect(r.counts.signs).toBe(0);
});
test('same command replay retains original; conflicting preview never overwrites it', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const s = window.hcp065,
      f = window.hcp065Fixture,
      id = crypto.randomUUID();
    const first = await s.reviewInboxSetup(
      f.identity,
      f.own.resolver,
      s.fixturePolicy(),
      s.setupInput(),
      () => 101
    );
    if (first.status !== 'review') throw new Error(first.reason);
    const a = await s.saveInboxPreferenceOperation(
        window.hcp065Repo,
        first.review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      ),
      b = await s.saveInboxPreferenceOperation(
        window.hcp065Repo,
        first.review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      );
    const different = await s.reviewInboxSetup(
      f.identity,
      f.own.resolver,
      s.fixturePolicy(),
      s.setupInput({ createdAt: 102 }),
      () => 102
    );
    if (different.status !== 'review') throw new Error(different.reason);
    const c = await s.saveInboxPreferenceOperation(
      window.hcp065Repo,
      different.review,
      f.own.resolver,
      id,
      'reviewed_global_inbox_replacement'
    );
    return {
      a: a.status,
      b: b.status,
      c: c.status,
      original:
        a.status === 'saved' ? s.publicRecordWire(a.record, f.owner, id) : null,
      retained:
        b.status === 'existing'
          ? s.publicRecordWire(b.record, f.owner, id)
          : null,
      counts: f.counts()
    };
  });
  expect(r.a).toBe('saved');
  expect(r.b).toBe('existing');
  expect(r.c).toBe('conflict');
  expect(r.retained).toBe(r.original);
  expect(r.counts.signs).toBe(0);
});
test('changed known head prevents durable admission before any extension or publication effect', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const s = window.hcp065,
      f = window.hcp065Fixture,
      r = await s.reviewInboxSetup(
        f.identity,
        f.own.resolver,
        s.fixturePolicy(),
        s.setupInput(),
        () => 101
      );
    if (r.status !== 'review') throw new Error(r.reason);
    const fresh = f.resolve(f.newerEvent);
    const saved = await s.saveInboxPreferenceOperation(
      window.hcp065Repo,
      r.review,
      fresh.resolver,
      crypto.randomUUID(),
      'reviewed_global_inbox_replacement'
    );
    const inv = await s.inspectPublicStorage(window.hcp065Repo);
    return {
      status: saved.status,
      count: inv.ok
        ? s.publicInventorySnapshot(window.hcp065Repo, inv.value)?.rows.length
        : -1,
      counts: f.counts()
    };
  });
  expect(r.status).toBe('conflict');
  expect(r.count).toBe(0);
  expect(r.counts.signs).toBe(0);
});
test('missing Web Locks or consent keeps browse usable and creates no preference', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const s = window.hcp065,
      f = window.hcp065Fixture,
      r = await s.reviewInboxSetup(
        f.identity,
        f.own.resolver,
        s.fixturePolicy(),
        s.setupInput(),
        () => 101
      );
    if (r.status !== 'review') throw new Error(r.reason);
    const denied = await s.saveInboxPreferenceOperation(
      window.hcp065Repo,
      r.review,
      f.own.resolver,
      crypto.randomUUID(),
      'connect'
    );
    Object.defineProperty(navigator, 'locks', {
      configurable: true,
      value: undefined
    });
    const unsupported = await s.saveInboxPreferenceOperation(
      window.hcp065Repo,
      r.review,
      f.own.resolver,
      crypto.randomUUID(),
      'reviewed_global_inbox_replacement'
    );
    const inv = await s.inspectPublicStorage(window.hcp065Repo);
    return {
      denied: denied.status,
      unsupported: unsupported.status,
      count: inv.ok
        ? s.publicInventorySnapshot(window.hcp065Repo, inv.value)?.rows.length
        : -1
    };
  });
  expect(r).toEqual({
    denied: 'invalid',
    unsupported: 'unavailable',
    count: 0
  });
  expect(page.url()).toContain('/search');
});
