import { EventStore, type EventStoreOptions } from 'applesauce-core';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';
import type { NostrEvent } from 'applesauce-core/helpers';

declare const ownedStore: unique symbol;
export type PublicStore = Readonly<{ readonly [ownedStore]: true }>;
interface Owner {
  readonly closed: () => boolean;
  readonly insert: (
    token: VerifiedEnvelope
  ) => 'accepted' | 'duplicate' | 'not_public' | 'rejected' | 'closed';
  readonly envelope: (id: string) => VerifiedEnvelope | undefined;
  readonly versions: (
    kind: number,
    pubkey: string,
    identifier: string
  ) => readonly VerifiedEnvelope[];
  readonly close: () => void;
}
const owners = new WeakMap<PublicStore, Owner>();
let lifetime: PublicStore | undefined;
const disposedOwner: Owner = {
  closed: () => true,
  insert: () => 'closed',
  envelope: () => undefined,
  versions: () => [],
  close: () => undefined
};

function signedFields(event: NostrEvent): NostrEvent {
  return {
    id: event.id,
    pubkey: event.pubkey,
    kind: event.kind,
    created_at: event.created_at,
    tags: event.tags.map((tag) => [...tag]),
    content: event.content,
    sig: event.sig
  };
}
// Only fresh signed fields cross SDK verification: verified/cache/decrypt/relay
// symbols and unprocessed extra metadata are never verification or cache proof.
function verifyStoredEvent(event: NostrEvent): boolean {
  return verifyEnvelope(JSON.stringify(signedFields(event))).ok;
}
const options: Readonly<EventStoreOptions> & {
  readonly verifyEvent: typeof verifyStoredEvent;
} = {
  keepOldVersions: true,
  keepDeleted: true,
  keepExpired: true,
  verifyEvent: verifyStoredEvent
};
// The same approved inventory gates public source observations and storage.
export function isPublicEnvelopeKind(kind: unknown): kind is number {
  return typeof kind === 'number' && [0, 5, 10050, 30402].includes(kind);
}

// Import/SSR is inert. The private SDK instance and mutable verifier setter are
// never exposed; acquisition accepts no caller options or private session state.
export function getPublicStore(): PublicStore | undefined {
  if (typeof window === 'undefined') return undefined;
  if (lifetime) {
    if (ownerOf(lifetime).closed()) throw new Error('public_store_closed');
    return lifetime;
  }
  const sdk = new EventStore(options);
  const proofs = new Map<string, VerifiedEnvelope>();
  let closed = false;
  const owner: Owner = {
    closed: () => closed,
    insert(token) {
      if (closed) return 'closed';
      const snapshot = verifiedEnvelopeSnapshot(token);
      if (!snapshot) return 'rejected';
      // This application's approved public inventory. Private wrappers, inner
      // plaintext and connection AUTH belong to separate owners, never here.
      if (!isPublicEnvelopeKind(snapshot.kind)) return 'not_public';
      const event = signedFields(snapshot);
      const sanitized = verifyEnvelope(JSON.stringify(event));
      if (!sanitized.ok) return 'rejected';
      if (proofs.has(event.id)) return 'duplicate';
      // Pinned core6.2.0 handles deletion and some replacement paths before
      // verifyEvent. Outer genuine proof/fresh verification precedes every SDK
      // branch. Kind5 stays exact evidence without activating DeleteManager.
      // keepOldVersions/keepExpired preserve verified incompatible/old heads;
      // SDK filtered queries are never product lifecycle authority. HCP033-037
      // own head/deletion/display-expiry/working-set disposal policy.
      if (event.kind !== 5 && sdk.add(event) === null) return 'rejected';
      proofs.set(event.id, sanitized.value);
      return 'accepted';
    },
    envelope(id) {
      return closed ? undefined : proofs.get(id);
    },
    versions(kind, pubkey, identifier) {
      if (closed) return [];
      return (
        sdk.getReplaceableHistory(kind, pubkey, identifier) ?? []
      ).flatMap((event) => {
        const proof = proofs.get(event.id);
        return proof ? [proof] : [];
      });
    },
    close() {
      if (closed) return;
      closed = true;
      proofs.clear();
      try {
        sdk.dispose();
      } finally {
        // Remove the owner's closures retaining the SDK and its signed data.
        // Terminal disposal is not a JavaScript secure-memory-erasure claim.
        owners.set(token, disposedOwner);
      }
    }
  };
  const token = Object.freeze({}) as PublicStore;
  owners.set(token, owner);
  lifetime = token;
  return token;
}
function ownerOf(token: PublicStore): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('public_store_invalid');
  return owner;
}
export function insertPublicEnvelope(
  owner: PublicStore,
  token: VerifiedEnvelope
): ReturnType<Owner['insert']> {
  return ownerOf(owner).insert(token);
}
export function publicStoreEnvelope(
  owner: PublicStore,
  id: string
): VerifiedEnvelope | undefined {
  return ownerOf(owner).envelope(id);
}
export function publicStoreVersions(
  owner: PublicStore,
  kind: number,
  pubkey: string,
  identifier: string
): readonly VerifiedEnvelope[] {
  return ownerOf(owner).versions(kind, pubkey, identifier);
}
export function closePublicStore(owner: PublicStore): void {
  ownerOf(owner).close();
}
