import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
import type * as Records from '../../src/lib/persistence/records.ts';
import type * as Quota from '../../src/lib/persistence/quota.ts';
import type {
  PublicOperationRecord,
  PreferenceOperationRecord
} from '../../src/lib/contracts/local-records.ts';
import { getEventHash, finalizeEvent } from 'applesauce-core/helpers';
import { randomUUID } from 'node:crypto';
import type * as Drafts from '../../src/lib/persistence/drafts.ts';
declare global {
  interface Window {
    hcp049: typeof Database & typeof Drafts & typeof Quota & typeof Records;
    hcp049Database: Database.BrowserDatabase;
    hcp049Repository: Drafts.PublicDraftRepository;
    hcp049Quota: Quota.PublicQuotaRepository;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
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
        name: 'hcp049-test-entry',
        resolveId(id) {
          if (id === 'virtual:hcp049') return '\0hcp049';
        },
        load(id) {
          if (id === '\0hcp049')
            return `export * from ${JSON.stringify(source('database.ts'))};export * from ${JSON.stringify(source('drafts.ts'))};export * from ${JSON.stringify(source('quota.ts'))};export * from ${JSON.stringify(source('records.ts'))};`;
        }
      }
    ],
    build: {
      write: false,
      minify: false,
      rolldownOptions: { input: 'virtual:hcp049' },
      lib: { entry: 'virtual:hcp049', name: 'hcp049', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing test bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP049_ACTUAL_MODULES_REAL_IDB',
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
    const api = window.hcp049;
    const opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    const repository = api.createPublicDraftRepository(
      opened.owner,
      expectedOwner
    );
    if (!repository) throw new Error('missing repository');
    window.hcp049Database = opened.owner;
    window.hcp049Repository = repository;
    const quota = api.createPublicQuotaRepository(opened.owner, expectedOwner);
    if (!quota) throw new Error('missing quota repository');
    window.hcp049Quota = quota;
  }, selectedOwner);
}
function operation(content = 'public quota fixture'): PublicOperationRecord {
  const id = randomUUID();
  const template = {
    pubkey: owner,
    kind: 30402,
    created_at: 100,
    tags: [
      ['d', 'quota'],
      ['published_at', '100']
    ],
    content
  };
  return {
    schema: 1,
    family: 'public_operation',
    owner,
    id,
    revision: 0,
    source: { type: 'draft', id, revision: 0 },
    capture: {
      kind: 30402,
      wire: JSON.stringify(template),
      hash: getEventHash(template),
      targets: ['wss://one.example.org'],
      policyFingerprint: 'a'.repeat(64)
    },
    artifact: null,
    receipts: []
  };
}
async function admit(
  page: Page,
  row: PublicOperationRecord | PreferenceOperationRecord
) {
  return page.evaluate(async (row) => {
    const api = window.hcp049;
    const decoded = api.decodePublicRecord(
      JSON.stringify(row),
      row.owner,
      row.id
    );
    if (!decoded.ok) throw new Error(decoded.reason);
    return api.admitPublicOperation(window.hcp049Quota, row.id, decoded.value);
  }, row);
}
async function seed(
  page: Page,
  rows: readonly (PublicOperationRecord | PreferenceOperationRecord)[]
) {
  await page.evaluate(async (rows) => {
    const api = window.hcp049;
    for (const row of rows) {
      const decoded = api.decodePublicRecord(
        JSON.stringify(row),
        row.owner,
        row.id
      );
      if (!decoded.ok) throw new Error(decoded.reason);
    }
    const transaction = api.browserDatabaseTransaction(
      window.hcp049Database,
      ['public_operations', 'preference_operations'],
      'readwrite'
    );
    for (const row of rows)
      transaction
        .objectStore(
          row.family === 'public_operation'
            ? 'public_operations'
            : 'preference_operations'
        )
        .put({ owner: row.owner, id: row.id, wire: JSON.stringify(row) });
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(new Error('fixture seed aborted'))
      );
    });
  }, rows);
}
test('two genuine tabs race the final operation slot; refusal preserves all original records', async ({
  page,
  context
}) => {
  await load(page);
  const initial = Array.from({ length: 99 }, () => operation());
  await seed(page, initial);
  const second = await context.newPage();
  await load(second);
  const candidates = [operation(), operation()];
  const results = await Promise.all([
    admit(page, candidates[0]),
    admit(second, candidates[1])
  ]);
  expect(results.filter((row) => row.ok)).toHaveLength(1);
  expect(results.filter((row) => !row.ok)).toEqual([
    { ok: false, reason: 'capacity' }
  ]);
  const snapshot = await page.evaluate(async () => {
    const result = await window.hcp049.inspectPublicStorage(window.hcp049Quota);
    if (!result.ok) throw new Error(result.reason);
    return window.hcp049.publicInventorySnapshot(
      window.hcp049Quota,
      result.value
    );
  });
  expect(snapshot?.rows).toHaveLength(100);
  for (const row of initial)
    expect(snapshot?.rows.some((stored) => stored.id === row.id)).toBe(true);
  await second.close();
});
test('aggregate 8MiB logical wire admission rejects before count100 and keeps the existing bytes', async ({
  page
}) => {
  await load(page);
  const sample = operation('p'.repeat(220000));
  const size = Buffer.byteLength(JSON.stringify(sample));
  const count = Math.floor(8388608 / size);
  expect(count).toBeLessThan(100);
  const rows = Array.from({ length: count }, () => ({
    ...sample,
    id: randomUUID()
  }));
  await seed(page, rows);
  expect(await admit(page, { ...sample, id: randomUUID() })).toEqual({
    ok: false,
    reason: 'capacity'
  });
  const result = await page.evaluate(async () => {
    const api = window.hcp049,
      inv = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inv.ok) throw new Error(inv.reason);
    return api.publicInventorySnapshot(window.hcp049Quota, inv.value);
  });
  expect(result?.rows).toHaveLength(count);
  expect(result?.rows.reduce((sum, row) => sum + row.logicalBytes, 0)).toBe(
    count * size
  );
});
test('reviewed draft cleanup reports local-loss consequences and atomically deletes only the selected record', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp049;
    const first = await api.createPublicDraft(window.hcp049Repository, form);
    const second = await api.createPublicDraft(window.hcp049Repository, {
      ...form,
      title: 'keep'
    });
    if (!first.ok || !second.ok) throw new Error('missing drafts');
    const inv = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inv.ok) throw new Error(inv.reason);
    const key = 'public_draft:' + first.value.id;
    const reviewed = api.reviewPublicCleanup(window.hcp049Quota, inv.value, [
      key
    ]);
    if (!reviewed.ok) throw new Error(reviewed.reason);
    const impact = api.publicCleanupSnapshot(
      window.hcp049Quota,
      reviewed.value
    );
    const committed = await api.commitPublicCleanup(
      window.hcp049Quota,
      reviewed.value
    );
    const rows = await api.listPublicDrafts(window.hcp049Repository);
    return { impact, committed, rows, second: second.value, key };
  }, form);
  expect(result.impact?.selected).toHaveLength(1);
  expect(result.impact?.consequences).toContain(
    'does not delete remote copies'
  );
  expect(result.committed).toEqual({ ok: true, value: [result.key] });
  expect(result.rows).toEqual({ ok: true, value: [result.second] });
});
test('pending capture is protected and changed inventory conflicts without partial deletion', async ({
  page
}) => {
  await load(page);
  const row = operation();
  expect((await admit(page, row)).ok).toBe(true);
  const result = await page.evaluate(
    async ({ form, row }) => {
      const api = window.hcp049;
      const draft = await api.createPublicDraft(window.hcp049Repository, form);
      if (!draft.ok) throw new Error(draft.reason);
      const inv = await api.inspectPublicStorage(window.hcp049Quota);
      if (!inv.ok) throw new Error(inv.reason);
      const protectedResult = api.reviewPublicCleanup(
        window.hcp049Quota,
        inv.value,
        ['public_operation:' + row.id]
      );
      const reviewed = api.reviewPublicCleanup(window.hcp049Quota, inv.value, [
        'public_draft:' + draft.value.id
      ]);
      if (!reviewed.ok) throw new Error(reviewed.reason);
      const updated = await api.savePublicDraft(
        window.hcp049Repository,
        draft.value.id,
        0,
        { ...form, title: 'new revision' }
      );
      const committed = await api.commitPublicCleanup(
        window.hcp049Quota,
        reviewed.value
      );
      const retained = await api.readPublicDraft(
        window.hcp049Repository,
        draft.value.id
      );
      return { protectedResult, updated, committed, retained };
    },
    { form, row }
  );
  expect(result.protectedResult).toEqual({ ok: false, reason: 'protected' });
  expect(result.committed).toEqual({ ok: false, reason: 'conflict' });
  expect(result.retained).toEqual(result.updated);
});
test('an unknown record added after review aborts cleanup; fixed public stores never enumerate private metadata', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp049;
    const draft = await api.createPublicDraft(window.hcp049Repository, form);
    if (!draft.ok) throw new Error(draft.reason);
    const inv = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inv.ok) throw new Error(inv.reason);
    const reviewed = api.reviewPublicCleanup(window.hcp049Quota, inv.value, [
      'public_draft:' + draft.value.id
    ]);
    if (!reviewed.ok) throw new Error(reviewed.reason);
    const transaction = api.browserDatabaseTransaction(
      window.hcp049Database,
      ['public_operations', 'private_sends'],
      'readwrite'
    );
    transaction.objectStore('public_operations').put({
      owner: draft.value.owner,
      id: '3d030bf5-901d-45e1-8251-41cbdf805e96',
      wire: '{"schema":99}'
    });
    transaction.objectStore('private_sends').put({
      owner: draft.value.owner,
      id: 'b1d6d23d-79f3-4779-9b03-e289712cc73f',
      wire: 'private metadata sentinel'
    });
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(new Error('fixture abort'))
      );
    });
    const acquired: string[][] = [];
    const descriptor = Object.getOwnPropertyDescriptor(
      IDBDatabase.prototype,
      'transaction'
    );
    if (!descriptor) throw new Error('missing transaction descriptor');
    const original = descriptor.value as IDBDatabase['transaction'];
    IDBDatabase.prototype.transaction = function (storeNames, mode, options) {
      acquired.push(
        typeof storeNames === 'string' ? [storeNames] : Array.from(storeNames)
      );
      return original.call(this, storeNames, mode, options);
    };
    try {
      const committed = await api.commitPublicCleanup(
        window.hcp049Quota,
        reviewed.value
      );
      const retained = await api.readPublicDraft(
        window.hcp049Repository,
        draft.value.id
      );
      return { committed, retained, acquired };
    } finally {
      IDBDatabase.prototype.transaction = original;
    }
  }, form);
  expect(result.committed).toEqual({ ok: false, reason: 'corrupt_record' });
  expect(result.retained.ok).toBe(true);
  expect(
    result.acquired
      .flat()
      .every((store) =>
        [
          'public_drafts',
          'public_operations',
          'preference_operations'
        ].includes(store)
      )
  ).toBe(true);
});

