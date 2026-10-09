import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateRecordIdentity,
  privateRecordWire,
  type PrivateRecordHandle
} from './private-records.ts';
import {
  listPrivateReceivedRecords,
  removePrivateReceivedRecords,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
declare const cleanupBrand: unique symbol;
export type ReceivedCleanup = Readonly<{ [cleanupBrand]: true }>;
type Result = Readonly<{
  status: 'removed' | 'invalid' | 'busy' | 'stopped' | PrivateStorageFailure;
  removed?: number;
}>;
type Snapshot = Readonly<{
  owner: string;
  state: 'review' | 'running' | 'removed' | 'needs_action' | 'stopped';
  selected: number;
  bytes: number;
  ids: readonly string[];
  localCopyLoss: true;
  relayRecovery: 'not_guaranteed';
  outbox: 'unchanged';
  reason: string | null;
}>;
type Controller = {
  run(review: unknown): Promise<Result>;
  snapshot(): Snapshot;
  stop(): void;
};
const actions = new WeakMap<ReceivedCleanup, Controller>();
// Complete bounded selection, copied before admission. IDs alone are not
// effect authority and no malformed tail is silently truncated.
export function selectReceivedCleanupIds(value: unknown): string[] | undefined {
  try {
    if (
      !Array.isArray(value) ||
      !value.length ||
      value.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes
    )
      return undefined;
    const copy: Array<unknown> = Array.from(value);
    if (!copy.every((x) => typeof x === 'string' && /^[0-9a-f]{64}$/.test(x)))
      return undefined;
    return [...new Set(copy as string[])];
  } catch {
    return undefined;
  }
}
// Explicit local-copy loss preview only, no delete/SDK/network operation.
// Genuine installed original private generation owns this opaque action;
// remembered owner strings and copied previews cannot grant deletion.
export async function captureReceivedCleanup(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  selection: unknown,
  review: unknown
): Promise<ReceivedCleanup | undefined> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_received_cleanup_selection' ||
    !navigator.locks?.request
  )
    return undefined;
  const ids = selectReceivedCleanupIds(selection),
    observed = privateSessionOwnership(session);
  if (!ids || !observed?.current()) return undefined;
  const ownership = observed;
  try {
    const scan = await listPrivateReceivedRecords(repository);
    if (!scan.ok || !ownership.current()) return undefined;
    let selected = Array.from<PrivateRecordHandle>([]),
      bytes = 0;
    for (const id of ids) {
      const row = scan.value.find((h) => privateRecordIdentity(h)?.id === id),
        identity = row && privateRecordIdentity(row),
        wire = row && privateRecordWire(row, ownership.owner, id);
      if (!row || identity?.owner !== ownership.owner || !wire)
        return undefined;
      selected = selected.concat(row);
      bytes += new TextEncoder().encode(
        JSON.stringify({ owner: ownership.owner, id, wire })
      ).length;
    }
    const token = Object.freeze({}) as ReceivedCleanup;
    const cancellation = new AbortController();
    let state: Snapshot['state'] = 'review',
      reason: string | null = null,
      used = false,
      stopped = false;
    let off = () => {};
    function stop() {
      if (stopped) return;
      stopped = true;
      cancellation.abort();
      state = 'stopped';
      selected = [];
      off();
    }
    actions.set(token, {
      snapshot: () => ({
        owner: ownership.owner,
        state,
        selected: ids.length,
        bytes,
        ids: ids.slice(),
        localCopyLoss: true,
        relayRecovery: 'not_guaranteed',
        outbox: 'unchanged',
        reason
      }),
      stop,
      async run(reviewed) {
        if (stopped || !ownership.current()) return { status: 'stopped' };
        if (reviewed !== 'reviewed_delete_local_received_copies' || used)
          return { status: 'invalid' };
        used = true;
        state = 'running';
        const original = selected.slice();
        let result: Result;
        try {
          result = await navigator.locks.request(
            'harvestcircle:owner:' + ownership.owner,
            { mode: 'exclusive', ifAvailable: true },
            async (lock): Promise<Result> => {
              if (!lock) return { status: 'busy' };
              if (stopped || !ownership.current()) return { status: 'stopped' };
              const committed = await removePrivateReceivedRecords(
                repository,
                session,
                original,
                'reviewed_delete_local_received_copies',
                cancellation.signal
              );
              if (!committed.ok)
                return {
                  status:
                    committed.reason === 'invalid_scope' &&
                    (stopped || !ownership.current())
                      ? 'stopped'
                      : committed.reason
                };
              // Native complete precedes this fresh strict RO absence verification.
              // A failed readback or owner loss cannot establish definite no deletion.
              const readback = await listPrivateReceivedRecords(repository);
              if (
                !readback.ok ||
                stopped ||
                !ownership.current() ||
                readback.value.some((h) =>
                  ids.includes(privateRecordIdentity(h)?.id ?? '')
                )
              )
                return { status: 'unknown_completion' };
              return { status: 'removed', removed: committed.value.removed };
            }
          );
        } catch {
          result = { status: 'unknown_completion' };
        }
        reason = result.status === 'removed' ? null : result.status;
        if (!stopped)
          state = result.status === 'removed' ? 'removed' : 'needs_action';
        selected = [];
        off();
        return result;
      }
    });
    off = subscribePrivateSessionClose(session, stop);
    if (!ownership.current()) {
      stop();
      return undefined;
    }
    return token;
  } catch {
    return undefined;
  }
}
export function receivedCleanupSnapshot(
  token: ReceivedCleanup
): Snapshot | undefined {
  return actions.get(token)?.snapshot();
}
export function deleteReviewedReceivedCopies(
  token: ReceivedCleanup,
  review: unknown
): Promise<Result> {
  return (
    actions.get(token)?.run(review) ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopReceivedCleanup(token: ReceivedCleanup): void {
  actions.get(token)?.stop();
}
