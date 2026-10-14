import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Reservations from './harness/private-send-reservations.ts';
import type { RumorPlan } from '../../src/lib/messaging/rumor-plan.ts';
declare global {
  interface Window {
    hcp074: typeof Reservations;
    hcp074Fixture: Awaited<ReturnType<typeof Reservations.makeFixture>>;
    hcp074Plan: RumorPlan;
    hcp074Release: () => void;
    hcp074Held: Promise<unknown>;
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
        name: 'hcp074',
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
      fixture: 'HCP074_ACTUAL_OWNER_LOCKS_IDB_RESERVATIONS',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Controlled local Chromium clock/provider; not actual operator/installed extension/real clock qualification'
    })
  );
});
test.afterAll(async () => server.close());
async function load(
  page: Page,
  shared?: Reservations.Shared,
  clock: 'controlled' | 'real' = 'controlled'
) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  return page.evaluate(
    async ({ shared, clock }) => {
      window.hcp074Fixture = await window.hcp074.makeFixture(shared, clock);
      window.hcp074Plan = window.hcp074Fixture.plan();
      return window.hcp074Fixture.shared();
    },
    { shared, clock }
  );
}
test('one command retries the identical rumor and stores only bounded metadata', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp074,
      f = window.hcp074Fixture,
      before = f.counts();
    const first = await f.reserve(s.command, window.hcp074Plan),
      retry = await f.reserve(s.command, window.hcp074Plan);
    const metadata =
      first.status === 'reserved'
        ? s.reservedSendSnapshot(first.identity)
        : undefined;
    const wire =
      first.status === 'reserved'
        ? s.reservedSendRumorWire(first.identity)
        : undefined;
    const rows = await f.rows();
    return {
      first: first.status,
      retry: retry.status,
      metadata,
      wirePresent: typeof wire === 'string',
      stored: JSON.stringify(rows),
      counts: f.counts(),
      before
    };
  });
  expect(result.first).toBe('reserved');
  expect(result.retry).toBe('existing');
  expect(result.wirePresent).toBe(true);
  expect(result.stored).not.toContain('Private reservation sentinel');
  expect(result.stored).not.toContain('Carrots');
  expect(result.stored).not.toContain('nostr:');
  expect(result.stored).not.toContain('"content"');
  expect(result.counts).toEqual(result.before);
  expect(result.metadata?.id).toBe('12345678-1234-4234-8234-123456789abc');
});
test('changed same command conflicts and distinct same-second identical intent cannot merge', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture,
      s = window.hcp074;
    await f.reserve(s.command, window.hcp074Plan);
    const changed = await f.reserve(s.command, f.plan('Different body'));
    const collision = await f.reserve(
      '12345678-1234-4234-8234-123456789abd',
      f.plan()
    );
    return {
      changed: changed.status,
      collision: collision.status,
      count: (await f.rows()).length
    };
  });
  expect(result).toEqual({
    changed: 'conflict',
    collision: 'clock_conflict',
    count: 1
  });
});
test('actual observed clock progress permits a distinct intent without future timestamp or custom tags', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp074,
      f = window.hcp074Fixture,
      original = f.shared().milliseconds;
    const first = await f.reserve(s.command, window.hcp074Plan);
    f.setTime(original + 1000);
    const nextPlan = f.plan();
    const next = await f.reserve(
      '12345678-1234-4234-8234-123456789abd',
      nextPlan
    );
    if (first.status !== 'reserved' || next.status !== 'reserved')
      throw Error('expected source reservations');
    const a = s.reservedSendSnapshot(first.identity)!,
      b = s.reservedSendSnapshot(next.identity)!;
    const wire = JSON.parse(s.reservedSendRumorWire(next.identity)!) as {
      created_at: number;
      tags: string[][];
    };
    const retry = await f.reserve(s.command, window.hcp074Plan);
    return {
      different: a.rumorHash !== b.rumorHash,
      observed: wire.created_at === Math.floor(Date.now() / 1000),
      pOnly: wire.tags.length === 1 && wire.tags[0][0] === 'p',
      retry: retry.status,
      count: (await f.rows()).length
    };
  });
  expect(result).toEqual({
    different: true,
    observed: true,
    pOnly: true,
    retry: 'existing',
    count: 2
  });
});
test('future clock is rejected and an old provisional plan binds the actual reservation second', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture,
      seconds = Math.floor(Date.now() / 1000);
    const future = await f.reserve(
      window.hcp074.command,
      f.plan('Future', seconds + 1)
    );
    const stale = await f.reserve(
      window.hcp074.command,
      f.plan('Stale', seconds - 1)
    );
    return {
      future: future.status,
      stale: stale.status,
      count: (await f.rows()).length
    };
  });
  expect(result).toEqual({
    future: 'clock_conflict',
    stale: 'reserved',
    count: 1
  });
});
test('two tabs share owner CAS and only one distinct identical intent can reserve', async ({
  page,
  context
}) => {
  const shared = await load(page),
    second = await context.newPage();
  await load(second, shared);
  const [a, b] = await Promise.all([
    page.evaluate(
      async () =>
        (
          await window.hcp074Fixture.reserve(
            window.hcp074.command,
            window.hcp074Plan
          )
        ).status
    ),
    second.evaluate(
      async () =>
        (
          await window.hcp074Fixture.reserve(
            '12345678-1234-4234-8234-123456789abd',
            window.hcp074Plan
          )
        ).status
    )
  ]);
  expect([a, b].filter((status) => status === 'reserved')).toHaveLength(1);
  expect(
    [a, b].filter((status) => status === 'busy' || status === 'clock_conflict')
  ).toHaveLength(1);
  expect(
    await page.evaluate(async () => (await window.hcp074Fixture.rows()).length)
  ).toBe(1);
});
test('concurrent calls on one genuine plan keep a busy loser protected after the winner binds', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const manager = navigator.locks,
      original = Reflect.get<LockManager, 'request'>(manager, 'request');
    let release = () => {},
      entered = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const committed = new Promise<void>((resolve) => {
      entered = resolve;
    });
    manager.request = (...args: unknown[]) => {
      const index = args.length - 1,
        callback = args[index] as (lock: Lock | null) => Promise<unknown>;
      const forwarded = args.slice();
      forwarded[index] = async (lock: Lock | null) => {
        const value = await callback(lock);
        if (lock) {
          entered();
          await held;
        }
        return value;
      };
      return Reflect.apply(original, manager, forwarded) as Promise<unknown>;
    };
    try {
      const f = window.hcp074Fixture,
        s = window.hcp074;
      // Both calls enter the boundary before the actual first lock callback.
      const winner = f.reserve(s.command, window.hcp074Plan),
        loser = f.reserve(s.command, window.hcp074Plan);
      const refused = await loser;
      await committed;
      const rows = await f.rows();
      release();
      const accepted = await winner;
      return { accepted: accepted.status, refused, count: rows.length };
    } finally {
      release();
      manager.request = original;
    }
  });
  expect(result).toEqual({
    accepted: 'reserved',
    refused: { status: 'busy', disposition: 'may_exist' },
    count: 1
  });
});

