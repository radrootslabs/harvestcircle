import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/message-metadata.ts';
declare global {
  interface Window {
    hcp094: typeof Fixture;
    hcp094Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
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
          new URL('./harness/message-metadata.ts', import.meta.url)
        ),
        name: 'hcp094',
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
    window.hcp094Fixture = await window.hcp094.makeFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp094Fixture?.close());
});
async function ready(page: import('@playwright/test').Page, cache = true) {
  return await page.evaluate(async (cache) => {
    const f = window.hcp094Fixture;
    await f.unlock();
    f.admit('inbound');
    f.captureCache();
    if (cache) f.cache();
    return await f.captureMetadata();
  }, cache);
}
test('metadata requires reviewed owner-generation scope without signing or persistence on capture', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    f.captureCache();
    const before = f.counts();
    const rejected = await f.captureMetadata('unreviewed');
    return { rejected, before, after: f.counts() };
  });
  expect(r.rejected).toBe(false);
  expect(r.after).toEqual(r.before);
});
test('unlock and fetch never mark a verified old rumor read', async ({
  page
}) => {
  expect(await ready(page)).toBe(true);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    return { unread: await f.unread(), raw: await f.rawMetadata() };
  });
  expect(r.unread).toMatchObject({ status: 'ready', count: 1 });
  expect(r.raw.pairs).toEqual([]);
  const rows = r.raw.received as { wire: string }[];
  expect(
    rows.map((x) => (JSON.parse(x.wire) as { read: unknown }).read)
  ).toEqual([null]);
});
test('two actual encrypted wraps of one rumor retain exactly one local unread', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    await f.anotherWrap();
    f.cache();
    return { unread: await f.unread(), raw: await f.rawMetadata() };
  });
  expect(r.unread).toMatchObject({ status: 'ready', count: 1 });
  expect(r.raw.received).toHaveLength(2);
});
test('only explicit displayed action persists minimum read and opaque pair fields without more SDK calls', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    const before = f.counts();
    return {
      save: await f.display(),
      unread: await f.unread(),
      raw: await f.rawMetadata(),
      before,
      after: f.counts(),
      id: f.rumorId,
      owner: f.owner,
      peer: f.peer
    };
  });
  expect(r.save.status).toBe('saved');
  expect(r.unread).toMatchObject({ status: 'ready', count: 0 });
  expect(r.after).toEqual(r.before);
  const received = JSON.parse(
    (r.raw.received as { wire: string }[])[0].wire
  ) as { read: { rumorHash: string; atMilliseconds: number } };
  expect(received.read.rumorHash).toBe(r.id);
  expect(Object.keys(received.read).sort()).toEqual([
    'atMilliseconds',
    'rumorHash'
  ]);
  expect(received.read.atMilliseconds).toBeGreaterThan(1700000000000);
  const pair = JSON.parse((r.raw.pairs as { wire: string }[])[0].wire) as {
    id: string;
    owner: string;
    peer: string;
    family: string;
    schema: number;
  };
  expect(Object.keys(pair).sort()).toEqual([
    'family',
    'id',
    'owner',
    'peer',
    'schema'
  ]);
  expect(pair).toMatchObject({
    owner: r.owner,
    peer: r.peer,
    family: 'conversation_handle',
    schema: 1
  });
  expect(pair.id).toMatch(/^[0-9a-f-]{36}$/);
  expect(JSON.stringify(r.raw)).not.toContain('Actual encrypted room fixture');
});
test('read fact suppresses later different wrap of same actual rumor', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    await f.display();
    await f.anotherWrap();
    f.cache();
    return {
      unread: await f.unread(),
      save: await f.display(),
      raw: await f.rawMetadata()
    };
  });
  expect(r.unread).toMatchObject({ status: 'ready', count: 0 });
  expect(r.save.status).toBe('saved');
  expect(r.raw.pairs).toHaveLength(1);
});
test('repeating explicit display reuses exact mapping and immutable first read time', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    const first = await f.display(),
      before = await f.rawMetadata(),
      second = await f.display(),
      after = await f.rawMetadata();
    return { first, second, before, after };
  });
  expect(r.first.status).toBe('saved');
  expect(r.second.status).toBe('existing');
  expect(r.after).toEqual(r.before);
});
test('unreviewed display refuses read and mapping writes', async ({ page }) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    return {
      save: await f.display('fetched_message'),
      unread: await f.unread(),
      raw: await f.rawMetadata()
    };
  });
  expect(r.save.status).toBe('invalid');
  expect(r.unread).toMatchObject({ count: 1 });
  expect(r.raw.pairs).toEqual([]);
});
test('forged and copied room custody cannot persist a local read', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    return {
      fake: await f.forgedDisplay(),
      copy: await f.copiedDisplay(),
      raw: await f.rawMetadata()
    };
  });
  expect(r.fake.status).toBe('invalid');
  expect(r.copy.status).toBe('invalid');
  expect(r.raw.pairs).toEqual([]);
});
test('authenticated room outside the accepted unlocked projection cannot mark read', async ({
  page
}) => {
  await ready(page, false);
  const r = await page.evaluate(() => window.hcp094Fixture.display());
  expect(r.status).toBe('invalid');
});
test('other actual owner has no prior read or mapping state', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    await f.display();
    return await f.otherOwnerMetadata();
  });
  expect(r).toMatchObject({ status: 'ready', readRumors: [], pairs: [] });
});
test('same owner different actual generation cannot capture original cache metadata authority', async ({
  page
}) => {
  await ready(page);
  expect(
    await page.evaluate(() => window.hcp094Fixture.wrongGenerationMetadata())
  ).toBe(false);
});
test('actual IndexedDB put abort grants no read or mapping credit and preserves unread', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    const restore = f.abortNextWrite();
    let save;
    try {
      save = await f.display();
    } finally {
      restore();
    }
    return { save, unread: await f.unread(), raw: await f.rawMetadata() };
  });
  expect(r.save.status).toBe('aborted');
  expect(r.unread).toMatchObject({ count: 1 });
  expect(r.raw.pairs).toEqual([]);
});
test('logout while owner lock is unavailable admits no late metadata write', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    return { save: await f.queuedLogout(), raw: await f.rawMetadata() };
  });
  expect(['busy', 'stopped']).toContain(r.save.status);
  expect(r.raw.pairs).toEqual([]);
});
test('unknown pair field fails closed without clearing corrupt or ciphertext evidence', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    await f.corruptPair();
    const before = await f.rawMetadata();
    return {
      save: await f.display(),
      snapshot: await f.metadataSnapshot(),
      before,
      after: await f.rawMetadata()
    };
  });
  expect(r.save.status).toBe('corrupt_record');
  expect(r.snapshot?.status).toBe('corrupt_record');
  expect(r.after).toEqual(r.before);
});
test('disclosure accurately names local peer and use-time exposure without encrypted-device or remote-read claims', async ({
  page
}) => {
  const text = await page.evaluate(() => window.hcp094Fixture.disclosure());
  expect(text).toContain('peers');
  expect(text).toContain('use times');
  expect(text).toContain('local');
  expect(text).toContain('not full-device encryption');
  expect(text).toContain('No remote Seen/Read');
});

