import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/unlocked-session.ts';
declare global {
  interface Window {
    hcp093: typeof Fixture;
    hcp093Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
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
          new URL('./harness/unlocked-session.ts', import.meta.url)
        ),
        name: 'hcp093',
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
    window.hcp093Fixture = await window.hcp093.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp093Fixture?.close());
});
test('cache capture requires explicit reviewed action and performs no extension or store effect', async ({
  page
}) => {
  const r = await page.evaluate(() => {
    const f = window.hcp093Fixture,
      before = f.counts();
    return {
      capture: f.captureCache('unreviewed'),
      messages: f.messages(),
      before,
      after: f.counts()
    };
  });
  expect(r.capture).toBe(false);
  expect(r.messages).toBeUndefined();
  expect(r.after).toEqual(r.before);
});
test('only actual admitted room enters current cache without another SDK operation', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    const before = f.counts();
    return {
      result: f.cache(),
      messages: f.messages(),
      snapshot: f.cacheSnapshot(),
      before,
      after: f.counts(),
      peer: f.peer,
      id: f.rumorId
    };
  });
  expect(r.result).toBe('added');
  expect(r.messages?.status).toBe('ready');
  expect(r.messages?.messages).toHaveLength(1);
  expect(r.messages?.messages[0]).toMatchObject({
    peer: r.peer,
    rumorId: r.id,
    role: 'inbound'
  });
  expect(r.snapshot).toMatchObject({ closed: false, count: 1 });
  expect(r.after).toEqual(r.before);
});
test('multiple genuine encrypted wraps of same immutable rumor create one logical projection', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    const first = f.cache();
    const next = await f.anotherWrap();
    return {
      first,
      next,
      second: f.cache(),
      messages: f.messages(),
      snapshot: f.cacheSnapshot(),
      id: f.rumorId
    };
  });
  expect(r.first).toBe('added');
  expect(r.second).toBe('duplicate');
  expect(r.next.rumorId).toBe(r.id);
  expect(r.messages?.messages).toHaveLength(1);
  expect(r.snapshot?.count).toBe(1);
});
test('mutating detached returned projection cannot alias a different sender or rumor', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    const copy = f.messages();
    if (copy?.status === 'ready') {
      const row = copy.messages[0] as unknown as {
        peer: string;
        rumorId: string;
        content: string;
      };
      row.peer = f.other;
      row.rumorId = '0'.repeat(64);
      row.content = 'MUTATED_CALLER_COPY';
    }
    return { fresh: f.messages(), peer: f.peer, id: f.rumorId };
  });
  expect(r.fresh?.messages[0]).toMatchObject({
    peer: r.peer,
    rumorId: r.id,
    content: 'Actual encrypted room fixture'
  });
});
test('forged and copied room capabilities cannot populate cache', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    return {
      fake: f.forgedCache(),
      copy: f.copiedCache(),
      messages: f.messages(),
      snapshot: f.cacheSnapshot()
    };
  });
  expect(r.fake).toBe('rejected');
  expect(r.copy).toBe('rejected');
  expect(r.messages?.messages).toEqual([]);
  expect(r.snapshot?.count).toBe(0);
});
test('same owner with different actual identity generation cannot import old room proof', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    return { other: await f.otherGeneration(), original: f.cache() };
  });
  expect(r.other.accepted).toBe('rejected');
  expect(r.other.snapshot.count).toBe(0);
  expect(r.original).toBe('added');
});
test('logout clears reachable projections and rejects late original admission', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    f.disconnect();
    return {
      messages: f.messages(),
      snapshot: f.cacheSnapshot(),
      raw: f.rawCacheSnapshot(),
      late: f.cache()
    };
  });
  expect(r.messages).toMatchObject({ status: 'closed', messages: [] });
  expect(r.snapshot).toMatchObject({ closed: true, count: 0, bytes: 0 });
  expect(r.late).toBe('closed');
  expect(r.raw).toMatchObject({ closed: true, count: 0, bytes: 0 });
});
test('explicit cache Stop discards preview and does not cancel or repeat provider calls', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    const before = f.counts();
    f.closeCache();
    return {
      messages: f.messages(),
      snapshot: f.cacheSnapshot(),
      before,
      after: f.counts()
    };
  });
  expect(r.messages).toMatchObject({ status: 'closed', messages: [] });
  expect(r.snapshot).toMatchObject({ closed: true, count: 0, bytes: 0 });
  expect(r.after).toEqual(r.before);
});
test('original private-session close clears cache while public identity may remain connected', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    f.closePrivate();
    return { messages: f.messages(), snapshot: f.cacheSnapshot() };
  });
  expect(r.messages).toMatchObject({ status: 'closed', messages: [] });
  expect(r.snapshot?.count).toBe(0);
});
test('actual fresh changed extension owner invalidates generation and cached preview', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    await f.changedOwner();
    return { messages: f.messages(), snapshot: f.cacheSnapshot() };
  });
  expect(r.messages).toMatchObject({ status: 'closed', messages: [] });
  expect(r.snapshot?.count).toBe(0);
});
test('actual scoped cache quota refuses new verified projection without eviction or automatic reset', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    return {
      quota: f.quota(),
      result: f.cache(),
      raw: f.rawCacheSnapshot(),
      snapshot: f.cacheSnapshot(),
      messages: f.messages()
    };
  });
  expect(r.quota).toBe(2000);
  expect(r.result).toBe('limit');
  expect(r.raw).toMatchObject({ closed: false, count: 2000, bytes: 4000 });
  expect(r.snapshot?.count).toBe(0);
  expect(r.messages?.messages).toEqual([]);
});
test('poisoned existing serialized cache cannot substitute another peer under a verified rumor hash', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp093Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    f.cache();
    return { injected: f.corruptCachedWire(), messages: f.messages() };
  });
  expect(r.injected).toBe('accepted');
  expect(r.messages).toMatchObject({ status: 'conflict', messages: [] });
});
