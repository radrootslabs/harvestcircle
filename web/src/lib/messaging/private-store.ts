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
  closePrivateCacheScope
} from '../nostr/private-cache-scope.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';

declare const storeBrand: unique symbol;
export type PrivateStore = Readonly<{ [storeBrand]: true }>;
type Snapshot = Readonly<{ closed: boolean; count: number; bytes: number }>;
type Result = 'accepted' | 'duplicate' | 'rejected' | 'limit' | 'closed';
type Controller = {
  insert(token: VerifiedEnvelope): Result;
  envelope(id: string): VerifiedEnvelope | undefined;
  snapshot(): Snapshot;
  close(): void;
};
const stores = new WeakMap<PrivateStore, Controller>();
const sessions = new WeakMap<PrivateSession, PrivateStore>();
// Only raw signed ciphertext is exposed now. Generic private memory ownership
// is not plaintext admission: seal/rumor/room proofs and projection consumers
// remain with the later validation owners. No public EventStore or shared SDK
// memoized decrypt cache ever receives these objects.
export function getPrivateStore(
  session: PrivateSession
): PrivateStore | undefined {
  if (typeof window === 'undefined') return undefined;
  const observed = privateSessionOwnership(session);
  if (!observed) throw new Error('private_session_invalid');
  const capture = observed;
  const prior = sessions.get(session);
  if (prior) {
    if (controller(prior).snapshot().closed)
      throw new Error('private_store_closed');
    return prior;
  }
  const selectedCache = getPrivateCacheScope(session, 'raw_ciphertext');
  if (!selectedCache) return undefined;
  const cache = selectedCache;
  const token = Object.freeze({}) as PrivateStore;
  let closed = false;
  let off = () => {};
  function close() {
    if (closed) return;
    closed = true;
    closePrivateCacheScope(cache);
    off();
    stores.set(token, {
      insert: () => 'closed',
      envelope: () => undefined,
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
  function admitted(
    proof: VerifiedEnvelope
  ):
    | Readonly<{ id: string; wire: string; proof: VerifiedEnvelope }>
    | undefined {
    const event = verifiedEnvelopeSnapshot(proof);
    if (
      !event ||
      event.kind !== 1059 ||
      event.tags.length !== 1 ||
      event.tags[0].length !== 2 ||
      event.tags[0][0] !== 'p' ||
      event.tags[0][1] !== capture.owner
    )
      return undefined;
    const wire = JSON.stringify({
      id: event.id,
      pubkey: event.pubkey,
      kind: event.kind,
      created_at: event.created_at,
      tags: event.tags.map((tag) => [...tag]),
      content: event.content,
      sig: event.sig
    });
    if (!boundedUtf8(wire, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes))
      return undefined;
    const checked = verifyEnvelope(wire);
    return checked.ok
      ? { id: event.id, wire, proof: checked.value }
      : undefined;
  }
  stores.set(token, {
    insert(proof) {
      if (!current()) return 'closed';
      const accepted = admitted(proof);
      if (!accepted) return 'rejected';
      if (!current()) return 'closed';
      const result = retainPrivateCacheWire(cache, accepted.id, accepted.wire);
      return result === 'conflict' ? 'rejected' : result;
    },
    envelope(id) {
      if (!current()) return undefined;
      const raw = privateCacheWire(cache, id);
      if (raw === undefined) return undefined;
      // Internal memory ownership never substitutes for actual signed admission.
      const checked = verifyEnvelope(raw);
      if (!checked.ok) return undefined;
      const accepted = admitted(checked.value);
      return accepted?.id === id && current() ? accepted.proof : undefined;
    },
    snapshot: () =>
      current()
        ? privateCacheSnapshot(cache)
        : { closed: true, count: 0, bytes: 0 },
    close
  });
  sessions.set(session, token);
  off = subscribePrivateSessionClose(session, close);
  if (!current()) {
    close();
    throw new Error('private_session_invalid');
  }
  return token;
}
function controller(store: PrivateStore): Controller {
  const state = stores.get(store);
  if (!state) throw new Error('private_store_invalid');
  return state;
}
export function insertPrivateEnvelope(
  store: PrivateStore,
  proof: VerifiedEnvelope
): Result {
  return controller(store).insert(proof);
}
export function privateStoreEnvelope(
  store: PrivateStore,
  id: string
): VerifiedEnvelope | undefined {
  return controller(store).envelope(id);
}
export function privateStoreSnapshot(store: PrivateStore): Snapshot {
  return controller(store).snapshot();
}
export function closePrivateStore(store: PrivateStore): void {
  controller(store).close();
}
