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
  artifactAgeSeconds = 0
) {
  outerHistoryPlan(Math.floor(Date.now() / 1000), previousOuterCheck);
  const originalClock = Date.now;
  const f = await (async () => {
    try {
      if (artifactAgeSeconds)
        Date.now = () => originalClock() - artifactAgeSeconds * 1000;
      // Actual SDK creates/signs/persists the original send while this isolated
      // fixture clock is old. Never edit a signed timestamp or regenerate it at delivery.
      return await makeRecoveryFixture();
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
  const capturedSession = session;
  const capture = privateSessionOwnership(session)!;
  const unlocked = captureUnlockedSession(session, 'reviewed_messages_unlock');
  if (!unlocked) throw Error('missing genuine unlock');
  const publicStore = getPublicStore()!;
  const self = (await f.row())!.record.self;
  let ingress: InboxSync | undefined;
  async function retireOriginalSync() {
    for (;;) {
      let incomplete = false;
      try {
        if (ingress) stopInboxSync(ingress);
      } catch (error) {
        if (
          !(error instanceof Error) ||
          error.message !== 'private_inbox_cleanup_required'
        )
          throw error;
        incomplete = true;
      }
      const sessionClean = closePrivateSession(capturedSession);
      if (!incomplete && sessionClean) {
        if (ingress && inboxSyncSnapshot(ingress)?.cleanupRequired)
          throw Error('actual original sync cleanup remains incomplete');
        return;
      }
      // Retry exact original disposal after its actual SDK close observers;
      // this wait starts no retrieval, permission or replacement session.
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    }
  }

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
        f.context.own,
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
    stop: retireOriginalSync,
    snapshot: () => ingress && inboxSyncSnapshot(ingress),
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
    navigateAway: retireOriginalSync,
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
    async close() {
      const release = releaseKey;
      releaseKey = undefined;
      restoreKey();
      release?.();
      restoreFault();
      await retireOriginalSync();
      closePrivateSession(session);
      f.close();
    }
  };
}
