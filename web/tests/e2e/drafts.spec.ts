import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
import type * as Drafts from '../../src/lib/persistence/drafts.ts';
declare global {
  interface Window {
    hcp047: typeof Database & typeof Drafts;
    hcp047Database: Database.BrowserDatabase;
    hcp047Repository: Drafts.PublicDraftRepository;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
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
        name: 'hcp047-test-entry',
        resolveId(id) {
          if (id === 'virtual:hcp047') return '\0hcp047';
        },
        load(id) {
          if (id === '\0hcp047')
            return `export * from ${JSON.stringify(source('database.ts'))};export * from ${JSON.stringify(source('drafts.ts'))};`;
        }
      }
    ],
    build: {
      write: false,
      minify: false,
      rolldownOptions: { input: 'virtual:hcp047' },
      lib: { entry: 'virtual:hcp047', name: 'hcp047', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing test bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP047_ACTUAL_MODULES_REAL_IDB',
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
    const api = window.hcp047;
    const opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    const repository = api.createPublicDraftRepository(
      opened.owner,
      expectedOwner
    );
    if (!repository) throw new Error('missing repository');
    window.hcp047Database = opened.owner;
    window.hcp047Repository = repository;
  }, selectedOwner);
}
test('guest GET and module initialization create no database; explicit create retains partial strings and detached snapshots', async ({
  page
}) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  expect(
    await page.evaluate(async () => (await indexedDB.databases()).length)
  ).toBe(0);
  await load(page);
  const result = await page.evaluate(async (input) => {
    const api = window.hcp047,
      repository = window.hcp047Repository;
    const created = await api.createPublicDraft(repository, input);
    if (!created.ok) throw new Error(created.reason);
    const id = created.value.id;
    const original = JSON.stringify(created.value);
    // Detached output mutation never edits the actual committed wire.
    const mutable = created.value as { form: { amount: string } };
    mutable.form.amount = 'other';
    const read = await api.readPublicDraft(repository, id);
    const listed = await api.listPublicDrafts(repository);
    api.closeBrowserDatabase(window.hcp047Database);
    const reopened = await api.openBrowserDatabase();
    if (reopened.state !== 'ready') throw new Error(reopened.reason);
    const next = api.createPublicDraftRepository(
      reopened.owner,
      created.value.owner
    );
    if (!next) throw new Error('missing reopened scope');
    const retained = await api.readPublicDraft(next, id);
    api.closeBrowserDatabase(reopened.owner);
    return {
      created: {
        ok: true as const,
        value: JSON.parse(original) as typeof created.value
      },
      read,
      listed,
      retained
    };
  }, form);
  expect(result.created.ok).toBe(true);
  expect(result.created.value.form).toEqual(form);
  expect(result.created.value.revision).toBe(0);
  expect(result.created.value.id).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
  );
  expect(result.read).toEqual(result.created);
  expect(result.retained).toEqual(result.created);
  expect(result.listed).toEqual({ ok: true, value: [result.created.value] });
});
test('two actual tabs serialize competing revision CAS and preserve the winning incomplete form', async ({
  page,
  context
}) => {
  await load(page);
  const second = await context.newPage();
  await load(second);
  const created = await page.evaluate(
    async (input) =>
      window.hcp047.createPublicDraft(window.hcp047Repository, input),
    form
  );
  if (!created.ok) throw new Error(created.reason);
  const id = created.value.id;
  const saves = await Promise.all(
    [page, second].map((tab, index) =>
      tab.evaluate(
        async ({ id, form }) =>
          window.hcp047.savePublicDraft(window.hcp047Repository, id, 0, form),
        {
          id,
          form: {
            ...form,
            location: index === 0 ? 'unfinished north' : 'unfinished south',
            amount: '1.'
          }
        }
      )
    )
  );
  expect(saves.filter((save) => save.ok)).toHaveLength(1);
  expect(saves.filter((save) => !save.ok)).toEqual([
    { ok: false, reason: 'conflict' }
  ]);
  const winner = saves.find((save) => save.ok);
  const read = await second.evaluate(
    async (id) => window.hcp047.readPublicDraft(window.hcp047Repository, id),
    id
  );
  expect(read).toEqual(winner);
  if (!read.ok) throw new Error(read.reason);
  expect(read.value.revision).toBe(1);
  expect(read.value.form.amount).toBe('1.');
  await second.close();
});
test('two tabs race the last owner slot and cap refusal preserves every existing draft', async ({
  page,
  context
}) => {
  await load(page);
  const initial = await page.evaluate(async (form) => {
    const rows = [];
    for (let i = 0; i < 19; i++)
      rows.push(
        await window.hcp047.createPublicDraft(window.hcp047Repository, form)
      );
    return rows;
  }, form);
  expect(initial.every((row) => row.ok)).toBe(true);
  const second = await context.newPage();
  await load(second);
  const raced = await Promise.all(
    [page, second].map((tab) =>
      tab.evaluate(
        async (form) =>
          window.hcp047.createPublicDraft(window.hcp047Repository, form),
        form
      )
    )
  );
  expect(raced.filter((row) => row.ok)).toHaveLength(1);
  expect(raced.filter((row) => !row.ok)).toEqual([
    { ok: false, reason: 'cap_reached' }
  ]);
  const after = await page.evaluate(async () =>
    window.hcp047.listPublicDrafts(window.hcp047Repository)
  );
  if (!after.ok) throw new Error(after.reason);
  expect(after.value).toHaveLength(20);
  for (const row of initial) {
    if (!row.ok) throw new Error(row.reason);
    expect(after.value.find((stored) => stored.id === row.value.id)).toEqual(
      row.value
    );
  }
  const separate = await second.evaluate(
    async ({ peer, form }) => {
      const repo = window.hcp047.createPublicDraftRepository(
        window.hcp047Database,
        peer
      );
      if (!repo) throw new Error('missing peer namespace');
      return window.hcp047.createPublicDraft(repo, form);
    },
    { peer, form }
  );
  expect(separate.ok).toBe(true);
  await second.close();
});
test('16KiB composed UTF8 inclusive admission, malformed getters and oversized update preserve the previous draft', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (input) => {
    const api = window.hcp047,
      repository = window.hcp047Repository;
    const bounded = { ...input, description: '菜'.repeat(5461), amount: 'x' };
    const created = await api.createPublicDraft(repository, bounded);
    if (!created.ok) throw new Error(created.reason);
    const overflow = await api.savePublicDraft(
      repository,
      created.value.id,
      0,
      { ...bounded, title: 'x' }
    );
    const getter = await api.savePublicDraft(repository, created.value.id, 0, {
      ...bounded,
      get title() {
        throw new Error('must not acquire getter');
      }
    });
    const badRevision = await api.savePublicDraft(
      repository,
      created.value.id,
      Number.MAX_SAFE_INTEGER,
      input
    );
    const proxy = await api.savePublicDraft(
      repository,
      created.value.id,
      0,
      new Proxy(input, {
        ownKeys() {
          throw new Error('hostile proxy');
        }
      })
    );
    let titleReads = 0,
      coercions = 0;
    const changingCreated = await api.createPublicDraft(
      repository,
      new Proxy(input, {
        get(target, property, receiver): unknown {
          if (property === 'title') {
            titleReads++;
            if (titleReads === 1) return 'captured-once';
            return {
              length: 0,
              isWellFormed() {
                coercions++;
                return true;
              },
              toString() {
                coercions++;
                return '';
              },
              toJSON() {
                coercions++;
                return '';
              }
            };
          }
          return Reflect.get(target, property, receiver);
        }
      })
    );
    const read = await api.readPublicDraft(repository, created.value.id);
    return {
      created,
      overflow,
      getter,
      badRevision,
      proxy,
      changingCreated,
      titleReads,
      coercions,
      read
    };
  }, form);
  expect(result.overflow).toEqual({ ok: false, reason: 'invalid_form' });
  expect(result.getter).toEqual({ ok: false, reason: 'invalid_form' });
  expect(result.badRevision).toEqual({ ok: false, reason: 'invalid_revision' });
  expect(result.proxy).toEqual({ ok: false, reason: 'invalid_form' });
  expect(result.titleReads).toBe(1);
  expect(result.coercions).toBe(0);
  if (!result.changingCreated.ok)
    throw new Error(result.changingCreated.reason);
  expect(result.changingCreated.value.form.title).toBe('captured-once');
  expect(result.read).toEqual(result.created);
});
test('foreign owner cannot read/update/list a draft, and its own cap is independent', async ({
  page,
  context
}) => {
  await load(page);
  const created = await page.evaluate(
    async (form) =>
      window.hcp047.createPublicDraft(window.hcp047Repository, form),
    form
  );
  if (!created.ok) throw new Error(created.reason);
  const second = await context.newPage();
  await load(second, peer);
  const foreign = await second.evaluate(
    async ({ id, form }) => ({
      read: await window.hcp047.readPublicDraft(window.hcp047Repository, id),
      save: await window.hcp047.savePublicDraft(
        window.hcp047Repository,
        id,
        0,
        form
      ),
      list: await window.hcp047.listPublicDrafts(window.hcp047Repository),
      create: await window.hcp047.createPublicDraft(
        window.hcp047Repository,
        form
      )
    }),
    { id: created.value.id, form }
  );
  expect(foreign.read).toEqual({ ok: false, reason: 'not_found' });
  expect(foreign.save).toEqual({ ok: false, reason: 'not_found' });
  expect(foreign.list).toEqual({ ok: true, value: [] });
  expect(foreign.create.ok).toBe(true);
  expect(
    await page.evaluate(
      async (id) => window.hcp047.readPublicDraft(window.hcp047Repository, id),
      created.value.id
    )
  ).toEqual(created);
  await second.close();
});
test('malformed persisted schema aborts reads and updates without overwriting its bytes', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp047,
      repo = window.hcp047Repository;
    const created = await api.createPublicDraft(repo, form);
    if (!created.ok) throw new Error(created.reason);
    const corruptWire = JSON.stringify({ ...created.value, schema: 2 });
    const tx = api.browserDatabaseTransaction(
      window.hcp047Database,
      ['public_drafts'],
      'readwrite'
    );
    tx.objectStore('public_drafts').put({
      owner: created.value.owner,
      id: created.value.id,
      wire: corruptWire
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () => reject(new Error('fixture abort')));
    });
    const read = await api.readPublicDraft(repo, created.value.id);
    const save = await api.savePublicDraft(repo, created.value.id, 0, form);
    const list = await api.listPublicDrafts(repo);
    const check = api.browserDatabaseTransaction(
      window.hcp047Database,
      ['public_drafts'],
      'readonly'
    );
    const request = check
      .objectStore('public_drafts')
      .get([created.value.owner, created.value.id]);
    const wire = await new Promise<unknown>((resolve) =>
      request.addEventListener('success', () => {
        const value: unknown = request.result;
        resolve(
          typeof value === 'object' && value !== null && 'wire' in value
            ? value.wire
            : undefined
        );
      })
    );
    return { read, save, list, wire, corruptWire };
  }, form);
  expect(result.read).toEqual({ ok: false, reason: 'corrupt_record' });
  expect(result.save).toEqual(result.read);
  expect(result.list).toEqual(result.read);
  expect(result.wire).toBe(result.corruptWire);
});
test('actual transaction abort after successful put never acknowledges or retains a draft', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const api = window.hcp047,
      repo = window.hcp047Repository;
    const descriptor = Object.getOwnPropertyDescriptor(
      IDBObjectStore.prototype,
      'put'
    );
    if (!descriptor) throw new Error('missing put descriptor');
    const original = descriptor.value as IDBObjectStore['put'];
    IDBObjectStore.prototype.put = function (...args) {
      const request = original.apply(this, args);
      const transaction = this.transaction;
      request.addEventListener('success', () => transaction.abort());
      return request;
    };
    try {
      const create = await api.createPublicDraft(repo, form);
      const list = await api.listPublicDrafts(repo);
      return { create, list };
    } finally {
      IDBObjectStore.prototype.put = original;
    }
  }, form);
  expect(result.create).toEqual({ ok: false, reason: 'aborted' });
  expect(result.list).toEqual({ ok: true, value: [] });
});
test('closed database scope rejects later writes and never silently reopens', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    window.hcp047.closeBrowserDatabase(window.hcp047Database);
    return window.hcp047.createPublicDraft(window.hcp047Repository, form);
  }, form);
  expect(result).toEqual({ ok: false, reason: 'invalid_scope' });
});

