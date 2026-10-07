import { createScopedRelayPool } from './pool-factory.ts';
import { deploymentRelayPolicy } from '../config/deployment-relays.ts';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import type { PublicFilter, PublicPoolMessage } from './exports.ts';
import { requireNip50Source } from './search-sources.ts';
import {
  takePreferencePublication,
  type PreferencePublication
} from './inbox-preference-publication.ts';
export type PreferenceAttemptResult = Readonly<{
  status: 'accepted' | 'refused' | 'timed_out' | 'unknown' | 'stopped';
}>;

declare const ownedPool: unique symbol;
export type PublicPool = Readonly<{ readonly [ownedPool]: true }>;
interface Owner {
  readonly policy: RelayPolicy;
  readonly token: PublicPool;
  readonly origins: readonly string[];
  readonly closed: () => boolean;
  readonly subscribe: (
    filters: readonly PublicFilter[],
    onMessage: (message: PublicPoolMessage) => void,
    sampleSource?: string
  ) => () => void;
  readonly close: () => void;
  readonly preference: (
    permission: PreferencePublication,
    origin: string,
    signal: AbortSignal,
    remaining: number
  ) => Promise<PreferenceAttemptResult>;
}
const owners = new WeakMap<PublicPool, Owner>();
let lifetime: Owner | undefined;

// Anonymous state exists only after browser acquisition. No socket, information
// fetch, identity or extension operation occurs during import or SSR.
export function getPublicPool(
  policy: RelayPolicy = deploymentRelayPolicy
): PublicPool | undefined {
  if (typeof window === 'undefined') return undefined;
  readRelayPolicy(policy);
  if (lifetime) {
    if (lifetime.closed()) throw new Error('public_pool_closed');
    if (lifetime.policy !== policy)
      throw new Error('public_pool_policy_changed');
    return lifetime.token;
  }
  const token = Object.freeze({}) as PublicPool;
  const origins = publicRelayTargets(policy, 'read');
  const sdk = createScopedRelayPool();
  const active = new Map<() => void, true>();
  let closed = false;
  let cleanupComplete = false;
  let publicationCleanupRequired = false;
  const owner: Owner = {
    policy,
    token,
    origins,
    closed: () => closed,
    async preference(permission, origin, signal, remaining) {
      const admitted = takePreferencePublication(permission, policy, origin);
      if (
        closed ||
        publicationCleanupRequired ||
        !admitted ||
        signal.aborted ||
        !Number.isFinite(remaining) ||
        remaining <= 0 ||
        remaining > 45000
      )
        return { status: 'stopped' };
      return await new Promise<PreferenceAttemptResult>((resolve) => {
        let ended = false;
        let release = () => {};
        let fenceTimer: ReturnType<typeof setTimeout> | undefined;
        function finish(status: PreferenceAttemptResult['status']) {
          if (ended) return;
          ended = true;
          clearTimeout(timer);
          if (fenceTimer !== undefined) clearTimeout(fenceTimer);
          signal.removeEventListener('abort', stopped);
          try {
            release();
          } catch {
            publicationCleanupRequired = true;
            status = 'unknown';
          }
          resolve({ status });
        }
        function stopped() {
          if (ended) {
            release();
            return;
          }
          finish('stopped');
        }
        function fence() {
          if (ended) return;
          if (closed || signal.aborted || !admitted!.current()) {
            finish('stopped');
            return;
          }
          fenceTimer = setTimeout(() => fence(), 50);
        }
        signal.addEventListener('abort', stopped, { once: true });
        // Arm our finite deadline before subscribing. The SDK uses the same
        // response shape for its timer and a relay refusal saying "Timeout";
        // only this owned deadline supplies retryable timeout evidence.
        const relay = sdk.relay(origin);
        const timeout =
          Number.isFinite(relay.eventTimeout) && relay.eventTimeout > 0
            ? Math.min(remaining, relay.eventTimeout)
            : remaining;
        const timer = setTimeout(() => finish('timed_out'), timeout);
        try {
          if (!admitted.current() || signal.aborted) {
            finish('stopped');
            return;
          }
          // This SDK invocation is an attempted effect. Stop cannot reverse an
          // already emitted/buffered EVENT, but teardown schedules no new one.
          const subscription = relay.event(admitted.event, 'EVENT').subscribe({
            next(response) {
              if (ended) return;
              if (
                (response.from !== origin && response.from !== origin + '/') ||
                typeof response.ok !== 'boolean'
              ) {
                finish('unknown');
                return;
              }
              if (response.ok) finish('accepted');
              else if (response.message === 'Timeout') finish('unknown');
              else if (
                typeof response.message === 'string' &&
                response.message.startsWith('auth-required:')
              )
                finish('unknown');
              else finish('refused');
            },
            error: () => finish('unknown'),
            complete: () => {
              if (!ended) finish('unknown');
            }
          });
          const stop = () => {
            subscription.unsubscribe();
            active.delete(stop);
          };
          release = () => {
            stop();
            active.delete(stopped);
          };
          active.set(stopped, true);
          if (ended) {
            try {
              release();
            } catch {
              publicationCleanupRequired = true;
            }
          } else {
            fence();
          }
        } catch {
          finish('unknown');
        }
      });
    },
    subscribe(filters, onMessage, sampleSource) {
      if (closed) throw new Error('public_pool_closed');
      let release = () => {};
      const subscription = sdk
        .req(
          sampleSource === undefined
            ? [...origins]
            : [requireNip50Source(policy, sampleSource)],
          [...filters],
          {
            waitForAuth: false,
            reconnect: false,
            resubscribe: false
          }
        )
        .subscribe({
          next: (message) => {
            if (!closed) onMessage(message);
          },
          complete: () => release(),
          error: () => release()
        });
      const stop = () => {
        try {
          subscription.unsubscribe();
        } finally {
          active.delete(stop);
        }
      };
      release = stop;
      if (closed) stop();
      else if (!subscription.closed) active.set(stop, true);
      return stop;
    },
    close() {
      if (cleanupComplete) return;
      closed = true;
      let failed = false;
      for (const stop of active.keys()) {
        try {
          stop();
        } catch {
          failed = true;
        }
      }
      try {
        sdk.close();
      } catch {
        failed = true;
      }
      cleanupComplete = !failed && active.size === 0;
      if (!cleanupComplete) throw new Error('public_pool_close_failed');
    }
  };
  owners.set(token, owner);
  lifetime = owner;
  return token;
}

