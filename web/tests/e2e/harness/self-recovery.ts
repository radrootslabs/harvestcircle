import { makeFixture as makeCryptoFixture } from './giftwrap-builder.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import { createPrivateStorageRepository } from '../../../src/lib/persistence/private-storage.ts';
import { reservedSendSnapshot } from '../../../src/lib/messaging/send-identity.ts';
import {
  captureEnvelopePreparation,
  prepareEnvelopeRole,
  preparedEnvelopeProof,
  closeEnvelopePreparation
} from '../../../src/lib/messaging/envelope-preparation.ts';
import {
  commitSelfRecovery,
  selfRecoveryAcknowledgementSnapshot
} from '../../../src/lib/persistence/private-sends.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  selfRecoveryPeerPreparation,
  stopSelfRecoveryPreparation
} from '../../../src/lib/messaging/prepare-send.ts';
export {
  commitSelfRecovery,
  selfRecoveryAcknowledgementSnapshot,
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  selfRecoveryPeerPreparation,
  stopSelfRecoveryPreparation
};
// Source-only genuine stock SDK plus actual Chromium IDB/Web Locks. Neither
// this disposable provider nor these synthetic faults qualify an operator.
export async function makeFixture() {
  const marker = 'HCP081_PRIVATE_TEXT_MEMORY_ONLY_SENTINEL';
  const cryptoFixture = await makeCryptoFixture(marker);
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('self storage unavailable');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, cryptoFixture.owner),
    foreignRepository = createPrivateStorageRepository(
      database,
      cryptoFixture.peer
    );
  if (!repository || !foreignRepository)
    throw Error('self storage namespace missing');
  const preparation = captureSelfRecoveryPreparation(
    repository,
    cryptoFixture.identity,
    cryptoFixture.reserved,
    'reviewed_self_recovery'
  );
  if (!preparation) throw Error('self preparation missing');
  let restore = () => {},
    releaseLock: (() => void) | undefined;
  const pairs = new Set<ReturnType<typeof captureEnvelopePreparation>>();
  async function proof(role: 'self' | 'peer') {
    const pair = captureEnvelopePreparation(
      cryptoFixture.identity,
      cryptoFixture.reserved,
      'reviewed_envelope_pair'
    );
    if (!pair) throw Error('crypto pair missing');
    pairs.add(pair);
    if (
      (await prepareEnvelopeRole(pair, 'self', 'reviewed_pair_role')).status !==
        'prepared' ||
      (role === 'peer' &&
        (await prepareEnvelopeRole(pair, 'peer', 'reviewed_pair_role'))
          .status !== 'complete')
    )
      throw Error('role proof missing');
    const proof = preparedEnvelopeProof(pair, role);
    if (!proof) throw Error('peer proof missing');
    return proof;
  }
  // The queued write and transaction are real. In unknown mode the fixture
  // loses its acknowledgement after the actual put, then makes abort fail;
  // actual completion/readback determine whether the original ciphertext exists.
  function fault(mode: 'abort' | 'unknown') {
    restore();
    const put = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      ),
      abort = Reflect.get<IDBTransaction, 'abort'>(
        IDBTransaction.prototype,
        'abort'
      );
    let hits = 0;
    const affected = new WeakSet<IDBTransaction>();
    IDBObjectStore.prototype.put = function (
      value: unknown,
      key?: IDBValidKey
    ) {
      const request =
        key === undefined ? put.call(this, value) : put.call(this, value, key);
      if (this.name === 'private_sends' && hits === 0) {
        hits++;
        if (mode === 'abort') abort.call(this.transaction);
        else {
          affected.add(this.transaction);
          throw Error('controlled lost acknowledgement');
        }
      }
      return request;
    };
    IDBTransaction.prototype.abort = function () {
      if (affected.has(this)) throw Error('controlled abort unavailable');
      return abort.call(this);
    };
    restore = () => {
      IDBObjectStore.prototype.put = put;
      IDBTransaction.prototype.abort = abort;
    };
    return () => hits;
  }
  async function holdOwner() {
    let entered: (() => void) | undefined;
    const ready = new Promise<void>((resolve) => {
      entered = resolve;
    });
    const held = navigator.locks.request(
      'harvestcircle:owner:' + cryptoFixture.owner,
      async () => {
        entered?.();
        await new Promise<void>((resolve) => {
          releaseLock = resolve;
        });
      }
    );
    await ready;
    return () => {
      releaseLock?.();
      return held;
    };
  }
  async function publicDraftRows() {
    const tx = browserDatabaseTransaction(
        database,
        ['public_drafts'],
        'readonly'
      ),
      request = tx.objectStore('public_drafts').getAll();
    return new Promise<string>((resolve, reject) => {
      tx.addEventListener('complete', () =>
        resolve(JSON.stringify(request.result))
      );
      tx.addEventListener('abort', () => reject(Error('inspection aborted')));
    });
  }
  async function corruptReservation() {
    const record = reservedSendSnapshot(cryptoFixture.reserved);
    if (!record) throw Error('missing original reservation');
    const tx = browserDatabaseTransaction(
      database,
      ['private_sends'],
      'readwrite'
    );
    tx.objectStore('private_sends').put({
      owner: record.owner,
      id: record.id,
      wire: '{"schema":999}'
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('fixture write aborted'))
      );
    });
  }
  return {
    ...cryptoFixture,
    marker,
    database,
    repository,
    foreignRepository,
    preparation,
    fault,
    peerProof: () => proof('peer'),
    selfProof: () => proof('self'),
    holdOwner,
    publicDraftRows,
    corruptReservation,
    closePeerPreparation: closeEnvelopePreparation,
    pendingEncryption: () => {
      const slot = cryptoFixture.slot();
      return slot?.state === 'active' && slot.pending === 'encrypt';
    },
    closeStorage: () => closeBrowserDatabase(database),
    close() {
      restore();
      releaseLock?.();
      stopSelfRecoveryPreparation(preparation);
      for (const pair of pairs) if (pair) closeEnvelopePreparation(pair);
      closeBrowserDatabase(database);
      cryptoFixture.close();
    }
  };
}
