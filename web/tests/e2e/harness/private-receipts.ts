import { makeFixture as makeDeliveryFixture } from './private-publisher.ts';
import { preparedPairedDelivery } from '../../../src/lib/messaging/prepare-send.ts';
import {
  commitPairedDeliveryReceipt,
  verifyPairedDeliveryAcknowledgement,
  type PairedDeliveryAcknowledgement
} from '../../../src/lib/persistence/private-sends.ts';
import {
  loadPrivateRecord,
  createPrivateStorageRepository
} from '../../../src/lib/persistence/private-storage.ts';
import {
  privateRecordSnapshot,
  privateRecordWire
} from '../../../src/lib/persistence/private-records.ts';
import {
  preparePrivateReceiptTransition,
  commitPrivateReceiptTransition
} from '../../../src/lib/persistence/private-receipts.ts';
import { privateSendStatus } from '../../../src/lib/messaging/send-status.ts';
import { reservedSendSnapshot } from '../../../src/lib/messaging/send-identity.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
export { commitPairedDeliveryReceipt };
const action = '12345678-1234-4234-8234-123456789abd';
export async function readExisting(owner: string, id: string) {
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('storage unavailable');
  try {
    const repository = createPrivateStorageRepository(opened.owner, owner);
    if (!repository) throw Error('namespace unavailable');
    const loaded = await loadPrivateRecord(repository, 'private_sends', id);
    if (!loaded.ok) throw Error('record unavailable');
    return {
      row: privateRecordSnapshot(loaded.value, owner, id),
      status: privateSendStatus(loaded.value, owner, id)
    };
  } finally {
    closeBrowserDatabase(opened.owner);
  }
}
export async function makeFixture() {
  const f = await makeDeliveryFixture();
  let restore = () => {},
    faults = 0;
  const command = reservedSendSnapshot(f.reserved);
  if (!command) throw Error('missing actual command');
  const id = command.id;
  async function handle() {
    const loaded = await loadPrivateRecord(f.repository, 'private_sends', id);
    if (!loaded.ok) throw Error('missing private record: ' + loaded.reason);
    return loaded.value;
  }
  async function state() {
    const h = await handle();
    return {
      row: privateRecordSnapshot(h, f.owner, id),
      wire: privateRecordWire(h, f.owner, id),
      status: privateSendStatus(h, f.owner, id)
    };
  }
  async function fact(
    role: 'peer' | 'self_archive',
    status: string,
    time = 100,
    readbackWire: string | null = null
  ) {
    const row = await f.storedPair();
    if (!row?.peerArtifact) throw Error('missing actual pair');
    return {
      actionId: action,
      role,
      origin: role === 'peer' ? f.peerOrigin : f.archiveOrigin,
      attempt: 1,
      eventId: role === 'peer' ? row.peerArtifact.eventId : row.self.eventId,
      status,
      observedAtMilliseconds: time,
      readbackWire
    };
  }
  async function observe(
    role: 'peer' | 'self_archive',
    status: string,
    time = 100,
    readbackWire: string | null = null
  ) {
    const receipt = preparedPairedDelivery(f.preparation);
    if (!receipt) throw Error('missing genuine pair acknowledgment');
    const wire = JSON.stringify(await fact(role, status, time, readbackWire));
    return commitPairedDeliveryReceipt(f.repository, receipt, wire);
  }
  async function race() {
    const base = await handle(),
      a = preparePrivateReceiptTransition(
        base,
        f.owner,
        id,
        JSON.stringify(await fact('peer', 'accepted'))
      ),
      b = preparePrivateReceiptTransition(
        base,
        f.owner,
        id,
        JSON.stringify(await fact('self_archive', 'accepted'))
      );
    if (!a.ok || !b.ok) throw Error('missing receipt transitions');
    return Promise.all([
      commitPrivateReceiptTransition(f.repository, a.value),
      commitPrivateReceiptTransition(f.repository, b.value)
    ]);
  }
  async function genuineRace() {
    const peer = await fact('peer', 'accepted'),
      archive = await fact('self_archive', 'accepted');
    const receipt = preparedPairedDelivery(f.preparation);
    if (!receipt) throw Error('missing genuine paired receipt');
    const results = await Promise.all([
      commitPairedDeliveryReceipt(f.repository, receipt, JSON.stringify(peer)),
      commitPairedDeliveryReceipt(
        f.repository,
        receipt,
        JSON.stringify(archive)
      )
    ]);
    const observed = await state();
    const winner =
      observed.row?.family === 'private_send_operation'
        ? observed.row.receipts?.[0].role
        : undefined;
    if (!winner) throw Error('missing winning receipt');
    return {
      results,
      observed,
      verified: await verified(),
      remaining: await f.deliver(winner === 'peer' ? 'self_archive' : 'peer')
    };
  }
  function failAfterPut(mode: 'abort' | 'readback') {
    const put = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      ),
      cursor = Reflect.get<IDBIndex, 'openCursor'>(
        IDBIndex.prototype,
        'openCursor'
      );
    IDBObjectStore.prototype.put = function (
      ...args: Parameters<IDBObjectStore['put']>
    ) {
      const request = put.apply(this, args);
      if (this.name === 'private_sends') {
        faults++;
        if (mode === 'abort') this.transaction.abort();
        else
          IDBIndex.prototype.openCursor = function (
            ...args: Parameters<IDBIndex['openCursor']>
          ) {
            if (this.objectStore.name === 'private_sends')
              throw Error('controlled actual receipt readback failure');
            return cursor.apply(this, args);
          };
      }
      return request;
    };
    restore = () => {
      IDBObjectStore.prototype.put = put;
      IDBIndex.prototype.openCursor = cursor;
    };
  }
  async function forged() {
    return commitPairedDeliveryReceipt(
      f.repository,
      {} as PairedDeliveryAcknowledgement,
      JSON.stringify(await fact('peer', 'accepted'))
    );
  }
  async function verified() {
    const receipt = preparedPairedDelivery(f.preparation);
    return receipt
      ? verifyPairedDeliveryAcknowledgement(f.repository, receipt)
      : false;
  }
  return {
    ...f,
    id,
    handle,
    state,
    fact,
    observe,
    race,
    genuineRace,
    failAfterPut,
    forged,
    verified,
    faults: () => faults,
    clearFault() {
      restore();
      restore = () => {};
    },
    close() {
      restore();
      f.close();
    }
  };
}
