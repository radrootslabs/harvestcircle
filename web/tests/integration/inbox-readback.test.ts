import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  inboxReadbackSnapshot,
  closeInboxReadback
} from '../../src/lib/nostr/inbox-readback.ts';
import { verifyInboxSetupOperation } from '../../src/lib/messaging/inbox-setup-verification.ts';
await test('SSR and forged readback controls grant no setup or publication authority', async () => {
  assert.equal(typeof window, 'undefined');
  assert.equal(inboxReadbackSnapshot({} as never), undefined);
  assert.doesNotThrow(() => closeInboxReadback({} as never));
  const result = await verifyInboxSetupOperation(
    {} as never,
    {} as never,
    undefined,
    {} as never,
    {} as never
  );
  assert.equal(result.status, 'invalid');
  assert.equal(result.setupComplete, false);
  assert.equal(result.newListingReady, false);
  assert.equal(result.sendReady, false);
});
