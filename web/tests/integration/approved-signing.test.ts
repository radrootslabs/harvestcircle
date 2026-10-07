import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createExtensionAdapter,
  signApprovedExtensionAdapter
} from '../../src/lib/nostr/extension.ts';
import {
  approveCapturedPublicSigning,
  type ApprovedPublicSigning
} from '../../src/lib/nostr/approved-signing.ts';
import type { PublicEffectLease } from '../../src/lib/runtime/effect-ownership.ts';
await test('approved signing import/SSR and forged approval have no provider or storage effect', async () => {
  assert.equal(typeof window, 'undefined');
  const adapter = createExtensionAdapter();
  assert.equal(
    approveCapturedPublicSigning(
      {} as never,
      '',
      '',
      'reviewed_captured_operation'
    ),
    undefined
  );
  assert.deepEqual(
    await signApprovedExtensionAdapter(
      adapter,
      {} as ApprovedPublicSigning,
      {} as PublicEffectLease
    ),
    { status: 'invalid_approval' }
  );
});
