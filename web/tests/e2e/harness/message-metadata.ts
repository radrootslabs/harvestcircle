import {
  captureMessageMetadata,
  messageMetadataSnapshot,
  markDisplayedMessage,
  type MessageMetadata,
  MESSAGE_METADATA_DISCLOSURE
} from '../../../src/lib/persistence/message-metadata.ts';
import { readLocalUnread } from '../../../src/lib/messaging/unread.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction,
  type BrowserDatabase
} from '../../../src/lib/persistence/database.ts';
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
export async function makeFixture(
  scenario: Scenario = 'inbound',
  initialise = true
) {
  const f = await makeRecoveryFixture(initialise),
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
  let database: BrowserDatabase | undefined,
    metadata: MessageMetadata | undefined;
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
    async captureMetadata(review: unknown = 'reviewed_local_message_metadata') {
      if (!database) {
        const opened = await openBrowserDatabase();
        if (opened.state !== 'ready')
          throw Error('actual metadata database unavailable');
        database = opened.owner;
      }
      if (!unlocked) return false;
      metadata = captureMessageMetadata(
        database,
        privateSession,
        unlocked,
        review
      );
      return !!metadata;
    },
    async unread() {
      return metadata && (await readLocalUnread(metadata));
    },
    async metadataSnapshot() {
      return metadata && (await messageMetadataSnapshot(metadata));
    },
    async display(review: unknown = 'displayed_message') {
      return metadata && room
        ? await markDisplayedMessage(metadata, room, review)
        : { status: 'invalid' };
    },
    async forgedDisplay() {
      return metadata
        ? await markDisplayedMessage(
            metadata,
            {} as AdmittedConversation,
            'displayed_message'
          )
        : { status: 'invalid' };
    },
    async copiedDisplay() {
      return metadata && room
        ? await markDisplayedMessage(metadata, { ...room }, 'displayed_message')
        : { status: 'invalid' };
    },
    disclosure() {
      return MESSAGE_METADATA_DISCLOSURE;
    },
    async rawMetadata() {
      if (!database) throw Error('no actual database');
      const tx = browserDatabaseTransaction(
        database,
        ['received_envelopes', 'conversations'],
        'readonly'
      );
      const received = tx.objectStore('received_envelopes').getAll(),
        pairs = tx.objectStore('conversations').getAll();
      return await new Promise<{ received: unknown[]; pairs: unknown[] }>(
        (resolve, reject) => {
          tx.addEventListener('complete', () =>
            resolve({ received: received.result, pairs: pairs.result })
          );
          tx.addEventListener('abort', () =>
            reject(Error('actual raw scan aborted'))
          );
        }
      );
    },
    async corruptPair() {
      if (!database) throw Error('no actual database');
      const tx = browserDatabaseTransaction(
        database,
        ['conversations'],
        'readwrite'
      );
      tx.objectStore('conversations').put({
        owner: f.owner,
        id: '12345678-1234-4234-8234-123456789abd',
        wire: JSON.stringify({
          schema: 1,
          family: 'conversation_handle',
          owner: f.owner,
          id: '12345678-1234-4234-8234-123456789abd',
          peer: f.peer,
          subject: 'FORBIDDEN_CONTEXT'
        })
      });
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('corruption fixture aborted'))
        );
      });
    },
    abortNextWrite() {
      const original = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      )?.value as IDBObjectStore['put'];
      let consumed = false;
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<typeof original>
      ) {
        const request = original.apply(this, args);
        if (this.name === 'received_envelopes' && !consumed) {
          consumed = true;
          this.transaction.abort();
        }
        return request;
      };
      return () => {
        IDBObjectStore.prototype.put = original;
      };
    },
    async otherOwnerMetadata() {
      f.mode('changed_key');
      otherIdentity = createIdentitySession();
      await connectIdentity(otherIdentity);
      await probeIdentityMessaging(otherIdentity, 'reviewed_self_copy');
      otherPrivateSession = await createPrivateSession(
        otherIdentity,
        'reviewed_private_session'
      );
      if (!database || !otherPrivateSession)
        throw Error('no genuine other owner');
      const scope = captureUnlockedSession(
        otherPrivateSession,
        'reviewed_messages_unlock'
      );
      if (!scope) throw Error('no genuine other cache');
      const m = captureMessageMetadata(
        database,
        otherPrivateSession,
        scope,
        'reviewed_local_message_metadata'
      );
      if (!m) throw Error('no genuine other metadata');
      return await messageMetadataSnapshot(m);
    },
    async wrongGenerationMetadata() {
      otherIdentity = createIdentitySession();
      await connectIdentity(otherIdentity);
      await probeIdentityMessaging(otherIdentity, 'reviewed_self_copy');
      otherPrivateSession = await createPrivateSession(
        otherIdentity,
        'reviewed_private_session'
      );
      if (!database || !otherPrivateSession || !unlocked)
        throw Error('no genuine generation test');
      return !!captureMessageMetadata(
        database,
        otherPrivateSession,
        unlocked,
        'reviewed_local_message_metadata'
      );
    },
    async queuedLogout() {
      if (!metadata || !room) throw Error('no genuine current metadata/room');
      let release!: () => void, held!: () => void;
      const ready = new Promise<void>((r) => {
        held = r;
      });
      const blocker = navigator.locks.request(
        'harvestcircle:owner:' + f.owner,
        async () => {
          held();
          await new Promise<void>((r) => {
            release = r;
          });
        }
      );
      await ready;
      const pending = markDisplayedMessage(metadata, room, 'displayed_message');
      f.disconnect();
      release();
      await blocker;
      return await pending;
    },
    failNextReadback() {
      const original = Object.getOwnPropertyDescriptor(
        IDBIndex.prototype,
        'openCursor'
      )?.value as IDBIndex['openCursor'];
      let reads = 0;
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<typeof original>
      ) {
        if (
          this.objectStore.name === 'received_envelopes' &&
          this.objectStore.transaction.mode === 'readonly' &&
          ++reads === 2
        )
          throw new DOMException('actual readback failure', 'AbortError');
        return original.apply(this, args);
      };
      return () => {
        IDBIndex.prototype.openCursor = original;
      };
    },
    async raceNextWrite() {
      if (!database) throw Error('no database');
      const rows = await this.rawMetadata(),
        raw = (
          rows.received as { owner: string; id: string; wire: string }[]
        ).find((x) => x.owner === f.owner);
      if (!raw) throw Error('missing received race row');
      const record = JSON.parse(raw.wire) as {
        revision: number;
        sources: string[];
      };
      const wire = JSON.stringify({
        ...record,
        revision: record.revision + 1,
        sources: [...record.sources, 'wss://race.example.org']
      });
      const original = Object.getOwnPropertyDescriptor(
        IDBDatabase.prototype,
        'transaction'
      )?.value as IDBDatabase['transaction'];
      let injected = false;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<typeof original>
      ) {
        const stores = args[0],
          mode = args[1];
        if (
          !injected &&
          mode === 'readwrite' &&
          Array.isArray(stores) &&
          stores.includes('received_envelopes')
        ) {
          injected = true;
          const rival = original.call(
            this,
            ['received_envelopes'],
            'readwrite'
          );
          rival.objectStore('received_envelopes').put({ ...raw, wire });
        }
        return original.apply(this, args);
      };
      return () => {
        IDBDatabase.prototype.transaction = original;
      };
    },
    async anotherWrap(lateOld = false) {
      const selectedTemplate = {
          ...template,
          created_at: 1,
          content: 'Late old actual encrypted rumor'
        },
        selectedRumor = lateOld
          ? { id: getEventHash(selectedTemplate), ...selectedTemplate }
          : rumor;
      let selectedSeal = seal;
      if (lateOld) {
        const peer = new Uint8Array(32).fill(84);
        try {
          selectedSeal = finalizeEvent(
            {
              kind: 13,
              created_at: 1,
              tags: [],
              content: encrypt(peer, f.owner, JSON.stringify(selectedRumor))
            },
            peer
          );
        } finally {
          peer.fill(0);
        }
      }

      const key = new Uint8Array(32).fill(86);
      let next;
      try {
        next = finalizeEvent(
          {
            kind: 1059,
            created_at: 1699999799,
            tags: [['p', f.owner]],
            content: encrypt(key, f.owner, JSON.stringify(selectedSeal))
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
      return { outerId: next.id, rumorId: selectedRumor.id };
    },
    stop() {
      if (reader) stopReceivedUnwrap(reader);
    },
    close() {
      if (database) closeBrowserDatabase(database);
      if (unlocked) closeUnlockedSession(unlocked);
      if (otherPrivateSession) closePrivateSession(otherPrivateSession);
      if (otherIdentity) disconnectIdentity(otherIdentity);
      if (reader) stopReceivedUnwrap(reader);
      closePrivateSession(privateSession);
      f.close();
    }
  };
}