test('actual reload preserves local read across a newly encrypted wrap of the same verified rumor', async ({
  page
}) => {
  await ready(page);
  expect(
    (await page.evaluate(() => window.hcp094Fixture.display())).status
  ).toBe('saved');
  await page.evaluate(() => window.hcp094Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp094Fixture = await window.hcp094.makeFixture('inbound', false);
  });
  await ready(page);
  expect(
    await page.evaluate(() => window.hcp094Fixture.unread())
  ).toMatchObject({ status: 'ready', count: 0 });
});
test('a genuinely decrypted delayed old rumor remains new locally after another rumor was read', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture;
    await f.display();
    const next = await f.anotherWrap(true),
      admit = f.cache();
    return { next, admit, unread: await f.unread(), original: f.rumorId };
  });
  expect(r.admit).toBe('added');
  expect(r.next.rumorId).not.toBe(r.original);
  expect(r.unread).toMatchObject({
    status: 'ready',
    count: 1,
    rumorIds: [r.next.rumorId]
  });
});
test('actual postcommit readback failure reports uncertainty rather than saved credit', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture,
      restore = f.failNextReadback();
    let save;
    try {
      save = await f.display();
    } finally {
      restore();
    }
    return { save, raw: await f.rawMetadata() };
  });
  expect(r.save).toMatchObject({
    status: 'unknown_completion',
    phase: 'readback',
    reason: 'aborted'
  });
  expect(
    (
      JSON.parse((r.raw.received as { wire: string }[])[0].wire) as {
        read: unknown;
      }
    ).read
  ).not.toBeNull();
  expect(r.raw.pairs).toHaveLength(1);
});
test('actual rival metadata transaction defeats stale whole-wire CAS without overwriting its evidence', async ({
  page
}) => {
  await ready(page);
  const r = await page.evaluate(async () => {
    const f = window.hcp094Fixture,
      restore = await f.raceNextWrite();
    let save;
    try {
      save = await f.display();
    } finally {
      restore();
    }
    return { save, raw: await f.rawMetadata() };
  });
  expect(r.save.status).toBe('conflict');
  const row = JSON.parse((r.raw.received as { wire: string }[])[0].wire) as {
    read: unknown;
    sources: string[];
  };
  expect(row.read).toBeNull();
  expect(row.sources).toContain('wss://race.example.org');
  expect(r.raw.pairs).toEqual([]);
});