test('stopping a fresh original before its real owner callback proves no attempt and permits a new draft', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture,
      s = window.hcp074,
      before = f.counts(),
      manager = navigator.locks,
      original = Reflect.get<LockManager, 'request'>(manager, 'request');
    manager.request = (...args: unknown[]) => {
      const index = args.length - 1,
        callback = args[index] as (lock: Lock | null) => Promise<unknown>,
        forwarded = args.slice();
      forwarded[index] = (lock: Lock | null) => {
        if (lock) s.stopRumorPlan(window.hcp074Plan);
        return callback(lock);
      };
      return Reflect.apply(original, manager, forwarded) as Promise<unknown>;
    };
    let stopped: Awaited<ReturnType<typeof f.reserve>>;
    try {
      stopped = await f.reserve(s.command, window.hcp074Plan);
    } finally {
      manager.request = original;
    }
    const absent = (await f.rows()).length === 0;
    const next = await f.reserve(s.command, f.plan('New deliberate body'));
    return {
      stopped,
      absent,
      next: next.status,
      count: (await f.rows()).length,
      unchangedSdk: JSON.stringify(before) === JSON.stringify(f.counts())
    };
  });
  expect(result).toEqual({
    stopped: { status: 'stopped', disposition: 'not_created' },
    absent: true,
    next: 'reserved',
    count: 1,
    unchangedSdk: true
  });
});

