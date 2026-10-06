import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Database from '../../src/lib/persistence/database.ts';
declare global {
  interface Window {
    hcp045: typeof Database;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
let schema: string;
let database: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  schema = await readFile(
    new URL('../../src/lib/persistence/schema.ts', import.meta.url),
    'utf8'
  );
  database = await readFile(
    new URL('../../src/lib/persistence/database.ts', import.meta.url),
    'utf8'
  );
  console.log(
    JSON.stringify({
      fixture: 'HCP045_REAL_CHROMIUM_NATIVE_IDB',
      schema: createHash('sha256').update(schema).digest('hex'),
      database: createHash('sha256').update(database).digest('hex'),
      exposure: 'test-only module URLs; no production route or dependency'
    })
  );
});
test.afterAll(async () => server.close());
function javascript(source: string): string {
  return ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext
    }
  }).outputText;
}
async function load(page: Page, futureVersionProbe = false) {
  // Production has no released old schema. A separately labelled version2
  // variant tests the native blocked-upgrade event, never a supported migration.
  const selected = futureVersionProbe
    ? schema.replace(
        'export const browserSchemaVersion = 1;',
        'export const browserSchemaVersion = 2;'
      )
    : schema;
  if (futureVersionProbe) expect(selected).not.toBe(schema);
  expect(database.split("from './schema.ts'")).toHaveLength(2);
  await page.route('**/__hcp045/schema.js', (route) =>
    route.fulfill({
      contentType: 'text/javascript',
      body: javascript(selected)
    })
  );
  await page.route('**/__hcp045/database.js', (route) =>
    route.fulfill({
      contentType: 'text/javascript',
      body:
        javascript(
          database.replace("from './schema.ts'", "from './schema.js'")
        ) +
        '\nglobalThis.hcp045={openBrowserDatabase,closeBrowserDatabase,browserDatabaseState,browserDatabaseTransaction};'
    })
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({
    url: server.url + '/__hcp045/database.js',
    type: 'module'
  });
}

test('fresh schema, acknowledged records, reopen, owner revocation and transaction admission use actual IndexedDB', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const api = window.hcp045;
    const opened = await api.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    const transaction = api.browserDatabaseTransaction(
      opened.owner,
      ['public_drafts'],
      'readwrite'
    );
    // Test-only sentinel, not a qualified public draft record or repository.
    transaction
      .objectStore('public_drafts')
      .put({ owner: 'test-owner', id: 'sentinel', value: 'preserve' });
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () => reject(new Error('abort')));
    });
    let rejected = 0;
    for (const stores of [
      [],
      ['public_drafts', 'public_drafts'],
      ['unknown']
    ]) {
      try {
        api.browserDatabaseTransaction(
          opened.owner,
          stores as ['public_drafts'],
          'readonly'
        );
      } catch {
        rejected++;
      }
    }
    api.closeBrowserDatabase(opened.owner);
    api.closeBrowserDatabase(opened.owner);
    const closed = api.browserDatabaseState(opened.owner);
    try {
      api.browserDatabaseTransaction(
        opened.owner,
        ['public_drafts'],
        'readonly'
      );
    } catch {
      rejected++;
    }
    const reopened = await api.openBrowserDatabase();
    if (reopened.state !== 'ready') throw new Error(reopened.reason);
    const read = api
      .browserDatabaseTransaction(reopened.owner, ['public_drafts'], 'readonly')
      .objectStore('public_drafts')
      .get(['test-owner', 'sentinel']);
    const value = await new Promise<unknown>((resolve, reject) => {
      read.addEventListener('success', () => resolve(read.result));
      read.addEventListener('error', () => reject(new Error('read_failed')));
    });
    api.closeBrowserDatabase(reopened.owner);
    return { closed, rejected, value };
  });
  expect(result).toEqual({
    closed: { state: 'closed', reason: 'explicit' },
    rejected: 4,
    value: { owner: 'test-owner', id: 'sentinel', value: 'preserve' }
  });
});

