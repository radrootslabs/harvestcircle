import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/conversation-directory.ts';
declare global {
  interface Window {
    hcp100: typeof Fixture;
    hcp100Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
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
          new URL('./harness/conversation-directory.ts', import.meta.url)
        ),
        name: 'hcp100',
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
    window.hcp100Fixture = await window.hcp100.makeFixture();
    await window.hcp100Fixture.ready();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp100Fixture?.close());
});
const missing = '12345678-1234-4234-8234-123456789abc';
test('directory capture and unknown GET are inert and never mark an unread rumor read', async ({
  page
}) => {
  const r = await page.evaluate(async (id) => {
    const f = window.hcp100Fixture;
    const before = await f.rawMetadata(),
      counts = f.counts();
    const captured = f.capture();
    const result = await f.resolve(id);
    return {
      captured,
      result,
      before,
      after: await f.rawMetadata(),
      counts,
      afterCounts: f.counts(),
      unread: await f.unread()
    };
  }, missing);
  expect(r.captured).toBe(true);
  expect(r.result).toEqual({ status: 'unavailable' });
  expect(r.after).toEqual(r.before);
  expect(r.afterCounts).toEqual(r.counts);
  expect(r.unread).toMatchObject({ count: 1 });
});
test('only explicitly reviewed admitted exchange creates a random owner mapping without read or SDK effects', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    const counts = f.counts();
    const save = await f.remember();
    const raw = await f.rawMetadata();
    return {
      save,
      raw,
      unread: await f.unread(),
      counts,
      afterCounts: f.counts()
    };
  });
  expect(r.save).toMatchObject({ status: 'saved' });
  expect(r.raw.pairs).toHaveLength(1);
  expect(r.unread).toMatchObject({ count: 1 });
  expect(r.afterCounts).toEqual(r.counts);
  const row = r.raw.pairs[0] as { owner: string; id: string; wire: string };
  expect(row.id).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
  );
  expect(Object.keys(JSON.parse(row.wire) as object).sort()).toEqual(
    ['schema', 'family', 'owner', 'id', 'peer'].sort()
  );
});
test('known route lookup reads original owner metadata without new writes prompts or hidden publication', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    const saved = await f.remember();
    const raw = await f.rawMetadata();
    const id = (raw.pairs[0] as { id: string }).id;
    const counts = f.counts();
    return {
      saved,
      id,
      result: await f.resolve(id),
      before: raw,
      after: await f.rawMetadata(),
      counts,
      afterCounts: f.counts(),
      owner: f.owner,
      peer: f.peer
    };
  });
  expect(r.result).toEqual({
    status: 'resolved',
    owner: r.owner,
    peer: r.peer,
    conversationId: r.id,
    href: '/messages/' + r.id
  });
  expect(r.after).toEqual(r.before);
  expect(r.afterCounts).toEqual(r.counts);
});
test('two distinct actual encrypted product contexts share one participant-pair room', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    const first = await f.product('About carrots: two kilograms'),
      second = await f.product('About apples: pickup tomorrow');
    return {
      first,
      second,
      raw: await f.rawMetadata(),
      messages: f.messages()
    };
  });
  expect(r.first).toMatchObject({ status: 'saved' });
  expect(r.second).toMatchObject({ status: 'existing' });
  expect(r.raw.pairs).toHaveLength(1);
  expect(r.messages).toMatchObject({ status: 'ready' });
  const messages = (
    r.messages as { messages: readonly { content: string }[] }
  ).messages.map((x) => x.content);
  expect(messages).toContain('About carrots: two kilograms');
  expect(messages).toContain('About apples: pickup tomorrow');
});
test('a genuine other account cannot infer an existing UUID peer or owner metadata', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    await f.remember();
    const raw = await f.rawMetadata();
    return await f.otherOwner((raw.pairs[0] as { id: string }).id);
  });
  expect(r).toEqual({ status: 'unavailable' });
});
test('copied directory and copied room cannot authorize reading or mapping creation', async ({
  page
}) => {
  const r = await page.evaluate(async (id) => {
    const f = window.hcp100Fixture;
    f.capture();
    const raw = await f.rawMetadata();
    return {
      copied: await f.copied(id),
      forged: await f.forgedRoom(),
      before: raw,
      after: await f.rawMetadata()
    };
  }, missing);
  expect(r.copied).toEqual({ status: 'unavailable' });
  expect(r.forged).toEqual({ status: 'invalid' });
  expect(r.after).toEqual(r.before);
});
test('missing explicit mapping review never creates a substitute or marks read', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    const raw = await f.rawMetadata();
    return {
      result: await f.remember('unreviewed'),
      before: raw,
      after: await f.rawMetadata(),
      unread: await f.unread()
    };
  });
  expect(r.result).toEqual({ status: 'invalid' });
  expect(r.after).toEqual(r.before);
  expect(r.unread).toMatchObject({ count: 1 });
});
test('original owner logout invalidates lookup and admitted creation without returning metadata', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    await f.remember();
    const raw = await f.rawMetadata();
    const id = (raw.pairs[0] as { id: string }).id;
    f.disconnect();
    return {
      resolve: await f.resolve(id),
      remember: await f.remember(),
      before: raw,
      after: await f.rawMetadata()
    };
  });
  expect(r.resolve).toEqual({ status: 'unavailable' });
  expect(r.remember).toEqual({ status: 'unavailable' });
  expect(r.after).toEqual(r.before);
});
test('corrupt native mapping fails safely without peer metadata or automatic reset', async ({
  page
}) => {
  const r = await page.evaluate(async (id) => {
    const f = window.hcp100Fixture;
    f.capture();
    await f.corruptPair();
    const raw = await f.rawMetadata();
    return {
      resolve: await f.resolve(id),
      before: raw,
      after: await f.rawMetadata()
    };
  }, missing);
  expect(r.resolve).toEqual({ status: 'unavailable' });
  expect(r.after).toEqual(r.before);
});
test('same handle in another actual browser storage context is unavailable without substitution', async ({
  page,
  browser
}) => {
  const id = await page.evaluate(async () => {
    const f = window.hcp100Fixture;
    f.capture();
    await f.remember();
    return ((await f.rawMetadata()).pairs[0] as { id: string }).id;
  });
  const context = await browser.newContext();
  try {
    const other = await context.newPage();
    await other.goto(server.url + '/search');
    await other.addScriptTag({ content: bundle });
    const r = await other.evaluate(async (id) => {
      const f = await window.hcp100.makeFixture();
      try {
        await f.ready();
        f.capture();
        return { result: await f.resolve(id), raw: await f.rawMetadata() };
      } finally {
        f.close();
      }
    }, id);
    expect(r.result).toEqual({ status: 'unavailable' });
    expect(r.raw.pairs).toEqual([]);
  } finally {
    await context.close();
  }
});
test('direct private reload has generic gates title noindex and a safe inbox link without recipient inference', async ({
  page
}) => {
  await page.goto(server.url + '/messages/' + missing);
  await expect(
    page.getByRole('heading', { name: 'Connect or unlock' })
  ).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Back to messages' })
  ).toHaveAttribute('href', '/messages');
  await expect(page).toHaveTitle('HarvestCircle');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute(
    'content',
    'noindex'
  );
  await expect(page.locator('body')).not.toContainText(
    'Actual encrypted room fixture'
  );
  expect(page.url()).toBe(server.url + '/messages/' + missing);
});

test('mutated detached observation cannot import original metadata into a genuine same-owner foreign generation', async ({
  page
}) => {
  const r = await page.evaluate(() =>
    window.hcp100Fixture.mutatedGenerationObservation()
  );
  console.log(
    JSON.stringify({
      fixture: 'HCP100_GENUINE_OBSERVATION_REBIND',
      sameOwner: r.sameOwner,
      differentGeneration: r.differentGeneration,
      foreignAccepted: r.foreignAccepted,
      originalAccepted: r.originalAccepted,
      rememberedStatus: r.remembered.status,
      actualPairs: r.raw.pairs.length,
      qualification: 'SOURCE_FIXTURE_NOT_OPERATOR_AUTHORITY'
    })
  );
  expect(r.sameOwner).toBe(true);
  expect(r.differentGeneration).toBe(true);
  expect(r.foreignAccepted).toBe(false);
  expect(r.originalAccepted).toBe(true);
  expect(r.remembered).toEqual({ status: 'unavailable' });
  expect(r.raw.pairs).toEqual([]);
});
