import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  createPrivateSession,
  closePrivateSession,
  privateSessionOwnership
} from '../../../src/lib/runtime/private-session.ts';
import { captureUnlockedSession } from '../../../src/lib/messaging/unlocked-session.ts';
import {
  captureInboxSync,
  startInboxSync,
  stopInboxSync,
  inboxSyncSnapshot,
  type InboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  captureReceivedCleanup,
  deleteReviewedReceivedCopies,
  receivedCleanupSnapshot,
  stopReceivedCleanup,
  type ReceivedCleanup
} from '../../../src/lib/persistence/inbox-retention.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  listPrivateReceivedRecords,
  commitPrivateRecord,
  createPrivateStorageRepository
} from '../../../src/lib/persistence/private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordIdentity,
  privateRecordWire,
  privateRecordSnapshot
} from '../../../src/lib/persistence/private-records.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
export async function makeFixture() {
  const f = await makeRecoveryFixture();
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('genuine session absent');
  const unlocked = captureUnlockedSession(session, 'reviewed_messages_unlock');
  if (!unlocked) throw Error('genuine unlock absent');
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('policy absent');
  const initial = (await f.row())!;
  const originalOutbox = JSON.stringify(initial.record);
  const ownerSecret = new Uint8Array(32).fill(83),
    outerSecret = new Uint8Array(32).fill(86),
    otherSecret = new Uint8Array(32).fill(85),
    otherOwner = getPublicKey(otherSecret);
  const ids: string[] = [];
  let outer: string = '',
    foreignWire: string = '';
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('native database absent');
  const foreignRepo = createPrivateStorageRepository(opened.owner, otherOwner);
  if (!foreignRepo) throw Error('foreign namespace absent');
  try {
    const original = JSON.parse(initial.record.self.wire) as {
      pubkey: string;
      content: string;
    };
    const key = nip44.v2.utils.getConversationKey(ownerSecret, original.pubkey);
    let seal: string;
    try {
      seal = nip44.v2.decrypt(original.content, key);
    } finally {
      key.fill(0);
    }
    function wrap(target: string, n: number) {
      const k = nip44.v2.utils.getConversationKey(outerSecret, target);
      try {
        return finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000 + n,
            tags: [['p', target]],
            content: nip44.v2.encrypt(seal, k)
          },
          outerSecret
        );
      } finally {
        k.fill(0);
      }
    }
    for (let i = 0; i < 3; i++) {
      const event = wrap(f.owner, i);
      const wire = JSON.stringify(event),
        admitted = admitInboxEnvelope(
          wire,
          f.owner,
          'wss://archive.example.org',
          100 + i
        );
      if (
        !admitted ||
        (await retainInboxEnvelope(f.repository, session, admitted)).status !==
          'retained'
      )
        throw Error('real retention absent');
      ids.push(event.id);
      if (!i) outer = wire;
    }
    const event = wrap(otherOwner, 4),
      decoded = decodePrivateRecord(
        JSON.stringify({
          schema: 1,
          family: 'received_envelope',
          owner: otherOwner,
          id: event.id,
          revision: 0,
          outer: JSON.stringify(event),
          observedAtMilliseconds: 100,
          sources: ['wss://archive.example.org'],
          read: null
        }),
        otherOwner,
        event.id
      );
    if (!decoded.ok) throw Error('foreign structural record absent');
    if (!(await commitPrivateRecord(foreignRepo, decoded.value, null)).ok)
      throw Error('foreign native row absent');
    foreignWire = privateRecordWire(decoded.value, otherOwner, event.id)!;
  } finally {
    ownerSecret.fill(0);
    outerSecret.fill(0);
    otherSecret.fill(0);
  }
  let cleanup: ReceivedCleanup | undefined, sync: InboxSync | undefined;
  let restore = () => {};
  const baseline = f.counts();
  async function rows() {
    const result = await listPrivateReceivedRecords(f.repository);
    if (!result.ok) throw Error('native scan failed');
    return result.value.map((h) => ({
      id: privateRecordIdentity(h)!.id,
      wire: privateRecordWire(h, f.owner, privateRecordIdentity(h)!.id)!
    }));
  }
  return {
    ...f,
    outer,
    ids,
    async select(
      selected: unknown = ids.slice(0, 2),
      review: unknown = 'reviewed_received_cleanup_selection'
    ) {
      cleanup = await captureReceivedCleanup(
        f.repository,
        session,
        selected,
        review
      );
      return !!cleanup;
    },
    snapshot: () => cleanup && receivedCleanupSnapshot(cleanup),
    purge: (review: unknown = 'reviewed_delete_local_received_copies') =>
      cleanup
        ? deleteReviewedReceivedCopies(cleanup, review)
        : Promise.resolve({ status: 'invalid' }),
    copied: () =>
      deleteReviewedReceivedCopies(
        {} as ReceivedCleanup,
        'reviewed_delete_local_received_copies'
      ),
    rows,
    async preserved() {
      const rows = await listPrivateReceivedRecords(foreignRepo);
      return (
        JSON.stringify((await f.row())!.record) === originalOutbox &&
        rows.ok &&
        rows.value.length === 1 &&
        privateRecordWire(
          rows.value[0],
          otherOwner,
          privateRecordIdentity(rows.value[0])!.id
        ) === foreignWire
      );
    },
    delta: () => ({
      decrypts: f.counts().decrypts - baseline.decrypts,
      encrypts: f.counts().encrypts - baseline.encrypts,
      signs: f.counts().signs - baseline.signs
    }),
    logout() {
      closePrivateSession(session);
    },
    async revise() {
      const scan = await listPrivateReceivedRecords(f.repository);
      if (!scan.ok) throw Error('scan absent');
      const base = scan.value.find(
        (h) => privateRecordIdentity(h)?.id === ids[0]
      )!;
      const row = privateRecordSnapshot(base, f.owner, ids[0])!;
      if (row.family !== 'received_envelope')
        throw Error('received row absent');
      const next = decodePrivateRecord(
        JSON.stringify({
          ...row,
          revision: row.revision + 1,
          sources: [...row.sources, 'wss://other.example.org']
        }),
        f.owner,
        ids[0]
      );
      if (
        !next.ok ||
        !(await commitPrivateRecord(f.repository, next.value, base)).ok
      )
        throw Error('actual CAS revision failed');
    },
    fault(kind: 'abort' | 'readback' | 'logout_cursor' | 'stop_cursor') {
      const remove = Reflect.get<IDBObjectStore, 'delete'>(
          IDBObjectStore.prototype,
          'delete'
        ),
        cursor = Reflect.get<IDBIndex, 'openCursor'>(
          IDBIndex.prototype,
          'openCursor'
        );
      let written = false;
      IDBObjectStore.prototype.delete = function (
        ...args: Parameters<IDBObjectStore['delete']>
      ) {
        const request = remove.apply(this, args);
        if (this.name === 'received_envelopes') {
          written = true;
          if (kind === 'abort') this.transaction.abort();
        }
        return request;
      };
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        if (this.objectStore.name === 'received_envelopes') {
          if (
            kind === 'stop_cursor' &&
            this.objectStore.transaction.mode === 'readwrite' &&
            cleanup
          )
            stopReceivedCleanup(cleanup);
          if (
            kind === 'readback' &&
            written &&
            this.objectStore.transaction.mode === 'readonly'
          )
            throw Error('actual post-delete readback failure');
          if (
            kind === 'logout_cursor' &&
            this.objectStore.transaction.mode === 'readwrite'
          )
            closePrivateSession(session);
        }
        return cursor.apply(this, args);
      };
      restore = () => {
        IDBObjectStore.prototype.delete = remove;
        IDBIndex.prototype.openCursor = cursor;
      };
    },
    restore() {
      restore();
    },
    start() {
      const ownership = privateSessionOwnership(session)!;
      sync = captureInboxSync(
        f.repository,
        session,
        policy,
        f.context.own,
        unlocked,
        (c) => ({
          ...c,
          receive: 'qualified_exercised',
          archive: 'qualified_exercised',
          current: ownership.current
        }),
        'reviewed_foreground_inbox'
      );
      return !!sync && startInboxSync(sync, 'reviewed_foreground_inbox');
    },
    sync: () => sync && inboxSyncSnapshot(sync),
    storm(contentBytes = 16) {
      const secret = new Uint8Array(32).fill(87);
      try {
        return finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000,
            tags: [['p', f.owner]],
            content: 'x'.repeat(contentBytes)
          },
          secret
        );
      } finally {
        secret.fill(0);
      }
    },
    close() {
      restore();
      if (cleanup) stopReceivedCleanup(cleanup);
      if (sync) stopInboxSync(sync);
      closePrivateSession(session);
      closeBrowserDatabase(opened.owner);
      f.close();
    }
  };
}
