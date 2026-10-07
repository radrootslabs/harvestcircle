import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  runInboxPreferenceOperation,
  stopInboxPreferenceOperation
} from '../../src/lib/messaging/inbox-setup-operation.ts';
import { preferencePublicationSnapshot } from '../../src/lib/nostr/inbox-preference-publication.ts';
await test('SSR and forged preference controls expose no arbitrary event publisher or effects', async () => {
  assert.equal(typeof window, 'undefined');
  assert.deepEqual(await runInboxPreferenceOperation({} as never), {
    status: 'invalid'
  });
  assert.equal(preferencePublicationSnapshot({} as never), undefined);
  assert.doesNotThrow(() => stopInboxPreferenceOperation({} as never));
});
