import {
  identityMessagingOwnership,
  recheckIdentityOwner,
  subscribeIdentityInvalidation,
  type IdentitySession
} from './identity-session.ts';
import type { PublicEffectCapture } from './effect-ownership.ts';
declare const privateBrand: unique symbol;
export type PrivateSession = Readonly<{ [privateBrand]: true }>;
type Controller = {
  identity: IdentitySession;
  capture: PublicEffectCapture;
  current(): boolean;
  close(): void;
  subscribe(listener: () => void): () => void;
};
const sessions = new WeakMap<PrivateSession, Controller>();
const currentSessions = new WeakMap<IdentitySession, PrivateSession>();
// No sockets, decryption, stores or readiness on construction. Only an explicit
// capability action may perform the fresh owner recheck through the page SDK
// scheduler. The generation is the real adapter's, not a caller-supplied key.
export async function createPrivateSession(
  identity: IdentitySession,
  review: unknown
): Promise<PrivateSession | undefined> {
  if (typeof window === 'undefined' || review !== 'reviewed_private_session')
    return undefined;
  const before = identityMessagingOwnership(identity);
  if (!before) return undefined;
  const checked = await recheckIdentityOwner(identity);
  const capture = identityMessagingOwnership(identity);
  if (
    !before.current() ||
    checked.state !== 'messaging_capable' ||
    checked.admission === 'busy' ||
    !capture ||
    capture.owner !== before.owner ||
    capture.session !== before.session
  )
    return undefined;
  const existing = currentSessions.get(identity);
  if (existing && sessions.get(existing)?.current()) return existing;
  const token = Object.freeze({}) as PrivateSession;
  const listeners = new Map<symbol, () => void>();
  let closed = false;
  let unsubscribe = () => {};
  const current = () => !closed && capture.current();
  function close() {
    if (closed) return;
    closed = true;
    unsubscribe();
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Pool retains failed cleanup until retry. */
      }
    }
    listeners.clear();
  }
  sessions.set(token, {
    identity,
    capture,
    current,
    close,
    subscribe(listener) {
      if (!current()) {
        close();
        listener();
        return () => {};
      }
      const id = Symbol();
      listeners.set(id, listener);
      return () => {
        listeners.delete(id);
      };
    }
  });
  unsubscribe = subscribeIdentityInvalidation(identity, () => {
    if (!current()) close();
  });
  // Observe again after registering: no missed invalidation/acquisition seam.
  if (!current()) {
    close();
    return undefined;
  }
  currentSessions.set(identity, token);
  return token;
}
export function privateSessionOwnership(
  token: PrivateSession
): PublicEffectCapture | undefined {
  const state = sessions.get(token);
  if (!state?.current()) {
    state?.close();
    return undefined;
  }
  return {
    owner: state.capture.owner,
    session: state.capture.session,
    current: () => state.current()
  };
}
export function privateSessionSnapshot(
  token: PrivateSession
): Readonly<{ owner: string; current: boolean }> | undefined {
  const state = sessions.get(token);
  return state && { owner: state.capture.owner, current: state.current() };
}
export function subscribePrivateSessionClose(
  token: PrivateSession,
  listener: () => void
): () => void {
  const state = sessions.get(token);
  if (!state) throw new Error('private_session_invalid');
  return state.subscribe(listener);
}
export function closePrivateSession(token: PrivateSession): void {
  sessions.get(token)?.close();
}
// Every new finite network action gets a fresh SDK owner observation; neither
// a remembered key nor a still-current presentation snapshot substitutes.
export async function recheckPrivateSession(
  token: PrivateSession
): Promise<boolean> {
  const state = sessions.get(token);
  if (!state?.current()) return false;
  const checked = await recheckIdentityOwner(state.identity);
  return (
    state.current() &&
    checked.state === 'messaging_capable' &&
    checked.publicKey === state.capture.owner &&
    checked.admission !== 'busy'
  );
}
