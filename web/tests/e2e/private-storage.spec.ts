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
  test.setTimeout(86_400_000);
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
  test.setTimeout(86_400_000);
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
  test.setTimeout(86_400_000);
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

test('CR008 probe CAS retains exact immutable ciphertext/bindings and native completion', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      reservation = f.probe(0),
      prepared = { ...reservation, revision: 1, self: f.probe(0, true).self },
      base = f.handle(reservation),
      next = f.handle(prepared);
    const create = await s.commitPrivateRecord(f.repository, base, null),
      commit = await s.commitPrivateRecord(f.repository, next, base),
      loaded = await s.loadPrivateRecord(
        f.repository,
        'private_sends',
        reservation.id
      ),
      retry = await s.commitPrivateRecord(f.repository, next, base);
    const before = JSON.stringify(await f.rows('private_sends'));
    const changes = [
      { rumorHash: 'e'.repeat(64) },
      { createdAt: prepared.createdAt + 1 },
      { self: f.probe(1, true).self },
      { binding: { ...prepared.binding, preferenceId: 'e'.repeat(64) } }
    ];
    const refused = [];
    for (const patch of changes) {
      const decoded = s.decodePrivateRecord(
        JSON.stringify({ ...prepared, revision: 2, ...patch }),
        s.owner,
        reservation.id
      );
      refused.push(
        !decoded.ok ||
          !(await s.commitPrivateRecord(f.repository, decoded.value, next)).ok
      );
    }
    const ordinary = { ...f.operation(0), revision: 2, self: prepared.self },
      decodedOrdinary = s.decodePrivateRecord(
        JSON.stringify(ordinary),
        s.owner,
        reservation.id
      );
    if (!decodedOrdinary.ok)
      throw Error('valid ordinary family transition control missing');
    refused.push(
      !(await s.commitPrivateRecord(f.repository, decodedOrdinary.value, next))
        .ok
    );
    return {
      create: create.ok,
      commit: commit.ok,
      retry: retry.ok && retry.value.state,
      original:
        loaded.ok &&
        s.privateRecordWire(loaded.value, s.owner, reservation.id) ===
          s.privateRecordWire(next, s.owner, reservation.id),
      refused,
      unchanged: before === JSON.stringify(await f.rows('private_sends')),
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({
    create: true,
    commit: true,
    retry: 'existing',
    original: true,
    refused: [true, true, true, true, true],
    unchanged: true,
    count: 1
  });
});

test('CR008 append-only probe receipts use whole-wire CAS under actual concurrent writers', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      reservation = f.probe(0),
      prepared = { ...reservation, revision: 1, self: f.probe(0, true).self! },
      base = f.handle(reservation),
      next = f.handle(prepared);
    await s.commitPrivateRecord(f.repository, base, null);
    await s.commitPrivateRecord(f.repository, next, base);
    const fact = {
        actionId: s.id(1),
        origin: prepared.binding.archiveTargets[0],
        role: 'access_archive',
        attempt: 1,
        eventId: prepared.self.eventId,
        status: 'unknown',
        observedAtMilliseconds: 1,
        readbackWire: null
      },
      a = { ...prepared, revision: 2, receipts: [fact] },
      b = {
        ...prepared,
        revision: 2,
        receipts: [{ ...fact, actionId: s.id(2) }]
      };
    const races = await Promise.all(
      [a, b].map((row) =>
        s.commitPrivateRecord(f.repository, f.handle(row), next)
      )
    );
    const loaded = await s.loadPrivateRecord(
      f.repository,
      'private_sends',
      prepared.id
    );
    if (!loaded.ok) throw Error('probe disappeared');
    const row = JSON.parse(
      s.privateRecordWire(loaded.value, s.owner, prepared.id)!
    ) as typeof a;
    const deleted = await s.commitPrivateRecord(
        f.repository,
        f.handle({ ...row, revision: 3, receipts: [] }),
        loaded.value
      ),
      relabelled = await s.commitPrivateRecord(
        f.repository,
        f.handle({
          ...row,
          revision: 3,
          receipts: [{ ...row.receipts[0], status: 'accepted' }]
        }),
        loaded.value
      ),
      append = await s.commitPrivateRecord(
        f.repository,
        f.handle({
          ...row,
          revision: 3,
          receipts: [...row.receipts, { ...fact, status: 'accepted' }]
        }),
        loaded.value
      );
    return {
      races: races.map((r) => (r.ok ? 'committed' : r.reason)).sort(),
      deleted: deleted.ok,
      relabelled: relabelled.ok,
      append: append.ok,
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({
    races: ['committed', 'conflict'],
    deleted: false,
    relabelled: false,
    append: true,
    count: 1
  });
});

