import { RelayPool } from 'applesauce-relay/pool';
import { deploymentRelayPolicy } from '../config/deployment-relays.ts';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import type { PublicFilter, PublicPoolMessage } from './exports.ts';

declare const ownedPool: unique symbol;
export type PublicPool = Readonly<{ readonly [ownedPool]: true }>;
interface Owner {
  readonly policy: RelayPolicy;
  readonly token: PublicPool;
  readonly origins: readonly string[];
  readonly closed: () => boolean;
  readonly subscribe: (
    filters: readonly PublicFilter[],
    onMessage: (message: PublicPoolMessage) => void
  ) => () => void;
  readonly close: () => void;
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
  const sdk = new RelayPool({
    keepAlive: 0,
    enablePing: false,
    requestReconnect: 0,
    subscriptionReconnect: 0
  });
  const active = new Map<() => void, true>();
  let closed = false;
  let cleanupComplete = false;
  const owner: Owner = {
    policy,
    token,
    origins,
    closed: () => closed,
    subscribe(filters, onMessage) {
      if (closed) throw new Error('public_pool_closed');
      let release = () => {};
      const subscription = sdk
        .req([...origins], [...filters], {
          waitForAuth: false,
          reconnect: false,
          resubscribe: false
        })
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
      cleanupComplete = !failed;
      if (failed) throw new Error('public_pool_close_failed');
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
  onMessage: (message: PublicPoolMessage) => void
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
  return owner.subscribe(filters, onMessage);
}

// Terminal browser-lifetime shutdown: cancel request controls before closing the
// SDK sockets and its keepalive/reconnect watchers. Routes cannot recreate it.
export function closePublicPool(token: PublicPool): void {
  ownerOf(token).close();
}
