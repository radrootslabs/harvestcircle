import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { finalizeEvent, getEventHash } from 'applesauce-core/helpers';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
import type * as Records from '../../src/lib/persistence/records.ts';
import type * as Quota from '../../src/lib/persistence/quota.ts';
import type * as Artifacts from '../../src/lib/persistence/artifact-records.ts';
import type {
  PublicOperationRecord,
  PublicTargetReceipt
} from '../../src/lib/contracts/local-records.ts';
declare global {
  interface Window {
    hcp050: typeof Database & typeof Records & typeof Quota & typeof Artifacts;
    hcp050Database: Database.BrowserDatabase;
    hcp050Quota: Quota.PublicQuotaRepository;
    hcp050Transition: Artifacts.PublicOperationTransition;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const source = (name: string) =>
    fileURLToPath(
      new URL('../../src/lib/persistence/' + name, import.meta.url)
    );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [
      {
        name: 'hcp050-test-entry',
        resolveId(id) {
          if (id === 'virtual:hcp050') return '\0hcp050';
        },
        load(id) {
          if (id === '\0hcp050')
            return [
              'database.ts',
              'records.ts',
              'quota.ts',
              'artifact-records.ts'
            ]
              .map((n) => `export * from ${JSON.stringify(source(n))};`)
              .join('');
        }
      }
    ],
    build: {
      write: false,
      minify: false,
      rolldownOptions: { input: 'virtual:hcp050' },
      lib: { entry: 'virtual:hcp050', name: 'hcp050', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP050_ACTUAL_MODULES_REAL_IDB',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      exposure: 'test-only virtual entry'
    })
  );
});
test.afterAll(async () => server.close());
function fixture(content = 'public operation transition fixture') {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const event = finalizeEvent(
      {
        kind: 30402,
        created_at: 100,
        tags: [
          ['d', 'transition'],
          ['published_at', '100']
        ],
        content
      },
      key
    );
    const id = randomUUID();
    const template = {
      pubkey: event.pubkey,
      kind: 30402,
      created_at: 100,
      tags: event.tags,
      content
    };
    const row: PublicOperationRecord = {
      schema: 1,
      family: 'public_operation',
      owner: event.pubkey,
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
    return { row, wire: JSON.stringify(event) };
  } finally {
    key.fill(0);
  }
}
async function load(page: Page, owner: string) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (owner) => {
    const api = window.hcp050;
    const opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    const repo = api.createPublicQuotaRepository(opened.owner, owner);
    if (!repo) throw new Error('missing quota');
    window.hcp050Database = opened.owner;
    window.hcp050Quota = repo;
  }, owner);
}
async function admit(page: Page, row: PublicOperationRecord) {
  return page.evaluate(async (row) => {
    const api = window.hcp050;
    const record = api.decodePublicRecord(
      JSON.stringify(row),
      row.owner,
      row.id
    );
    if (!record.ok) throw new Error(record.reason);
    return api.admitPublicOperation(window.hcp050Quota, row.id, record.value);
  }, row);
}
async function prepare(page: Page, f: ReturnType<typeof fixture>) {
  return page.evaluate((f) => {
    const api = window.hcp050;
    const record = api.decodePublicRecord(
      JSON.stringify(f.row),
      f.row.owner,
      f.row.id
    );
    if (!record.ok) throw new Error(record.reason);
    const artifact = api.bindCapturedArtifact(
      f.wire,
      f.row.owner,
      30402,
      f.row.capture.hash
    );
    if (!artifact.ok) throw new Error(artifact.reason);
    const transition = api.preparePublicArtifactTransition(
      record.value,
      f.row.owner,
      f.row.id,
      artifact.value
    );
    if (!transition.ok) throw new Error(transition.reason);
    window.hcp050Transition = transition.value;
    return api.publicTransitionSnapshot(
      transition.value,
      f.row.owner,
      f.row.id
    );
  }, f);
}
async function commit(page: Page) {
  return page.evaluate(() =>
    window.hcp050.commitPublicOperationTransition(
      window.hcp050Quota,
      window.hcp050Transition
    )
  );
}
async function observe(page: Page) {
  return page.evaluate(() =>
    window.hcp050.observePublicOperationTransition(
      window.hcp050Quota,
      window.hcp050Transition
    )
  );
}
function receipt(row: PublicOperationRecord): PublicTargetReceipt {
  return {
    actionId: randomUUID(),
    origin: row.capture.targets[0],
    role: 'publication',
    attempt: 1,
    eventId: row.capture.hash,
    status: 'accepted',
    observedAtMilliseconds: 1000,
    readbackWire: null
  };
}
async function prepareReceipt(
  page: Page,
  row: PublicOperationRecord,
  fact: PublicTargetReceipt
) {
  await page.evaluate(
    ({ row, fact }) => {
      const api = window.hcp050;
      const base = api.decodePublicRecord(
        JSON.stringify(row),
        row.owner,
        row.id
      );
      if (!base.ok) throw new Error(base.reason);
      const next = api.preparePublicReceiptTransition(
        base.value,
        row.owner,
        row.id,
        JSON.stringify(fact)
      );
      if (!next.ok) throw new Error(next.reason);
      window.hcp050Transition = next.value;
    },
    { row, fact }
  );
}
test('signed artifact commits exactly once and original full-wire observation distinguishes base and committed', async ({
  page
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  expect((await admit(page, f.row)).ok).toBe(true);
  const capture = await prepare(page, f);
  expect(await observe(page)).toEqual({ state: 'base_observed' });
  expect(await commit(page)).toEqual({
    ok: true,
    value: { id: f.row.id, revision: 1 }
  });
  expect(await commit(page)).toEqual({
    ok: true,
    value: { id: f.row.id, revision: 1 }
  });
  expect(await observe(page)).toEqual({
    state: 'committed',
    id: f.row.id,
    revision: 1
  });
  expect(capture?.baseWire).toBe(JSON.stringify(f.row));
});
test('two genuine tabs race distinct receipts at one revision; stale request conflicts and winning exact replay is idempotent', async ({
  page,
  context
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  await admit(page, f.row);
  const capture = await prepare(page, f);
  await commit(page);
  const row = JSON.parse(capture?.nextWire ?? 'null') as PublicOperationRecord;
  const other = await context.newPage();
  await load(other, row.owner);
  await prepareReceipt(page, row, receipt(row));
  await prepareReceipt(other, row, receipt(row));
  const results = await Promise.all([commit(page), commit(other)]);
  expect(results.filter((r) => r.ok)).toHaveLength(1);
  expect(results.filter((r) => !r.ok)).toEqual([
    { ok: false, reason: 'conflict' }
  ]);
  const winner = results[0].ok ? page : other,
    loser = results[0].ok ? other : page;
  expect((await commit(winner)).ok).toBe(true);
  expect(await observe(loser)).toEqual({ state: 'conflict' });
  await other.close();
});
test('changed capture or targets cannot replace the already captured operation', async ({
  page
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  await admit(page, f.row);
  const changed = {
    row: {
      ...f.row,
      capture: { ...f.row.capture, targets: ['wss://other.example.org'] }
    },
    wire: f.wire
  };
  await prepare(page, changed);
  expect(await commit(page)).toEqual({ ok: false, reason: 'conflict' });
  expect(await observe(page)).toEqual({ state: 'conflict' });
});
test('native put request success followed by abort is never a durable artifact acknowledgment', async ({
  page
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  await admit(page, f.row);
  await prepare(page, f);
  const result = await page.evaluate(async () => {
    const descriptor = Object.getOwnPropertyDescriptor(
      IDBObjectStore.prototype,
      'put'
    );
    if (!descriptor || typeof descriptor.value !== 'function')
      throw new Error('missing put');
    const original = descriptor.value as IDBObjectStore['put'];
    let successes = 0;
    Object.defineProperty(IDBObjectStore.prototype, 'put', {
      ...descriptor,
      value: function (
        this: IDBObjectStore,
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        const request = Reflect.apply(original, this, args) as IDBRequest;
        const transaction = this.transaction;
        request.addEventListener(
          'success',
          () => {
            successes++;
            transaction.abort();
          },
          { once: true }
        );
        return request;
      }
    });
    try {
      return {
        result: await window.hcp050.commitPublicOperationTransition(
          window.hcp050Quota,
          window.hcp050Transition
        ),
        successes
      };
    } finally {
      Object.defineProperty(IDBObjectStore.prototype, 'put', descriptor);
    }
  });
  expect(result).toEqual({
    result: { ok: false, reason: 'aborted' },
    successes: 1
  });
  expect(await observe(page)).toEqual({ state: 'base_observed' });
});
test('unknown completion reconciles original expected payload after real native commit without another write', async ({
  page
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  await admit(page, f.row);
  await prepare(page, f);
  const result = await page.evaluate(async () => {
    const put = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      ),
      abort = Object.getOwnPropertyDescriptor(
        IDBTransaction.prototype,
        'abort'
      );
    if (!put || !abort || typeof put.value !== 'function')
      throw new Error('missing methods');
    const original = put.value as IDBObjectStore['put'];
    let requests = 0;
    Object.defineProperty(IDBObjectStore.prototype, 'put', {
      ...put,
      value: function (
        this: IDBObjectStore,
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        Reflect.apply(original, this, args);
        requests++;
        throw new Error('controlled delivery loss after native request queued');
      }
    });
    Object.defineProperty(IDBTransaction.prototype, 'abort', {
      ...abort,
      value() {
        throw new Error('controlled unavailable abort channel');
      }
    });
    try {
      return {
        result: await window.hcp050.commitPublicOperationTransition(
          window.hcp050Quota,
          window.hcp050Transition
        ),
        requests
      };
    } finally {
      Object.defineProperty(IDBObjectStore.prototype, 'put', put);
      Object.defineProperty(IDBTransaction.prototype, 'abort', abort);
    }
  });
  expect(result).toEqual({
    result: { ok: false, reason: 'unknown_completion' },
    requests: 1
  });
  expect(await observe(page)).toEqual({
    state: 'committed',
    id: f.row.id,
    revision: 1
  });
});
test('artifact replacement rechecks aggregate8MiB and preserves every original row on refusal', async ({
  page
}) => {
  const f = fixture('p'.repeat(120000));
  await load(page, f.row.owner);
  const size = Buffer.byteLength(JSON.stringify(f.row));
  const count = Math.floor(8388608 / size);
  const rows: PublicOperationRecord[] = Array.from({ length: count }, () => ({
    ...f.row,
    id: randomUUID()
  }));
  rows[0] = f.row;
  await page.evaluate(async (rows) => {
    const api = window.hcp050;
    for (const row of rows) {
      const decoded = api.decodePublicRecord(
        JSON.stringify(row),
        row.owner,
        row.id
      );
      if (!decoded.ok) throw new Error(decoded.reason);
    }
    const transaction = api.browserDatabaseTransaction(
      window.hcp050Database,
      ['public_operations'],
      'readwrite'
    );
    for (const row of rows)
      transaction
        .objectStore('public_operations')
        .put({ owner: row.owner, id: row.id, wire: JSON.stringify(row) });
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(new Error('seed aborted'))
      );
    });
  }, rows);
  await prepare(page, f);
  expect(await commit(page)).toEqual({ ok: false, reason: 'capacity' });
  expect(await observe(page)).toEqual({ state: 'base_observed' });
  const inventory = await page.evaluate(async () => {
    const api = window.hcp050;
    const result = await api.inspectPublicStorage(window.hcp050Quota);
    if (!result.ok) throw new Error(result.reason);
    return api.publicInventorySnapshot(window.hcp050Quota, result.value);
  });
  expect(inventory?.rows).toHaveLength(count);
});
test('unknown stored row aborts artifact transition and owner lifetime closes admission', async ({
  page
}) => {
  const f = fixture();
  await load(page, f.row.owner);
  await admit(page, f.row);
  await prepare(page, f);
  await page.evaluate(async (row) => {
    const transaction = window.hcp050.browserDatabaseTransaction(
      window.hcp050Database,
      ['preference_operations'],
      'readwrite'
    );
    transaction.objectStore('preference_operations').put({
      owner: row.owner,
      id: '3d030bf5-901d-45e1-8251-41cbdf805e96',
      wire: 'unknown schema'
    });
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(new Error('seed aborted'))
      );
    });
  }, f.row);
  expect(await commit(page)).toEqual({ ok: false, reason: 'corrupt_record' });
  expect(await observe(page)).toEqual({ state: 'unavailable' });
  await page.evaluate(() =>
    window.hcp050.closeBrowserDatabase(window.hcp050Database)
  );
  expect(await commit(page)).toEqual({ ok: false, reason: 'invalid_scope' });
});

