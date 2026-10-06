import test from 'node:test';
import assert from 'node:assert/strict';
await test('actual draft module import and fabricated-owner rejection never acquire IndexedDB during SSR', async () => {
  const before = Object.getOwnPropertyDescriptor(globalThis, 'indexedDB');
  Object.defineProperty(globalThis, 'indexedDB', {
    configurable: true,
    get() {
      throw new Error('SSR cannot acquire IndexedDB');
    }
  });
  try {
    const api = await import('../../src/lib/persistence/drafts.ts');
    const database = {
      kind: 'browser_database'
    } as import('../../src/lib/persistence/database.ts').BrowserDatabase;
    assert.equal(
      api.createPublicDraftRepository(
        database,
        '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
      ),
      undefined
    );
    const fake = Object.freeze(
      {}
    ) as import('../../src/lib/persistence/drafts.ts').PublicDraftRepository;
    assert.deepEqual(await api.listPublicDrafts(fake), {
      ok: false,
      reason: 'invalid_scope'
    });
  } finally {
    if (before) Object.defineProperty(globalThis, 'indexedDB', before);
    else Reflect.deleteProperty(globalThis, 'indexedDB');
  }
});
