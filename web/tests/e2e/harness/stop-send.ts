import {
  stopPrivateSend,
  privateStopSendSnapshot
} from '../../../src/lib/messaging/stop-send.ts';
import { privateSendStatus } from '../../../src/lib/messaging/send-status.ts';
import { reconcilePrivateSendAttempt } from '../../../src/lib/persistence/private-send-settlements.ts';
import {
  capturePrivatePublication,
  capturePrivatePublicationAction
} from '../../../src/lib/nostr/private-publisher.ts';
import {
  publishPrivateGiftWrapAttempt,
  type PrivateGiftWrapAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
import { createInboxRoutePlan } from '../../../src/lib/messaging/inbox-routing.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  getPrivatePool,
  closePrivatePool
} from '../../../src/lib/nostr/private-pool.ts';
import {
  resumeEncryptedPreparation,
  qualifyRecoveredPrivatePair,
  resumePreparationSnapshot,
  resumeRecoveredRumor,
  stopResumePreparation
} from '../../../src/lib/messaging/resume-preparation.ts';
import { rumorPlanSnapshot } from '../../../src/lib/messaging/rumor-plan.ts';
import { decodePrivateRecord } from '../../../src/lib/persistence/private-records.ts';
import {
  capturePrivateRetry,
  runPrivateRetry,
  type PrivateRetry
} from '../../../src/lib/messaging/retry-send.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  commitPairedDeliveryReceipt,
  type PairedDeliveryAcknowledgement
} from '../../../src/lib/persistence/private-sends.ts';
export async function makeFixture(initialise = true) {
  const f = await makeRecoveryFixture(initialise);
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('invalid private fixture policy');
  const plan = createInboxRoutePlan(
    policy,
    f.owner,
    f.peer,
    f.context.own,
    f.context.other
  );
  if (!plan) throw Error('missing private route plan');
  const context = { ...f.context, policy, plan };
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('missing genuine private session');
  const pool = getPrivatePool(session, policy, [
    'wss://peer.example.org',
    'wss://archive.example.org'
  ]);
  if (!pool) throw Error('missing private pool');
  const resume = f.capture();
  let receipt: PairedDeliveryAcknowledgement | undefined,
    retry: PrivateRetry | undefined;
  let observed: PrivateGiftWrapAttemptResult | undefined;
  let restoreFault = () => {},
    injected = 0;
  async function prepare() {
    const recovered = await resumeEncryptedPreparation(
      resume,
      context,
      'reviewed_private_resume'
    );
    if (!['prepared', 'already_prepared'].includes(recovered.status))
      return recovered;
    const qualified = await qualifyRecoveredPrivatePair(
      resume,
      context,
      'reviewed_stored_private_retry'
    );
    if ('receipt' in qualified) receipt = qualified.receipt;
    return { status: qualified.status };
  }
  function capture(review: unknown = 'reviewed_private_retry') {
    retry = receipt
      ? capturePrivateRetry(
          f.repository,
          session!,
          receipt,
          context,
          pool!,
          review
        )
      : undefined;
    return !!retry;
  }
  return {
    ...f,
    async row() {
      const value = await f.row();
      if (!value) throw Error('missing actual paired row');
      return value;
    },
    async sendStatus() {
      const row = await f.row();
      const decoded = row && decodePrivateRecord(row.wire, f.owner, f.command);
      return decoded?.ok
        ? privateSendStatus(decoded.value, f.owner, f.command)
        : undefined;
    },
    async publishObserved() {
      if (!receipt) throw Error('missing genuine pair');
      const action = capturePrivatePublicationAction(
        f.repository,
        session,
        receipt,
        context,
        'reviewed_private_retry'
      );
      const permission =
        action &&
        capturePrivatePublication(
          f.repository,
          session,
          receipt,
          context,
          'peer',
          'wss://peer.example.org',
          'reviewed_private_delivery',
          action
        );
      if (!permission) throw Error('missing genuine permission');
      observed = await publishPrivateGiftWrapAttempt(
        pool,
        permission,
        new AbortController().signal
      );
      return observed;
    },
    settleObserved(clone = false) {
      if (!observed) throw Error('missing actual SDK result');
      return reconcilePrivateSendAttempt(clone ? { ...observed } : observed);
    },
    async replaceImmutableScope() {
      let db: IDBDatabase | undefined;
      const native = Reflect.get<IDBIndex, 'openCursor'>(
        IDBIndex.prototype,
        'openCursor'
      );
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        if (this.objectStore.name === 'private_sends')
          db = this.objectStore.transaction.db;
        return native.apply(this, args);
      };
      let row;
      try {
        row = await f.row();
      } finally {
        IDBIndex.prototype.openCursor = native;
      }
      if (!db || !row) throw Error('missing native row');
      const wire = JSON.stringify({
        ...row.record,
        revision: row.record.revision + 1,
        rumorHash: 'b'.repeat(64)
      });
      if (!decodePrivateRecord(wire, f.owner, f.command).ok)
        throw Error('invalid controlled successor');
      const tx = db.transaction(['private_sends'], 'readwrite');
      tx.objectStore('private_sends').put({
        owner: f.owner,
        id: f.command,
        wire
      });
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () => reject(Error('fixture abort')));
      });
      return wire;
    },
    faultSettlement(kind: 'abort' | 'readback') {
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
        if (this.name === 'private_sends') {
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
          this.objectStore.name === 'private_sends' &&
          this.objectStore.transaction.mode === 'readonly'
        )
          throw Error('actual post-put readback failure');
        return cursor.apply(this, args);
      };
      restoreFault = () => {
        IDBObjectStore.prototype.put = put;
        IDBIndex.prototype.openCursor = cursor;
      };
    },
    prepare,
    capture,
    run: (review: unknown = 'reviewed_private_retry') =>
      retry
        ? runPrivateRetry(retry, review)
        : Promise.resolve({ status: 'invalid' as const }),
    snapshot: () => retry && privateStopSendSnapshot(retry),
    proof: () => receipt && pairedDeliveryAcknowledgementSnapshot(receipt),
    recovered: () => ({
      snapshot: resumePreparationSnapshot(resume),
      rumor: !!rumorPlanSnapshot(resumeRecoveredRumor(resume)!)
    }),
    stop: () => {
      if (retry) stopPrivateSend(retry);
    },
    clearRecovery: () => stopResumePreparation(resume),
    countsValue: () => f.counts(),
    async controlledArchiveSuccessor() {
      if (!receipt) throw Error('missing genuine custody');
      const row = (await f.row())?.record;
      if (!row) throw Error('missing actual ciphertext');
      // Controlled metadata race, not a claim of an additional relay ACK.
      return commitPairedDeliveryReceipt(
        f.repository,
        receipt,
        JSON.stringify({
          actionId: '12345678-1234-4234-8234-123456789abe',
          role: 'self_archive',
          origin: 'wss://archive.example.org',
          attempt: 1,
          eventId: row.self.eventId,
          status: 'accepted',
          observedAtMilliseconds: 100,
          readbackWire: null
        })
      );
    },
    async injectUnacknowledgedSuccessBetweenReads() {
      const stored = await f.row();
      if (!stored?.record.peerArtifact || !stored.record.deliveryPlan)
        throw Error('missing real paired record');
      const row = stored.record;
      const facts = [
        row.deliveryPlan!.routes.peer,
        row.deliveryPlan!.routes.archive
      ].flatMap((route) =>
        route.targets.map((origin) => ({
          actionId: '12345678-1234-4234-8234-123456789abf',
          role: route.role,
          origin,
          attempt: 1,
          eventId:
            route.role === 'peer'
              ? row.peerArtifact!.eventId
              : row.self.eventId,
          status: 'accepted',
          observedAtMilliseconds: 100,
          readbackWire: null
        }))
      );
      const wire = JSON.stringify({
        ...row,
        revision: row.revision + 1,
        receipts: [...(row.receipts ?? []), ...facts]
      });
      if (!decodePrivateRecord(wire, f.owner, f.command).ok)
        throw Error('invalid controlled race row');
      const native = Reflect.get<IDBIndex, 'openCursor'>(
        IDBIndex.prototype,
        'openCursor'
      );
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        const request = native.apply(this, args);
        if (this.objectStore.name === 'private_sends' && injected === 0) {
          injected++;
          // Queue real RW behind first genuine verification and ahead of the
          // next load. No opaque acknowledgement is renewed by this mutation.
          const transaction = this.objectStore.transaction.db.transaction(
            ['private_sends'],
            'readwrite'
          );
          transaction
            .objectStore('private_sends')
            .put({ owner: f.owner, id: f.command, wire });
        }
        return request;
      };
      restoreFault = () => {
        IDBIndex.prototype.openCursor = native;
      };
      return {
        originalWire: stored.wire,
        changedWire: wire,
        revision: row.revision
      };
    },
    injected: () => injected,
    clearFault() {
      restoreFault();
      restoreFault = () => {};
    },
    close() {
      restoreFault();
      if (retry) stopPrivateSend(retry);
      stopResumePreparation(resume);
      closePrivatePool(pool);
      closePrivateSession(session);
      f.close();
    }
  };
}
