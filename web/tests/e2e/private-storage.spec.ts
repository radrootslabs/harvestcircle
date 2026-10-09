import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Storage from './harness/private-storage.ts';
declare global {
  interface Window {
    hcp080: typeof Storage;
    hcp080Fixture: Awaited<ReturnType<typeof Storage.makeFixture>>;
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
          new URL('./harness/private-storage.ts', import.meta.url)
        ),
        name: 'hcp080',
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
      fixture: 'HCP080_REAL_IDB_CIPHERTEXT_SCHEMA_QUOTAS',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Controlled source-only IDB/structural signed ciphertext; no nested admission or actual operator Q'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp080Fixture = await window.hcp080.makeFixture();
  });
}
test('durable rows contain only ciphertext/minimal metadata and preserve original reservation retry', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    await f.reserve(0);
    const base = f.handle(f.reservation(0)),
      next = f.handle(f.operation(0));
    const commit = await s.commitPrivateRecord(f.repository, next, base),
      retry = await f.reserve(0);
    const receive = f.received(0),
      received = await s.commitPrivateRecord(
        f.repository,
        f.handle(receive),
        null
      );
    const sends = await f.rows('private_sends'),
      inbox = await f.rows('received_envelopes');
    return {
      commit,
      retry: retry.ok && retry.state,
      received: received.ok,
      stored: JSON.stringify([...sends, ...inbox]),
      publicCount: (await f.rows('public_drafts')).length
    };
  });
  expect(result.commit.ok).toBe(true);
  expect(result.retry).toBe('existing');
  expect(result.received).toBe(true);
  expect(result.publicCount).toBe(0);
  for (const value of [
    'HCP080 private plaintext sentinel',
    '"body"',
    '"subject"',
    '"contact"',
    '"product"'
  ])
    expect(result.stored).not.toContain(value);
});
test('duplicate signed outer fields grant no handle and preserve existing local evidence', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    await f.reserve(0);
    const before = JSON.stringify(await f.rows('private_sends')),
      operation = f.operation(0),
      received = f.received(0);
    const poison = (wire: string) =>
      '{"\\u0063ontent":"PRIVATE_DUPLICATE_SENTINEL",' + wire.slice(1);
    const values = [
      s.decodePrivateRecord(
        JSON.stringify({
          ...operation,
          self: { ...operation.self, wire: poison(operation.self.wire) }
        }),
        s.owner,
        operation.id
      ),
      s.decodePrivateRecord(
        JSON.stringify({ ...received, outer: poison(received.outer) }),
        s.owner,
        received.id
      )
    ];
    return {
      accepted: values.filter((v) => v.ok).length,
      unchanged: before === JSON.stringify(await f.rows('private_sends')),
      received: (await f.rows('received_envelopes')).length
    };
  });
  expect(result).toEqual({ accepted: 0, unchanged: true, received: 0 });
});
test('conflicting ciphertext CAS preserves the original artifact and exact retries reconcile', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    await f.reserve(0);
    const base = f.handle(f.reservation(0)),
      a = f.handle(f.operation(0)),
      b = f.handle(f.operation(0));
    const [first, second] = await Promise.all([
      s.commitPrivateRecord(f.repository, a, base),
      s.commitPrivateRecord(f.repository, b, base)
    ]);
    const winner = first.ok ? a : b,
      before = JSON.stringify(await f.rows('private_sends'));
    const retry = await s.commitPrivateRecord(f.repository, winner, base);
    return {
      accepted: [first, second].filter((r) => r.ok).length,
      refused: [first, second].filter((r) => !r.ok && r.reason === 'conflict')
        .length,
      retry: retry.ok && retry.value.state,
      unchanged: before === JSON.stringify(await f.rows('private_sends')),
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({
    accepted: 1,
    refused: 1,
    retry: 'existing',
    unchanged: true,
    count: 1
  });
});
test('owner-bound handles and loads reject cross-owner or reflected capabilities', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      record = f.received(0);
    await s.commitPrivateRecord(f.repository, f.handle(record), null);
    const foreign = { ...f.reservation(1), owner: s.peer, peer: s.owner },
      decoded = s.decodePrivateRecord(
        JSON.stringify(foreign),
        s.peer,
        foreign.id
      );
    const cross = decoded.ok
      ? await s.commitPrivateRecord(f.repository, decoded.value, null)
      : undefined;
    const reflected = await s.commitPrivateRecord(
      {} as typeof f.repository,
      f.handle(record),
      null
    );
    const wrong = await s.loadPrivateRecord(
      f.repository,
      'received_envelopes',
      'a'.repeat(64)
    );
    return {
      cross,
      reflected,
      wrong,
      count: (await f.rows('received_envelopes')).length
    };
  });
  expect(result.cross?.ok).toBe(false);
  expect(result.reflected.ok).toBe(false);
  expect(result.wrong.ok).toBe(false);
  expect(result.count).toBe(1);
});
test('unknown schema or corrupt owner records remain unchanged, never reset to an empty inbox', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      good = f.received(0),
      bad = f.received(1);
    await s.commitPrivateRecord(f.repository, f.handle(good), null);
    await f.inject('received_envelopes', [
      {
        owner: s.owner,
        id: bad.id,
        wire: JSON.stringify({ ...bad, schema: 999 })
      }
    ]);
    const before = JSON.stringify(await f.rows('received_envelopes')),
      next = f.received(2),
      result = await s.commitPrivateRecord(f.repository, f.handle(next), null);
    return {
      result,
      unchanged: before === JSON.stringify(await f.rows('received_envelopes'))
    };
  });
  expect(result.result).toEqual({ ok: false, reason: 'corrupt_record' });
  expect(result.unchanged).toBe(true);
});
test('two actual connections race for the last send slot without evicting existing evidence', async ({
  page,
  context
}) => {
  await load(page);
  await page.evaluate(async () => {
    const f = window.hcp080Fixture;
    for (let i = 0; i < 99; i++) {
      const r = await f.reserve(i);
      if (!r.ok) throw Error('seed failure');
    }
  });
  const second = await context.newPage();
  await load(second);
  const [a, b] = await Promise.all([
    page.evaluate(() => window.hcp080Fixture.reserve(100)),
    second.evaluate(() => window.hcp080Fixture.reserve(101))
  ]);
  expect([a, b].filter((value) => value.ok)).toHaveLength(1);
  expect(
    [a, b].filter((value) => !value.ok && value.reason === 'capacity')
  ).toHaveLength(1);
  expect(
    await page.evaluate(
      async () => (await window.hcp080Fixture.rows('private_sends')).length
    )
  ).toBe(100);
});
test('private-send stored UTF8 bytes refuse oversized updates and preserve their exact base', async ({
  page
}) => {
  test.setTimeout(1_800_000);
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    let stopped = false,
      count = 0;
    for (let i = 0; i < 100; i++) {
      const reservation = await f.reserve(i);
      if (!reservation.ok) throw Error('reservation seed failed');
      const before = JSON.stringify(await f.rows('private_sends'));
      const update = await s.commitPrivateRecord(
        f.repository,
        f.handle(f.operation(i, true)),
        f.handle(f.reservation(i))
      );
      if (!update.ok) {
        if (update.reason !== 'capacity') throw Error('wrong byte refusal');
        stopped = before === JSON.stringify(await f.rows('private_sends'));
        count = i;
        break;
      }
    }
    return {
      stopped,
      count,
      received: (await f.rows('received_envelopes')).length
    };
  });
  expect(result.stopped).toBe(true);
  expect(result.count).toBeGreaterThan(0);
  expect(result.count).toBeLessThan(100);
  expect(result.received).toBe(0);
});
test('received count cap remains separate from a full unresolved outbox and refuses without eviction', async ({
  page
}) => {
  test.setTimeout(1_800_000);
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    for (let i = 0; i < 100; i++) {
      const r = await f.reserve(i);
      if (!r.ok) throw Error('send seed failure');
    }
    const rows = [];
    for (let i = 0; i < 2000; i++) {
      const r = f.received(i);
      rows.push({
        owner: s.owner,
        id: r.id,
        wire: s.privateRecordWire(f.handle(r), s.owner, r.id)
      });
    }
    await f.inject('received_envelopes', rows);
    const candidate = f.received(2001);
    const before = JSON.stringify(await f.rows('received_envelopes')),
      result = await s.commitPrivateRecord(
        f.repository,
        f.handle(candidate),
        null
      );
    return {
      result,
      unchanged: before === JSON.stringify(await f.rows('received_envelopes')),
      sends: (await f.rows('private_sends')).length,
      count: (await f.rows('received_envelopes')).length
    };
  });
  expect(result.result).toEqual({ ok: false, reason: 'capacity' });
  expect(result.unchanged).toBe(true);
  expect(result.sends).toBe(100);
  expect(result.count).toBe(2000);
});
test('received 48MiB byte cap refuses below count cap and preserves both independent namespaces', async ({
  page
}) => {
  test.setTimeout(1_800_000);
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      rows = [];
    let bytes = 0;
    let candidate = f.received(0, true);
    while (true) {
      const wire = s.privateRecordWire(
        f.handle(candidate),
        s.owner,
        candidate.id
      )!;
      const row = { owner: s.owner, id: candidate.id, wire },
        size = new TextEncoder().encode(JSON.stringify(row)).length;
      if (bytes + size > 50331648) break;
      rows.push(row);
      bytes += size;
      candidate = f.received(rows.length, true);
    }
    await f.inject('received_envelopes', rows);
    await f.reserve(0);
    const before = JSON.stringify(await f.rows('received_envelopes')),
      result = await s.commitPrivateRecord(
        f.repository,
        f.handle(candidate),
        null
      );
    return {
      result,
      unchanged: before === JSON.stringify(await f.rows('received_envelopes')),
      count: rows.length,
      bytes,
      sends: (await f.rows('private_sends')).length
    };
  });
  expect(result.result).toEqual({ ok: false, reason: 'capacity' });
  expect(result.unchanged).toBe(true);
  expect(result.count).toBeLessThan(2000);
  expect(result.bytes).toBeLessThanOrEqual(50331648);
  expect(result.sends).toBe(1);
});
test('real transaction abort never acknowledges or retains a new ciphertext row', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      r = f.received(0),
      original = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      );
    IDBObjectStore.prototype.put = function (...args) {
      const request = original.apply(this, args);
      request.addEventListener('success', () => this.transaction.abort(), {
        once: true
      });
      return request;
    };
    let result;
    try {
      result = await s.commitPrivateRecord(f.repository, f.handle(r), null);
    } finally {
      IDBObjectStore.prototype.put = original;
    }
    return { result, count: (await f.rows('received_envelopes')).length };
  });
  expect(result.result.ok).toBe(false);
  expect(result.count).toBe(0);
});
