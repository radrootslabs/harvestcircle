import { EventStore, type EventStoreOptions } from 'applesauce-core';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
import { isPublicEnvelopeKind } from './public-kinds.ts';
export { isPublicEnvelopeKind } from './public-kinds.ts';
import {
  createPublicRetention,
  retainPublicEnvelope,
  publicRetentionEnvelope,
  publicRetentionSnapshot,
  publicRetentionKnown,
  closePublicRetention,
  type RetentionSnapshot,
  type KnownPublicEvidence
} from '../catalog/retention.ts';
import type { PublicHead } from '../catalog/heads.ts';

declare const ownedStore: unique symbol;
export type PublicStore = Readonly<{ readonly [ownedStore]: true }>;
interface Owner {
  readonly closed: () => boolean;
  readonly insert: (
    token: VerifiedEnvelope
  ) =>
    'accepted' | 'duplicate' | 'not_public' | 'rejected' | 'limit' | 'closed';
  readonly envelope: (id: string) => VerifiedEnvelope | undefined;
  readonly versions: (
    kind: number,
    pubkey: string,
    identifier: string
  ) => readonly VerifiedEnvelope[];
  readonly retention: () => RetentionSnapshot;
  readonly known: (head: PublicHead) => KnownPublicEvidence;
  readonly close: () => void;
}
const owners = new WeakMap<PublicStore, Owner>();
let lifetime: PublicStore | undefined;

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
// Import/SSR is inert. The private SDK instance and mutable verifier setter are
// never exposed; acquisition accepts no caller options or private session state.
export function getPublicStore(): PublicStore | undefined {
  if (typeof window === 'undefined') return undefined;
  if (lifetime) {
    if (ownerOf(lifetime).closed()) throw new Error('public_store_closed');
    return lifetime;
  }
  const sdk = new EventStore(options);
  const retention = createPublicRetention();
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
      // Kind5 is retained proof without activating SDK DeleteManager.
      // Admission reserves bounded coherent evidence BEFORE SDK installation.
      return retainPublicEnvelope(
        retention,
        sanitized.value,
        () => event.kind === 5 || sdk.add(event) !== null
      );
    },
    envelope(id) {
      return closed ? undefined : publicRetentionEnvelope(retention, id);
    },
    versions(kind, pubkey, identifier) {
      if (closed || publicRetentionSnapshot(retention).stopped) return [];
      return (
        sdk.getReplaceableHistory(kind, pubkey, identifier) ?? []
      ).flatMap((event) => {
        const proof = publicRetentionEnvelope(retention, event.id);
        return proof ? [proof] : [];
      });
    },
    retention: () => publicRetentionSnapshot(retention),
    known: (head) => publicRetentionKnown(retention, head),
    close() {
      if (closed) return;
      closed = true;
      closePublicRetention(retention);
      try {
        sdk.dispose();
      } finally {
        // Remove the owner's closures retaining the SDK and its signed data.
        // Terminal disposal is not a JavaScript secure-memory-erasure claim.
        const final = publicRetentionSnapshot(retention);
        owners.set(token, {
          closed: () => true,
          insert: () => 'closed',
          envelope: () => undefined,
          versions: () => [],
          retention: () => ({ ...final }),
          known: (head) => ({ head, requests: [] }),
          close: () => undefined
        });
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

export function publicStoreRetention(owner: PublicStore): RetentionSnapshot {
  return ownerOf(owner).retention();
}
export function publicStoreKnownEvidence(
  owner: PublicStore,
  head: PublicHead
): KnownPublicEvidence {
  return ownerOf(owner).known(head);
}