test('real version change closes the owned connection and rejects the unsupported future schema', async ({
  page,
  context
}) => {
  await load(page);
  await page.evaluate(async () => {
    const opened = await window.hcp045.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    // Retain only a test closure, never an exported raw production connection.
    Object.assign(window, {
      readHcp045State: () => window.hcp045.browserDatabaseState(opened.owner)
    });
  });
  const other = await context.newPage();
  await other.goto(server.url + '/search');
  expect(
    await other.evaluate(
      () =>
        new Promise<number>((resolve, reject) => {
          const request = indexedDB.open('harvestcircle_browser', 2);
          request.addEventListener('success', () => {
            const version = request.result.version;
            request.result.close();
            resolve(version);
          });
          request.addEventListener('error', () =>
            reject(new Error('upgrade_failed'))
          );
        })
    )
  ).toBe(2);
  const state = await page.evaluate(() => {
    const host = window as unknown as { readHcp045State: () => unknown };
    return host.readHcp045State();
  });
  expect(state).toEqual({ state: 'closed', reason: 'version_change' });
  expect(
    await page.evaluate(() => window.hcp045.openBrowserDatabase())
  ).toMatchObject({ state: 'unavailable', reason: 'incompatible_version' });
});

test('actual other-tab blocked upgrade fails closed; late release cannot expose or migrate an abandoned owner', async ({
  page,
  context
}) => {
  const incumbent = await context.newPage();
  await incumbent.goto(server.url + '/search');
  await incumbent.evaluate(
    () =>
      new Promise<void>((resolve, reject) => {
        const request = indexedDB.open('harvestcircle_browser', 1);
        request.addEventListener('success', () => {
          Object.assign(window, {
            closeIncumbent: () => request.result.close()
          });
          resolve();
        });
        request.addEventListener('error', () =>
          reject(new Error('incumbent_failed'))
        );
      })
  );
  await load(page, true);
  const blocked = await page.evaluate(() =>
    window.hcp045.openBrowserDatabase()
  );
  expect(blocked).toMatchObject({ state: 'unavailable', reason: 'blocked' });
  await incumbent.evaluate(() => {
    (window as unknown as { closeIncumbent: () => void }).closeIncumbent();
  });
  // A following request serializes behind the abandoned real upgrade. Its v1
  // success proves that the late upgrade aborted and no hidden v2 reset won.
  expect(
    await incumbent.evaluate(
      () =>
        new Promise<number>((resolve, reject) => {
          const request = indexedDB.open('harvestcircle_browser', 1);
          request.addEventListener('success', () => {
            const version = request.result.version;
            request.result.close();
            resolve(version);
          });
          request.addEventListener('error', () =>
            reject(new Error('late_upgrade_mutated_state'))
          );
        })
    )
  ).toBe(1);
});

test('actual migration transaction abort is reported; only an explicit new attempt initializes', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const original = Object.getOwnPropertyDescriptor(
      IDBDatabase.prototype,
      'createObjectStore'
    );
    if (!original) throw new Error('missing_browser_prototype');
    IDBDatabase.prototype.createObjectStore = function () {
      throw new DOMException('test-only failure', 'QuotaExceededError');
    };
    let failed;
    try {
      failed = await window.hcp045.openBrowserDatabase();
    } finally {
      Object.defineProperty(
        IDBDatabase.prototype,
        'createObjectStore',
        original
      );
    }
    const retry = await window.hcp045.openBrowserDatabase();
    if (retry.state === 'ready')
      window.hcp045.closeBrowserDatabase(retry.owner);
    return { failed, retry: retry.state };
  });
  expect(result.failed).toMatchObject({
    state: 'unavailable',
    reason: 'migration_failed'
  });
  expect(result.retry).toBe('ready');
});

test('corrupt existing schema and sentinel data are preserved without reset', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.open('harvestcircle_browser', 1);
      request.addEventListener('upgradeneeded', () =>
        request.result
          .createObjectStore('unknown_records')
          .put('retain', 'sentinel')
      );
      request.addEventListener('success', () => {
        request.result.close();
        resolve();
      });
      request.addEventListener('error', () =>
        reject(new Error('fixture_failed'))
      );
    });
    const failed = await window.hcp045.openBrowserDatabase();
    const sentinel = await new Promise<unknown>((resolve, reject) => {
      const request = indexedDB.open('harvestcircle_browser', 1);
      request.addEventListener('success', () => {
        const read = request.result
          .transaction('unknown_records')
          .objectStore('unknown_records')
          .get('sentinel');
        read.addEventListener('success', () => {
          request.result.close();
          resolve(read.result);
        });
        read.addEventListener('error', () =>
          reject(new Error('sentinel_failed'))
        );
      });
      request.addEventListener('error', () =>
        reject(new Error('fixture_reopen_failed'))
      );
    });
    return { failed, sentinel };
  });
  expect(result.failed).toMatchObject({
    state: 'unavailable',
    reason: 'corrupt_schema'
  });
  expect(result.sentinel).toBe('retain');
});

