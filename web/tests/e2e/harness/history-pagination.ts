import {
  captureOlderInbox,
  loadOlderInbox,
  olderInboxSnapshot,
  stopOlderInbox,
  type OlderInbox
} from '../../../src/lib/messaging/history-pagination.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import { outerHistoryPlan } from '../../../src/lib/messaging/history-cursor.ts';
import { finalizeEvent } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  captureUnlockedSession,
  closeUnlockedSession
} from '../../../src/lib/messaging/unlocked-session.ts';
import { identitySessionSnapshot } from '../../../src/lib/runtime/identity-session.ts';
import {
  browserExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../../src/lib/nostr/extension-scheduler.ts';
import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  createPrivateSession,
  closePrivateSession,
  privateSessionOwnership
} from '../../../src/lib/runtime/private-session.ts';
import {
  captureInboxSync,
  startInboxSync,
  stopInboxSync,
  inboxSyncSnapshot,
  type InboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
import { loadPrivateRecord } from '../../../src/lib/persistence/private-storage.ts';
import { privateRecordSnapshot } from '../../../src/lib/persistence/private-records.ts';
import {
  getPublicStore,
  publicStoreEnvelope
} from '../../../src/lib/nostr/public-store.ts';
export async function makeFixture(
  previousOuterCheck?: number,
  artifactAgeSeconds = 0,
  newStorage = true
) {
  outerHistoryPlan(Math.floor(Date.now() / 1000), previousOuterCheck);
  const originalClock = Date.now;
  const f = await (async () => {
    try {
      if (artifactAgeSeconds)
        Date.now = () => originalClock() - artifactAgeSeconds * 1000;
      // Actual SDK creates/signs/persists the original send while this isolated
      // fixture clock is old. Never edit a signed timestamp or regenerate it at delivery.
      return await makeRecoveryFixture(newStorage);
    } finally {
      Date.now = originalClock;
    }
  })();
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('fixture policy unavailable');
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('missing genuine session');
  const capture = privateSessionOwnership(session)!;
  const unlocked = captureUnlockedSession(session, 'reviewed_messages_unlock');
  if (!unlocked) throw Error('missing genuine unlock');
  const scheduler = createPublicScheduler({
    now: () => 0,
    schedule: () => () => {}
  });
  const fixtureSecret = new Uint8Array(32).fill(83);
  let ownEvent: ReturnType<typeof finalizeEvent>;
  try {
    ownEvent = finalizeEvent(
      {
        kind: 10050,
        created_at: 101,
        tags: [
          ['relay', 'wss://archive.example.org'],
          ['relay', 'wss://peer.example.org']
        ],
        content: ''
      },
      fixtureSecret
    );
  } finally {
    fixtureSecret.fill(0);
  }
  const publicRun = createPublicRun(scheduler, policy);
  const own = createInboxResolver(
    publicRun,
    f.owner,
    (next) =>
      openInboxRequest(
        publicRun,
        f.owner,
        (sink) => {
          sink({
            type: 'EVENT',
            from: 'wss://discovery.example.org',
            id: 'fixture',
            event: ownEvent
          });
          sink({
            type: 'EOSE',
            from: 'wss://discovery.example.org',
            id: 'fixture'
          });
          return () => {};
        },
        next
      ),
    () => capture.current()
  );
  const publicStore = getPublicStore()!;
  const self = (await f.row())!.record.self;
  let ingress: InboxSync | undefined;
  let older: OlderInbox | undefined;
  let authorized = true;
  let restoreFault = () => {};
  let releaseKey: (() => void) | undefined;
  let restoreKey = () => {};
  return {
    ...f,
    outer: self.wire,
    outerId: self.eventId,
    oldSignedOuter(createdAt: number) {
      const ownerSecret = new Uint8Array(32).fill(83),
        randomSecret = new Uint8Array(32).fill(86);
      let oldKey: Uint8Array | undefined, nextKey: Uint8Array | undefined;
      try {
        const old = JSON.parse(self.wire) as {
          pubkey: string;
          content: string;
        };
        oldKey = nip44.v2.utils.getConversationKey(ownerSecret, old.pubkey);
        const seal = nip44.v2.decrypt(old.content, oldKey);
        nextKey = nip44.v2.utils.getConversationKey(randomSecret, f.owner);
        return JSON.stringify(
          finalizeEvent(
            {
              kind: 1059,
              created_at: createdAt,
              tags: [['p', f.owner]],
              content: nip44.v2.encrypt(seal, nextKey)
            },
            randomSecret
          )
        );
      } finally {
        ownerSecret.fill(0);
        randomSecret.fill(0);
        oldKey?.fill(0);
        nextKey?.fill(0);
      }
    },
    capture(
      access = true,
      review: unknown = 'reviewed_foreground_inbox',
      outerCheck = previousOuterCheck
    ) {
      const candidate = captureInboxSync(
        f.repository,
        session,
        policy,
        own,
        unlocked,
        access
          ? (context) => ({
              ...context,
              receive: 'qualified_exercised',
              archive: 'qualified_exercised',
              current: () => authorized && capture.current()
            })
          : undefined,
        review,
        outerCheck
      );
      if (candidate) ingress = candidate;
      return !!candidate;
    },
    start(review: unknown = 'reviewed_foreground_inbox') {
      if (!ingress) return false;
      return startInboxSync(ingress, review);
    },
    stop() {
      if (ingress) stopInboxSync(ingress);
    },
    snapshot: () => ingress && inboxSyncSnapshot(ingress),
    captureOlder(review: unknown = 'reviewed_load_older') {
      if (!ingress) return false;
      const candidate = captureOlderInbox(ingress, review);
      if (candidate) older = candidate;
      return !!candidate;
    },
    loadOlder(review: unknown = 'reviewed_load_older') {
      return older ? loadOlderInbox(older, review) : Promise.resolve(false);
    },
    copiedOlderStart() {
      return older
        ? loadOlderInbox(Object.freeze({ ...older }), 'reviewed_load_older')
        : Promise.resolve(false);
    },
    olderSnapshot: () => older && olderInboxSnapshot(older),
    stopOlder() {
      if (older) stopOlderInbox(older);
    },
    async seed(wire: string, origin = 'wss://archive.example.org') {
      const envelope = admitInboxEnvelope(wire, f.owner, origin, Date.now());
      if (!envelope) throw Error('invalid genuinefixture envelope');
      const result = await retainInboxEnvelope(f.repository, session, envelope);
      if (result.status !== 'retained' && result.status !== 'duplicate')
        throw Error('genuinefixture retention failed');
    },
    async stored(id = self.eventId) {
      const row = await loadPrivateRecord(
        f.repository,
        'received_envelopes',
        id
      );
      return row.ok ? privateRecordSnapshot(row.value, f.owner, id) : undefined;
    },
    publicContains: (id = self.eventId) =>
      !!publicStoreEnvelope(publicStore, id),
    countsValue: () => f.counts(),
    closeUnlock() {
      closeUnlockedSession(unlocked);
    },
    navigateAway() {
      if (ingress) stopInboxSync(ingress);
    },
    changeKey() {
      f.mode('changed_key');
    },
    holdNextKey() {
      const provider = window.nostr as
        { getPublicKey: () => Promise<string> } | undefined;
      if (!provider) throw Error('missing genuine fixture provider');
      const original = provider.getPublicKey;
      provider.getPublicKey = async () => {
        const owner = await original.call(provider);
        return await new Promise<string>((resolve) => {
          releaseKey = () => resolve(owner);
        });
      };
      restoreKey = () => {
        provider.getPublicKey = original;
      };
    },
    pendingKey: () => !!releaseKey,
    keyJobOccupied() {
      const scheduler = browserExtensionScheduler();
      const slot = scheduler && extensionSchedulerSnapshot(scheduler);
      return slot?.state === 'active' && slot.pending === 'key';
    },
    identityState: () => identitySessionSnapshot(f.identity).state,
    settleKey() {
      const release = releaseKey;
      releaseKey = undefined;
      restoreKey();
      release?.();
    },
    revokeAccess() {
      authorized = false;
    },
    logout() {
      closePrivateSession(session);
    },
    logoutIdentity() {
      f.disconnect();
      closePrivateSession(session);
    },
    restoreFault() {
      restoreFault();
    },
    faultRetention(kind: 'abort' | 'readback') {
      const put = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      );
      const cursor = Reflect.get<IDBIndex, 'openCursor'>(
        IDBIndex.prototype,
        'openCursor'
      );
      let written = false;
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        const request = put.apply(this, args);
        if (this.name === 'received_envelopes') {
          written = true;
          if (kind === 'abort') this.transaction.abort();
        }
        return request;
      };
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        if (
          kind === 'readback' &&
          written &&
          this.objectStore.name === 'received_envelopes' &&
          this.objectStore.transaction.mode === 'readonly'
        )
          throw Error('actual post-put inbox readback failure');
        return cursor.apply(this, args);
      };
      restoreFault = () => {
        IDBObjectStore.prototype.put = put;
        IDBIndex.prototype.openCursor = cursor;
      };
    },
    close() {
      const release = releaseKey;
      releaseKey = undefined;
      restoreKey();
      release?.();
      restoreFault();
      if (older) stopOlderInbox(older);
      closePublicScheduler(scheduler);
      if (ingress) stopInboxSync(ingress);
      closePrivateSession(session);
      f.close();
    }
  };
}
