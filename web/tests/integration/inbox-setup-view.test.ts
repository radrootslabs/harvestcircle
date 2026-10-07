import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createInboxSetupView,
  inboxSetupViewSnapshot
} from '../../src/lib/messaging/inbox-setup-view.ts';
import { createIdentitySession } from '../../src/lib/runtime/identity-session.ts';
import { deploymentRelayPolicy } from '../../src/lib/config/deployment-relays.ts';
await test('SSR setup construction acquires no browser identity, storage or network', () => {
  assert.equal(typeof window, 'undefined');
  let lookups = 0;
  const view = createInboxSetupView({
    identity: createIdentitySession(),
    policy: deploymentRelayPolicy,
    lookup: () => {
      lookups++;
      return Promise.reject(new Error('SSR lookup forbidden'));
    }
  });
  assert.equal(view, undefined);
  assert.equal(lookups, 0);
  assert.equal(inboxSetupViewSnapshot(view).setupComplete, false);
});