test('denied capability branch returns browse-only explanation and keeps the real public page usable', async ({
  page
}) => {
  await load(page);
  const failed = await page.evaluate(async () => {
    // Browser capability denial branch injection, not private-mode qualification.
    const original = Object.getOwnPropertyDescriptor(window, 'indexedDB');
    Object.defineProperty(window, 'indexedDB', {
      configurable: true,
      get() {
        throw new DOMException('test-only denied', 'SecurityError');
      }
    });
    try {
      return await window.hcp045.openBrowserDatabase();
    } finally {
      if (original) Object.defineProperty(window, 'indexedDB', original);
      else Reflect.deleteProperty(window, 'indexedDB');
    }
  });
  expect(failed).toMatchObject({
    state: 'unavailable',
    reason: 'denied',
    message: expect.stringContaining('still browse')
  });
  await expect(
    page.getByRole('textbox', { name: 'What are you looking for?' })
  ).toBeVisible();
});

test('immediate cancellation owns the pending real open and permits only a separate explicit retry', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const controller = new AbortController();
    const pending = window.hcp045.openBrowserDatabase(controller.signal);
    controller.abort();
    const cancelled = await pending;
    const retry = await window.hcp045.openBrowserDatabase();
    if (retry.state === 'ready')
      window.hcp045.closeBrowserDatabase(retry.owner);
    return { cancelled, retry: retry.state };
  });
  expect(result.cancelled).toMatchObject({
    state: 'unavailable',
    reason: 'cancelled'
  });
  expect(result.retry).toBe('ready');
});

for (const defect of [
  'key_path',
  'auto_increment',
  'unique_index',
  'missing_index',
  'extra_index'
] as const) {
  test(`exact schema rejects ${defect} and preserves existing data`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (defect) => {
      const stores = [
        'public_drafts',
        'public_operations',
        'preference_operations',
        'private_sends',
        'received_envelopes',
        'conversations',
        'local_preferences'
      ];
      await new Promise<void>((resolve, reject) => {
        const request = indexedDB.open('harvestcircle_browser', 1);
        request.addEventListener('upgradeneeded', () => {
          for (const name of stores) {
            const selected = name === 'public_drafts';
            const store = request.result.createObjectStore(name, {
              keyPath:
                selected &&
                (defect === 'key_path' || defect === 'auto_increment')
                  ? 'id'
                  : ['owner', 'id'],
              autoIncrement: selected && defect === 'auto_increment'
            });
            if (!(selected && defect === 'missing_index'))
              store.createIndex('by_owner', 'owner', {
                unique: selected && defect === 'unique_index'
              });
            if (selected && defect === 'extra_index')
              store.createIndex('unexpected', 'id');
          }
          request.transaction
            ?.objectStore('local_preferences')
            .put({ owner: 'test-owner', id: 'sentinel', value: 'preserve' });
        });
        request.addEventListener('success', () => {
          request.result.close();
          resolve();
        });
        request.addEventListener('error', () =>
          reject(new Error('fixture_failed'))
        );
      });
      const failed = await window.hcp045.openBrowserDatabase();
      const sentinel = await new Promise<unknown>((resolve, reject) => {
        const request = indexedDB.open('harvestcircle_browser', 1);
        request.addEventListener('success', () => {
          const read = request.result
            .transaction('local_preferences')
            .objectStore('local_preferences')
            .get(['test-owner', 'sentinel']);
          read.addEventListener('success', () => {
            request.result.close();
            resolve(read.result);
          });
          read.addEventListener('error', () =>
            reject(new Error('read_failed'))
          );
        });
        request.addEventListener('error', () =>
          reject(new Error('reopen_failed'))
        );
      });
      return { failed, sentinel };
    }, defect);
    expect(result.failed).toMatchObject({
      state: 'unavailable',
      reason: 'corrupt_schema'
    });
    expect(result.sentinel).toEqual({
      owner: 'test-owner',
      id: 'sentinel',
      value: 'preserve'
    });
  });
}
