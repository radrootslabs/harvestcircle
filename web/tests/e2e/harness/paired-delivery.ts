import { makeFixture as makeSelfFixture } from './self-recovery.ts';
import type { finalizeEvent, EventTemplate } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxRoutePlan,
  inboxRoutePlanSnapshot
} from '../../../src/lib/messaging/inbox-routing.ts';
import { verifiedOutboundSnapshot } from '../../../src/lib/nostr/verify-outbound-envelope.ts';
export { verifiedOutboundSnapshot, inboxRoutePlanSnapshot };
import {
  prepareSelfRecovery,
  preparedSelfRecovery,
  selfRecoveryPeerPreparation,
  preparePairedDelivery,
  preparedPairedDelivery
} from '../../../src/lib/messaging/prepare-send.ts';
import {
  commitPairedDelivery,
  pairedDeliveryAcknowledgementSnapshot,
  selfRecoveryAcknowledgementSnapshot
} from '../../../src/lib/persistence/private-sends.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot
} from '../../../src/lib/persistence/private-records.ts';
import { loadPrivateRecord } from '../../../src/lib/persistence/private-storage.ts';
import { browserDatabaseTransaction } from '../../../src/lib/persistence/database.ts';
import { createPrivateSendReservationRepository } from '../../../src/lib/persistence/private-send-reservations.ts';
import {
  reserveSendIdentity,
  reservedSendSnapshot
} from '../../../src/lib/messaging/send-identity.ts';
export {
  prepareSelfRecovery,
  preparedSelfRecovery,
  selfRecoveryPeerPreparation,
  preparePairedDelivery,
  preparedPairedDelivery,
  commitPairedDelivery,
  pairedDeliveryAcknowledgementSnapshot,
  selfRecoveryAcknowledgementSnapshot,
  decodePrivateRecord
};
const discovery = 'wss://discovery.example.org',
  peerOrigin = 'wss://peer.example.org',
  archiveOrigin = 'wss://archive.example.org';
// Disposable existing source fixture provider signs actual public10050 facts;
// local discovery observations qualify route semantics only, never relay Q.
export async function makeFixture() {
  const f = await makeSelfFixture(),
    policy = validateRelayPolicy(
      JSON.stringify({
        schemaVersion: 1,
        public: [{ origin: discovery, read: true, write: false, nip50: false }],
        inbox: [peerOrigin, archiveOrigin].map((origin) => ({
          origin,
          read: true,
          write: true
        })),
        postingEnabled: false,
        messagingEnabled: false,
        operatorDenylist: []
      })
    );
  if (!policy || !window.nostr) throw Error('missing fixture policy/provider');
  const provider = window.nostr as Readonly<{
    signEvent(
      template: EventTemplate
    ): Promise<ReturnType<typeof finalizeEvent>>;
  }>;
  const schedulers: ReturnType<typeof createPublicScheduler>[] = [];
  let current = true;
  async function preference(peer: boolean) {
    f.mode(peer ? 'wrong_author' : 'normal');
    const signed = await provider.signEvent({
      kind: 10050,
      created_at: 100,
      tags: [['relay', peer ? peerOrigin : archiveOrigin]],
      content: ''
    });
    f.mode('normal');
    return {
      id: signed.id,
      pubkey: signed.pubkey,
      sig: signed.sig,
      kind: signed.kind,
      created_at: signed.created_at,
      tags: signed.tags,
      content: signed.content
    };
  }
  const ownEvent = await preference(false),
    peerEvent = await preference(true);
  function resolve(event: typeof ownEvent) {
    const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
    schedulers.push(scheduler);
    const run = createPublicRun(scheduler, policy!);
    return createInboxResolver(
      run,
      event.pubkey,
      (next) =>
        openInboxRequest(
          run,
          event.pubkey,
          (sink) => {
            sink({ type: 'EVENT', from: discovery, id: 'fixture', event });
            sink({ type: 'EOSE', from: discovery, id: 'fixture' });
            return () => {};
          },
          next
        ),
      () => current
    );
  }
  const own = resolve(ownEvent),
    other = resolve(peerEvent),
    plan = createInboxRoutePlan(policy, f.owner, f.peer, own, other);
  if (!plan) {
    f.close();
    throw Error('missing genuine route plan');
  }
  async function distinctIntent() {
    const repository = createPrivateSendReservationRepository(
      f.database,
      f.owner
    );
    const observed = Date.now(),
      realNow = Date.now;
    Date.now = () => observed;
    try {
      const rumor = f.textPlan(f.marker);
      if (!repository || !rumor) throw Error('missing distinct intent fixture');
      const result = await reserveSendIdentity(
        repository,
        f.identity,
        '22345678-1234-4234-8234-123456789abc',
        rumor,
        'reviewed_private_intent'
      );
      return {
        status: result.status,
        identity: 'identity' in result ? result.identity : undefined,
        id:
          'identity' in result
            ? reservedSendSnapshot(result.identity)?.id
            : undefined
      };
    } finally {
      Date.now = realNow;
    }
  }
  async function changePairRevision() {
    const identity = reservedSendSnapshot(f.reserved);
    if (!identity) throw Error('missing original fixture command');
    const loaded = await loadPrivateRecord(
      f.repository,
      'private_sends',
      identity.id
    );
    const row =
      loaded.ok && privateRecordSnapshot(loaded.value, f.owner, identity.id);
    if (!row || row.family !== 'private_send_operation' || !row.deliveryPlan)
      throw Error('missing original pair');
    const wire = JSON.stringify({ ...row, revision: row.revision + 1 });
    if (!decodePrivateRecord(wire, f.owner, identity.id).ok)
      throw Error('invalid competing fixture row');
    const tx = browserDatabaseTransaction(
      f.database,
      ['private_sends'],
      'readwrite'
    );
    tx.objectStore('private_sends').put({
      owner: f.owner,
      id: identity.id,
      wire
    });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('competing fixture write aborted'))
      );
    });
  }
  async function storedPair() {
    const identity = reservedSendSnapshot(f.reserved);
    if (!identity) return undefined;
    const loaded = await loadPrivateRecord(
      f.repository,
      'private_sends',
      identity.id
    );
    const row =
      loaded.ok && privateRecordSnapshot(loaded.value, f.owner, identity.id);
    return row && row.family === 'private_send_operation' ? row : undefined;
  }
  return {
    ...f,
    context: { policy, own, other, plan },
    distinctIntent,
    changePairRevision,
    storedPair,
    staleRoutes() {
      current = false;
    },
    close() {
      for (const scheduler of schedulers) closePublicScheduler(scheduler);
      f.close();
    }
  };
}
