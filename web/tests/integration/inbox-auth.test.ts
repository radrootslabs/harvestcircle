import test from 'node:test';
import assert from 'node:assert/strict';
import {
  beginInboxAuthentication,
  respondInboxAuthentication
} from '../../src/lib/nostr/inbox-auth.ts';
void test('SSR AUTH admits no private connection, extension or forged action', async () => {
  assert.equal(typeof window, 'undefined');
  assert.equal(
    await beginInboxAuthentication(
      {} as never,
      'wss://inbox.example.org',
      'reviewed_connection_auth'
    ),
    undefined
  );
  assert.deepEqual(await respondInboxAuthentication({} as never), {
    status: 'invalid'
  });
});