test('explicit draft changes acquire only the public draft store and make no fetch or socket attempt', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async (form) => {
    const transactionDescriptor = Object.getOwnPropertyDescriptor(
      IDBDatabase.prototype,
      'transaction'
    );
    const fetchDescriptor = Object.getOwnPropertyDescriptor(window, 'fetch');
    const socketDescriptor = Object.getOwnPropertyDescriptor(
      window,
      'WebSocket'
    );
    if (!transactionDescriptor || !fetchDescriptor || !socketDescriptor)
      throw new Error('missing native descriptors');
    const transaction =
      transactionDescriptor.value as IDBDatabase['transaction'];
    const selected: string[][] = [];
    let effects = 0;
    IDBDatabase.prototype.transaction = function (...args) {
      selected.push(
        typeof args[0] === 'string' ? [args[0]] : Array.from(args[0])
      );
      return transaction.apply(this, args);
    };
    Object.defineProperty(window, 'fetch', {
      configurable: true,
      value: () => {
        effects++;
        throw new Error('unexpected fetch');
      }
    });
    Object.defineProperty(window, 'WebSocket', {
      configurable: true,
      value: class {
        constructor() {
          effects++;
          throw new Error('unexpected socket');
        }
      }
    });
    try {
      const api = window.hcp047,
        repo = window.hcp047Repository;
      const created = await api.createPublicDraft(repo, form);
      if (!created.ok) throw new Error(created.reason);
      const saved = await api.savePublicDraft(repo, created.value.id, 0, {
        ...form,
        amount: '1.'
      });
      if (!saved.ok) throw new Error(saved.reason);
      const read = await api.readPublicDraft(repo, created.value.id);
      const list = await api.listPublicDrafts(repo);
      return { saved, read, list, selected, effects };
    } finally {
      Object.defineProperty(
        IDBDatabase.prototype,
        'transaction',
        transactionDescriptor
      );
      Object.defineProperty(window, 'fetch', fetchDescriptor);
      Object.defineProperty(window, 'WebSocket', socketDescriptor);
    }
  }, form);
  expect(result.read).toEqual(result.saved);
  expect(result.list.ok).toBe(true);
  expect(result.effects).toBe(0);
  expect(result.selected).toEqual(
    Array.from({ length: 4 }, () => ['public_drafts'])
  );
});