test('CR008 probes and ordinary sends share exactly 100 retained rows in both admission directions', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    const originals = Array.from({ length: 99 }, (_, n) =>
      n < 50 ? f.probe(n) : f.reservation(n)
    );
    await f.inject(
      'private_sends',
      originals.map((row) => ({
        owner: s.owner,
        id: row.id,
        wire: s.privateRecordWire(f.handle(row), s.owner, row.id)
      }))
    );
    const ordinary = await f.reserve(100),
      before = JSON.stringify(await f.rows('private_sends')),
      probe = await s.commitPrivateRecord(
        f.repository,
        f.handle(f.probe(101)),
        null
      ),
      full = await f.reserve(102),
      collision = await f.reserve(0);
    return {
      ordinary: ordinary.ok,
      probe: probe.ok ? 'unexpected' : probe.reason,
      full: full.ok ? 'unexpected' : full.reason,
      collision: collision.ok ? 'unexpected' : collision.reason,
      unchanged: before === JSON.stringify(await f.rows('private_sends')),
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({
    ordinary: true,
    probe: 'capacity',
    full: 'capacity',
    collision: 'conflict',
    unchanged: true,
    count: 100
  });
});

test('CR008 outbox bytes include complete probe artifact/readback metadata below the row ceiling', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    const rows = [];
    let total = 0;
    for (let index = 0; index < 100; index++) {
      const probe = f.probe(index, true, true),
        self = probe.self!,
        record = {
          ...probe,
          revision: 2,
          receipts: [
            {
              actionId: s.id(500),
              origin: probe.binding.readTargets[0],
              role: 'access_readback',
              attempt: 1,
              eventId: self.eventId,
              status: 'readback',
              observedAtMilliseconds: 1,
              readbackWire: self.wire
            }
          ]
        },
        row = {
          owner: s.owner,
          id: record.id,
          wire: s.privateRecordWire(f.handle(record), s.owner, record.id)
        },
        bytes = new TextEncoder().encode(JSON.stringify(row)).length;
      if (total + bytes > 8 * 1024 * 1024) break;
      total += bytes;
      rows.push(row);
    }
    if (!rows.length || rows.length >= 100)
      throw Error('byte boundary not exercised');
    await f.inject('private_sends', rows);
    const original = JSON.parse(rows[rows.length - 1].wire!) as {
        owner: string;
        id: string;
        revision: number;
        receipts: {
          actionId: string;
          origin: string;
          role: string;
          attempt: number;
          eventId: string;
          status: string;
          observedAtMilliseconds: number;
          readbackWire: string;
        }[];
      },
      next = {
        ...original,
        revision: original.revision + 1,
        receipts: [...original.receipts]
      },
      oldBytes = new TextEncoder().encode(
        JSON.stringify(rows[rows.length - 1])
      ).length;
    let growth = 0;
    for (
      let index = 0;
      index < 10 && total + growth <= 8 * 1024 * 1024;
      index++
    ) {
      next.receipts.push({
        ...original.receipts[0],
        actionId: s.id(600 + index)
      });
      const row = {
        owner: next.owner,
        id: next.id,
        wire: s.privateRecordWire(f.handle(next), next.owner, next.id)
      };
      growth = new TextEncoder().encode(JSON.stringify(row)).length - oldBytes;
    }
    if (total + growth <= 8 * 1024 * 1024)
      throw Error('bounded update did not cross byte cap');
    const before = JSON.stringify(await f.rows('private_sends')),
      refused = await s.commitPrivateRecord(
        f.repository,
        f.handle(next),
        f.handle(original)
      ),
      inventory = await s.inspectPrivateStorage(f.repository, 'private_sends'),
      physicalBytes = (await f.rows('private_sends')).reduce<number>(
        (sum, row) =>
          sum + new TextEncoder().encode(JSON.stringify(row)).length,
        0
      );
    return {
      refused: refused.ok ? 'unexpected' : refused.reason,
      actualBytes: physicalBytes === total,
      meteredBytes: inventory.ok && inventory.value.bytes === total,
      belowCountCap: rows.length < 100,
      unchanged: before === JSON.stringify(await f.rows('private_sends'))
    };
  });
  expect(result).toEqual({
    refused: 'capacity',
    actualBytes: true,
    meteredBytes: true,
    belowCountCap: true,
    unchanged: true
  });
});

test('CR008 a probe can take the mixed last slot while the next ordinary reservation is refused', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080;
    const records = Array.from({ length: 99 }, (_, n) =>
      n < 50 ? f.reservation(n) : f.probe(n)
    );
    await f.inject(
      'private_sends',
      records.map((row) => ({
        owner: row.owner,
        id: row.id,
        wire: s.privateRecordWire(f.handle(row), row.owner, row.id)
      }))
    );
    const probe = await s.commitPrivateRecord(
        f.repository,
        f.handle(f.probe(100)),
        null
      ),
      before = JSON.stringify(await f.rows('private_sends')),
      ordinary = await f.reserve(101);
    return {
      probe: probe.ok,
      ordinary: ordinary.ok ? 'unexpected' : ordinary.reason,
      unchanged: before === JSON.stringify(await f.rows('private_sends')),
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({
    probe: true,
    ordinary: 'capacity',
    unchanged: true,
    count: 100
  });
});

