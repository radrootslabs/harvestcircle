import {
  privateSessionOwnership,
  privateSessionCleanupRequired,
  subscribePrivateSessionClose,
  closePrivateSession,
  type PrivateSession
} from './private-session.ts';
declare const visibilityBrand: unique symbol;
export type PrivateVisibilityScope = Readonly<{ [visibilityBrand]: true }>;
type Snapshot = Readonly<{ active: boolean; cleanupRequired: boolean }>;
type Controller = { snapshot(): Snapshot; close(): boolean };
const scopes = new WeakMap<PrivateVisibilityScope, Controller>();
const sessions = new WeakMap<PrivateSession, PrivateVisibilityScope>();

// Explicit internal Messages lifetime; no automatic unlock, fetch, decrypt,
// provider request or hidden notification. Later Messages activation owns the
// call and route disposal. Visibility returning does not revive a closed scope.
export function getPrivateVisibilityScope(
  session: PrivateSession
): PrivateVisibilityScope | undefined {
  if (typeof window === 'undefined' || typeof document === 'undefined')
    return undefined;
  const observed = privateSessionOwnership(session);
  if (!observed) throw new Error('private_session_invalid');
  const capture = observed;
  const prior = sessions.get(session);
  if (prior) return prior;
  const token = Object.freeze({}) as PrivateVisibilityScope;
  let closed = false,
    registered = false;
  let off = () => {};
  function cleanupVisibility() {
    closed = true;
    if (registered) {
      document.removeEventListener('visibilitychange', onVisibility);
      registered = false;
    }
    off();
  }
  function close() {
    // Session closure drains all caches and sockets even if one cleanup fails.
    const cleaned = closePrivateSession(session);
    try {
      cleanupVisibility();
    } catch {
      return false;
    }
    if (!cleaned) return false;
    scopes.set(token, {
      snapshot: () => ({ active: false, cleanupRequired: false }),
      close: () => true
    });
    return true;
  }
  function onVisibility() {
    if (document.visibilityState !== 'visible') close();
  }
  scopes.set(token, {
    snapshot: () => ({
      active:
        !closed && capture.current() && document.visibilityState === 'visible',
      cleanupRequired:
        closed && (registered || privateSessionCleanupRequired(session))
    }),
    close
  });
  sessions.set(session, token);
  off = subscribePrivateSessionClose(session, cleanupVisibility);
  if (!closed && capture.current() && document.visibilityState === 'visible') {
    // Own the registration before its synchronous host call can reenter.
    registered = true;
    try {
      document.addEventListener('visibilitychange', onVisibility);
    } catch {
      close();
      throw new Error('private_visibility_unavailable');
    }
  }
  if (!capture.current() || document.visibilityState !== 'visible') close();
  return token;
}
function controller(token: PrivateVisibilityScope): Controller {
  const state = scopes.get(token);
  if (!state) throw new Error('private_visibility_invalid');
  return state;
}
export function privateVisibilitySnapshot(
  token: PrivateVisibilityScope
): Snapshot {
  return controller(token).snapshot();
}
export function closePrivateVisibilityScope(
  token: PrivateVisibilityScope
): boolean {
  return controller(token).close();
}