test('an actual unknown completion reconciles the original full row without a second command', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const original = Reflect.get<IDBObjectStore, 'put'>(
      IDBObjectStore.prototype,
      'put'
    );
    IDBObjectStore.prototype.put = function (...args) {
      const request = Reflect.apply(original, this, args);
      this.transaction.abort = () => {
        throw Error('controlled lost abort capability');
      };
      throw Error(
        'controlled lost result after queued write ' +
          String(request.readyState)
      );
    };
    const f = window.hcp074Fixture,
      s = window.hcp074;
    let uncertain: Awaited<ReturnType<typeof f.reserve>>;
    try {
      uncertain = await f.reserve(s.command, window.hcp074Plan);
    } finally {
      IDBObjectStore.prototype.put = original;
    }
    const before = await f.rows(),
      retry = await f.reserve(s.command, window.hcp074Plan),
      after = await f.rows();
    return {
      uncertain,
      retry: retry.status,
      count: after.length,
      unchanged: JSON.stringify(before) === JSON.stringify(after),
      counts: f.counts()
    };
  });
  expect(result).toMatchObject({
    uncertain: { status: 'unknown_completion', disposition: 'may_exist' },
    retry: 'existing',
    count: 1,
    unchanged: true,
    counts: { signs: 0, encrypts: 1, decrypts: 1 }
  });
});
test('held origin owner lock reports busy and identity disconnect cannot mint a reservation', async ({
  page
}) => {
  await load(page);
  await page.evaluate(async () => {
    let entered = () => {};
    const ready = new Promise<void>((resolve) => {
      entered = resolve;
    });
    window.hcp074Held = navigator.locks.request(
      'harvestcircle:owner:' + window.hcp074.owner,
      () =>
        new Promise<void>((resolve) => {
          window.hcp074Release = resolve;
          entered();
        })
    );
    await ready;
  });
  expect(
    await page.evaluate(
      async () =>
        (
          await window.hcp074Fixture.reserve(
            window.hcp074.command,
            window.hcp074Plan
          )
        ).status
    )
  ).toBe('busy');
  await page.evaluate(async () => {
    window.hcp074Release();
    await window.hcp074Held;
    window.hcp074Fixture.disconnect();
  });
  const result = await page.evaluate(async () => ({
    status: (
      await window.hcp074Fixture.reserve(
        window.hcp074.command,
        window.hcp074Plan
      )
    ).status,
    count: (await window.hcp074Fixture.rows()).length
  }));
  expect(result.status).toBe('stopped');
  expect(result.count).toBe(0);
});
test('corrupt owner records stay preserved without implicit reset', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture;
    await f.inject({
      owner: window.hcp074.owner,
      id: '12345678-1234-4234-8234-123456789abe',
      wire: '{bad'
    });
    const before = JSON.stringify(await f.rows()),
      value = await f.reserve(window.hcp074.command, window.hcp074Plan);
    return {
      status: value.status,
      unchanged: before === JSON.stringify(await f.rows())
    };
  });
  expect(result).toEqual({ status: 'corrupt_record', unchanged: true });
});
test('another owner namespace is ignored and remains preserved', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture,
      other =
        'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
    const foreign = {
      owner: other,
      id: window.hcp074.command,
      wire: JSON.stringify({
        schema: 1,
        family: 'private_send_reservation',
        owner: other,
        id: window.hcp074.command,
        revision: 0,
        peer: window.hcp074.owner,
        rumorHash: 'f'.repeat(64),
        createdAt: Math.floor(Date.now() / 1000)
      })
    };
    await f.inject(foreign);
    const value = await f.reserve(window.hcp074.command, window.hcp074Plan);
    const rows = await f.rows();
    return {
      status: value.status,
      count: rows.length,
      foreignPreserved: rows.some(
        (row) => JSON.stringify(row) === JSON.stringify(foreign)
      )
    };
  });
  expect(result).toEqual({
    status: 'reserved',
    count: 2,
    foreignPreserved: true
  });
});
test('real transaction abort never grants a reserved identity or leaves metadata', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const original = Reflect.get<IDBObjectStore, 'put'>(
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
    let value: Awaited<ReturnType<typeof window.hcp074Fixture.reserve>>;
    try {
      value = await window.hcp074Fixture.reserve(
        window.hcp074.command,
        window.hcp074Plan
      );
    } finally {
      IDBObjectStore.prototype.put = original;
    }
    return {
      status: value.status,
      disposition: value.disposition,
      identityPresent: 'identity' in value,
      count: (await window.hcp074Fixture.rows()).length
    };
  });
  expect(['aborted', 'unknown_completion']).toContain(result.status);
  expect(result.identityPresent).toBe(false);
  expect(result.disposition).toBe('not_created');
  expect(result.count).toBe(0);
});
test('100 unfinished metadata reservations enforce the existing count cap without eviction', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp074,
      f = window.hcp074Fixture;
    for (let index = 0; index < 100; index++) {
      const id =
        '12345678-1234-4234-8234-' + index.toString(16).padStart(12, '0');
      const value = await f.reserve(id, f.plan('Intent ' + index));
      if (value.status !== 'reserved')
        throw Error('capacity fixture not admitted');
    }
    const value = await f.reserve(s.command, window.hcp074Plan);
    return { status: value.status, count: (await f.rows()).length };
  });
  expect(result).toEqual({ status: 'capacity', count: 100 });
});

test('actual advancing browser wall clock reserves once at the owner boundary and preserves original retry identity', async ({
  page
}) => {
  await load(page, undefined, 'real');
  const result = await page.evaluate(async () => {
    const f = window.hcp074Fixture,
      s = window.hcp074;
    const before = Date.now();
    await new Promise<void>((resolve) => setTimeout(resolve, 1100));
    const lower = Math.floor(Date.now() / 1000);
    const first = await f.reserve(s.command, window.hcp074Plan);
    if (first.status !== 'reserved')
      throw Error('advancing real-clock reservation failed');
    const original = s.reservedSendSnapshot(first.identity)!;
    const upper = Math.floor(Date.now() / 1000);
    await new Promise<void>((resolve) => setTimeout(resolve, 1100));
    const retry = await f.reserve(s.command, window.hcp074Plan);
    if (retry.status !== 'existing')
      throw Error('original reconciliation failed');
    return {
      advanced: Date.now() - before >= 2200,
      lower,
      upper,
      original,
      repeated: s.reservedSendSnapshot(retry.identity),
      count: (await f.rows()).length
    };
  });
  expect(result.advanced).toBe(true);
  expect(result.original.createdAt).toBeGreaterThanOrEqual(result.lower);
  expect(result.original.createdAt).toBeLessThanOrEqual(result.upper);
  expect(result.repeated).toEqual(result.original);
  expect(result.count).toBe(1);
});