test('CR008 actual probe growth leaves insufficient physical bytes for an ordinary reservation', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      cap = 8 * 1024 * 1024;
    const makeRecord = (index: number) => {
      const probe = f.probe(index, true, true),
        self = probe.self!;
      return {
        ...probe,
        self,
        revision: 2,
        receipts: [
          {
            actionId: s.id(500),
            origin: probe.binding.readTargets[0],
            role: 'access_readback',
            attempt: 1,
            eventId: self.eventId,
            status: 'readback',
            observedAtMilliseconds: 1,
            readbackWire: self.wire
          }
        ]
      };
    };
    type Record = ReturnType<typeof makeRecord>;
    const stored = (record: { owner: string; id: string }) => ({
      owner: record.owner,
      id: record.id,
      wire: s.privateRecordWire(f.handle(record), record.owner, record.id)
    });
    const bytes = (record: { owner: string; id: string }) =>
      new TextEncoder().encode(JSON.stringify(stored(record))).length;
    // Signed-wire whitespace varies structural fixture charging, not inner
    // nonce/source proof. Every readback retains the same exact outer wire.
    const withWire = (record: Record, wire: string): Record => ({
      ...record,
      self: { ...record.self, wire },
      receipts: record.receipts.map((fact) => ({ ...fact, readbackWire: wire }))
    });
    const records: Record[] = [];
    let total = 0;
    for (let index = 0; index < 100; index++) {
      const record = makeRecord(index),
        size = bytes(record);
      if (total + size > cap) break;
      total += size;
      records.push(record);
    }
    if (!records.length || records.length >= 100)
      throw Error('byte-only fixture missing');
    const original = records[records.length - 1],
      originalBytes = bytes(original),
      next = { ...original, revision: 3, receipts: [...original.receipts] };
    for (
      let index = 0;
      index < 10 && total - originalBytes + bytes(next) <= cap;
      index++
    )
      next.receipts.push({
        ...original.receipts[0],
        actionId: s.id(600 + index)
      });
    const ordinaryBytes = bytes(f.reservation(999)),
      overflow = total - originalBytes + bytes(next) - cap,
      perCharacter =
        bytes(next) - bytes(withWire(next, original.self.wire.slice(0, -1))),
      trim = Math.ceil(
        (overflow + Math.floor(ordinaryBytes / 2)) / perCharacter
      );
    if (
      overflow <= 0 ||
      perCharacter <= 0 ||
      trim <= 0 ||
      !original.self.wire.endsWith('\n'.repeat(trim))
    )
      throw Error('invalid byte boundary adjustment');
    const wire = original.self.wire.slice(0, -trim),
      baseline = withWire(original, wire),
      candidate = withWire(next, wire);
    records[records.length - 1] = baseline;
    const baselineBytes = total - originalBytes + bytes(baseline),
      expected = baselineBytes - bytes(baseline) + bytes(candidate);
    if (expected > cap || cap - expected >= ordinaryBytes)
      throw Error('ordinary denial boundary not reached');
    await f.inject('private_sends', records.map(stored));
    const growth = await s.commitPrivateRecord(
        f.repository,
        f.handle(candidate),
        f.handle(baseline)
      ),
      before = JSON.stringify(await f.rows('private_sends')),
      ordinary = await f.reserve(999),
      inventory = await s.inspectPrivateStorage(f.repository, 'private_sends'),
      actual = (await f.rows('private_sends')).reduce<number>(
        (sum, row) =>
          sum + new TextEncoder().encode(JSON.stringify(row)).length,
        0
      );
    return {
      growth: growth.ok,
      ordinary: ordinary.ok ? 'unexpected' : ordinary.reason,
      bytesExact:
        actual === expected &&
        inventory.ok &&
        inventory.value.bytes === expected,
      belowCountCap: records.length < 100,
      unchanged: before === JSON.stringify(await f.rows('private_sends'))
    };
  });
  expect(result).toEqual({
    growth: true,
    ordinary: 'capacity',
    bytesExact: true,
    belowCountCap: true,
    unchanged: true
  });
});

test('CR008 ordinary reservation rejects a corrupt probe tail without skipping owner rows', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      good = f.probe(0),
      corrupt = f.probe(99);
    await f.inject('private_sends', [
      {
        owner: s.owner,
        id: good.id,
        wire: s.privateRecordWire(f.handle(good), s.owner, good.id)
      },
      {
        owner: s.owner,
        id: corrupt.id,
        wire: JSON.stringify({
          ...corrupt,
          nonce: 'PRIVATE_CORRUPTION_SENTINEL'
        })
      }
    ]);
    const before = JSON.stringify(await f.rows('private_sends')),
      result = await f.reserve(100);
    return {
      refused: result.ok ? 'unexpected' : result.reason,
      unchanged: before === JSON.stringify(await f.rows('private_sends'))
    };
  });
  expect(result).toEqual({ refused: 'corrupt_record', unchanged: true });
});

test('CR008 native abort cannot acknowledge or retain a new probe reservation', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp080Fixture,
      s = window.hcp080,
      row = f.probe(0),
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
    let committed;
    try {
      committed = await s.commitPrivateRecord(
        f.repository,
        f.handle(row),
        null
      );
    } finally {
      IDBObjectStore.prototype.put = original;
    }
    return {
      acknowledged: committed.ok,
      count: (await f.rows('private_sends')).length
    };
  });
  expect(result).toEqual({ acknowledged: false, count: 0 });
});
