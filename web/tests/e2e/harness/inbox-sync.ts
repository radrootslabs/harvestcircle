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
export async function makeFixture() {
  const f = await makeRecoveryFixture();
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
  const publicStore = getPublicStore()!;
  const self = (await f.row())!.record.self;
  let ingress: InboxSync | undefined;
  let authorized = true;
  let restoreFault = () => {};
  let releaseKey: (() => void) | undefined;
  let restoreKey = () => {};
  return {
    ...f,
    outer: self.wire,
    outerId: self.eventId,
    capture(access = true, review: unknown = 'reviewed_foreground_inbox') {
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
        review
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
      if (ingress) stopInboxSync(ingress);
      closePrivateSession(session);
      f.close();
    }
  };
}
