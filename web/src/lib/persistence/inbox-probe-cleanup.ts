import type { RelayPolicy } from '../config/relays.ts';
import {
  inboxAccessConstructionBinding,
  subscribeInboxAccessControllerClose
} from '../messaging/inbox-access-controller.ts';
import {
  acquireInboxProbeCleanupLease,
  inboxProbeCleanupReady,
  inboxProbeCleanupLeaseCurrent,
  releaseInboxProbeCleanupLease,
  retireInboxProbeCleanupLease
} from '../nostr/private-publisher.ts';
import {
  privateSessionOwnership,
  recheckPrivateSession,
  subscribePrivateSessionClose,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  captureInboxProbeCleanupSelection,
  inboxProbeCleanupSelectionBinding,
  type InboxProbeCleanupSelection,
  type InboxProbeNonceAcknowledgement
} from './inbox-probes.ts';
import { privateRecordWire } from './private-records.ts';
import {
  observePrivateRecord,
  removePrivateInboxProbe,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';

declare const cleanupBrand: unique symbol;
export type InboxProbeCleanup = Readonly<{ [cleanupBrand]: true }>;
declare const nativeCleanupBrand: unique symbol;
export type InboxProbeNativeCleanup = Readonly<{ [nativeCleanupBrand]: true }>;
declare const removalAcknowledgementBrand: unique symbol;
export type InboxProbeRemovalAcknowledgement = Readonly<{
  [removalAcknowledgementBrand]: true;
}>;
export type InboxProbeCleanupResult = Readonly<{
  status: 'removed' | 'invalid' | 'busy' | 'stopped' | PrivateStorageFailure;
  removed?: 1;
  acknowledgement?: InboxProbeRemovalAcknowledgement;
}>;
type NativeAdmission = Readonly<{
  repository: PrivateStorageRepository;
  session: PrivateSession;
  selection: InboxProbeCleanupSelection;
  owner: string;
  id: string;
  wire: string;
  signal: AbortSignal;
  current(): boolean;
  take(): boolean;
}>;
const nativeAdmissions = new WeakMap<
  InboxProbeNativeCleanup,
  NativeAdmission
>();
const actions = new WeakMap<
  InboxProbeCleanup,
  Readonly<{
    run(review: unknown): Promise<InboxProbeCleanupResult>;
    stop(): void;
  }>
>();
const removals = new WeakMap<
  InboxProbeRemovalAcknowledgement,
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    policy: RelayPolicy;
    owner: string;
    command: string;
    eventId: string;
    preferenceId: string;
    readTargets: readonly string[];
    archiveTargets: readonly string[];
    operatorWire: string;
    current(): boolean;
  }>
>();

// Only this module issues these tickets, while its actual owner WebLock and SDK
// job are both held. No public capture returns a native ticket or raw validator.
export function inboxProbeCleanupAdmission(
  ticket: InboxProbeNativeCleanup,
  repository: PrivateStorageRepository,
  session: PrivateSession
) {
  const saved = nativeAdmissions.get(ticket);
  return saved?.repository === repository &&
    saved.session === session &&
    saved.current()
    ? saved
    : undefined;
}

