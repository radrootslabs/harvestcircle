import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createPublicQuotaRepository,
  commitPublicOperationTransition,
  observePublicOperationTransition,
  type PublicQuotaRepository
} from '../../src/lib/persistence/quota.ts';
import type { PublicOperationTransition } from '../../src/lib/persistence/artifact-records.ts';
await test('SSR import and fabricated transition capabilities cannot open public or private storage', async () => {
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  assert.equal(
    createPublicQuotaRepository({ kind: 'browser_database' }, owner),
    undefined
  );
  const repository = {} as PublicQuotaRepository,
    transition = {} as PublicOperationTransition;
  assert.deepEqual(
    await commitPublicOperationTransition(repository, transition),
    { ok: false, reason: 'invalid_scope' }
  );
  assert.deepEqual(
    await observePublicOperationTransition(repository, transition),
    { state: 'unavailable' }
  );
});
