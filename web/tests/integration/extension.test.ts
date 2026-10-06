import test from 'node:test';
import assert from 'node:assert/strict';
// Actual module imports and adapter construction are inert under SSR. No browser
// provider, native storage, signer request or process-global user is created.
import {
  createIdentitySession,
  identitySessionSnapshot,
  connectIdentity
} from '../../src/lib/runtime/identity-session.ts';
await test('actual signer dependency/runtime imports and SSR construction perform no extension action', async () => {
  assert.equal(typeof window, 'undefined');
  const session = createIdentitySession();
  assert.deepEqual(identitySessionSnapshot(session), {
    state: 'guest',
    reason: 'disconnected'
  });
  assert.deepEqual(await connectIdentity(session), {
    state: 'guest',
    reason: 'unavailable'
  });
});