export async function captureInboxProbeCleanup(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  nonce: InboxProbeNonceAcknowledgement,
  observations: unknown,
  review: unknown
): Promise<InboxProbeCleanup | undefined> {
  const selection = captureInboxProbeCleanupSelection(
      repository,
      session,
      policy,
      nonce,
      observations,
      review
    ),
    selected =
      selection &&
      inboxProbeCleanupSelectionBinding(selection, repository, session, policy);
  if (!selection || !selected) return undefined;
  try {
    const scope = inboxAccessConstructionBinding(
        selected.access,
        session,
        policy
      ),
      ownership = privateSessionOwnership(session);
    if (!scope || !ownership?.current()) return undefined;
    const observed = await observePrivateRecord(
      repository,
      'private_sends',
      selected.record.id
    );
    if (
      !observed.ok ||
      observed.value.wire !== selected.wire ||
      !selected.current() ||
      !scope.current() ||
      !ownership.current()
    )
      return undefined;
    const token = Object.freeze({}) as InboxProbeCleanup,
      cancellation = new AbortController();
    let stopped = false,
      used = false;
    const listeners = new Map<symbol, () => void>();
    function forget() {
      for (const [id, off] of listeners) {
        off();
        listeners.delete(id);
      }
    }
    function stop() {
      stopped = true;
      cancellation.abort();
      forget();
    }
    function current() {
      return (
        !stopped &&
        !cancellation.signal.aborted &&
        selected!.current() &&
        scope!.current() &&
        ownership!.current()
      );
    }
    actions.set(token, {
      stop,
      async run(consent) {
        if (consent !== 'reviewed_delete_local_inbox_probe' || used)
          return { status: 'invalid' };
        if (!current()) return { status: 'stopped' };
        used = true;
        let attempted = false;
        try {
          if (!navigator.locks?.request) return { status: 'unavailable' };
          return await navigator.locks.request(
            'harvestcircle:owner:' + selected.record.owner,
            { mode: 'exclusive', ifAvailable: true },
            async (lock): Promise<InboxProbeCleanupResult> => {
              if (!lock) return { status: 'busy' };
              if (!current()) return { status: 'stopped' };
              if (
                !inboxProbeCleanupReady(repository, session, policy, selection)
              )
                return { status: current() ? 'busy' : 'stopped' };
              // No provider request is made while a foreign owner lock is held.
              // The actual fresh key wait finishes before the short SDK/IDB job.
              if (!(await recheckPrivateSession(session)))
                return { status: current() ? 'busy' : 'stopped' };
              if (!current()) return { status: 'stopped' };
              const job =
                await withIdlePrivateLocalStorage<InboxProbeCleanupResult>(
                  session,
                  async () => {
                    if (!current()) return { status: 'stopped' };
                    if (!selected.begin()) return { status: 'busy' };
                    const lease = acquireInboxProbeCleanupLease(
                      repository,
                      session,
                      policy,
                      selection
                    );
                    let ticket: InboxProbeNativeCleanup | undefined,
                      active = false,
                      taken = false;
                    try {
                      if (!lease)
                        return { status: current() ? 'busy' : 'stopped' };
                      const read = await observePrivateRecord(
                        repository,
                        'private_sends',
                        selected.record.id
                      );
                      if (!read.ok) {
                        if (current()) releaseInboxProbeCleanupLease(lease);
                        return { status: read.reason };
                      }
                      if (
                        read.value.wire !== selected.wire ||
                        privateRecordWire(
                          read.value.handle,
                          selected.record.owner,
                          selected.record.id
                        ) !== selected.wire
                      ) {
                        if (current()) releaseInboxProbeCleanupLease(lease);
                        return { status: 'conflict' };
                      }
                      if (!current() || !inboxProbeCleanupLeaseCurrent(lease))
                        return { status: 'stopped' };
                      active = true;
                      ticket = Object.freeze({}) as InboxProbeNativeCleanup;
                      nativeAdmissions.set(ticket, {
                        repository,
                        session,
                        selection,
                        owner: selected.record.owner,
                        id: selected.record.id,
                        wire: selected.wire,
                        signal: cancellation.signal,
                        current: () =>
                          active &&
                          current() &&
                          inboxProbeCleanupLeaseCurrent(lease),
                        take() {
                          if (taken) return false;
                          taken = true;
                          return true;
                        }
                      });
                      attempted = true;
                      const removed = await removePrivateInboxProbe(
                        repository,
                        session,
                        ticket
                      );
                      if (!removed.ok) {
                        if (removed.disposition === 'not_removed' && current())
                          releaseInboxProbeCleanupLease(lease);
                        return { status: removed.reason };
                      }
                      if (!current() || !inboxProbeCleanupLeaseCurrent(lease))
                        return { status: 'unknown_completion' };
                      if (!retireInboxProbeCleanupLease(lease, removed.value))
                        return { status: 'unknown_completion' };
                      // Old nonce/ACKs are now deliberately gone. The removal fact
                      // follows the separately captured real controller/session.
                      if (stopped || !scope.current() || !ownership.current())
                        return { status: 'unknown_completion' };
                      const acknowledgement = Object.freeze(
                        {}
                      ) as InboxProbeRemovalAcknowledgement;
                      removals.set(acknowledgement, {
                        repository,
                        session,
                        policy,
                        owner: selected.record.owner,
                        command: selected.record.id,
                        eventId: selected.record.self!.eventId,
                        preferenceId: selected.record.binding.preferenceId,
                        readTargets:
                          selected.record.binding.readTargets.slice(),
                        archiveTargets:
                          selected.record.binding.archiveTargets.slice(),
                        operatorWire: selected.record.binding.operatorWire,
                        current: () => scope.current() && ownership.current()
                      });
                      return { status: 'removed', removed: 1, acknowledgement };
                    } finally {
                      active = false;
                      if (ticket) nativeAdmissions.delete(ticket);
                      selected.finish();
                    }
                  }
                );
              return job.ok
                ? job.value
                : {
                    status: attempted
                      ? 'unknown_completion'
                      : current()
                        ? 'busy'
                        : 'stopped'
                  };
            }
          );
        } catch {
          return {
            status: attempted
              ? 'unknown_completion'
              : current()
                ? 'unavailable'
                : 'stopped'
          };
        } finally {
          forget();
        }
      }
    });
    const offSession = subscribePrivateSessionClose(session, stop);
    listeners.set(Symbol(), offSession);
    const offAccess = subscribeInboxAccessControllerClose(
      selected.access,
      stop
    );
    listeners.set(Symbol(), offAccess);
    if (!current()) {
      stop();
      return undefined;
    }
    return token;
  } catch {
    return undefined;
  }
}

export function deleteReviewedInboxProbe(
  token: InboxProbeCleanup,
  review: unknown
): Promise<InboxProbeCleanupResult> {
  return (
    actions.get(token)?.run(review) ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopInboxProbeCleanup(token: InboxProbeCleanup): void {
  actions.get(token)?.stop();
}
export function inboxProbeRemovalAcknowledgementBinding(
  acknowledgement: InboxProbeRemovalAcknowledgement,
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy
) {
  const saved = removals.get(acknowledgement);
  return saved?.repository === repository &&
    saved.session === session &&
    saved.policy === policy &&
    saved.current()
    ? {
        owner: saved.owner,
        command: saved.command,
        eventId: saved.eventId,
        preferenceId: saved.preferenceId,
        readTargets: saved.readTargets.slice(),
        archiveTargets: saved.archiveTargets.slice(),
        operatorWire: saved.operatorWire,
        current: saved.current
      }
    : undefined;
}
