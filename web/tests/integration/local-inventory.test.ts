import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createPublicQuotaRepository,
  inspectPublicStorage,
  reviewPublicCleanup,
  commitPublicCleanup,
  type PublicQuotaRepository,
  type PublicInventory,
  type PublicCleanup
} from '../../src/lib/persistence/quota.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
await test('actual public quota API denies fabricated guest capabilities without private storage acquisition', async () => {
  const database = Object.freeze({
    kind: 'browser_database'
  }) as BrowserDatabase;
  assert.equal(
    createPublicQuotaRepository(
      database,
      '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
    ),
    undefined
  );
  const repository = Object.freeze({}) as PublicQuotaRepository;
  assert.deepEqual(await inspectPublicStorage(repository), {
    ok: false,
    reason: 'invalid_scope'
  });
  assert.deepEqual(
    reviewPublicCleanup(repository, Object.freeze({}) as PublicInventory, []),
    { ok: false, reason: 'invalid_scope' }
  );
  assert.deepEqual(
    await commitPublicCleanup(repository, Object.freeze({}) as PublicCleanup),
    { ok: false, reason: 'invalid_scope' }
  );
});
