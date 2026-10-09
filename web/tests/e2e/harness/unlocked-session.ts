import {
  captureUnlockedSession,
  acceptUnlockedConversation,
  readUnlockedMessages,
  unlockedSessionSnapshot,
  closeUnlockedSession,
  type UnlockedSession
} from '../../../src/lib/messaging/unlocked-session.ts';
import {
  getPrivateCacheScope,
  retainPrivateCacheWire,
  clearPrivateCacheScope,
  privateCacheSnapshot,
  type PrivateCacheScope
} from '../../../src/lib/nostr/private-cache-scope.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  recheckIdentityOwner
} from '../../../src/lib/runtime/identity-session.ts';
import {
  getPublicKey,
  getEventHash,
  finalizeEvent,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  captureReceivedUnwrap,
  unwrapReceivedEnvelope,
  stopReceivedUnwrap,
  type ReceivedUnwrap,
  type ReceivedNestedEnvelope
} from '../../../src/lib/nostr/unwrap-admission.ts';
import {
  admitReceivedConversation,
  conversationSnapshot,
  type AdmittedConversation
} from '../../../src/lib/messaging/admit-conversation.ts';
type Scenario =
  | 'inbound'
  | 'self_archive'
  | 'third_party'
  | 'hostile_text'
  | 'missing_parent';