test('cleanup selection reads the reviewed indexed array rather than a replaced iterator', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp049;
    const first = await api.createPublicDraft(window.hcp049Repository, form);
    const second = await api.createPublicDraft(window.hcp049Repository, form);
    if (!first.ok || !second.ok) throw new Error('missing drafts');
    const inventory = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inventory.ok) throw new Error(inventory.reason);
    const firstKey = 'public_draft:' + first.value.id;
    const secondKey = 'public_draft:' + second.value.id;
    const selection = [firstKey];
    let iteratorReads = 0;
    selection[Symbol.iterator] = () => {
      iteratorReads++;
      return [secondKey].values();
    };
    const reviewed = api.reviewPublicCleanup(
      window.hcp049Quota,
      inventory.value,
      selection
    );
    if (!reviewed.ok) throw new Error(reviewed.reason);
    return {
      impact: api.publicCleanupSnapshot(window.hcp049Quota, reviewed.value),
      firstKey,
      iteratorReads
    };
  }, form);
  expect(result.iteratorReads).toBe(0);
  expect(result.impact?.selected.map((row) => row.key)).toEqual([
    result.firstKey
  ]);
});

function preference(
  content = 'public inbox preference fixture'
): PreferenceOperationRecord {
  const id = randomUUID();
  const template = {
    pubkey: owner,
    kind: 10050,
    created_at: 100,
    tags: [],
    content
  };
  return {
    schema: 1,
    family: 'preference_operation',
    owner,
    id,
    revision: 0,
    consent: 'explicit_review',
    source: { type: 'inbox_head', wire: null },
    capture: {
      kind: 10050,
      wire: JSON.stringify(template),
      hash: getEventHash(template),
      targets: ['wss://one.example.org'],
      policyFingerprint: 'a'.repeat(64)
    },
    artifact: null,
    receipts: []
  };
}
test('public and preference journals share the count ceiling and exact-wire idempotent replay', async ({
  page
}) => {
  await load(page);
  const first = operation();
  expect((await admit(page, first)).ok).toBe(true);
  expect(await admit(page, first)).toEqual({
    ok: true,
    value: { id: first.id, revision: 0 }
  });
  await seed(
    page,
    Array.from({ length: 98 }, (_, index) =>
      index % 2 ? operation() : preference()
    )
  );
  const last = preference();
  expect((await admit(page, last)).ok).toBe(true);
  expect(await admit(page, operation())).toEqual({
    ok: false,
    reason: 'capacity'
  });
  const snapshot = await page.evaluate(async () => {
    const api = window.hcp049,
      inv = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inv.ok) throw new Error(inv.reason);
    return api.publicInventorySnapshot(window.hcp049Quota, inv.value);
  });
  expect(snapshot?.rows).toHaveLength(100);
  expect(
    snapshot?.rows.filter((row) => row.family === 'preference_operation')
  ).toHaveLength(50);
});
test('mixed preference/public exact stored wires share the 8MiB aggregate ceiling', async ({
  page
}) => {
  await load(page);
  const publicSample = operation('p'.repeat(220000)),
    preferenceSample = preference('q'.repeat(220000));
  let bytes = 0;
  const rows: (PublicOperationRecord | PreferenceOperationRecord)[] = [];
  for (let index = 0; index < 100; index++) {
    const row = {
      ...(index % 2 ? preferenceSample : publicSample),
      id: randomUUID()
    };
    const size = Buffer.byteLength(JSON.stringify(row));
    if (bytes + size > 8388608) break;
    rows.push(row);
    bytes += size;
  }
  await seed(page, rows);
  expect(await admit(page, { ...preferenceSample, id: randomUUID() })).toEqual({
    ok: false,
    reason: 'capacity'
  });
  const snapshot = await page.evaluate(async () => {
    const api = window.hcp049,
      inv = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inv.ok) throw new Error(inv.reason);
    return api.publicInventorySnapshot(window.hcp049Quota, inv.value);
  });
  expect(snapshot?.rows.reduce((sum, row) => sum + row.logicalBytes, 0)).toBe(
    bytes
  );
  expect(
    snapshot?.rows.some((row) => row.family === 'preference_operation')
  ).toBe(true);
});
function signedSettledOperation(): PublicOperationRecord {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const base = finalizeEvent(
      {
        kind: 30402,
        created_at: 100,
        tags: [['d', 'quota']],
        content: 'public signature fixture'
      },
      key
    );
    const template = {
      pubkey: base.pubkey,
      kind: 30402,
      created_at: 101,
      tags: [['d', 'quota']],
      content: 'public settled artifact fixture'
    };
    const hash = getEventHash(template);
    const wire = JSON.stringify(template);
    const artifact = finalizeEvent(
      { ...template, tags: template.tags.map((tag) => [...tag]) },
      key
    );
    const id = randomUUID();
    return {
      schema: 1,
      family: 'public_operation',
      owner: base.pubkey,
      id,
      revision: 0,
      source: { type: 'draft', id, revision: 0 },
      capture: {
        kind: 30402,
        wire,
        hash,
        targets: ['wss://one.example.org'],
        policyFingerprint: 'a'.repeat(64)
      },
      artifact: { eventId: artifact.id, wire: JSON.stringify(artifact) },
      receipts: [
        {
          actionId: randomUUID(),
          origin: 'wss://one.example.org',
          role: 'publication',
          attempt: 1,
          eventId: artifact.id,
          status: 'accepted',
          observedAtMilliseconds: 1000,
          readbackWire: null
        }
      ]
    };
  } finally {
    key.fill(0);
  }
}
test('actual verified signed settled operation permits reviewed local cleanup; unresolved history remains protected', async ({
  page
}) => {
  const settled = signedSettledOperation();
  await load(page, settled.owner);
  const uncertain: PublicOperationRecord = {
    ...settled,
    id: randomUUID(),
    receipts: [
      ...settled.receipts,
      {
        ...settled.receipts[0],
        status: 'unknown',
        observedAtMilliseconds: 1001
      }
    ]
  };
  expect((await admit(page, settled)).ok).toBe(true);
  expect((await admit(page, uncertain)).ok).toBe(true);
  const result = await page.evaluate(
    async ({ settled, uncertain }) => {
      const api = window.hcp049,
        inv = await api.inspectPublicStorage(window.hcp049Quota);
      if (!inv.ok) throw new Error(inv.reason);
      const denied = api.reviewPublicCleanup(window.hcp049Quota, inv.value, [
        'public_operation:' + uncertain.id
      ]);
      const reviewed = api.reviewPublicCleanup(window.hcp049Quota, inv.value, [
        'public_operation:' + settled.id
      ]);
      if (!reviewed.ok) throw new Error(reviewed.reason);
      const removed = await api.commitPublicCleanup(
        window.hcp049Quota,
        reviewed.value
      );
      const next = await api.inspectPublicStorage(window.hcp049Quota);
      if (!next.ok) throw new Error(next.reason);
      return {
        denied,
        removed,
        remaining: api.publicInventorySnapshot(window.hcp049Quota, next.value)
      };
    },
    { settled, uncertain }
  );
  expect(result.denied).toEqual({ ok: false, reason: 'protected' });
  expect(result.removed).toEqual({
    ok: true,
    value: ['public_operation:' + settled.id]
  });
  expect(result.remaining?.rows.map((row) => row.id)).toEqual([uncertain.id]);
});
test('successful native put and delete requests followed by transaction abort never acknowledge or lose records', async ({
  page
}) => {
  await load(page);
  const row = operation();
  const result = await page.evaluate(
    async ({ row, form }) => {
      const api = window.hcp049;
      const decoded = api.decodePublicRecord(
        JSON.stringify(row),
        row.owner,
        row.id
      );
      if (!decoded.ok) throw new Error(decoded.reason);
      const putDescriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      );
      const deleteDescriptor = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'delete'
      );
      if (!putDescriptor || !deleteDescriptor)
        throw new Error('missing native methods');
      const nativePut = putDescriptor.value as IDBObjectStore['put'];
      const nativeDelete = deleteDescriptor.value as IDBObjectStore['delete'];
      let putSuccesses = 0,
        deleteSuccesses = 0;
      IDBObjectStore.prototype.put = function (...args) {
        const request = nativePut.apply(this, args);
        request.addEventListener('success', () => {
          putSuccesses++;
          this.transaction.abort();
        });
        return request;
      };
      let inserted: Quota.PublicQuotaResult<
        Readonly<{ id: string; revision: number }>
      >;
      try {
        inserted = await api.admitPublicOperation(
          window.hcp049Quota,
          row.id,
          decoded.value
        );
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', putDescriptor);
      }
      const draft = await api.createPublicDraft(window.hcp049Repository, form);
      if (!draft.ok) throw new Error(draft.reason);
      const inventory = await api.inspectPublicStorage(window.hcp049Quota);
      if (!inventory.ok) throw new Error(inventory.reason);
      const reviewed = api.reviewPublicCleanup(
        window.hcp049Quota,
        inventory.value,
        ['public_draft:' + draft.value.id]
      );
      if (!reviewed.ok) throw new Error(reviewed.reason);
      IDBObjectStore.prototype.delete = function (...args) {
        const request = nativeDelete.apply(this, args);
        request.addEventListener('success', () => {
          deleteSuccesses++;
          this.transaction.abort();
        });
        return request;
      };
      let deleted: Quota.PublicQuotaResult<readonly string[]>;
      try {
        deleted = await api.commitPublicCleanup(
          window.hcp049Quota,
          reviewed.value
        );
      } finally {
        Object.defineProperty(
          IDBObjectStore.prototype,
          'delete',
          deleteDescriptor
        );
      }
      const retained = await api.readPublicDraft(
        window.hcp049Repository,
        draft.value.id
      );
      const next = await api.inspectPublicStorage(window.hcp049Quota);
      if (!next.ok) throw new Error(next.reason);
      return {
        inserted,
        deleted,
        putSuccesses,
        deleteSuccesses,
        retained,
        inventory: api.publicInventorySnapshot(window.hcp049Quota, next.value)
      };
    },
    { row, form }
  );
  expect(result.putSuccesses).toBe(1);
  expect(result.deleteSuccesses).toBe(1);
  expect(result.inserted).toEqual({ ok: false, reason: 'aborted' });
  expect(result.deleted).toEqual({ ok: false, reason: 'aborted' });
  expect(result.retained.ok).toBe(true);
  expect(result.inventory?.rows).toHaveLength(1);
  expect(result.inventory?.rows[0].family).toBe('public_draft');
});

test('negative proxy-array length cannot mint an empty cleanup authorization', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp049;
    const draft = await api.createPublicDraft(window.hcp049Repository, form);
    if (!draft.ok) throw new Error(draft.reason);
    const inventory = await api.inspectPublicStorage(window.hcp049Quota);
    if (!inventory.ok) throw new Error(inventory.reason);
    let indexedReads = 0;
    const selection = new Proxy(['public_draft:' + draft.value.id], {
      get(target, key, receiver) {
        if (key === 'length') return -1;
        if (key === '0') indexedReads++;
        return Reflect.get(target, key, receiver) as unknown;
      }
    });
    const reviewed = api.reviewPublicCleanup(
      window.hcp049Quota,
      inventory.value,
      selection
    );
    return { reviewed, indexedReads };
  }, form);
  expect(result.reviewed).toEqual({ ok: false, reason: 'invalid_selection' });
  expect(result.indexedReads).toBe(0);
});
