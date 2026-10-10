import type { PublicDraftRecord } from '../../src/lib/contracts/local-records.ts';
import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/listing-editor.ts';
declare global {
  interface Window {
    hcp115: typeof Fixture;
    hcp115Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp115Observation: ReturnType<typeof Fixture.nativeObservation>;
    hcp115Pending: Promise<unknown>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
const partial = {
  title: 'Unfinished local product',
  description: '  Retain exact lines\n',
  location: '',
  amount: '2,50',
  currency: '',
  unit: 'kg',
  quantity: '03',
  contactType: '',
  contactValue: ''
} as const;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const built = await build({
    configFile: false,
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/listing-editor.ts', import.meta.url)
        ),
        name: 'hcp115',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('actual bundle missing');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  await server?.close();
});
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.WebSocket = class extends WebSocket {
      constructor() {
        super(
          (() => {
            throw Error('HCP115 source assertions authorize no relay traffic');
          })()
        );
      }
    };
  });
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp115Fixture = await window.hcp115.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  await page.evaluate(async () => {
    window.hcp115Observation?.restore();
    await window.hcp115Fixture?.close();
  });
});
test('new GET hydrates nine empty strings without creating a draft or requesting a signature', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcp115Fixture;
    const before = f.counts();
    const opened = await f.open({ mode: 'new' });
    return { opened, rows: await f.inspect(), before, after: f.counts() };
  });
  expect(result.opened.status).toBe('ready');
  if (result.opened.status !== 'ready') return;
  expect(result.opened.snapshot?.form).toEqual({
    title: '',
    description: '',
    location: '',
    amount: '',
    currency: '',
    unit: '',
    quantity: '',
    contactType: '',
    contactValue: ''
  });
  expect(result.opened.snapshot?.draftId).toBeNull();
  expect(result.rows.drafts).toHaveLength(0);
  expect(result.after).toEqual(result.before);
});
test('sign-only owner saves incomplete exact strings without messaging admission or invalid paint', async ({
  page
}) => {
  await page.evaluate(async () => {
    await window.hcp115Fixture.render({ mode: 'new' });
  });
  await expect(page.locator('#hcp115-editor form')).toBeVisible();
  await page.getByLabel('Product name', { exact: true }).fill(partial.title);
  await page.getByLabel('Price', { exact: true }).fill(partial.amount);
  await page.getByLabel('Per', { exact: true }).selectOption('kg');
  await page
    .getByLabel('Advertised quantity (optional)', { exact: true })
    .fill('03');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const actual = await page.evaluate(async () => ({
    rows: await window.hcp115Fixture.inspect(),
    identity: window.hcp115Fixture.identity(),
    counts: window.hcp115Fixture.counts()
  }));
  const rows = actual.rows.drafts as { wire: string }[];
  expect(rows).toHaveLength(1);
  expect((JSON.parse(rows[0].wire) as PublicDraftRecord).form).toMatchObject({
    title: partial.title,
    amount: '2,50',
    currency: '',
    unit: 'kg',
    quantity: '03'
  });
  expect(actual.counts.signs).toBe(0);
  await expect(
    page.locator('#hcp115-editor [aria-invalid="true"]')
  ).toHaveCount(0);
});
test('resume opens the exact original incomplete draft without replacement or validation paint', async ({
  page
}) => {
  const id = await page.evaluate(async (form) => {
    const f = window.hcp115Fixture;
    await f.open({ mode: 'new' });
    if (!(await f.edit(form))) throw Error('actual partial save failed');
    return f.snapshot()?.draftId;
  }, partial);
  expect(id).toBeTruthy();
  const result = await page.evaluate(async (id) => {
    const f = window.hcp115Fixture;
    const before = await f.inspect();
    const opened = await f.open({ mode: 'draft', draftId: id! });
    await f.render({ mode: 'draft', draftId: id! });
    return { before, opened, after: await f.inspect() };
  }, id);
  expect(result.opened.status).toBe('ready');
  if (result.opened.status === 'ready')
    expect(result.opened.snapshot?.form).toEqual(partial);
  expect(result.after).toEqual(result.before);
  await expect(page.locator('#hcp115-editor form')).toBeVisible();
  await expect(
    page.locator('#hcp115-editor [aria-invalid="true"]')
  ).toHaveCount(0);
});
test('all three modes share mandatory fields and optional public contact without SKU image or shipping', async ({
  page
}) => {
  const prototype = await page.evaluate(
    async () => await window.hcp115Fixture.prototype()
  );
  for (const route of [
    { mode: 'new' } as const,
    { mode: 'draft', draftId: prototype.draftId } as const,
    { mode: 'edit', naddr: prototype.naddr } as const
  ]) {
    await page.evaluate(async (route) => {
      await window.hcp115Fixture.render(route);
    }, route);
    await expect(page.locator('#hcp115-editor form')).toBeVisible();
    await expect(
      page.locator(
        '#hcp115-editor input, #hcp115-editor textarea, #hcp115-editor select'
      )
    ).toHaveCount(9);
    await expect(
      page.getByLabel('Product name', { exact: true })
    ).toBeVisible();
    await expect(page.getByLabel('Public area', { exact: true })).toBeVisible();
    await expect(page.getByLabel('Price', { exact: true })).toHaveAttribute(
      'type',
      'text'
    );
    await page
      .getByText('Add public contact details (optional)', { exact: true })
      .click();
    await expect(
      page.getByText('Only add contact details you want to include publicly.', {
        exact: true
      })
    ).toBeVisible();
    await expect(page.locator('#hcp115-editor')).not.toContainText(
      /SKU|shipping/i
    );
    await expect(
      page.locator(
        '#hcp115-editor input[type="file"],#hcp115-editor img,[aria-invalid="true"]'
      )
    ).toHaveCount(0);
  }
});
test('edit hydrates only genuine exact original source values and remains read-only without effects', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcp115Fixture,
      p = await f.prototype(),
      before = await f.inspect(),
      counts = f.counts();
    const opened = await f.open({ mode: 'edit', naddr: p.naddr });
    const edit = await f.edit(window.hcp115.blank()),
      flush = await f.flush();
    return {
      p,
      before,
      opened,
      edit,
      flush,
      after: await f.inspect(),
      counts,
      afterCounts: f.counts()
    };
  });
  expect(result.opened.status).toBe('ready');
  if (result.opened.status === 'ready') {
    expect(result.opened.snapshot?.editable).toBe(false);
    expect(result.opened.snapshot?.sourceEventId).toBe(result.p.eventId);
    const native = result.before.drafts as { wire: string }[];
    const source = native
      .map((row) => JSON.parse(row.wire) as PublicDraftRecord)
      .find((row) => row.id === result.p.draftId);
    if (!source) throw Error('actual original source draft missing');
    expect(result.opened.snapshot?.form).toEqual(source.form);
  }
  expect(result.edit).toBe(false);
  expect(result.flush).toBe(false);
  expect(result.after).toEqual(result.before);
  expect(result.afterCounts).toEqual(result.counts);
});
for (const damage of [
  'advanced',
  'deleted',
  'mismatched',
  'ambiguous',
  'external'
] as const)
  test(`edit refuses ${damage} original source without capture inversion or data reset`, async ({
    page
  }) => {
    const result = await page.evaluate(async (damage) => {
      const f = window.hcp115Fixture,
        p = await f.prototype();
      await f.damageSource(damage, p.draftId);
      const before = await f.inspect(),
        counts = f.counts(),
        opened = await f.open({ mode: 'edit', naddr: p.naddr });
      return {
        before,
        opened,
        after: await f.inspect(),
        counts,
        afterCounts: f.counts()
      };
    }, damage);
    expect(result.opened).toEqual({ status: 'unavailable', reason: 'source' });
    expect(result.after).toEqual(result.before);
    expect(result.afterCounts).toEqual(result.counts);
  });
