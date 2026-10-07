import test from 'node:test';
import assert from 'node:assert/strict';
import {
  browserExtensionScheduler,
  createExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../src/lib/nostr/extension-scheduler.ts';
await test('actual scheduler and extension imports stay inert in Node SSR', async () => {
  assert.equal(typeof window, 'undefined');
  assert.equal(browserExtensionScheduler(), undefined);
  const identity = await import('../../src/lib/runtime/identity-session.ts');
  const scheduler = createExtensionScheduler();
  assert.deepEqual(extensionSchedulerSnapshot(scheduler), { state: 'idle' });
  assert.deepEqual(
    identity.identitySessionSnapshot(identity.createIdentitySession()),
    { state: 'guest', reason: 'disconnected' }
  );
});
