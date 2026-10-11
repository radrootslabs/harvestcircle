import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Reservations from './harness/private-send-reservations.ts';
import type { RumorPlan } from '../../src/lib/messaging/rumor-plan.ts';
declare global {
  interface Window {
    hcp135: typeof Reservations;
    hcp135Fixture: Awaited<ReturnType<typeof Reservations.makeFixture>>;
    hcp135Plan: RumorPlan;
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
          new URL('./harness/private-send-reservations.ts', import.meta.url)
        ),
        name: 'hcp135',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP135_NATIVE_MULTITAB_ORIGINAL_COMMAND',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Real Chromium IDB/WebLocks and actual page loss; controlled provider/clock, not installed extension, real clock or independent-client Q.'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page, shared?: Reservations.Shared) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  return page.evaluate(async (shared) => {
    window.hcp135Fixture = await window.hcp135.makeFixture(shared);
    window.hcp135Plan = window.hcp135Fixture.plan();
    return window.hcp135Fixture.shared();
  }, shared);
}
async function invoke(page: Page) {
  return page.evaluate(async () => {
    const f = window.hcp135Fixture,
      s = window.hcp135,
      before = f.counts();
    const result = await f.reserve(s.command, window.hcp135Plan);
    return {
      status: result.status,
      before,
      after: f.counts(),
      metadata:
        result.status === 'reserved' || result.status === 'existing'
          ? s.reservedSendSnapshot(result.identity)
          : undefined,
      wire:
        result.status === 'reserved' || result.status === 'existing'
          ? s.reservedSendRumorWire(result.identity)
          : undefined,
      rows: await f.rows()
    };
  });
}
test('same command races retain one original rumor after actual tab loss; distinct intent stays distinct', async ({
  page,
  context
}) => {
  const shared = await load(page),
    second = await context.newPage();
  await load(second, shared);
  const raced = await Promise.all([invoke(page), invoke(second)]);
  expect(raced.filter((r) => r.status === 'reserved')).toHaveLength(1);
  expect(
    raced.filter((r) => r.status === 'busy' || r.status === 'existing')
  ).toHaveLength(1);
  for (const r of raced) expect(r.after).toEqual(r.before);
  const original = raced.find((r) => r.status === 'reserved')!;
  expect(original.rows).toHaveLength(1);
  expect(JSON.parse((original.rows[0] as { wire: string }).wire)).toEqual(
    original.metadata
  );
  expect(original.wire).toContain('Private reservation sentinel');
  expect(JSON.stringify(original.rows)).not.toContain(
    'Private reservation sentinel'
  );
  const joined = await invoke(second);
  expect(joined.status).toBe('existing');
  expect(joined.metadata).toEqual(original.metadata);
  expect(joined.wire).toBe(original.wire);
  expect(joined.rows).toEqual(original.rows);
  expect(joined.after).toEqual(joined.before);
  await page.close();
  const survived = await invoke(second);
  expect(survived.status).toBe('existing');
  expect(survived.metadata).toEqual(original.metadata);
  expect(survived.wire).toBe(original.wire);
  expect(survived.rows).toEqual(original.rows);
  expect(survived.after).toEqual(survived.before);
  const distinct = await second.evaluate(async () => {
    const f = window.hcp135Fixture,
      s = window.hcp135,
      before = f.counts();
    const id = '12345678-1234-4234-8234-123456789abd';
    const collision = await f.reserve(id, f.plan()),
      beforeProgress = await f.rows();
    const observed = f.shared().milliseconds;
    f.setTime(observed + 1000);
    const next = await f.reserve(id, f.plan());
    if (next.status !== 'reserved')
      throw Error('expected distinct source reservation');
    return {
      collision: collision.status,
      beforeProgress,
      before,
      after: f.counts(),
      metadata: s.reservedSendSnapshot(next.identity),
      wire: s.reservedSendRumorWire(next.identity),
      rows: await f.rows(),
      observedSeconds: Math.floor((observed + 1000) / 1000)
    };
  });
  expect(distinct.collision).toBe('clock_conflict');
  expect(distinct.beforeProgress).toEqual(original.rows);
  expect(distinct.metadata?.createdAt).toBe(distinct.observedSeconds);
  expect(distinct.metadata?.createdAt).toBe(original.metadata!.createdAt + 1);
  expect(distinct.metadata?.id).not.toBe(original.metadata!.id);
  expect(distinct.metadata?.rumorHash).not.toBe(original.metadata!.rumorHash);
  expect(distinct.after).toEqual(distinct.before);
  expect(distinct.rows).toHaveLength(2);
  expect(distinct.rows).toContainEqual(original.rows[0]);
  expect(
    distinct.rows.map(
      (row) => JSON.parse((row as { wire: string }).wire) as unknown
    )
  ).toContainEqual(distinct.metadata);
  const wire = JSON.parse(distinct.wire!) as {
    created_at: number;
    tags: string[][];
  };
  expect(wire.created_at).toBe(distinct.observedSeconds);
  expect(wire.tags).toEqual(
    (JSON.parse(original.wire!) as { tags: string[][] }).tags
  );
  expect(JSON.stringify(distinct.rows)).not.toContain(
    'Private reservation sentinel'
  );
});
test('missing WebLocks refuses new reservations in both tabs while guest search remains usable', async ({
  page,
  context
}) => {
  const shared = await load(page),
    second = await context.newPage();
  await load(second, shared);
  for (const tab of [page, second]) {
    await tab.evaluate(() =>
      Object.defineProperty(navigator, 'locks', {
        configurable: true,
        value: undefined
      })
    );
    const result = await invoke(tab);
    expect(result.status).toBe('unavailable');
    expect(result.rows).toEqual([]);
    expect(result.after).toEqual(result.before);
    await expect(
      tab.getByRole('heading', { name: 'Search food', exact: true })
    ).toBeVisible();
  }
});
