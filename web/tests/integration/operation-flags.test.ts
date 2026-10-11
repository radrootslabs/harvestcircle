import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../src/lib/config/relays.ts';
import {
  makeSetupFixture,
  fixturePolicy,
  setupInput,
  reviewInboxSetup,
  inboxSetupPreview
} from '../e2e/harness/inbox-setup.ts';

void test('HCP126 posting off preserves genuine deliberate messaging-only preference review without signing', async () => {
  const f = await makeSetupFixture();
  try {
    const policy = validateRelayPolicy(
      JSON.stringify({
        ...readRelayPolicy(fixturePolicy()),
        postingEnabled: false,
        messagingEnabled: true
      })
    );
    if (!policy) throw Error('original messaging-only policy missing');
    const before = f.counts();
    const result = await reviewInboxSetup(
      f.identity,
      f.own.resolver,
      policy,
      setupInput(),
      () => 101
    );
    assert.equal(result.status, 'review');
    assert.match(
      inboxSetupPreview(result.review)?.globalEffect ?? '',
      /does not publish/
    );
    assert.deepEqual(f.counts(), before);
  } finally {
    f.close();
  }
});
void test('HCP126 messaging off denies genuine preference preparation even when public posting is on', async () => {
  const f = await makeSetupFixture();
  try {
    const policy = validateRelayPolicy(
      JSON.stringify({
        ...readRelayPolicy(fixturePolicy()),
        postingEnabled: true,
        messagingEnabled: false
      })
    );
    if (!policy) throw Error('original posting-only policy missing');
    const before = f.counts();
    assert.deepEqual(
      await reviewInboxSetup(
        f.identity,
        f.own.resolver,
        policy,
        setupInput(),
        () => 101
      ),
      { status: 'blocked', reason: 'unavailable' }
    );
    assert.deepEqual(f.counts(), before);
  } finally {
    f.close();
  }
});
