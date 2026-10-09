import {
  readRelayPolicy,
  inboxRelayTargets,
  publicRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { inboxPreferenceSnapshot } from './inbox-preferences.ts';
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
  closePrivatePool
} from './private-pool.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../persistence/inbox-envelope-repository.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
declare const ingressBrand: unique symbol;
export type InboxIngress = Readonly<{ [ingressBrand]: true }>;
type State =
  'ready' | 'running' | 'finite_page_ended' | 'stopped' | 'needs_action';
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
}>;
type Controller = {
  start(review: unknown): boolean;
  stop(): void;
  snapshot(): Snapshot;
};
const ingress = new WeakMap<InboxIngress, Controller>();
// Explicit finite receive admission. A remembered key, allowlist or EOSE alone
// is not exercised own inbox access. No production observation port is supplied
// here; controlled fixtures do not qualify real operator relay/AUTH access.
export function captureInboxIngress(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  own: InboxResolver,
  observeAccess: ObserveInboxAccess | undefined,
  review: unknown
): InboxIngress | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_private_inbox_receive' ||
    !observeAccess
  )
    return undefined;
  try {
    const ownership = privateSessionOwnership(session),
      manifest = readRelayPolicy(policy),
      resolution = inboxResolutionSnapshot(own);
    if (
      !ownership?.current() ||
      !manifest.messagingEnabled ||
      resolution.author !== ownership.owner ||
      resolution.status !== 'ready' ||
      resolution.coverage !== 'bounded-eose' ||
      !resolution.head ||
      !resolution.knownBase
    )
      return undefined;
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
    const current = () => {
      const latest = inboxResolutionSnapshot(own);
      return (
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
    const token = Object.freeze({}) as InboxIngress;
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
      ended = false,
      failed = false,
      cleaned = false,
      cleanupRequired = false;
    let release = () => {},
      off = () => {};
    let queue = Promise.resolve();
    function stop() {
      if (cleaned) return;
      stopped = true;
      state = 'stopped';
      let failure = false;
      try {
        release();
      } catch {
        failure = true;
      }
      try {
        closePrivatePool(pool!);
      } catch {
        failure = true;
      }
      cleanupRequired = failure;
      if (!failure) {
        cleaned = true;
        off();
      } else throw new Error('private_inbox_cleanup_required');
    }
    function settled() {
      if (ended && pending === 0 && !stopped && state === 'running')
        state = 'finite_page_ended';
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
        cleanupRequired
      }),
      stop,
      start(reviewed) {
        if (
          reviewed !== 'reviewed_private_inbox_receive' ||
          consumed ||
          stopped ||
          !current()
        )
          return false;
        consumed = true;
        state = 'running';
        release = subscribePrivatePage(
          pool,
          PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay,
          (message) => {
            if (stopped) return;
            if (!current()) {
              stop();
              return;
            }
            if (message.type === 'end') {
              ended = true;
              if (state !== 'needs_action') reason = message.reason;
              settled();
              return;
            }
            if (failed) return;
            candidates++;
            const size = new TextEncoder().encode(message.wire).length;
            bytes += size;
            // Existing private SDK page also charges every candidate BEFORE
            // crypto/dedup and enforces finite deliveries/bytes/network elapsed.
            if (
              candidates > PRIVATE_TRANSPORT_BUDGETS.deliveries ||
              bytes > PRIVATE_TRANSPORT_BUDGETS.processedBytes
            ) {
              state = 'needs_action';
              reason = 'budget';
              release();
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
            queue = queue
              .then(async () => {
                if (failed || stopped) return;
                if (!current()) {
                  stop();
                  return;
                }
                const result = await retainInboxEnvelope(
                  repository,
                  session,
                  envelope
                );
                if (stopped || !current()) {
                  if (!stopped) stop();
                  return;
                }
                if (result.status === 'retained') retained++;
                else if (result.status === 'duplicate') duplicates++;
                else {
                  failed = true;
                  state = 'needs_action';
                  reason = result.status;
                  release();
                }
              })
              .catch(() => {
                failed = true;
                state = stopped ? 'stopped' : 'needs_action';
                reason = 'unknown_completion';
                try {
                  release();
                } catch {
                  cleanupRequired = true;
                }
              })
              .finally(() => {
                pending--;
                settled();
              });
          }
        );
        return true;
      }
    });
    off = subscribePrivateSessionClose(session, stop);
    if (!current()) {
      stop();
      return undefined;
    }
    return token;
  } catch {
    return undefined;
  }
}
export function startInboxIngress(
  token: InboxIngress,
  review: unknown
): boolean {
  return ingress.get(token)?.start(review) ?? false;
}
export function stopInboxIngress(token: InboxIngress): void {
  ingress.get(token)?.stop();
}
export function inboxIngressSnapshot(
  token: InboxIngress
): Snapshot | undefined {
  return ingress.get(token)?.snapshot();
}
