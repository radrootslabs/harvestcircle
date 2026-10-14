import { canonicalLocalId } from '../private-handles.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  privateSessionOwnership,
  recheckPrivateSession,
  subscribePrivateSessionClose,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateRecordIdentity,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle
} from './private-records.ts';
import {
  listPrivateSendRecords,
  removePrivateSettledSendRecords,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
import { privateRecordSettlement } from './local-inventory.ts';
declare const cleanupBrand: unique symbol;
export type PrivateOutboxCleanup = Readonly<{ [cleanupBrand]: true }>;
type Result = Readonly<{
  status: 'removed' | 'invalid' | 'busy' | 'stopped' | PrivateStorageFailure;
  removed?: number;
}>;
type Controller = { run(review: unknown): Promise<Result>; stop(): void };
const actions = new WeakMap<PrivateOutboxCleanup, Controller>();
function selectIds(selection: unknown): string[] | undefined {
  try {
    if (
      !Array.isArray(selection) ||
      !selection.length ||
      selection.length > LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
    )
      return undefined;
    const ids = Array.from<unknown>(selection).map(canonicalLocalId);
    return ids.some((id) => !id) || new Set(ids).size !== ids.length
      ? undefined
      : (ids as string[]);
  } catch {
    return undefined;
  }
}
// Explicit current-owner whole-wire preview. Ciphertext handles remain private
// to this one-use action; copied IDs/objects never grant local deletion.
export async function capturePrivateOutboxCleanup(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  selection: unknown,
  review: unknown
): Promise<PrivateOutboxCleanup | undefined> {
  const ownership = privateSessionOwnership(session),
    ids = selectIds(selection);
  if (
    typeof window === 'undefined' ||
    !navigator.locks?.request ||
    review !== 'reviewed_settled_outbox_selection' ||
    !ownership?.current() ||
    !ids
  )
    return undefined;
  try {
    const scan = await listPrivateSendRecords(repository);
    if (!scan.ok || !ownership.current()) return undefined;
    const selected = ids.map((id) => {
      const handle = scan.value.find(
          (row) => privateRecordIdentity(row)?.id === id
        ),
        record = handle && privateRecordSnapshot(handle, ownership.owner, id),
        wire = handle && privateRecordWire(handle, ownership.owner, id);
      if (
        !handle ||
        !record ||
        !wire ||
        privateRecordSettlement(record) !== 'settled'
      )
        return undefined;
      return handle;
    });
    if (selected.some((handle) => !handle)) return undefined;
    const token = Object.freeze({}) as PrivateOutboxCleanup,
      cancellation = new AbortController();
    let captured = selected as PrivateRecordHandle[],
      stopped = false,
      used = false,
      off = () => {};
    function stop() {
      if (stopped) return;
      stopped = true;
      cancellation.abort();
      captured = [];
      off();
    }
    actions.set(token, {
      stop,
      async run(consent) {
        if (stopped || !ownership.current()) return { status: 'stopped' };
        if (consent !== 'reviewed_delete_local_settled_sends' || used)
          return { status: 'invalid' };
        used = true;
        const original = captured.slice();
        try {
          if (
            !(await recheckPrivateSession(session)) ||
            stopped ||
            !ownership.current()
          )
            return {
              status: stopped || !ownership.current() ? 'stopped' : 'busy'
            };
          return await navigator.locks.request(
            'harvestcircle:owner:' + ownership.owner,
            { mode: 'exclusive', ifAvailable: true },
            async (lock): Promise<Result> => {
              if (!lock) return { status: 'busy' };
              if (stopped || !ownership.current()) return { status: 'stopped' };
              let attempted = false;
              const job = await withIdlePrivateLocalStorage<Result>(
                session,
                async () => {
                  if (stopped || !ownership.current())
                    return { status: 'stopped' };
                  attempted = true;
                  const committed = await removePrivateSettledSendRecords(
                    repository,
                    session,
                    original,
                    consent,
                    cancellation.signal
                  );
                  if (!committed.ok) return { status: committed.reason };
                  const readback = await listPrivateSendRecords(repository);
                  if (
                    !readback.ok ||
                    stopped ||
                    !ownership.current() ||
                    readback.value.some(
                      (row) =>
                        !privateRecordIdentity(row) ||
                        ids.includes(privateRecordIdentity(row)!.id)
                    )
                  )
                    return { status: 'unknown_completion' };
                  return {
                    status: 'removed',
                    removed: committed.value.removed
                  };
                }
              );
              return job.ok
                ? job.value
                : {
                    status: attempted
                      ? 'unknown_completion'
                      : stopped || !ownership.current()
                        ? 'stopped'
                        : 'busy'
                  };
            }
          );
        } catch {
          return { status: 'unknown_completion' };
        } finally {
          captured = [];
          off();
        }
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
export function deleteReviewedSettledSends(
  token: PrivateOutboxCleanup,
  review: unknown
): Promise<Result> {
  return (
    actions.get(token)?.run(review) ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopPrivateOutboxCleanup(token: PrivateOutboxCleanup): void {
  actions.get(token)?.stop();
}
