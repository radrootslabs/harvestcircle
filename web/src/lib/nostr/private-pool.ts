import { privateInboxQueries } from './inbox-queries.ts';
import { createScopedRelayPool } from './pool-factory.ts';
import { NEVER } from 'rxjs';
import {
  privatePublicationSnapshot,
  takePrivatePublication,
  type PrivatePublication,
  type PrivateDeliveryRole
} from './private-publisher.ts';
import {
  takeInboxAuthAdmission,
  takeGuardedInboxAuthResponse,
  type InboxAuthAdmission,
  type GuardedInboxAuthResponse
} from './inbox-auth.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  boundedEnvelopeTags
} from './verified-envelope.ts';
import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';
import {
  PRIVATE_TRANSPORT_BUDGETS,
  PRIVATE_LIVE_BUDGETS,
  PRIVATE_AUTH_BUDGETS,
  PRIVATE_PUBLICATION_BUDGETS,
  RELAY_BUDGETS
} from '../config/budgets.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  recheckPrivateSession,
  invalidatePrivateSessionGeneration,
  type PrivateSession
} from '../runtime/private-session.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
declare const poolBrand: unique symbol;
export type PrivatePool = Readonly<{ [poolBrand]: true }>;
export type PrivateGiftWrapAttemptResult = Readonly<{
  status: 'accepted' | 'refused' | 'unknown' | 'timed_out' | 'stopped';
  role?: PrivateDeliveryRole;
  origin?: string;
  eventId?: string;
  actionId?: string;
  attempt?: number;
}>;
type AttemptEvidence = Readonly<{
  repository: PrivateStorageRepository;
  owner: string;
  command: string;
  pairWire: string;
  receiptWire: string;
}>;
const attemptEvidence = new WeakMap<
  PrivateGiftWrapAttemptResult,
  AttemptEvidence
>();
// Only this module's actual admitted SDK EVENT port mints these observations.
// Detached/cloned status objects cannot impersonate an attempted operation.
export function privateGiftWrapAttemptEvidence(
  result: PrivateGiftWrapAttemptResult
) {
  const evidence = attemptEvidence.get(result);
  return evidence && { ...evidence };
}
export type PrivatePageMessage =
  | Readonly<{ type: 'candidate'; from: string; wire: string }>
  | Readonly<{
      type: 'end';
      deliveries?: number;
      reason:
        | 'complete'
        | 'closed'
        | 'stale'
        | 'error'
        | 'budget'
        | 'elapsed'
        | 'stopped';
    }>;
export type PrivateLiveMessage =
  PrivatePageMessage | Readonly<{ type: 'open' }>;