function ownerOf(token: PublicPool): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('public_pool_invalid');
  return owner;
}
export function publicPoolOrigins(token: PublicPool): readonly string[] {
  return [...ownerOf(token).origins];
}

// Filter construction and event verification/work budgets have separate owners.
// Accept no SDK filter callback (which would expose a raw relay), arbitrary
// destination, AUTH method, information fetch or retry option through this API.
export function subscribePublicPool(
  token: PublicPool,
  filters: readonly PublicFilter[],
  onMessage: (message: PublicPoolMessage) => void,
  sampleSource?: string
): () => void {
  const owner = ownerOf(token);
  if (owner.closed()) throw new Error('public_pool_closed');
  if (
    !Array.isArray(filters) ||
    filters.length === 0 ||
    filters.some(
      (filter) => !filter || typeof filter !== 'object' || Array.isArray(filter)
    )
  )
    throw new Error('public_pool_filters_invalid');
  if (sampleSource !== undefined)
    requireNip50Source(owner.policy, sampleSource);
  if (
    sampleSource === undefined &&
    filters.some((filter) => 'search' in filter)
  )
    throw new Error('public_pool_search_requires_source');
  return owner.subscribe(filters, onMessage, sampleSource);
}

// Terminal browser-lifetime shutdown: cancel request controls before closing the
// SDK sockets and its keepalive/reconnect watchers. Routes cannot recreate it.
export function closePublicPool(token: PublicPool): void {
  ownerOf(token).close();
}
// Narrow once-only preference capability; no raw event, arbitrary SDK options,
// AUTH or general UI publisher. Pool remains the anonymous lifetime owner.
export function publishPublicPreferenceAttempt(
  token: PublicPool,
  permission: PreferencePublication,
  policy: RelayPolicy,
  origin: string,
  signal: AbortSignal,
  remaining: number
): Promise<PreferenceAttemptResult> {
  try {
    const owner = ownerOf(token);
    if (owner.policy !== policy) return Promise.resolve({ status: 'stopped' });
    return owner.preference(permission, origin, signal, remaining);
  } catch {
    return Promise.resolve({ status: 'unknown' });
  }
}
