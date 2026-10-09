import { makeFixture as recoveryFixture } from './resume-preparation.ts';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
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
  stopResumePreparation
} from '../../../src/lib/messaging/resume-preparation.ts';
import {
  capturePrivateRetry,
  runPrivateRetry,
  type PrivateRetry
} from '../../../src/lib/messaging/retry-send.ts';
import {
  capturePrivateResumeRouting,
  privateResumeRoutingSnapshot,
  approvePrivateResumeRouting,
  type PrivateResumeRouting
} from '../../../src/lib/messaging/resume-routing.ts';
import type { PairedDeliveryAcknowledgement } from '../../../src/lib/persistence/private-sends.ts';
import type { PairedDeliveryContext } from '../../../src/lib/messaging/prepare-send.ts';
export async function makeFixture() {
  const f = await recoveryFixture();
  const origins = [
    'wss://peer.example.org',
    'wss://archive.example.org',
    'wss://new-peer.example.org'
  ];
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true,
      inbox: origins.map((origin) => ({ origin, read: true, write: true }))
    })
  );
  if (!policy) throw Error('missing qualified fixture policy');
  const plan = createInboxRoutePlan(
    policy,
    f.owner,
    f.peer,
    f.context.own,
    f.context.other
  );
  if (!plan) throw Error('missing original route');
  const original = { ...f.context, policy, plan };
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('missing private session');
  const pool = getPrivatePool(session, policy, origins);
  if (!pool) throw Error('missing private pool');
  const resume = f.capture();
  await resumeEncryptedPreparation(resume, original, 'reviewed_private_resume');
  const prepared = await qualifyRecoveredPrivatePair(
    resume,
    original,
    'reviewed_stored_private_retry'
  );
  if (!('receipt' in prepared)) throw Error('missing genuine pair');
  const receipt: PairedDeliveryAcknowledgement = prepared.receipt;
  const schedulers: ReturnType<typeof createPublicScheduler>[] = [];
  let context: PairedDeliveryContext = original,
    token: PrivateResumeRouting | undefined,
    heldRetry: PrivateRetry | undefined,
    current = true;
  let restore = () => {};
  function resolve(
    origin: string,
    role: 'peer' | 'self_archive',
    peerSwitch = false,
    complete = true
  ) {
    const key = new Uint8Array(32).fill(
      role === 'self_archive' ? 83 : peerSwitch ? 85 : 84
    );
    let event;
    try {
      event = finalizeEvent(
        {
          kind: 10050,
          created_at:
            origin === 'wss://peer.example.org' ||
            origin === 'wss://archive.example.org'
              ? 100
              : 101,
          tags: [['relay', origin]],
          content: ''
        },
        key
      );
    } finally {
      key.fill(0);
    }
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
            sink({
              type: 'EVENT',
              from: 'wss://discovery.example.org',
              id: 'routing-fixture',
              event
            });
            if (complete)
              sink({
                type: 'EOSE',
                from: 'wss://discovery.example.org',
                id: 'routing-fixture'
              });
            return () => {};
          },
          next
        ),
      () => current
    );
  }
  return {
    ...f,
    capture(
      destination = 'wss://new-peer.example.org',
      archive = 'wss://archive.example.org',
      peerSwitch = false,
      complete = true
    ) {
      const own = resolve(archive, 'self_archive'),
        other = resolve(destination, 'peer', peerSwitch, complete);
      const alternate = new Uint8Array(32).fill(85);
      let intended;
      try {
        intended = peerSwitch ? getPublicKey(alternate) : f.peer;
      } finally {
        alternate.fill(0);
      }
      const next = createInboxRoutePlan(policy, f.owner, intended, own, other);
      token = capturePrivateResumeRouting(
        f.repository,
        f.identity,
        receipt,
        { policy, own, other },
        'reviewed_private_route_resolution'
      );
      // A changed recipient cannot produce a reviewed same-peer update.
      if (!next) {
        return false;
      }
      context = { policy, plan: next, own, other };
      return !!token;
    },
    snapshot: () => token && privateResumeRoutingSnapshot(token),
    async approve(review: unknown = 'reviewed_private_destination_update') {
      if (!token) return { status: 'invalid' as const };
      try {
        return await approvePrivateResumeRouting(token, review);
      } finally {
        restore();
      }
    },
    async retry(old = false) {
      const retry = capturePrivateRetry(
        f.repository,
        session,
        receipt,
        old ? original : context,
        pool,
        'reviewed_private_retry'
      );
      return retry
        ? runPrivateRetry(retry, 'reviewed_private_retry')
        : { status: 'invalid' as const };
    },
    captureHeld() {
      heldRetry = capturePrivateRetry(
        f.repository,
        session,
        receipt,
        original,
        pool,
        'reviewed_private_retry'
      );
      return !!heldRetry;
    },
    runHeld() {
      return heldRetry
        ? runPrivateRetry(heldRetry, 'reviewed_private_retry')
        : Promise.resolve({ status: 'invalid' as const });
    },
    stale() {
      current = false;
    },
    fault(kind: 'abort' | 'readback') {
      const put = Reflect.get<IDBObjectStore, 'put'>(
          IDBObjectStore.prototype,
          'put'
        ),
        cursor = Reflect.get<IDBIndex, 'openCursor'>(
          IDBIndex.prototype,
          'openCursor'
        );
      let armed = false;
      IDBObjectStore.prototype.put = function (value, key) {
        const request =
          key === undefined
            ? put.call(this, value)
            : put.call(this, value, key);
        if (this.name === 'private_sends') {
          armed = true;
          if (kind === 'abort') this.transaction.abort();
        }
        return request;
      };
      IDBIndex.prototype.openCursor = function (...args) {
        if (
          armed &&
          kind === 'readback' &&
          this.objectStore.name === 'private_sends' &&
          this.objectStore.transaction.mode === 'readonly'
        )
          throw new DOMException(
            'fixture actual post-put readback failure',
            'InvalidStateError'
          );
        return cursor.apply(this, args);
      };
      restore = () => {
        IDBObjectStore.prototype.put = put;
        IDBIndex.prototype.openCursor = cursor;
      };
    },
    close() {
      restore();
      current = false;
      for (const s of schedulers) closePublicScheduler(s);
      stopResumePreparation(resume);
      closePrivatePool(pool);
      closePrivateSession(session);
      f.close();
    }
  };
}
