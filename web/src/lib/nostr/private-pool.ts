import { createScopedRelayPool } from './pool-factory.ts';
import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS, RELAY_BUDGETS } from '../config/budgets.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  recheckPrivateSession,
  type PrivateSession
} from '../runtime/private-session.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
declare const poolBrand: unique symbol;
export type PrivatePool = Readonly<{ [poolBrand]: true }>;
export type PrivatePageMessage =
  | Readonly<{ type: 'candidate'; from: string; wire: string }>
  | Readonly<{
      type: 'end';
      reason:
        | 'complete'
        | 'closed'
        | 'stale'
        | 'error'
        | 'budget'
        | 'elapsed'
        | 'stopped';
    }>;
type Owner = {
  session: PrivateSession;
  policy: RelayPolicy;
  origins: readonly string[];
  token: PrivatePool;
  closed(): boolean;
  cleaned(): boolean;
  subscribe(
    limit: number,
    listener: (message: PrivatePageMessage) => void
  ): () => void;
  close(): void;
};
const pools = new WeakMap<PrivatePool, Owner>();
let lifetime: Owner | undefined;
// Fixed scope selection is not authenticated owner10050 membership/readiness.
// No production receive workflow calls this until its later inbox owner exists.
export function getPrivatePool(
  session: PrivateSession,
  policy: RelayPolicy,
  selected: readonly string[]
): PrivatePool | undefined {
  if (typeof window === 'undefined') return undefined;
  const capture = privateSessionOwnership(session);
  if (!capture) throw new Error('private_session_invalid');
  const manifest = readRelayPolicy(policy);
  if (!manifest.messagingEnabled) throw new Error('private_pool_disabled');
  const length = Array.isArray(selected) ? selected.length : 0;
  if (length < 1 || length > RELAY_BUDGETS.inbox)
    throw new Error('private_pool_origins_invalid');
  const selectedOrigins = new Map<string, true>();
  for (let i = 0; i < length; i++) {
    const origin = selected[i];
    if (
      typeof origin !== 'string' ||
      selectedOrigins.has(origin) ||
      !manifest.inbox.some((row) => row.origin === origin && row.read)
    )
      throw new Error('private_pool_origins_invalid');
    selectedOrigins.set(origin, true);
  }
  const origins = [...selectedOrigins.keys()];
  // Untrusted selection access can run code. Recheck before factory admission.
  if (!capture.current()) throw new Error('private_session_invalid');
  if (lifetime) {
    if (!lifetime.cleaned()) {
      if (lifetime.closed()) throw new Error('private_pool_closed');
      if (
        lifetime.session !== session ||
        lifetime.policy !== policy ||
        lifetime.origins.length !== origins.length ||
        lifetime.origins.some((origin, i) => origin !== origins[i])
      )
        throw new Error('private_pool_in_use');
      return lifetime.token;
    }
  }
  const token = Object.freeze({}) as PrivatePool;
  const sdk = createScopedRelayPool();
  let closed = false,
    cleaned = false,
    busy = false,
    cleanupRequired = false;
  let active = () => {};
  let off = () => {};
  const owner: Owner = {
    session,
    policy,
    origins,
    token,
    closed: () => closed,
    cleaned: () => cleaned,
    subscribe(limit, listener) {
      if (closed || !capture.current()) {
        owner.close();
        throw new Error('private_pool_closed');
      }
      if (cleanupRequired) throw new Error('private_page_cleanup_required');
      if (busy) throw new Error('private_page_busy');
      busy = true;
      let ended = false,
        deliveries = 0,
        bytes = 0;
      let release = () => {};
      let timer: ReturnType<typeof setTimeout> | undefined;
      const completed = new Map<string, true>();
      let started = 0;
      function emit(message: PrivatePageMessage) {
        try {
          listener(message);
        } catch {
          /* Consumer cannot interrupt cleanup. */
        }
      }
      function finish(
        reason: Extract<PrivatePageMessage, { type: 'end' }>['reason'],
        propagate = false
      ) {
        if (ended) {
          if (cleanupRequired && propagate) {
            release();
            cleanupRequired = false;
            busy = false;
          }
          return;
        }
        ended = true;
        if (timer !== undefined) clearTimeout(timer);
        let failure: unknown;
        try {
          release();
        } catch (error) {
          cleanupRequired = true;
          failure = error;
        } finally {
          busy = cleanupRequired;
          emit({ type: 'end', reason: cleanupRequired ? 'error' : reason });
        }
        // SDK and timer callbacks report finite error state without leaking an
        // exception. Explicit stop/close can report failure and retry release.
        if (cleanupRequired && propagate) throw failure;
      }
      active = () => {
        finish('closed', true);
        release();
      };
      // Reserve the page before awaiting fresh extension ownership. Stop/logout
      // fence the continuation and no socket may open from a stale key result.
      void recheckPrivateSession(session)
        .then((fresh) => {
          if (ended) return;
          if (!fresh || closed || !capture.current()) {
            finish('stale');
            return;
          }
          started = performance.now();
          if (ended || closed || !capture.current()) {
            if (!ended) finish('stale');
            return;
          }
          const subscription = sdk
            .req(
              [...origins],
              [{ kinds: [1059], '#p': [capture.owner], limit }],
              {
                waitForAuth: false,
                reconnect: false,
                resubscribe: false
              }
            )
            .subscribe({
              next(message) {
                if (ended) return;
                if (closed || !capture.current()) {
                  finish('stale');
                  owner.close();
                  return;
                }
                const elapsed = performance.now() - started;
                if (ended || closed || !capture.current()) {
                  if (!ended) finish('stale');
                  owner.close();
                  return;
                }
                if (elapsed >= PRIVATE_TRANSPORT_BUDGETS.pageMilliseconds) {
                  finish('elapsed');
                  return;
                }
                if (message.type === 'EOSE') {
                  completed.set(message.from, true);
                  if (completed.size === origins.length) finish('complete');
                } else if (message.type === 'EVENT') {
                  deliveries++;
                  if (deliveries > PRIVATE_TRANSPORT_BUDGETS.deliveries) {
                    finish('budget');
                    return;
                  }
                  const wire = JSON.stringify(message.event);
                  // Charge actual encoded bytes, including rejected envelopes. An
                  // input beyond the remaining budget stops before encoding it.
                  if (
                    typeof wire !== 'string' ||
                    !boundedUtf8(
                      wire,
                      PRIVATE_TRANSPORT_BUDGETS.processedBytes - bytes
                    )
                  ) {
                    finish('budget');
                    return;
                  }
                  bytes += new TextEncoder().encode(wire).length;
                  const accepted = boundedUtf8(
                    wire,
                    PRIVATE_TRANSPORT_BUDGETS.envelopeBytes
                  );
                  if (accepted)
                    emit({ type: 'candidate', from: message.from, wire });
                } else if (message.type === 'ERROR') finish('error');
              },
              complete: () => finish('complete'),
              error: () => finish('error')
            });
          release = () => {
            subscription.unsubscribe();
          };
          if (ended || closed || !capture.current()) {
            release();
            if (!ended) finish('stale');
          } else {
            timer = setTimeout(
              () => finish('elapsed'),
              PRIVATE_TRANSPORT_BUDGETS.pageMilliseconds
            );
          }
        })
        .catch(() => {
          if (!ended) finish('error');
        });
      return () => finish('stopped', true);
    },
    close() {
      if (cleaned) return;
      closed = true;
      let failed = false;
      try {
        active();
      } catch {
        failed = true;
      }
      try {
        sdk.close();
      } catch {
        failed = true;
      }
      if (!failed) {
        cleanupRequired = false;
        cleaned = true;
        off();
      }
      if (failed) throw new Error('private_pool_close_failed');
    }
  };
  // Install ownership before observer registration; reentry cannot allocate a
  // second private SDK. A failed close retains this unavailable page lifetime.
  pools.set(token, owner);
  lifetime = owner;
  off = subscribePrivateSessionClose(session, () => owner.close());
  if (!capture.current()) {
    owner.close();
    throw new Error('private_session_invalid');
  }
  return token;
}
function ownerOf(token: PrivatePool): Owner {
  const owner = pools.get(token);
  if (!owner) throw new Error('private_pool_invalid');
  return owner;
}
export function privatePoolOrigins(token: PrivatePool): readonly string[] {
  return [...ownerOf(token).origins];
}
export function subscribePrivatePage(
  token: PrivatePool,
  requested: number,
  listener: (message: PrivatePageMessage) => void
): () => void {
  if (
    !Number.isInteger(requested) ||
    requested < 1 ||
    requested > PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay
  )
    throw new Error('private_page_limit_invalid');
  return ownerOf(token).subscribe(requested, listener);
}
export function closePrivatePool(token: PrivatePool): void {
  ownerOf(token).close();
}
