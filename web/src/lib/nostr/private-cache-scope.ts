import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  PRIVATE_TRANSPORT_BUDGETS,
  LOCAL_PERSISTENCE_BUDGETS
} from '../config/budgets.ts';

declare const cacheBrand: unique symbol;
export type PrivateCacheScope = Readonly<{ [cacheBrand]: true }>;
export type PrivateCachePurpose = 'raw_ciphertext' | 'projection_ownership';
export type PrivateCacheResult =
  'accepted' | 'duplicate' | 'conflict' | 'rejected' | 'limit' | 'closed';
type Snapshot = Readonly<{ closed: boolean; count: number; bytes: number }>;
type Controller = {
  current(): boolean;
  retain(id: string, wire: unknown): PrivateCacheResult;
  wire(id: string): string | undefined;
  snapshot(): Snapshot;
  clear(): void;
  close(): void;
};
const scopes = new WeakMap<PrivateCacheScope, Controller>();
const sessions = new WeakMap<
  PrivateSession,
  Map<PrivateCachePurpose, PrivateCacheScope>
>();
// Internal serialized-memory ownership only. This does not validate a message,
// seal, participant or plaintext projection. Current product use is ciphertext;
// later validated proofs must precede projection admission by its typed owner.
// Primitive JSON strings cannot import SDK verification/decrypt/helper Symbols
// or enter Applesauce module-global memoized references.
export function getPrivateCacheScope(
  session: PrivateSession,
  purpose: PrivateCachePurpose
): PrivateCacheScope | undefined {
  if (typeof window === 'undefined') return undefined;
  const observed = privateSessionOwnership(session);
  if (!observed) throw new Error('private_session_invalid');
  const capture = observed;
  if (purpose !== 'raw_ciphertext' && purpose !== 'projection_ownership')
    throw new Error('private_cache_purpose_invalid');
  let owned = sessions.get(session);
  const prior = owned?.get(purpose);
  if (prior) {
    if (!controller(prior).current()) throw new Error('private_cache_closed');
    return prior;
  }
  if (!owned) {
    owned = new Map();
    sessions.set(session, owned);
  }
  const token = Object.freeze({}) as PrivateCacheScope;
  const entries = new Map<string, string>();
  let closed = false,
    bytes = 0;
  let off = () => {};
  function close() {
    if (closed) return;
    closed = true;
    entries.clear();
    bytes = 0;
    off();
    // Retained opaque handles no longer keep the old cache, session or closure.
    // Returned copies in caller memory cannot be securely erased by JavaScript.
    scopes.set(token, {
      current: () => false,
      retain: () => 'closed',
      wire: () => undefined,
      snapshot: () => ({ closed: true, count: 0, bytes: 0 }),
      clear: () => {},
      close: () => {}
    });
  }
  function current() {
    if (closed || !capture.current()) {
      close();
      return false;
    }
    return true;
  }
  const state: Controller = {
    current,
    retain(id, raw) {
      if (!current()) return 'closed';
      if (
        typeof id !== 'string' ||
        !/^[0-9a-f]{64}$/.test(id) ||
        typeof raw !== 'string' ||
        !boundedUtf8(raw, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
      )
        return 'rejected';
      let wire: string;
      try {
        wire = JSON.stringify(JSON.parse(raw));
      } catch {
        return 'rejected';
      }
      if (!boundedUtf8(wire, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes))
        return 'rejected';
      const existing = entries.get(id);
      if (existing !== undefined)
        return existing === wire ? 'duplicate' : 'conflict';
      const size = new TextEncoder().encode(wire).length;
      if (
        entries.size >= LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
        size > LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes - bytes
      )
        return 'limit';
      if (!current()) return 'closed';
      entries.set(id, wire);
      bytes += size;
      return 'accepted';
    },
    wire: (id) => (current() ? entries.get(id) : undefined),
    snapshot: () =>
      current()
        ? { closed: false, count: entries.size, bytes }
        : { closed: true, count: 0, bytes: 0 },
    clear() {
      if (current()) {
        entries.clear();
        bytes = 0;
      }
    },
    close
  };
  scopes.set(token, state);
  owned.set(purpose, token);
  off = subscribePrivateSessionClose(session, close);
  if (!current()) {
    close();
    throw new Error('private_session_invalid');
  }
  return token;
}
function controller(scope: PrivateCacheScope): Controller {
  const state = scopes.get(scope);
  if (!state) throw new Error('private_cache_invalid');
  return state;
}
export function retainPrivateCacheWire(
  scope: PrivateCacheScope,
  id: string,
  wire: unknown
): PrivateCacheResult {
  return controller(scope).retain(id, wire);
}
export function privateCacheWire(
  scope: PrivateCacheScope,
  id: string
): string | undefined {
  return controller(scope).wire(id);
}
export function privateCacheSnapshot(scope: PrivateCacheScope): Snapshot {
  return controller(scope).snapshot();
}
export function clearPrivateCacheScope(scope: PrivateCacheScope): void {
  controller(scope).clear();
}
export function closePrivateCacheScope(scope: PrivateCacheScope): void {
  controller(scope).close();
}
