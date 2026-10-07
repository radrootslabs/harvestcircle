import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as boundary from '../../src/lib/nostr/private-envelope-capability.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  recheckIdentityOwner
} from '../../src/lib/runtime/identity-session.ts';
import { createPrivateSession } from '../../src/lib/runtime/private-session.ts';

// HC_TEST_ONLY_PROVIDER: actual SDK ownership; echo is not encryption qualification.
await test('outer-only construction permission follows actual SDK private-session generations without creating keys', async () => {
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  const other =
    'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
  assert.equal(typeof window, 'undefined');
  assert.equal(boundary.getPrivateEnvelopeCapability({} as never), undefined);
  let key = owner,
    prompts = 0,
    signatures = 0;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey: () => {
          prompts++;
          return Promise.resolve(key);
        },
        signEvent: () => {
          signatures++;
          return Promise.reject(new Error('HC_TEST_ONLY_UNEXPECTED_SIGN'));
        },
        nip44: {
          encrypt: (_peer: string, text: string) =>
            Promise.resolve('fixture:' + text),
          decrypt: (_peer: string, text: string) =>
            Promise.resolve(text.slice(8))
        }
      }
    }
  });
  const identity = createIdentitySession();
  async function connect() {
    assert.equal((await connectIdentity(identity)).state, 'signing_only');
    assert.equal(
      (await probeIdentityMessaging(identity, 'reviewed_self_copy')).state,
      'messaging_capable'
    );
    const session = await createPrivateSession(
      identity,
      'reviewed_private_session'
    );
    assert.ok(session);
    return session;
  }
  try {
    assert.throws(
      () => boundary.getPrivateEnvelopeCapability({} as never),
      /private_session_invalid/
    );
    assert.throws(
      () => boundary.getPrivateEnvelopeCapability(identity as never),
      /private_session_invalid/
    );
    assert.equal(prompts, 0);
    const session = await connect();
    const before = prompts;
    const token = boundary.getPrivateEnvelopeCapability(session);
    assert.ok(token);
    assert.equal(boundary.getPrivateEnvelopeCapability(session), token);
    assert.deepEqual(Reflect.ownKeys(token), []);
    const captured = boundary.privateEnvelopeConstructionOwnership(token);
    assert.ok(captured);
    assert.equal(captured.kind, 1059);
    assert.equal(captured.owner, owner);
    assert.equal(captured.current(), true);
    assert.deepEqual(Object.keys(captured).sort(), [
      'current',
      'kind',
      'owner',
      'session'
    ]);
    assert.equal(prompts, before);
    assert.equal(signatures, 0);
    disconnectIdentity(identity);
    assert.equal(captured.current(), false);
    assert.equal(
      boundary.privateEnvelopeConstructionOwnership(token),
      undefined
    );
    const again = boundary.getPrivateEnvelopeCapability(await connect());
    assert.ok(again);
    assert.notEqual(again, token);
    assert.equal(
      boundary.privateEnvelopeConstructionOwnership(token),
      undefined
    );
    key = other;
    assert.equal((await recheckIdentityOwner(identity)).state, 'guest');
    assert.equal(
      boundary.privateEnvelopeConstructionOwnership(again),
      undefined
    );
    const replacementSession = await connect();
    const replacement =
      boundary.getPrivateEnvelopeCapability(replacementSession);
    assert.ok(replacement);
    assert.equal(
      boundary.privateEnvelopeConstructionOwnership(replacement)?.owner,
      other
    );
    boundary.closePrivateEnvelopeCapability(replacement);
    boundary.closePrivateEnvelopeCapability(replacement);
    assert.equal(
      boundary.privateEnvelopeConstructionOwnership(replacement),
      undefined
    );
    assert.throws(
      () => boundary.getPrivateEnvelopeCapability(replacementSession),
      /private_envelope_capability_closed/
    );
    assert.equal(signatures, 0);
  } finally {
    disconnectIdentity(identity);
    Reflect.deleteProperty(globalThis, 'window');
  }
});
