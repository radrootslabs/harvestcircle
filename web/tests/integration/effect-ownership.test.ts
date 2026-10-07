import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createExtensionAdapter,
  extensionOwnershipCapture
} from '../../src/lib/nostr/extension.ts';
import {
  publicEffectSnapshot,
  stopPublicEffect,
  type PublicEffectLease
} from '../../src/lib/runtime/effect-ownership.ts';
await test('actual owner/lock modules remain inert and cannot manufacture a connected SSR capture', () => {
  assert.equal(typeof window, 'undefined');
  assert.equal(extensionOwnershipCapture(createExtensionAdapter()), undefined);
  const fake = {} as PublicEffectLease;
  assert.equal(publicEffectSnapshot(fake), undefined);
  stopPublicEffect(fake);
  assert.equal(publicEffectSnapshot(fake), undefined);
});
