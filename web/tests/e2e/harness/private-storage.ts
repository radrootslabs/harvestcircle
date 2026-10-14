import { generateSecretKey, finalizeEvent } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  openBrowserDatabase,
  browserDatabaseTransaction,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import {
  decodePrivateRecord,
  privateRecordWire
} from '../../../src/lib/persistence/private-records.ts';
import {
  createPrivateStorageRepository,
  commitPrivateRecord,
  loadPrivateRecord,
  inspectPrivateStorage
} from '../../../src/lib/persistence/private-storage.ts';
import {
  createPrivateSendReservationRepository,
  reservePrivateSendReservation
} from '../../../src/lib/persistence/private-send-reservations.ts';
export {
  decodePrivateRecord,
  privateRecordWire,
  commitPrivateRecord,
  loadPrivateRecord,
  inspectPrivateStorage
};
export const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
export const peer =
  'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
export const command = '12345678-1234-4234-8234-123456789abc';
export const id = (index: number) =>
  '12345678-1234-4234-8234-' + index.toString(16).padStart(12, '0');
// Real browser IDB; generated disposable fixture crypto only, not a genuine
// nested exchange or installed extension/operator qualification.
export async function makeFixture() {
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('browser storage unavailable');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, owner),
    reservations = createPrivateSendReservationRepository(database, owner);
  if (!repository || !reservations) throw Error('invalid fixture namespace');
  const repo = repository,
    reservedRepo = reservations;
  function reservation(index: number) {
    return {
      schema: 1,
      family: 'private_send_reservation',
      owner,
      id: id(index),
      revision: 0,
      peer,
      rumorHash: index.toString(16).padStart(64, '0'),
      createdAt: 1700000000
    };
  }
  function outer(destination = owner, index = 0, padded = false) {
    const secret = generateSecretKey();
    let conversation: Uint8Array | undefined;
    try {
      conversation = nip44.v2.utils.getConversationKey(secret, destination);
      const event = finalizeEvent(
        {
          kind: 1059,
          created_at: 1700000000 + index,
          tags: [['p', destination]],
          content: nip44.v2.encrypt(
            'HCP080 private plaintext sentinel',
            conversation
          )
        },
        secret
      );
      const wire = JSON.stringify(event);
      // Detached valid signed raw JSON whitespace fixtures measure actual
      // stored JSON UTF8 bytes; they are not factory/nested admission claims.
      return {
        eventId: event.id,
        wire: padded
          ? wire + '\n'.repeat(32768 - new TextEncoder().encode(wire).length)
          : wire
      };
    } finally {
      conversation?.fill(0);
      secret.fill(0);
    }
  }
  function operation(index: number, padded = false) {
    return {
      ...reservation(index),
      family: 'private_send_operation',
      revision: 1,
      self: outer(owner, index, padded),
      peerArtifact: padded ? outer(peer, index, padded) : null
    };
  }
  function probe(index: number, prepared = false, padded = false) {
    const origin = 'wss://inbox.example.org',
      policyWire = JSON.stringify({
        schemaVersion: 1,
        public: [
          {
            origin: 'wss://public.example.org',
            read: true,
            write: false,
            nip50: false
          }
        ],
        inbox: [{ origin, read: true, write: true }],
        postingEnabled: false,
        messagingEnabled: true,
        operatorDenylist: []
      });
    return {
      schema: 1,
      family: 'private_access_probe',
      owner,
      id: id(index),
      revision: prepared ? 1 : 0,
      rumorHash: index.toString(16).padStart(64, '0'),
      createdAt: 1700000000,
      binding: {
        preferenceId: 'f'.repeat(64),
        preferenceCreatedAt: 1699999999,
        readTargets: [origin],
        archiveTargets: [origin],
        policyWire,
        operatorWire: JSON.stringify({
          schemaVersion: 1,
          revision: 1,
          authority: 'source_fixture',
          policyWire,
          evidenceSha256: 'a'.repeat(64),
          relays: [
            {
              origin,
              auth: 'not_required',
              recipientRead: 'supported',
              namedArchiveAck: 'supported',
              exactReadback: 'supported',
              retention: 'operator_defined'
            }
          ]
        })
      },
      self: prepared ? outer(owner, index, padded) : null,
      receipts: []
    };
  }
  function received(index: number, padded = false) {
    const artifact = outer(owner, index, padded);
    return {
      schema: 1,
      family: 'received_envelope',
      owner,
      id: artifact.eventId,
      revision: 0,
      outer: artifact.wire,
      observedAtMilliseconds: 1700000000000,
      sources: ['wss://inbox.example.org'],
      read: null
    };
  }
  function handle<T extends { owner: string; id: string }>(record: T) {
    const result = decodePrivateRecord(
      JSON.stringify(record),
      record.owner,
      record.id
    );
    if (!result.ok) throw Error('invalid fixture record');
    return result.value;
  }
  async function rows(
    store: 'private_sends' | 'received_envelopes' | 'public_drafts'
  ) {
    const tx = browserDatabaseTransaction(database, [store], 'readonly'),
      request = tx.objectStore(store).getAll();
    return await new Promise<unknown[]>((resolve, reject) => {
      tx.addEventListener('complete', () =>
        resolve(request.result as unknown[])
      );
      tx.addEventListener('abort', () => reject(Error('inspection aborted')));
    });
  }
  async function inject(
    store: 'private_sends' | 'received_envelopes',
    values: unknown[]
  ) {
    const tx = browserDatabaseTransaction(database, [store], 'readwrite');
    for (const value of values) tx.objectStore(store).put(value);
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () => reject(Error('injection aborted')));
    });
  }
  return {
    database,
    repository: repo,
    reservation,
    operation,
    probe,
    received,
    handle,
    rows,
    inject,
    reserve: (index: number) =>
      reservePrivateSendReservation(
        reservedRepo,
        JSON.stringify(reservation(index)),
        1700000000,
        () => true
      ),
    close: () => closeBrowserDatabase(database)
  };
}