type FiniteBudget = {
  elapsed(): number;
  count(): number;
  remainingBytes(): number;
  charge(bytes: number): boolean;
  finish(milliseconds: number): void;
};
function finiteBudget(): FiniteBudget {
  let deliveries = 0,
    bytes = 0,
    elapsed = 0;
  return {
    elapsed: () => elapsed,
    count: () => deliveries,
    remainingBytes: () => PRIVATE_TRANSPORT_BUDGETS.processedBytes - bytes,
    charge(size) {
      deliveries++;
      bytes += size;
      return (
        deliveries <= PRIVATE_TRANSPORT_BUDGETS.deliveries &&
        bytes <= PRIVATE_TRANSPORT_BUDGETS.processedBytes
      );
    },
    finish(milliseconds) {
      elapsed += milliseconds;
    }
  };
}
type Owner = {
  session: PrivateSession;
  policy: RelayPolicy;
  origins: readonly string[];
  readOrigins: readonly string[];
  token: PrivatePool;
  closed(): boolean;
  cleaned(): boolean;
  publish(
    permission: PrivatePublication,
    signal: AbortSignal
  ): Promise<PrivateGiftWrapAttemptResult>;
  auth(
    origin: string,
    admission: InboxAuthAdmission
  ): Promise<PrivateAuthConnection | undefined>;
  subscribe(
    limit: number,
    listener: (message: PrivatePageMessage) => void,
    since?: number,
    budget?: FiniteBudget,
    until?: number,
    source?: string
  ): () => void;
  subscribeLive(listener: (message: PrivateLiveMessage) => void): () => void;
  close(): void;
};
declare const connectionBrand: unique symbol;
export type PrivateAuthConnection = Readonly<{ [connectionBrand]: true }>;
export type PrivateAuthChallenge = Readonly<{
  owner: string;
  session: symbol;
  connection: symbol;
  relay: string;
  challenge: string;
  current(): boolean;
}>;
type AuthControl = {
  invalidate(proof: PrivateAuthChallenge): boolean;
  challenge(): PrivateAuthChallenge | undefined;
  stopped(): boolean;
  reserve(proof: PrivateAuthChallenge): boolean;
  send(
    proof: PrivateAuthChallenge,
    response: GuardedInboxAuthResponse
  ): Promise<'accepted' | 'refused' | 'unknown' | 'stopped'>;
  close(): void;
};
const authConnections = new WeakMap<PrivateAuthConnection, AuthControl>();
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
  const captured = privateSessionOwnership(session);
  if (!captured) throw new Error('private_session_invalid');
  const capture = captured;
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
      !manifest.inbox.some(
        ({ origin: allowed, read, write }) =>
          allowed === origin && (read || write)
      )
    )
      throw new Error('private_pool_origins_invalid');
    selectedOrigins.set(origin, true);
  }
  const origins = [...selectedOrigins.keys()];
  const readOrigins = origins.filter((origin) =>
    manifest.inbox.some((row) => row.origin === origin && row.read)
  );
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
  let liveActive = () => {},
    liveBusy = false,
    liveCleanupRequired = false;
  let off = () => {};
  const authCleanup = new Map<string, () => void>();
  const publicationStops = new Map<() => void, true>();
  let publishing = false,
    publicationCleanupRequired = false;
  const owner: Owner = {
    session,
    policy,
    origins,
    readOrigins,
    token,
    closed: () => closed,
    cleaned: () => cleaned,
    async publish(permission, signal) {
      const view = privatePublicationSnapshot(permission);
      if (
        !view ||
        publishing ||
        closed ||
        cleanupRequired ||
        publicationCleanupRequired ||
        signal.aborted ||
        !capture.current() ||
        view.owner !== capture.owner ||
        !origins.includes(view.origin) ||
        !manifest.inbox.some(
          ({ origin: allowed, write }) => allowed === view.origin && write
        ) ||
        typeof window === 'undefined' ||
        !navigator.locks?.request
      )
        return { status: 'stopped' };
      publishing = true;
      try {
        // No owner lock is held across the serialized fresh extension job.
        if (
          !(await recheckPrivateSession(session)) ||
          closed ||
          signal.aborted ||
          !capture.current()
        )
          return { status: 'stopped' };
        return await navigator.locks.request(
          'harvestcircle:owner:' + capture.owner,
          { mode: 'exclusive', ifAvailable: true },
          async (lock) => {
            if (!lock || closed || signal.aborted || !capture.current())
              return { status: 'stopped' as const };
            const admitted = await takePrivatePublication(
              permission,
              session,
              policy,
              view.origin
            );
            if (
              !admitted ||
              closed ||
              signal.aborted ||
              !capture.current() ||
              !admitted.current()
            )
              return { status: 'stopped' as const };
            const network = admitted.beginNetwork();
            if (admitted.networkMetered && !network)
              return { status: 'stopped' as const };
            return await new Promise<PrivateGiftWrapAttemptResult>(
              (resolve) => {
                let ended = false,
                  attempted = false,
                  release = () => {},
                  fenceTimer: ReturnType<typeof setTimeout> | undefined;
                function finish(
                  status: PrivateGiftWrapAttemptResult['status']
                ) {
                  if (ended) return;
                  ended = true;
                  network?.finish();
                  clearTimeout(timer);
                  if (fenceTimer !== undefined) clearTimeout(fenceTimer);
                  signal.removeEventListener('abort', stop);
                  try {
                    release();
                    publicationStops.delete(stop);
                  } catch {
                    publicationCleanupRequired = true;
                    status = 'unknown';
                  }
                  const result: PrivateGiftWrapAttemptResult = {
                    status,
                    role: admitted!.role,
                    origin: admitted!.origin,
                    eventId: admitted!.eventId,
                    ...(network
                      ? { actionId: network.actionId, attempt: network.attempt }
                      : {})
                  };
                  if (attempted && network) {
                    attemptEvidence.set(result, {
                      repository: admitted!.repository,
                      owner: admitted!.owner,
                      command: admitted!.command,
                      pairWire: admitted!.pairWire,
                      receiptWire: JSON.stringify({
                        actionId: network.actionId,
                        role: admitted!.role,
                        origin: admitted!.origin,
                        eventId: admitted!.eventId,
                        attempt: network.attempt,
                        status,
                        observedAtMilliseconds: Date.now(),
                        readbackWire: null
                      })
                    });
                  }
                  resolve(result);
                }
                function stop() {
                  if (ended) {
                    release();
                    publicationStops.delete(stop);
                    return;
                  }
                  finish('stopped');
                }
                function fence() {
                  if (ended) return;
                  if (network && network.remaining() <= 0) {
                    finish('timed_out');
                    return;
                  }
                  if (
                    closed ||
                    signal.aborted ||
                    !capture.current() ||
                    !admitted!.current()
                  ) {
                    finish('stopped');
                    return;
                  }
                  fenceTimer = setTimeout(() => fence(), 50);
                }
                const relay = sdk.relay(view.origin);
                const allowance =
                  network?.remaining() ??
                  PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds;
                const timeout =
                  Number.isFinite(relay.eventTimeout) && relay.eventTimeout > 0
                    ? Math.min(allowance, relay.eventTimeout)
                    : allowance;
                const timer = setTimeout(() => finish('timed_out'), timeout);
                signal.addEventListener('abort', stop, { once: true });
                publicationStops.set(stop, true);
                try {
                  if (
                    closed ||
                    signal.aborted ||
                    !capture.current() ||
                    !admitted.current()
                  ) {
                    finish('stopped');
                    return;
                  }
                  // The only private EVENT port accepts the freshly read-back1059;
                  // no caller event, key, relay template or SDK option crosses it.
                  attempted = true;
                  const subscription = relay
                    .event(admitted.event, 'EVENT')
                    .subscribe({
                      next(response) {
                        if (ended) return;
                        if (network && network.remaining() <= 0) {
                          finish('timed_out');
                          return;
                        }
                        if (
                          (response.from !== view.origin &&
                            response.from !== view.origin + '/') ||
                          typeof response.ok !== 'boolean'
                        )
                          finish('unknown');
                        else if (response.ok) finish('accepted');
                        else if (response.message === 'Timeout')
                          finish('unknown');
                        else finish('refused');
                      },
                      error: () => finish('unknown'),
                      complete: () => {
                        if (!ended) finish('unknown');
                      }
                    });
                  release = () => subscription.unsubscribe();
                  if (ended) {
                    try {
                      release();
                      publicationStops.delete(stop);
                    } catch {
                      publicationCleanupRequired = true;
                    }
                  } else fence();
                } catch {
                  finish('unknown');
                }
              }
            );
          }
        );
      } catch {
        return { status: 'stopped' };
      } finally {
        publishing = false;
      }
    },
    async auth(origin, admission) {
      if (
        closed ||
        cleanupRequired ||
        !origins.includes(origin) ||
        !readOrigins.includes(origin) ||
        authCleanup.has(origin) ||
        !capture.current()
      )
        return undefined;
      if (!takeInboxAuthAdmission(admission, owner.token, origin))
        return undefined;
      // Reserve before any provider job. No duplicate reviewed action may reset
      // a live connection's response admission counter.
      let stopped = false,
        responded = false,
        responses = 0,
        challenge: string | undefined;
      let generation: symbol | undefined;
      const proofs = new WeakMap<PrivateAuthChallenge, true>();
      let reservation: PrivateAuthChallenge | undefined;
      let sent = false;
      const releases = new Map<symbol, () => void>();
      function cleanup() {
        stopped = true;
        challenge = undefined;
        generation = undefined;
        let failed = false;
        for (const release of releases.values()) {
          try {
            release();
          } catch {
            failed = true;
          }
        }
        if (failed) throw new Error('private_auth_cleanup_required');
      }
      authCleanup.set(origin, cleanup);
      const halt = () => {
        if (stopped) return;
        stopped = true;
        try {
          owner.close();
        } catch {
          /* Actual control retained for retry. */
        }
      };
      if (
        !(await recheckPrivateSession(session)) ||
        closed ||
        stopped ||
        !capture.current()
      ) {
        halt();
        return undefined;
      }
      const relay = sdk.relay(origin);
      const token = Object.freeze({}) as PrivateAuthConnection;
      // A private page may already own this actual SDK connection. Its open$
      // Subject does not replay; initialize from its genuine connected state.
      if (relay.connected$.value) generation = Symbol();
      function current() {
        return (
          !stopped && !closed && capture.current() && generation !== undefined
        );
      }
      const control: AuthControl = {
        invalidate(proof) {
          if (
            !proofs.has(proof) ||
            !proof.current() ||
            !current() ||
            proof.connection !== generation ||
            proof.challenge !== challenge
          )
            return false;
          return invalidatePrivateSessionGeneration(session);
        },
        stopped: () => !current(),
        challenge() {
          if (!current() || challenge === undefined || responded)
            return undefined;
          const exact = challenge,
            stamp = generation!;
          const proof: PrivateAuthChallenge = {
            owner: capture.owner,
            session: capture.session,
            connection: stamp,
            relay: relay.url,
            challenge: exact,
            current: () =>
              current() && generation === stamp && challenge === exact
          };
          proofs.set(proof, true);
          return proof;
        },
        reserve(proof) {
          if (
            !proofs.has(proof) ||
            reservation !== undefined ||
            !proof.current() ||
            !current() ||
            responded ||
            proof.connection !== generation ||
            proof.challenge !== challenge ||
            responses >= PRIVATE_AUTH_BUDGETS.responsesPerConnectionAction
          )
            return false;
          responses++;
          reservation = proof;
          sent = false;
          return true;
        },
        async send(proof, approved) {
          const wire = takeGuardedInboxAuthResponse(approved, proof);
          const verified = verifyEnvelope(wire);
          const event = verified.ok
            ? verifiedEnvelopeSnapshot(verified.value)
            : undefined;
          if (
            !proofs.has(proof) ||
            reservation !== proof ||
            sent ||
            !event ||
            event.kind !== 22242 ||
            event.pubkey !== capture.owner ||
            event.content !== '' ||
            JSON.stringify(event.tags) !==
              JSON.stringify([
                ['relay', relay.url],
                ['challenge', proof.challenge]
              ]) ||
            !proof.current() ||
            !current() ||
            proof.connection !== generation ||
            proof.challenge !== challenge
          )
            return 'stopped';
          try {
            sent = true;
            const response = await relay.auth(event);
            if (!proof.current() || !current()) return 'stopped';
            if (response.ok) {
              responded = true;
              return 'accepted';
            }
            const outcome =
              response.message === 'Timeout' ? 'unknown' : 'refused';
            halt();
            return outcome;
          } catch {
            halt();
            return 'unknown';
          }
        },
        close: () => owner.close()
      };
      authConnections.set(token, control);
      const opened = relay.open$.subscribe(() => {
        if (!stopped && !closed && capture.current()) generation = Symbol();
      });
      releases.set(Symbol(), () => opened.unsubscribe());
      const lost = relay.close$.subscribe(halt);
      releases.set(Symbol(), () => lost.unsubscribe());
      const closing = relay.closing$.subscribe(halt);
      releases.set(Symbol(), () => closing.unsubscribe());
      const challenges = relay.challenge$.subscribe((next: unknown) => {
        if (stopped || closed || !capture.current()) {
          halt();
          return;
        }
        if (next === null) return;
        if (
          typeof next !== 'string' ||
          !next ||
          !boundedEnvelopeTags([
            ['relay', relay.url],
            ['challenge', next]
          ]) ||
          !generation
        ) {
          halt();
          return;
        }
        if (next === challenge) return;
        if (
          responses >= PRIVATE_AUTH_BUDGETS.responsesPerConnectionAction ||
          (challenge !== undefined && !responded)
        ) {
          halt();
          return;
        }
        challenge = next;
        responded = false;
        reservation = undefined;
      });
      releases.set(Symbol(), () => challenges.unsubscribe());
      if (closed || stopped || !capture.current()) {
        cleanup();
        owner.close();
        return undefined;
      }
      // Official Observable filter input holds the watchTower without emitting
      // a filter/REQ. Its own CLOSE at teardown is ordinary SDK control cleanup.
      const hold = relay
        .req(NEVER, {
          waitForAuth: false,
          reconnect: false,
          resubscribe: false
        })
        .subscribe({ error: halt, complete: halt });
      releases.set(Symbol(), () => hold.unsubscribe());
      if (closed || stopped || !capture.current()) {
        cleanup();
        owner.close();
        return undefined;
      }
      return token;
    },
    subscribeLive(listener) {
      if (!readOrigins.length) throw Error('private_read_origins_unavailable');
      if (closed || !capture.current()) throw Error('private_pool_closed');
      if (liveBusy || liveCleanupRequired) throw Error('private_live_busy');
      liveBusy = true;
      let ended = false,
        release = () => {},
        serial = 0;
      const opened = new Map<string, true>(),
        meters = new Map<string, Map<number, { at: number; bytes: number }>>();
      function emit(message: PrivateLiveMessage) {
        try {
          listener(message);
        } catch {
          /* Keep cleanup owned even when a consumer throws. */
        }
      }
      function finish(
        reason: Extract<PrivatePageMessage, { type: 'end' }>['reason'],
        propagate = false
      ) {
        if (ended) {
          if (liveCleanupRequired && propagate) {
            release();
            liveCleanupRequired = false;
            liveBusy = false;
          }
          return;
        }
        ended = true;
        let failure: unknown;
        try {
          release();
          liveCleanupRequired = false;
          liveBusy = false;
        } catch (error) {
          failure = error;
          liveCleanupRequired = true;
        }
        meters.clear();
        opened.clear();
        emit({ type: 'end', reason: liveCleanupRequired ? 'error' : reason });
        if (liveCleanupRequired && propagate) throw failure;
      }
      liveActive = () => {
        finish('closed', true);
        release();
      };
      void recheckPrivateSession(session)
        .then((fresh) => {
          if (ended) return;
          if (!fresh || closed || !capture.current()) {
            finish('stale');
            return;
          }
          const subscription = sdk
            .req(
              [...readOrigins],
              [...privateInboxQueries(capture.owner, 'live')],
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
                  return;
                }
                const origin = message.from.endsWith('/')
                  ? message.from.slice(0, -1)
                  : message.from;
                if (!readOrigins.includes(origin)) {
                  finish('error');
                  return;
                }
                if (message.type === 'OPEN') {
                  opened.set(origin, true);
                  if (opened.size === readOrigins.length)
                    emit({ type: 'open' });
                } else if (message.type === 'EVENT') {
                  const now = performance.now();
                  let meter = meters.get(origin);
                  if (!meter) {
                    meter = new Map();
                    meters.set(origin, meter);
                  }
                  let bytes = 0;
                  for (const [key, item] of meter) {
                    if (
                      now - item.at >=
                      PRIVATE_LIVE_BUDGETS.windowMilliseconds
                    )
                      meter.delete(key);
                    else bytes += item.bytes;
                  }
                  // Count every actual delivery, including invalid envelopes/duplicates.
                  if (meter.size >= PRIVATE_LIVE_BUDGETS.deliveries) {
                    finish('budget');
                    return;
                  }
                  const wire = JSON.stringify(message.event);
                  if (
                    typeof wire !== 'string' ||
                    !boundedUtf8(
                      wire,
                      PRIVATE_LIVE_BUDGETS.processedBytes - bytes
                    )
                  ) {
                    finish('budget');
                    return;
                  }
                  meter.set(++serial, {
                    at: now,
                    bytes: new TextEncoder().encode(wire).length
                  });
                  if (
                    boundedUtf8(wire, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
                  )
                    emit({ type: 'candidate', from: origin, wire });
                } else if (message.type === 'CLOSED') finish('closed');
                else if (message.type === 'ERROR') finish('error');
                // EOSE ends retained history only; the unthrottled live owner remains.
              },
              error: () => finish('error'),
              complete: () => finish('closed')
            });
          release = () => subscription.unsubscribe();
          if (ended || closed || !capture.current()) {
            release();
            if (!ended) finish('stale');
          }
        })
        .catch(() => {
          if (!ended) finish('error');
        });
      return () => finish('stopped', true);
    },
    subscribe(limit, listener, since, sharedBudget, until, source) {
      const targets =
        source === undefined
          ? readOrigins
          : readOrigins.filter((origin) => origin === source);
      if (targets.length === 0)
        throw new Error('private_read_origins_unavailable');
      if (closed || !capture.current()) {
        owner.close();
        throw new Error('private_pool_closed');
      }
      if (cleanupRequired) throw new Error('private_page_cleanup_required');
      if (busy) throw new Error('private_page_busy');
      busy = true;
      let ended = false;
      const budget = sharedBudget ?? finiteBudget();
      const baselineDeliveries = budget.count();
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
        if (started > 0) budget.finish(performance.now() - started);
        if (timer !== undefined) clearTimeout(timer);
        let failure: unknown;
        try {
          release();
        } catch (error) {
          cleanupRequired = true;
          failure = error;
        } finally {
          busy = cleanupRequired;
          emit({
            type: 'end',
            reason: cleanupRequired ? 'error' : reason,
            ...(source === undefined
              ? {}
              : { deliveries: budget.count() - baselineDeliveries })
          });
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
          if (budget.elapsed() >= PRIVATE_TRANSPORT_BUDGETS.pageMilliseconds) {
            finish('elapsed');
            return;
          }
          started = performance.now();
          if (ended || closed || !capture.current()) {
            if (!ended) finish('stale');
            return;
          }
          const subscription = sdk
            .req(
              [...targets],
              privateInboxQueries(capture.owner, 'backfill', since, until).map(
                (filter) => ({
                  ...filter,
                  limit
                })
              ),
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
                const elapsed = budget.elapsed() + performance.now() - started;
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
                  if (completed.size === targets.length) finish('complete');
                } else if (message.type === 'EVENT') {
                  const wire = JSON.stringify(message.event);
                  // Charge actual encoded bytes, including rejected envelopes. An
                  // input beyond the remaining budget stops before encoding it.
                  if (
                    typeof wire !== 'string' ||
                    !boundedUtf8(wire, budget.remainingBytes())
                  ) {
                    finish('budget');
                    return;
                  }
                  if (!budget.charge(new TextEncoder().encode(wire).length)) {
                    finish('budget');
                    return;
                  }
                  const accepted = boundedUtf8(
                    wire,
                    PRIVATE_TRANSPORT_BUDGETS.envelopeBytes
                  );
                  if (accepted)
                    emit({ type: 'candidate', from: message.from, wire });
                } else if (message.type === 'CLOSED') finish('closed');
                else if (message.type === 'ERROR') finish('error');
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
              Math.max(
                0,
                PRIVATE_TRANSPORT_BUDGETS.pageMilliseconds - budget.elapsed()
              )
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
        liveActive();
      } catch {
        failed = true;
      }
      try {
        active();
      } catch {
        failed = true;
      }
      for (const cleanup of authCleanup.values()) {
        try {
          cleanup();
        } catch {
          failed = true;
        }
      }
      for (const stop of publicationStops.keys()) {
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
// Two bounded windows, one actual original pool and shared pre-dedup work meter.
// Each fresh provider ownership wait is outside cumulative active network time.
export function subscribePrivateHistory(
  token: PrivatePool,
  since: number,
  listener: (message: PrivatePageMessage) => void
): () => void {
  if (!Number.isSafeInteger(since) || since < 0)
    throw Error('private_history_since_invalid');
  const owner = ownerOf(token),
    budget: FiniteBudget = finiteBudget();
  let stopped = false,
    release = () => {};
  function window(overlap: boolean) {
    if (stopped) return;
    try {
      release = owner.subscribe(
        PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay,
        (message) => {
          if (stopped) return;
          if (
            !overlap &&
            message.type === 'end' &&
            message.reason === 'complete'
          ) {
            // finish has released its busy page before notifying this callback.
            queueMicrotask(() => window(true));
          } else listener(message);
        },
        overlap ? since : undefined,
        budget
      );
    } catch {
      listener({ type: 'end', reason: 'error' });
    }
  }
  window(false);
  return () => {
    stopped = true;
    release();
  };
}

declare const historyRunBrand: unique symbol;
export type PrivateHistoryRun = Readonly<{ [historyRunBrand]: true }>;
const historyRuns = new WeakMap<
  PrivateHistoryRun,
  { pool: PrivatePool; budget: FiniteBudget }
>();
// Meter only; original pool's current owner/access admission remains mandatory.
export function createPrivateHistoryRun(pool: PrivatePool): PrivateHistoryRun {
  const owner = ownerOf(pool);
  if (owner.closed()) throw Error('private_pool_closed');
  const token = Object.freeze({}) as PrivateHistoryRun;
  historyRuns.set(token, { pool, budget: finiteBudget() });
  return token;
}
export function subscribePrivateSourcePage(
  pool: PrivatePool,
  run: PrivateHistoryRun,
  source: string,
  until: number | undefined,
  listener: (message: PrivatePageMessage) => void
): () => void {
  const stored = historyRuns.get(run),
    owner = ownerOf(pool);
  if (
    !stored ||
    stored.pool !== pool ||
    !owner.readOrigins.includes(source) ||
    (until !== undefined && (!Number.isSafeInteger(until) || until < 0))
  )
    throw Error('private_history_scope_invalid');
  return owner.subscribe(
    PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay,
    listener,
    undefined,
    stored.budget,
    until,
    source
  );
}
export function closePrivatePool(token: PrivatePool): void {
  ownerOf(token).close();
}
export function publishPrivateGiftWrapAttempt(
  pool: PrivatePool,
  permission: PrivatePublication,
  signal: AbortSignal
): Promise<PrivateGiftWrapAttemptResult> {
  try {
    return ownerOf(pool).publish(permission, signal);
  } catch {
    return Promise.resolve({ status: 'stopped' });
  }
}
export function beginPrivateAuthConnection(
  pool: PrivatePool,
  origin: string,
  admission: InboxAuthAdmission
): Promise<PrivateAuthConnection | undefined> {
  return ownerOf(pool).auth(origin, admission);
}
export function privateAuthChallenge(
  connection: PrivateAuthConnection
): PrivateAuthChallenge | undefined {
  return authConnections.get(connection)?.challenge();
}
export function privateAuthStopped(connection: PrivateAuthConnection): boolean {
  return authConnections.get(connection)?.stopped() ?? true;
}
export function reservePrivateAuthResponse(
  connection: PrivateAuthConnection,
  proof: PrivateAuthChallenge
): boolean {
  return authConnections.get(connection)?.reserve(proof) ?? false;
}
export function sendPrivateAuthResponse(
  connection: PrivateAuthConnection,
  proof: PrivateAuthChallenge,
  response: GuardedInboxAuthResponse
) {
  return (
    authConnections.get(connection)?.send(proof, response) ??
    Promise.resolve('stopped' as const)
  );
}
export function closePrivateAuthConnection(
  connection: PrivateAuthConnection
): void {
  authConnections.get(connection)?.close();
}
export function invalidatePrivateAuthGeneration(
  connection: PrivateAuthConnection,
  proof: PrivateAuthChallenge
): boolean {
  return authConnections.get(connection)?.invalidate(proof) ?? false;
}

// One foreground live lane per selected private relay on the original pool.
export function subscribePrivateLive(
  pool: PrivatePool,
  listener: (message: PrivateLiveMessage) => void
): () => void {
  return ownerOf(pool).subscribeLive(listener);
}