export async function makeFixture(scenario: Scenario = 'inbound') {
  const f = await makeRecoveryFixture(),
    privateSession = await createPrivateSession(
      f.identity,
      'reviewed_private_session'
    );
  if (!privateSession) throw Error('missing genuine private session');
  const ownerSecret = new Uint8Array(32).fill(83),
    peerSecret = new Uint8Array(32).fill(84),
    outerSecret = new Uint8Array(32).fill(85),
    other = getPublicKey(outerSecret);
  const senderSecret = scenario === 'self_archive' ? ownerSecret : peerSecret,
    sender = getPublicKey(senderSecret),
    recipient =
      scenario === 'self_archive'
        ? f.peer
        : scenario === 'third_party'
          ? other
          : f.owner;
  const tags: string[][] = [['p', recipient]];
  if (scenario === 'hostile_text') tags.push(['subject', other]);
  if (scenario === 'missing_parent')
    tags.push(['e', 'a'.repeat(64), 'wss://untrusted.example.org']);
  const template: UnsignedEvent = {
      pubkey: sender,
      kind: 14,
      created_at: 1700000000,
      tags,
      content:
        scenario === 'hostile_text'
          ? `Contact ${other} https://untrusted.example.org`
          : 'Actual encrypted room fixture'
    },
    rumor = { id: getEventHash(template), ...template };
  function encrypt(key: Uint8Array, target: string, plain: string) {
    const conversation = nip44.v2.utils.getConversationKey(key, target);
    try {
      return nip44.v2.encrypt(plain, conversation);
    } finally {
      conversation.fill(0);
    }
  }
  const seal = finalizeEvent(
    {
      kind: 13,
      created_at: 1699999900,
      tags: [],
      content: encrypt(senderSecret, f.owner, JSON.stringify(rumor))
    },
    senderSecret
  );
  const outer = finalizeEvent(
    {
      kind: 1059,
      created_at: 1699999800,
      tags: [['p', f.owner]],
      content: encrypt(outerSecret, f.owner, JSON.stringify(seal))
    },
    outerSecret
  );
  ownerSecret.fill(0);
  peerSecret.fill(0);
  outerSecret.fill(0);
  const admitted = admitInboxEnvelope(
    JSON.stringify(outer),
    f.owner,
    'wss://archive.example.org',
    100
  );
  if (!admitted) throw Error('missing signed addressed outer');
  const saved = await retainInboxEnvelope(
    f.repository,
    privateSession,
    admitted
  );
  if (saved.status !== 'retained') throw Error('no actual retained cipher');
  let reader: ReceivedUnwrap | undefined,
    nested: ReceivedNestedEnvelope | undefined,
    room: AdmittedConversation | undefined;
  let unlocked: UnlockedSession | undefined;
  let rawCache: PrivateCacheScope | undefined;
  let otherPrivateSession: Awaited<ReturnType<typeof createPrivateSession>>;
  let otherIdentity: ReturnType<typeof createIdentitySession> | undefined;
  return {
    ...f,
    other,
    rumorId: rumor.id,
    async unlock() {
      reader = captureReceivedUnwrap(
        f.repository,
        f.identity,
        outer.id,
        'reviewed_inbox_unlock'
      );
      if (!reader) return 'invalid';
      const result = await unwrapReceivedEnvelope(reader);
      if (result.status === 'authenticated') nested = result.envelope;
      return result.status;
    },
    admit(role: unknown) {
      room = nested && admitReceivedConversation(nested, role);
      return room && conversationSnapshot(room);
    },
    snapshot() {
      return room && conversationSnapshot(room);
    },
    forged() {
      return admitReceivedConversation({} as ReceivedNestedEnvelope, 'inbound');
    },
    copied() {
      return room && conversationSnapshot({ ...room });
    },
    captureCache(review: unknown = 'reviewed_messages_unlock') {
      unlocked = captureUnlockedSession(privateSession, review);
      if (unlocked)
        rawCache = getPrivateCacheScope(privateSession, 'projection_ownership');
      return !!unlocked;
    },
    cache() {
      return unlocked && room
        ? acceptUnlockedConversation(unlocked, room)
        : 'rejected';
    },
    messages() {
      return unlocked && readUnlockedMessages(unlocked);
    },
    cacheSnapshot() {
      return unlocked && unlockedSessionSnapshot(unlocked);
    },
    rawCacheSnapshot() {
      return rawCache && privateCacheSnapshot(rawCache);
    },
    forgedCache() {
      return (
        unlocked &&
        acceptUnlockedConversation(unlocked, {} as AdmittedConversation)
      );
    },
    copiedCache() {
      return (
        unlocked && room && acceptUnlockedConversation(unlocked, { ...room })
      );
    },
    closeCache() {
      if (unlocked) closeUnlockedSession(unlocked);
    },
    closePrivate() {
      closePrivateSession(privateSession);
    },
    async changedOwner() {
      f.mode('changed_key');
      return await recheckIdentityOwner(f.identity);
    },
    async otherGeneration() {
      otherIdentity = createIdentitySession();
      await connectIdentity(otherIdentity);
      await probeIdentityMessaging(otherIdentity, 'reviewed_self_copy');
      otherPrivateSession = await createPrivateSession(
        otherIdentity,
        'reviewed_private_session'
      );
      if (!otherPrivateSession)
        throw Error('missing other genuine private session');
      const scope = captureUnlockedSession(
        otherPrivateSession,
        'reviewed_messages_unlock'
      );
      if (!scope || !room) throw Error('missing genuine scope/room');
      return {
        accepted: acceptUnlockedConversation(scope, room),
        snapshot: unlockedSessionSnapshot(scope)
      };
    },
    quota() {
      const scope = getPrivateCacheScope(
        privateSession,
        'projection_ownership'
      );
      if (!scope) throw Error('missing actual scoped cache');
      for (let i = 0; i < 2000; i++) {
        const result = retainPrivateCacheWire(
          scope,
          i.toString(16).padStart(64, '0'),
          '{}'
        );
        if (result !== 'accepted')
          throw Error('unexpected quota fixture result ' + result);
      }
      return 2000;
    },
    corruptCachedWire() {
      const scope = getPrivateCacheScope(
        privateSession,
        'projection_ownership'
      );
      if (!scope || !room) throw Error('missing actual cache');
      const snapshot = conversationSnapshot(room);
      if (!snapshot) throw Error('no current room');
      clearPrivateCacheScope(scope);
      return retainPrivateCacheWire(
        scope,
        snapshot.rumorId,
        JSON.stringify({
          ...snapshot,
          peer: other,
          content: 'HOSTILE_CACHE_POISON'
        })
      );
    },
    async anotherWrap() {
      const key = new Uint8Array(32).fill(86);
      let next;
      try {
        next = finalizeEvent(
          {
            kind: 1059,
            created_at: 1699999799,
            tags: [['p', f.owner]],
            content: encrypt(key, f.owner, JSON.stringify(seal))
          },
          key
        );
      } finally {
        key.fill(0);
      }
      const candidate = admitInboxEnvelope(
        JSON.stringify(next),
        f.owner,
        'wss://archive.example.org',
        101
      );
      if (!candidate) throw Error('no second outer');
      const result = await retainInboxEnvelope(
        f.repository,
        privateSession,
        candidate
      );
      if (result.status !== 'retained') throw Error('no second retained outer');
      reader = captureReceivedUnwrap(
        f.repository,
        f.identity,
        next.id,
        'reviewed_inbox_unlock'
      );
      if (!reader) throw Error('no genuine second reader');
      const opened = await unwrapReceivedEnvelope(reader);
      if (opened.status !== 'authenticated')
        throw Error('no actual second decrypt');
      nested = opened.envelope;
      room = admitReceivedConversation(
        nested,
        scenario === 'self_archive' ? 'self_archive' : 'inbound'
      );
      return { outerId: next.id, rumorId: rumor.id };
    },
    stop() {
      if (reader) stopReceivedUnwrap(reader);
    },
    close() {
      if (unlocked) closeUnlockedSession(unlocked);
      if (otherPrivateSession) closePrivateSession(otherPrivateSession);
      if (otherIdentity) disconnectIdentity(otherIdentity);
      if (reader) stopReceivedUnwrap(reader);
      closePrivateSession(privateSession);
      f.close();
    }
  };
}
