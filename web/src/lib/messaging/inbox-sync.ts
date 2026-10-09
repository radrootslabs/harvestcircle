import {
  unlockedSessionOwnership,
  subscribeUnlockedSessionClose,
  type UnlockedSession
} from './unlocked-session.ts';
import {
  getPrivateVisibilityScope,
  privateVisibilitySnapshot,
  closePrivateVisibilityScope
} from '../runtime/dispose.ts';
import {
  readRelayPolicy,
  inboxRelayTargets,
  publicRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { inboxPreferenceSnapshot } from '../nostr/inbox-preferences.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from '../messaging/resolve-inbox.ts';
import type { ObserveInboxAccess } from '../messaging/readiness.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  getPrivatePool,
  subscribePrivatePage,
  subscribePrivateLive,
  type PrivatePageMessage,
  closePrivatePool
} from '../nostr/private-pool.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../persistence/inbox-envelope-repository.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
declare const ingressBrand: unique symbol;
export type InboxSync = Readonly<{ [ingressBrand]: true }>;
type State = 'ready' | 'running' | 'live' | 'stopped' | 'needs_action';
type Snapshot = Readonly<{
  owner: string;
  state: State;
  candidates: number;
  bytes: number;
  retained: number;
  duplicates: number;
  rejected: number;
  reason: string | null;
  pending: number;
  cleanupRequired: boolean;
  backfill: string;
  historyComplete: false;
  foregroundOnly: true;
}>;
type Controller = {
  start(review: unknown): boolean;
  stop(): void;
  snapshot(): Snapshot;
};
const ingress = new WeakMap<InboxSync, Controller>();
const sessions = new WeakMap<PrivateSession, InboxSync>();
// Explicit foreground live-before-finite receive admission. A remembered key, allowlist or EOSE alone
// is not exercised own inbox access. No production observation port is supplied
// here; controlled fixtures do not qualify real operator relay/AUTH access.
// No closed-tab notification, unattended decrypt or complete-inbox promise.
export function captureInboxSync(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  own: InboxResolver,
  unlocked: UnlockedSession,
  observeAccess: ObserveInboxAccess | undefined,
  review: unknown
): InboxSync | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_foreground_inbox' ||
    !observeAccess
  )
    return undefined;
  try {
    const previous = sessions.get(session);
    if (previous && ingress.get(previous)?.snapshot().state !== 'stopped')
      return undefined;
    const observedOwnership = privateSessionOwnership(session),
      manifest = readRelayPolicy(policy),
      resolution = inboxResolutionSnapshot(own);
    if (
      !observedOwnership?.current() ||
      !manifest.messagingEnabled ||
      resolution.author !== observedOwnership.owner ||
      resolution.status !== 'ready' ||
      resolution.coverage !== 'bounded-eose' ||
      !resolution.head ||
      !resolution.knownBase
    )
      return undefined;
    const ownership = observedOwnership;
    const preference = inboxPreferenceSnapshot(resolution.head),
      discovery = publicRelayTargets(policy, 'read');
    if (
      !preference ||
      preference.status !== 'supported' ||
      preference.author !== ownership.owner ||
      preference.id !== resolution.knownBase.id ||
      !discovery.length ||
      discovery.length !== resolution.sources.length ||
      !discovery.every((origin) =>
        resolution.sources.some(
          (source) => source.source === origin && source.state === 'eose'
        )
      )
    )
      return undefined;
    const targets = inboxRelayTargets(policy, preference.relays, 'read'),
      archive = inboxRelayTargets(policy, preference.relays, 'write');
    if (!targets.length || !archive.length) return undefined;
    const access = observeAccess({
      owner: ownership.owner,
      session: ownership.session,
      preferenceId: preference.id,
      readTargets: targets.slice(),
      archiveTargets: archive.slice()
    });
    if (
      !access ||
      access.owner !== ownership.owner ||
      access.session !== ownership.session ||
      access.preferenceId !== preference.id ||
      access.receive !== 'qualified_exercised' ||
      access.archive !== 'qualified_exercised' ||
      JSON.stringify(access.readTargets) !== JSON.stringify(targets) ||
      JSON.stringify(access.archiveTargets) !== JSON.stringify(archive)
    )
      return undefined;
    const cache = unlockedSessionOwnership(unlocked);
    if (
      !cache?.current() ||
      cache.owner !== ownership.owner ||
      cache.session !== ownership.session
    )
      return undefined;
    const observedVisibility = getPrivateVisibilityScope(session);
    if (!observedVisibility) return undefined;
    const visibility = observedVisibility;
    const current = () => {
      const latest = inboxResolutionSnapshot(own);
      return (
        privateVisibilitySnapshot(visibility).active &&
        unlockedSessionOwnership(unlocked)?.session === ownership.session &&
        !!unlockedSessionOwnership(unlocked)?.current() &&
        latest.status === 'ready' &&
        latest.coverage === 'bounded-eose' &&
        latest.author === ownership.owner &&
        latest.knownBase?.id === preference.id &&
        access.current() &&
        ownership.current() &&
        inboxResolverCurrentAfterSample(own)
      );
    };
    if (!current()) return undefined;
    const pool = getPrivatePool(session, policy, targets);
    if (!pool) return undefined;
    const token = Object.freeze({}) as InboxSync;
    let state: State = 'ready',
      candidates = 0,
      bytes = 0,
      retained = 0,
      duplicates = 0,
      rejected = 0,
      pending = 0,
      reason: string | null = null,
      consumed = false,
      stopped = false,
      backfill = 'not_started',
      pendingBytes = 0,
      failed = false,
      cleaned = false,
      cleanupRequired = false;
    let release = () => {},
      releaseLive = () => {},
      offUnlock = () => {},
      off = () => {};
    let queue = Promise.resolve();
    function releaseOwners() {
      let failure = false;
      for (const cleanup of [
        release,
        releaseLive,
        () => closePrivatePool(pool!)
      ]) {
        try {
          cleanup();
        } catch {
          failure = true;
        }
      }
      cleanupRequired = failure;
      if (failure) throw Error('private_inbox_cleanup_required');
    }
    function stop() {
      if (cleaned) return;
      stopped = true;
      state = 'stopped';
      off();
      offUnlock();
      let failure = false;
      try {
        releaseOwners();
      } catch {
        failure = true;
      }
      try {
        if (!closePrivateVisibilityScope(visibility)) failure = true;
      } catch {
        failure = true;
      }
      cleanupRequired = failure;
      if (!failure) cleaned = true;
      else throw Error('private_inbox_cleanup_required');
    }
    function pause(why: string) {
      if (stopped || failed) return;
      failed = true;
      state = 'needs_action';
      reason = why;
      try {
        releaseOwners();
      } catch {
        cleanupRequired = true;
      }
    }
    function receive(message: PrivatePageMessage) {
      if (stopped || failed) return;
      if (!current()) {
        stop();
        return;
      }
      if (message.type === 'end') {
        backfill = message.reason;
        if (message.reason !== 'complete') pause(message.reason);
        return;
      }
      candidates++;
      const size = new TextEncoder().encode(message.wire).length;
      bytes += size;
      // Bound outstanding ciphertext work across both lanes before queuing.
      if (
        pending >= PRIVATE_TRANSPORT_BUDGETS.deliveries ||
        pendingBytes + size > PRIVATE_TRANSPORT_BUDGETS.processedBytes
      ) {
        pause('budget');
        return;
      }
      const origin = message.from.endsWith('/')
        ? message.from.slice(0, -1)
        : message.from;
      if (!targets.includes(origin)) {
        rejected++;
        return;
      }
      const envelope = admitInboxEnvelope(
        message.wire,
        ownership.owner,
        origin,
        Date.now()
      );
      if (!envelope) {
        rejected++;
        return;
      }
      pending++;
      pendingBytes += size;
      queue = queue
        .then(async () => {
          if (stopped || failed) return;
          if (!current()) {
            stop();
            return;
          }
          const result = await retainInboxEnvelope(
            repository,
            session,
            envelope
          );
          if (stopped || failed || !current()) {
            if (!stopped && !failed) stop();
            return;
          }
          if (result.status === 'retained') retained++;
          else if (result.status === 'duplicate') duplicates++;
          else pause(result.status);
        })
        .catch(() => pause('unknown_completion'))
        .finally(() => {
          pending--;
          pendingBytes -= size;
        });
    }
    ingress.set(token, {
      snapshot: () => ({
        owner: ownership.owner,
        state,
        candidates,
        bytes,
        retained,
        duplicates,
        rejected,
        reason,
        pending,
        cleanupRequired,
        backfill,
        historyComplete: false,
        foregroundOnly: true
      }),
      stop,
      start(reviewed) {
        if (
          reviewed !== 'reviewed_foreground_inbox' ||
          consumed ||
          stopped ||
          !current()
        )
          return false;
        consumed = true;
        state = 'running';
        try {
          releaseLive = subscribePrivateLive(pool, (message) => {
            if (stopped || failed) return;
            if (!current()) {
              stop();
              return;
            }
            if (message.type === 'open') {
              // Actual SDK OPEN follows its REQ send. Defer so synchronous cached
              // connections cannot begin backfill before the live release binds.
              if (backfill !== 'not_started') return;
              backfill = 'running';
              queueMicrotask(() => {
                if (stopped || failed) return;
                if (!current()) {
                  stop();
                  return;
                }
                state = 'live';
                try {
                  release = subscribePrivatePage(
                    pool,
                    PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay,
                    receive
                  );
                } catch {
                  pause('backfill_unavailable');
                }
              });
            } else if (message.type === 'end') pause(message.reason);
            else receive(message);
          });
        } catch {
          state = 'needs_action';
          reason = 'live_unavailable';
          return false;
        }
        return true;
      }
    });
    sessions.set(session, token);
    off = subscribePrivateSessionClose(session, stop);
    offUnlock = subscribeUnlockedSessionClose(unlocked, stop);
    if (!current()) {
      stop();
      return undefined;
    }
    return token;
  } catch {
    return undefined;
  }
}
export function startInboxSync(token: InboxSync, review: unknown): boolean {
  return ingress.get(token)?.start(review) ?? false;
}
export function stopInboxSync(token: InboxSync): void {
  ingress.get(token)?.stop();
}
export function inboxSyncSnapshot(token: InboxSync): Snapshot | undefined {
  return ingress.get(token)?.snapshot();
}