test('genuine preference artifact and receipt CAS preserves source, consent, target role and public aggregate store', async ({
  page
}) => {
  const key = crypto.getRandomValues(new Uint8Array(32));
  let event: ReturnType<typeof finalizeEvent>;
  try {
    event = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [['relay', 'wss://inbox.example.org']],
        content: ''
      },
      key
    );
  } finally {
    key.fill(0);
  }
  const id = randomUUID();
  const template = {
    pubkey: event.pubkey,
    kind: 10050,
    created_at: 100,
    tags: event.tags,
    content: event.content
  };
  const row = {
    schema: 1 as const,
    family: 'preference_operation' as const,
    owner: event.pubkey,
    id,
    revision: 0,
    source: { type: 'inbox_head' as const, wire: null },
    consent: 'explicit_review' as const,
    capture: {
      kind: 10050 as const,
      wire: JSON.stringify(template),
      hash: getEventHash(template),
      targets: ['wss://one.example.org'],
      policyFingerprint: 'a'.repeat(64)
    },
    artifact: null,
    receipts: []
  };
  await load(page, row.owner);
  const result = await page.evaluate(
    async ({ row, wire, actionId }) => {
      const api = window.hcp050;
      const record = api.decodePublicRecord(
        JSON.stringify(row),
        row.owner,
        row.id
      );
      if (!record.ok) throw new Error(record.reason);
      const admitted = await api.admitPublicOperation(
        window.hcp050Quota,
        row.id,
        record.value
      );
      if (!admitted.ok) throw new Error(admitted.reason);
      const artifact = api.bindCapturedArtifact(
        wire,
        row.owner,
        10050,
        row.capture.hash
      );
      if (!artifact.ok) throw new Error(artifact.reason);
      const prepared = api.preparePublicArtifactTransition(
        record.value,
        row.owner,
        row.id,
        artifact.value
      );
      if (!prepared.ok) throw new Error(prepared.reason);
      const committed = await api.commitPublicOperationTransition(
        window.hcp050Quota,
        prepared.value
      );
      if (!committed.ok) throw new Error(committed.reason);
      const capture = api.publicTransitionSnapshot(
        prepared.value,
        row.owner,
        row.id
      );
      const signed = api.decodePublicRecord(
        capture?.nextWire,
        row.owner,
        row.id
      );
      if (!signed.ok) throw new Error(signed.reason);
      const fact = {
        actionId,
        origin: row.capture.targets[0],
        role: 'preference',
        attempt: 1,
        eventId: row.capture.hash,
        status: 'accepted',
        observedAtMilliseconds: 1000,
        readbackWire: wire
      };
      const receipt = api.preparePublicReceiptTransition(
        signed.value,
        row.owner,
        row.id,
        JSON.stringify(fact)
      );
      if (!receipt.ok) throw new Error(receipt.reason);
      const receiptCommit = await api.commitPublicOperationTransition(
        window.hcp050Quota,
        receipt.value
      );
      if (!receiptCommit.ok) throw new Error(receiptCommit.reason);
      const observed = await api.observePublicOperationTransition(
        window.hcp050Quota,
        receipt.value
      );
      const state = api.publicTransitionSnapshot(
        receipt.value,
        row.owner,
        row.id
      );
      const final = api.decodePublicRecord(state?.nextWire, row.owner, row.id);
      if (!final.ok) throw new Error(final.reason);
      const duplicate = api.preparePublicReceiptTransition(
        final.value,
        row.owner,
        row.id,
        JSON.stringify(fact)
      );
      if (!duplicate.ok) throw new Error(duplicate.reason);
      return {
        committed,
        receiptCommit,
        observed,
        duplicate: await api.commitPublicOperationTransition(
          window.hcp050Quota,
          duplicate.value
        ),
        record: api.publicRecordSnapshot(final.value, row.owner, row.id)
      };
    },
    { row, wire: JSON.stringify(event), actionId: randomUUID() }
  );
  expect(result.committed).toEqual({ ok: true, value: { id, revision: 1 } });
  expect(result.receiptCommit).toEqual({
    ok: true,
    value: { id, revision: 2 }
  });
  expect(result.observed).toEqual({ state: 'committed', id, revision: 2 });
  expect(result.duplicate).toEqual({ ok: true, value: { id, revision: 2 } });
  expect(result.record).toMatchObject({
    ...row,
    revision: 2,
    artifact: { eventId: event.id, wire: JSON.stringify(event) },
    receipts: [{ role: 'preference', readbackWire: JSON.stringify(event) }]
  });
});
