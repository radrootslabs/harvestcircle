import { expect, it } from 'vitest';
import {
  getPrivateCacheScope,
  retainPrivateCacheWire,
  privateCacheWire,
  privateCacheSnapshot,
  clearPrivateCacheScope,
  closePrivateCacheScope,
  type PrivateCacheScope
} from '../../src/lib/nostr/private-cache-scope.ts';
import {
  acceptUnlockedConversation,
  readUnlockedMessages,
  unlockedSessionSnapshot,
  unlockedSessionOwnership,
  unlockedConversationCurrent,
  type UnlockedSession
} from '../../src/lib/messaging/unlocked-session.ts';
import {
  receivedNestedEnvelopeSnapshot,
  receivedNestedEnvelopeOwnership,
  type ReceivedNestedEnvelope
} from '../../src/lib/nostr/unwrap-admission.ts';
import {
  conversationSnapshot,
  conversationOwnership,
  type AdmittedConversation
} from '../../src/lib/messaging/admit-conversation.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
it('detached data, imported verification marks, getters and proxy copies cannot mint private cache or nested room custody', () => {
  let inspected = 0;
  const getter = Object.defineProperty({}, 'owner', {
    get() {
      inspected++;
      return '0'.repeat(64);
    }
  });
  const proxy = new Proxy(
    {},
    {
      get() {
        inspected++;
        throw new Error('untrusted getter');
      }
    }
  );
  const revoked = Proxy.revocable({}, {});
  revoked.revoke();
  const data = {
    owner: '0'.repeat(64),
    peer: '1'.repeat(64),
    rumorId: '2'.repeat(64),
    content: 'Detached private-data canary',
    verified: true
  };
  const candidates = [
    {},
    Object.freeze({}),
    { ...data },
    Object.freeze({ ...data }),
    JSON.parse(JSON.stringify(data)) as object,
    Object.create(data) as object,
    { [Symbol.for('verified')]: true },
    getter,
    proxy,
    revoked.proxy
  ];
  expect(candidates).toHaveLength(10);
  expect(typeof window).toBe('undefined');
  for (const candidate of candidates) {
    const cache = candidate as PrivateCacheScope,
      unlocked = candidate as UnlockedSession;
    const nested = candidate as ReceivedNestedEnvelope,
      room = candidate as AdmittedConversation;
    expect(
      getPrivateCacheScope(candidate as PrivateSession, 'projection_ownership')
    ).toBeUndefined();
    expect(
      getPrivateCacheScope(candidate as PrivateSession, 'raw_ciphertext')
    ).toBeUndefined();
    for (const action of [
      () => retainPrivateCacheWire(cache, data.rumorId, JSON.stringify(data)),
      () => privateCacheWire(cache, data.rumorId),
      () => privateCacheSnapshot(cache),
      () => clearPrivateCacheScope(cache),
      () => closePrivateCacheScope(cache)
    ])
      expect(action).toThrow('private_cache_invalid');
    expect(acceptUnlockedConversation(unlocked, room)).toBe('rejected');
    expect(readUnlockedMessages(unlocked)).toEqual({
      status: 'rejected',
      messages: []
    });
    expect(unlockedSessionSnapshot(unlocked)).toEqual({
      closed: true,
      count: 0,
      bytes: 0
    });
    expect(unlockedSessionOwnership(unlocked)).toBeUndefined();
    expect(unlockedConversationCurrent(unlocked, room)).toBe(false);
    expect(receivedNestedEnvelopeSnapshot(nested)).toBeUndefined();
    expect(receivedNestedEnvelopeOwnership(nested)).toBeUndefined();
    expect(conversationSnapshot(room)).toBeUndefined();
    expect(conversationOwnership(room)).toBeUndefined();
  }
  expect(inspected).toBe(0);
});
