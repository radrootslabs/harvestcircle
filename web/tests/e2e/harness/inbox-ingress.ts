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
  captureInboxIngress,
  startInboxIngress,
  stopInboxIngress,
  inboxIngressSnapshot,
  type InboxIngress
} from '../../../src/lib/nostr/inbox-ingress.ts';
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
  const publicStore = getPublicStore()!;
  const self = (await f.row())!.record.self;
  let ingress: InboxIngress | undefined;
  let authorized = true;
  let restoreFault = () => {};
  return {
    ...f,
    outer: self.wire,
    outerId: self.eventId,
    capture(access = true, review: unknown = 'reviewed_private_inbox_receive') {
      ingress = captureInboxIngress(
        f.repository,
        session,
        policy,
        f.context.own,
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
      return !!ingress;
    },
    start(review: unknown = 'reviewed_private_inbox_receive') {
      if (!ingress) return false;
      return startInboxIngress(ingress, review);
    },
    stop() {
      if (ingress) stopInboxIngress(ingress);
    },
    snapshot: () => ingress && inboxIngressSnapshot(ingress),
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
    revokeAccess() {
      authorized = false;
    },
    logout() {
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
      restoreFault();
      if (ingress) stopInboxIngress(ingress);
      closePrivateSession(session);
      f.close();
    }
  };
}