test('foreign edit author is rejected before native opening or hydration', async ({
  page
}) => {
  const actual = await page.evaluate(async () => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    return {
      result: await window.hcp115Fixture.open({
        mode: 'edit',
        naddr: window.hcp115.reference('foreign')!
      }),
      native: window.hcp115Observation.state()
    };
  });
  expect(actual.result).toEqual({ status: 'unavailable', reason: 'resource' });
  expect(actual.native).toMatchObject({ opens: 0, gets: 0, puts: 0 });
});
test('missing original draft never creates a replacement', async ({ page }) => {
  const actual = await page.evaluate(async () => ({
    result: await window.hcp115Fixture.open({
      mode: 'draft',
      draftId: crypto.randomUUID()
    }),
    rows: await window.hcp115Fixture.inspect()
  }));
  expect(actual.result.status).toBe('unavailable');
  expect(actual.rows.drafts).toHaveLength(0);
});
test('copied editor cannot subscribe close save or flush while original remains usable', async ({
  page
}) => {
  const actual = await page.evaluate(async (form) => {
    const f = window.hcp115Fixture;
    await f.open({ mode: 'new' });
    const copied = await f.edit(form, true),
      flush = await f.flush(true),
      calls = f.copiedSubscribe();
    f.copiedClose();
    const before = await f.inspect(),
      alive = !!f.snapshot(),
      original = await f.edit(form);
    return {
      copied,
      flush,
      calls,
      before,
      alive,
      original,
      after: await f.inspect()
    };
  }, partial);
  expect(actual).toMatchObject({
    copied: false,
    flush: false,
    calls: 0,
    alive: true,
    original: true
  });
  expect(actual.before.drafts).toHaveLength(0);
  expect(actual.after.drafts).toHaveLength(1);
});
test('malformed route and copied identity are rejected before native opening', async ({
  page
}) => {
  const actual = await page.evaluate(async () => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    const f = window.hcp115Fixture;
    return {
      malformed: await f.open({ mode: 'draft', draftId: '../other' }),
      copied: await f.open({ mode: 'new' }, true),
      native: window.hcp115Observation.state()
    };
  });
  expect(actual.malformed.status).toBe('unavailable');
  expect(actual.copied).toEqual({ status: 'unavailable', reason: 'owner' });
  expect(actual.native).toMatchObject({ opens: 0, gets: 0, puts: 0 });
});
test('logout hides protected mounted values and retains original native draft bytes', async ({
  page
}) => {
  await page.evaluate(async () => {
    await window.hcp115Fixture.render({ mode: 'new' });
  });
  await expect(page.locator('#hcp115-editor form')).toBeVisible();
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Private unfinished input');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const before = await page.evaluate(
    async () => await window.hcp115Fixture.inspect()
  );
  await page.evaluate(() => window.hcp115Fixture.disconnect());
  await expect(page.locator('#hcp115-editor form')).toHaveCount(0);
  await expect(page.locator('#hcp115-editor')).not.toContainText(
    'Private unfinished input'
  );
  expect(
    await page.evaluate(async () => await window.hcp115Fixture.inspect())
  ).toEqual(before);
});
test('cancel during genuine open completion closes the late connection before protected hydration', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.holdOpen();
    window.hcp115Pending = window.hcp115Fixture.open({ mode: 'new' });
  });
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().held)
    )
    .toBe(true);
  await page.evaluate(() => {
    window.hcp115Fixture.cancel();
    window.hcp115Observation.release();
  });
  const result = await page.evaluate(async () => ({
    result: await window.hcp115Pending,
    native: window.hcp115Observation.state(),
    snapshot: window.hcp115Fixture.snapshot()
  }));
  expect(result.result).toMatchObject({ status: 'unavailable' });
  expect(result.native).toMatchObject({
    opens: 1,
    closes: 1,
    gets: 0,
    puts: 0,
    completions: 1
  });
  expect(result.snapshot).toBeUndefined();
});
test('cancellation between original open resolution and await custody closes the actual ready owner', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.afterOpen(() => window.hcp115Fixture.cancel());
    return {
      result: await window.hcp115Fixture.open({ mode: 'new' }),
      native: window.hcp115Observation.state()
    };
  });
  expect(result.result).toMatchObject({ status: 'unavailable' });
  expect(result.native).toMatchObject({
    opens: 1,
    closes: 1,
    gets: 0,
    puts: 0,
    completions: 1
  });
});
test('cancel after original draft native read completion retires the late controller without writes', async ({
  page
}) => {
  const id = await page.evaluate(async (form) => {
    const f = window.hcp115Fixture;
    await f.open({ mode: 'new' });
    await f.edit(form);
    return f.snapshot()?.draftId;
  }, partial);
  await page.evaluate((id) => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.holdDraft();
    window.hcp115Pending = window.hcp115Fixture.open({
      mode: 'draft',
      draftId: id!
    });
  }, id);
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().held)
    )
    .toBe(true);
  await page.evaluate(() => {
    window.hcp115Fixture.cancel();
    window.hcp115Observation.release();
  });
  const result = await page.evaluate(async () => ({
    result: await window.hcp115Pending,
    native: window.hcp115Observation.state(),
    snapshot: window.hcp115Fixture.snapshot()
  }));
  expect(result.result).toMatchObject({ status: 'unavailable' });
  expect(result.native.gets).toBeGreaterThan(0);
  expect(result.native.puts).toBe(0);
  expect(result.native.closes).toBeGreaterThan(0);
  expect(result.snapshot).toBeUndefined();
});
test('superseded pending acquisition cannot replace or retire the newer editor', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.holdOpen();
    window.hcp115Pending = window.hcp115Fixture.open({ mode: 'new' });
  });
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().held)
    )
    .toBe(true);
  const newer = await page.evaluate(
    async () => await window.hcp115Fixture.open({ mode: 'new' })
  );
  expect(newer.status).toBe('ready');
  await page.evaluate(() => window.hcp115Observation.release());
  const actual = await page.evaluate(async () => ({
    old: await window.hcp115Pending,
    current: window.hcp115Fixture.snapshot(),
    native: window.hcp115Observation.state()
  }));
  expect(actual.old).toMatchObject({ status: 'unavailable' });
  expect(actual.current?.editable).toBe(true);
  expect(actual.native.puts).toBe(0);
  expect(actual.native.closes).toBeGreaterThan(0);
});
test('component unmount cancels genuine pending acquisition before late resources arrive', async ({
  page
}) => {
  await page.evaluate(async () => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.holdOpen();
    await window.hcp115Fixture.render({ mode: 'new' });
  });
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().held)
    )
    .toBe(true);
  await page.evaluate(async () => {
    await window.hcp115Fixture.unmount();
    window.hcp115Observation.release();
  });
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().closes)
    )
    .toBe(1);
  await expect(page.locator('#hcp115-editor')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp115Observation.state().puts)).toBe(
    0
  );
});
test('genuine submitted save shows Saving and late acknowledgment cannot repaint after unmount', async ({
  page
}) => {
  await page.evaluate(
    async () => await window.hcp115Fixture.render({ mode: 'new' })
  );
  await expect(page.locator('#hcp115-editor form')).toBeVisible();
  await page.evaluate(() => {
    window.hcp115Observation = window.hcp115.nativeObservation();
    window.hcp115Observation.holdSave();
  });
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Actual submitted native partial draft');
  await expect
    .poll(
      async () =>
        await page.evaluate(() => window.hcp115Observation.state().held)
    )
    .toBe(true);
  await expect(
    page.getByText('Saving in this browser.', { exact: true })
  ).toBeVisible();
  await page.evaluate(async () => {
    await window.hcp115Fixture.unmount();
    window.hcp115Observation.release();
  });
  await expect(page.locator('#hcp115-editor')).toHaveCount(0);
  const actual = await page.evaluate(async () => ({
    rows: await window.hcp115Fixture.inspect(),
    native: window.hcp115Observation.state()
  }));
  expect(actual.rows.drafts).toHaveLength(1);
  expect(
    (
      JSON.parse(
        (actual.rows.drafts[0] as { wire: string }).wire
      ) as PublicDraftRecord
    ).form.title
  ).toBe('Actual submitted native partial draft');
  expect(actual.native.puts).toBe(1);
});
test('actual compiled guest routes never hydrate protected listing fields or connect implicitly', async ({
  page
}) => {
  const naddr = await page.evaluate(() => window.hcp115.reference('guest')!);
  for (const route of [
    '/sell',
    '/selling/drafts/' + crypto.randomUUID(),
    '/products/' + naddr + '/edit'
  ]) {
    await page.goto(server.url + route);
    await expect(page.locator('main h1')).toBeVisible();
    await expect(page.locator('main form')).toHaveCount(0);
    await expect(
      page
        .getByRole('button', { name: 'Connect extension', exact: true })
        .last()
    ).toBeVisible();
  }
});
test('actual compiled connected routes share original new draft and read-only edit hydration', async ({
  page
}) => {
  const prototype = await page.evaluate(
    async () => await window.hcp115Fixture.prototype()
  );
  async function navigate(path: string) {
    await page.evaluate((path) => {
      const anchor = document.createElement('a');
      anchor.href = path;
      anchor.textContent = 'HCP115 controlled local route';
      anchor.id = 'hcp115-route-link';
      document.body.append(anchor);
    }, path);
    await page.locator('#hcp115-route-link').click();
    await expect(page).toHaveURL(server.url + path);
    await page.evaluate(() =>
      document.querySelector('#hcp115-route-link')?.remove()
    );
  }
  await navigate('/sell');
  await page
    .getByRole('button', { name: 'Connect extension', exact: true })
    .last()
    .click();
  await expect(page.locator('main form')).toBeVisible();
  await expect(
    page.locator('main input,main textarea,main select')
  ).toHaveCount(9);
  await expect(page.getByLabel('Product name', { exact: true })).toHaveValue(
    ''
  );
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Compiled partial route value');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const native = await page.evaluate(
    async () => await window.hcp115.inspectNative()
  );
  const draft = (native.drafts as { wire: string }[])
    .map((row) => JSON.parse(row.wire) as PublicDraftRecord)
    .find((row) => row.form.title === 'Compiled partial route value');
  expect(draft).toBeTruthy();
  if (!draft) throw Error('actual compiled route draft missing');
  await navigate('/selling/drafts/' + draft.id);
  await expect(page.getByLabel('Product name', { exact: true })).toHaveValue(
    'Compiled partial route value'
  );
  await expect(
    page.locator('main input,main textarea,main select')
  ).toHaveCount(9);
  await navigate('/products/' + prototype.naddr + '/edit');
  await expect(page.locator('main form')).toBeVisible();
  await expect(
    page.getByLabel('Product name', { exact: true })
  ).toHaveAttribute('readonly', '');
  await expect(page.getByLabel('Per', { exact: true })).toBeDisabled();
  await expect(
    page.getByText(
      'These retained local values are read-only until the current listing is checked.',
      { exact: true }
    )
  ).toBeVisible();
});
test('actual native versionchange retires editor instead of saving new input through a closed owner', async ({
  page
}) => {
  await page.evaluate(
    async () => await window.hcp115Fixture.render({ mode: 'new' })
  );
  await expect(page.locator('#hcp115-editor form')).toBeVisible();
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Persisted before native closure');
  await expect(
    page.getByText('Saved in this browser.', { exact: true })
  ).toBeVisible();
  const native = await page.evaluate(
    async () => await window.hcp115.nativeVersionChange()
  );
  expect(native.drafts).toHaveLength(1);
  await page
    .getByLabel('Product name', { exact: true })
    .fill('Must not save through closed native owner');
  await expect(page.locator('#hcp115-editor form')).toHaveCount(0);
  await expect(
    page.getByText(
      'Browser storage is unavailable. Your existing local data has not been reset.',
      { exact: true }
    )
  ).toBeVisible();
  expect(
    (
      JSON.parse(
        (native.drafts[0] as { wire: string }).wire
      ) as PublicDraftRecord
    ).form.title
  ).toBe('Persisted before native closure');
});

test('foreign retained draft stays unavailable to a different genuine connected author without reset', async ({
  page
}) => {
  const actual = await page.evaluate(async () => {
    const first = window.hcp115Fixture,
      prototype = await first.prototype(),
      owner = first.owner,
      before = await first.inspect();
    await first.close();
    window.hcp115Fixture = await window.hcp115.createFixture();
    const next = window.hcp115Fixture;
    if (next.owner === owner)
      throw Error('controlled independent authors must differ');
    const opened = await next.open({
      mode: 'draft',
      draftId: prototype.draftId
    });
    await next.render({ mode: 'draft', draftId: prototype.draftId });
    return {
      opened,
      snapshot: next.snapshot(),
      before,
      after: await next.inspect(),
      counts: next.counts()
    };
  });
  expect(actual.opened.status).toBe('unavailable');
  expect(actual.snapshot).toBeUndefined();
  expect(actual.after).toEqual(actual.before);
  expect(actual.counts.signs).toBe(0);
  await expect(
    page.getByText('This draft is not available here.', { exact: true })
  ).toBeVisible();
  await expect(page.locator('#hcp115-editor form')).toHaveCount(0);
});
