import { createScopedRelayPool } from './pool-factory.ts';
import { NEVER } from 'rxjs';
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
  PRIVATE_AUTH_BUDGETS,
  RELAY_BUDGETS
} from '../config/budgets.ts';
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
  auth(
    origin: string,
    admission: InboxAuthAdmission
  ): Promise<PrivateAuthConnection | undefined>;
  subscribe(
    limit: number,
    listener: (message: PrivatePageMessage) => void
  ): () => void;
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
  const authCleanup = new Map<string, () => void>();
  const owner: Owner = {
    session,
    policy,
    origins,
    token,
    closed: () => closed,
    cleaned: () => cleaned,
    async auth(origin, admission) {
      if (
        closed ||
        cleanupRequired ||
        !origins.includes(origin) ||
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
      for (const cleanup of authCleanup.values()) {
        try {
          cleanup();
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
export function closePrivatePool(token: PrivatePool): void {
  ownerOf(token).close();
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
