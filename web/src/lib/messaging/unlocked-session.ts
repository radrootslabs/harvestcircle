import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  getPrivateCacheScope,
  retainPrivateCacheWire,
  privateCacheWire,
  privateCacheSnapshot,
  clearPrivateCacheScope
} from '../nostr/private-cache-scope.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  conversationSnapshot,
  conversationOwnership,
  type AdmittedConversation,
  type ConversationData
} from './admit-conversation.ts';
declare const unlockedBrand: unique symbol;
export type UnlockedSession = Readonly<{ [unlockedBrand]: true }>;
export type UnlockedAdmission =
  'added' | 'duplicate' | 'conflict' | 'rejected' | 'limit' | 'closed';
type View = Readonly<{
  status: 'ready' | 'conflict' | 'closed' | 'rejected';
  messages: readonly ConversationData[];
}>;
type Snapshot = Readonly<{ closed: boolean; count: number; bytes: number }>;
type Controller = {
  current(): boolean;
  ownership(): ReturnType<typeof privateSessionOwnership>;
  contains(room: AdmittedConversation): boolean;
  accept(room: AdmittedConversation): UnlockedAdmission;
  read(): View;
  snapshot(): Snapshot;
  close(): void;
};
const closeObservers = new WeakMap<UnlockedSession, Map<() => void, true>>();
const controllers = new WeakMap<UnlockedSession, Controller>(),
  sessions = new WeakMap<PrivateSession, UnlockedSession>();
// Explicit owner-generation memory scope only. No decryption, shared SDK
// memoized helpers, public EventStore, persistence or network side effect.
export function captureUnlockedSession(
  session: PrivateSession,
  review: unknown
): UnlockedSession | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_messages_unlock')
    return undefined;
  const observed = privateSessionOwnership(session);
  if (!observed) return undefined;
  const capture = observed;
  const previous = sessions.get(session);
  if (previous && controllers.get(previous)?.current()) return previous;
  let observedCache;
  try {
    observedCache = getPrivateCacheScope(session, 'projection_ownership');
  } catch {
    return undefined;
  }
  if (!observedCache) return undefined;
  const cache = observedCache;
  const token = Object.freeze({}) as UnlockedSession,
    entries = new Map<string, string>();
  let closed = false,
    bytes = 0,
    off = () => {};
  function close() {
    if (closed) return;
    closed = true;
    const callbacks = closeObservers.get(token);
    closeObservers.delete(token);
    if (callbacks)
      for (const callback of callbacks.keys()) {
        try {
          callback();
        } catch {
          /* Continue original cache disposal. */
        }
      }
    entries.clear();
    bytes = 0;
    clearPrivateCacheScope(cache);
    off();
    // Drop our reachable private refs. Detached caller copies and JavaScript
    // heap/garbage-collection timing cannot be securely zeroized or recalled.
    controllers.set(token, {
      current: () => false,
      ownership: () => undefined,
      contains: () => false,
      accept: () => 'closed',
      read: () => ({ status: 'closed', messages: [] }),
      snapshot: () => ({ closed: true, count: 0, bytes: 0 }),
      close: () => {}
    });
  }
  function current() {
    if (closed || !capture.current() || privateCacheSnapshot(cache).closed) {
      close();
      return false;
    }
    return true;
  }
  const state: Controller = {
    current,
    ownership: () => (current() ? { ...capture, current } : undefined),
    contains(room) {
      if (!current()) return false;
      const own = conversationOwnership(room),
        data = conversationSnapshot(room);
      if (
        !own?.current() ||
        own.owner !== capture.owner ||
        own.session !== capture.session ||
        !data
      )
        return false;
      const wire = JSON.stringify(data);
      return (
        current() &&
        entries.get(data.rumorId) === wire &&
        privateCacheWire(cache, data.rumorId) === wire
      );
    },
    accept(room) {
      if (!current()) return 'closed';
      // Authenticate original room and generation before even a dedup lookup.
      const ownership = conversationOwnership(room);
      if (
        !ownership?.current() ||
        ownership.owner !== capture.owner ||
        ownership.session !== capture.session
      )
        return 'rejected';
      const data = conversationSnapshot(room);
      if (
        !data ||
        data.owner !== capture.owner ||
        !current() ||
        !ownership.current()
      )
        return 'rejected';
      const wire = JSON.stringify(data),
        size = new TextEncoder().encode(wire).length,
        existing = entries.get(data.rumorId);
      if (existing !== undefined)
        return existing === wire &&
          privateCacheWire(cache, data.rumorId) === wire
          ? 'duplicate'
          : 'conflict';
      if (
        entries.size >= LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
        size > LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes - bytes
      )
        return 'limit';
      const result = retainPrivateCacheWire(cache, data.rumorId, wire);
      if (result !== 'accepted' && result !== 'duplicate') return result;
      if (!current() || !ownership.current()) return 'closed';
      if (privateCacheWire(cache, data.rumorId) !== wire) return 'conflict';
      entries.set(data.rumorId, wire);
      bytes += size;
      return 'added';
    },
    read() {
      if (!current()) return { status: 'closed', messages: [] };
      for (const [id, wire] of entries) {
        // A generic internal cache port is not projection authority. Exact own
        // immutable bytes must agree; poisoning never substitutes another row.
        if (privateCacheWire(cache, id) !== wire)
          return { status: 'conflict', messages: [] };
      }
      const messages = Array.from(
        entries.values(),
        (wire) => JSON.parse(wire) as ConversationData
      );
      return current()
        ? { status: 'ready', messages }
        : { status: 'closed', messages: [] };
    },
    snapshot: () =>
      current()
        ? { closed: false, count: entries.size, bytes }
        : { closed: true, count: 0, bytes: 0 },
    close
  };
  controllers.set(token, state);
  sessions.set(session, token);
  off = subscribePrivateSessionClose(session, close);
  return current() ? token : undefined;
}
export function acceptUnlockedConversation(
  scope: UnlockedSession,
  room: AdmittedConversation
): UnlockedAdmission {
  return controllers.get(scope)?.accept(room) ?? 'rejected';
}
export function readUnlockedMessages(scope: UnlockedSession): View {
  return controllers.get(scope)?.read() ?? { status: 'rejected', messages: [] };
}
export function unlockedSessionSnapshot(scope: UnlockedSession): Snapshot {
  return (
    controllers.get(scope)?.snapshot() ?? { closed: true, count: 0, bytes: 0 }
  );
}
export function closeUnlockedSession(scope: UnlockedSession): void {
  controllers.get(scope)?.close();
}

// Original accepted-cache custody observations only. Copies are not authority.
export function unlockedSessionOwnership(scope: UnlockedSession) {
  return controllers.get(scope)?.ownership();
}
export function unlockedConversationCurrent(
  scope: UnlockedSession,
  room: AdmittedConversation
): boolean {
  return controllers.get(scope)?.contains(room) ?? false;
}

// Original opaque controller lifetime only, not a copied snapshot capability.
export function subscribeUnlockedSessionClose(
  scope: UnlockedSession,
  callback: () => void
): () => void {
  if (!controllers.get(scope)?.current()) {
    callback();
    return () => {};
  }
  let observers = closeObservers.get(scope);
  if (!observers) {
    observers = new Map();
    closeObservers.set(scope, observers);
  }
  observers.set(callback, true);
  const captured = observers;
  return () => {
    captured.delete(callback);
  };
}
