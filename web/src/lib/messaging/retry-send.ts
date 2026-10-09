import {
  privateRecordSnapshot,
  type PrivateRecordHandle
} from '../persistence/private-records.ts';
import {
  loadPrivateRecord,
  type PrivateStorageRepository
} from '../persistence/private-storage.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  pairedDeliveryRecordMatches,
  pairedDeliveryTargetAccepted,
  verifyPairedDeliveryAcknowledgement,
  commitPairedDeliveryReceipt,
  type PairedDeliveryAcknowledgement
} from '../persistence/private-sends.ts';
import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  capturePrivatePublicationAction,
  capturePrivatePublication
} from '../nostr/private-publisher.ts';
import {
  publishPrivateGiftWrapAttempt,
  type PrivatePool
} from '../nostr/private-pool.ts';
import { PRIVATE_PUBLICATION_BUDGETS } from '../config/budgets.ts';
import type { PairedDeliveryContext } from './prepare-send.ts';
// Codec readback metadata alone is not a qualified authenticated remote query.
// Only named acceptance suppresses retry here; separate readback remains visible.
export function privateRetryTargets(
  handle: PrivateRecordHandle,
  owner: unknown,
  id: unknown
) {
  const row = privateRecordSnapshot(handle, owner, id);
  if (
    !row ||
    row.family !== 'private_send_operation' ||
    !row.peerArtifact ||
    !row.deliveryPlan
  )
    return undefined;
  const pair = row.peerArtifact;
  return [
    row.deliveryPlan.routes.peer,
    row.deliveryPlan.routes.archive
  ].flatMap((route) => {
    const eventId = route.role === 'peer' ? pair.eventId : row.self.eventId;
    return route.targets
      .filter(
        (origin) =>
          !(row.receipts ?? []).some(
            (f) =>
              f.role === route.role &&
              f.origin === origin &&
              f.eventId === eventId &&
              f.status === 'accepted'
          )
      )
      .map((origin) => ({ role: route.role, origin, eventId }));
  });
}
declare const retryBrand: unique symbol;
export type PrivateRetry = Readonly<{ [retryBrand]: true }>;
type Result = Readonly<{
  status: 'completed' | 'needs_action' | 'stopped' | 'invalid' | 'busy';
}>;
type Controller = {
  run(review: unknown): Promise<Result>;
  stop(): void;
  snapshot(): Readonly<{
    owner: string;
    command: string;
    state: Result['status'] | 'ready';
    attempts: number;
  }>;
};
const retries = new WeakMap<PrivateRetry, Controller>();
// Capture is explicit, memory-only and opens no socket. Genuine custody and
// actual whole-wire readback are rechecked before every externally visible step.
export function capturePrivateRetry(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  pool: PrivatePool,
  review: unknown
): PrivateRetry | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_private_retry')
    return undefined;
  const ownership = privateSessionOwnership(session),
    row = pairedDeliveryAcknowledgementSnapshot(receipt);
  if (!ownership?.current() || !row || row.owner !== ownership.owner)
    return undefined;
  const action = capturePrivatePublicationAction(
    repository,
    session,
    receipt,
    context,
    review
  );
  if (!action) return undefined;
  const abort = new AbortController();
  let consumed = false,
    busy = false,
    attempts = 0,
    state: Result['status'] | 'ready' = 'ready';
  const token = Object.freeze({}) as PrivateRetry;
  function finish(status: Result['status']): Result {
    state = status;
    return { status };
  }
  retries.set(token, {
    stop() {
      abort.abort();
      state = 'stopped';
    },
    snapshot() {
      return { owner: row.owner, command: row.id, state, attempts };
    },
    async run(reviewed) {
      if (reviewed !== 'reviewed_private_retry') return { status: 'invalid' };
      if (abort.signal.aborted || !ownership.current())
        return finish('stopped');
      if (busy) return { status: 'busy' };
      if (consumed) return { status: 'invalid' };
      consumed = true;
      busy = true;
      try {
        if (!(await verifyPairedDeliveryAcknowledgement(repository, receipt)))
          return finish('needs_action');
        const loaded = await loadPrivateRecord(
          repository,
          'private_sends',
          row.id
        );
        if (abort.signal.aborted || !ownership.current())
          return finish('stopped');
        if (
          !loaded.ok ||
          !pairedDeliveryRecordMatches(receipt, loaded.value) ||
          !(await verifyPairedDeliveryAcknowledgement(repository, receipt))
        )
          return finish('needs_action');
        if (abort.signal.aborted || !ownership.current())
          return finish('stopped');
        const targets =
          loaded.ok && privateRetryTargets(loaded.value, row.owner, row.id);
        if (!targets) return finish('needs_action');
        for (const target of targets) {
          let accepted = false;
          for (
            let index = 0;
            index < PRIVATE_PUBLICATION_BUDGETS.attemptsPerTargetAction;
            index++
          ) {
            if (abort.signal.aborted || !ownership.current())
              return finish('stopped');
            if (
              pairedDeliveryTargetAccepted(
                receipt,
                target.role,
                target.origin,
                target.eventId
              )
            ) {
              if (
                !(await verifyPairedDeliveryAcknowledgement(
                  repository,
                  receipt
                ))
              )
                return finish('needs_action');
              if (abort.signal.aborted || !ownership.current())
                return finish('stopped');
              accepted = true;
              break;
            }
            const permission = capturePrivatePublication(
              repository,
              session,
              receipt,
              context,
              target.role,
              target.origin,
              'reviewed_private_delivery',
              action
            );
            if (!permission) return finish('needs_action');
            const result = await publishPrivateGiftWrapAttempt(
              pool,
              permission,
              abort.signal
            );
            if (abort.signal.aborted || !ownership.current())
              return finish('stopped');
            if (
              !result.actionId &&
              pairedDeliveryTargetAccepted(
                receipt,
                target.role,
                target.origin,
                target.eventId
              ) &&
              (await verifyPairedDeliveryAcknowledgement(repository, receipt))
            ) {
              if (abort.signal.aborted || !ownership.current())
                return finish('stopped');
              accepted = true;
              break;
            }
            if (
              !result.actionId ||
              !result.attempt ||
              result.role !== target.role ||
              result.origin !== target.origin ||
              result.eventId !== target.eventId
            )
              return finish('needs_action');
            attempts++;
            const observedAtMilliseconds = Date.now();
            if (
              !Number.isSafeInteger(observedAtMilliseconds) ||
              observedAtMilliseconds < 0
            )
              return finish('needs_action');
            const saved = await commitPairedDeliveryReceipt(
              repository,
              receipt,
              JSON.stringify({
                actionId: result.actionId,
                role: target.role,
                origin: target.origin,
                eventId: target.eventId,
                attempt: result.attempt,
                status: result.status,
                observedAtMilliseconds,
                readbackWire: null
              })
            );
            if (abort.signal.aborted || !ownership.current())
              return finish('stopped');
            if (!('record' in saved)) return finish('needs_action');
            if (result.status === 'accepted') {
              accepted = true;
              break;
            }
            if (result.status === 'refused' || result.status === 'stopped')
              return finish('needs_action');
          }
          if (!accepted) return finish('needs_action');
        }
        return finish('completed');
      } catch {
        return finish(
          abort.signal.aborted || !ownership.current()
            ? 'stopped'
            : 'needs_action'
        );
      } finally {
        busy = false;
      }
    }
  });
  return token;
}
export function runPrivateRetry(
  token: PrivateRetry,
  review: unknown
): Promise<Result> {
  return (
    retries.get(token)?.run(review) ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopPrivateRetry(token: PrivateRetry): void {
  retries.get(token)?.stop();
}
export function privateRetrySnapshot(token: PrivateRetry) {
  return retries.get(token)?.snapshot();
}
