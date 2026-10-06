import { PUBLIC_RETENTION_BUDGETS } from '../config/budgets.ts';
import { isPublicEnvelopeKind } from '../nostr/public-kinds.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  admitDeletionRequest,
  deletionRequestSnapshot,
  canonicalDeletionCoordinate,
  type DeletionRequest
} from '../nostr/deletion-adapter.ts';
import {
  createPublicHeadCandidate,
  publicHeadKey,
  publicHeadEnvelope,
  selectPublicHead,
  type PublicHead
} from './heads.ts';

declare const retentionBrand: unique symbol;
export type PublicRetention = Readonly<{ readonly [retentionBrand]: true }>;
export type RetentionSnapshot = Readonly<{
  payloadBytes: number;
  events: number;
  stopped: boolean;
  closed: boolean;
  reason: undefined | 'payload_limit' | 'installation_unavailable';
}>;
export type KnownPublicEvidence = Readonly<{
  head: PublicHead;
  requests: readonly DeletionRequest[];
}>;
type Admission =
  'accepted' | 'duplicate' | 'not_public' | 'rejected' | 'limit' | 'closed';
interface Owner {
  readonly admit: (
    proof: VerifiedEnvelope,
    install: () => boolean
  ) => Admission;
  readonly snapshot: () => RetentionSnapshot;
  readonly envelope: (id: string) => VerifiedEnvelope | undefined;
  readonly known: (head: PublicHead) => KnownPublicEvidence;
  readonly close: () => void;
}
const owners = new WeakMap<PublicRetention, Owner>();
function ownerOf(token: PublicRetention): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('public_retention_invalid');
  return owner;
}
export function createPublicRetention(): PublicRetention {
  const proofs = new Map<string, VerifiedEnvelope>(),
    heads = new Map<string, PublicHead>(),
    exact = new Map<string, DeletionRequest>(),
    addresses = new Map<string, DeletionRequest>();
  let bytes = 0,
    closed = false,
    reason: RetentionSnapshot['reason'];
  const key = (author: string, target: string) =>
    JSON.stringify([author, target]);
  const owner: Owner = {
    admit(proof, install) {
      if (closed) return 'closed';
      if (reason) return 'limit';
      const event = verifiedEnvelopeSnapshot(proof);
      if (!event) return 'rejected';
      if (!isPublicEnvelopeKind(event.kind)) return 'not_public';
      if (proofs.has(event.id)) return 'duplicate';
      // Logical serialized public payload, not a JS heap or transport bound.
      const charge = new TextEncoder().encode(JSON.stringify(event)).length;
      if (charge > PUBLIC_RETENTION_BUDGETS.payloadBytes - bytes) {
        reason = 'payload_limit';
        return 'limit';
      }
      // Reserve evidence before a trusted SDK installer can synchronously reenter.
      proofs.set(event.id, proof);
      bytes += charge;
      const candidate = createPublicHeadCandidate(proof);
      if (candidate) {
        const coordinate = publicHeadKey(candidate);
        heads.set(
          coordinate,
          selectPublicHead(heads.get(coordinate), candidate).head
        );
      }
      if (event.kind === 5) {
        const admitted = admitDeletionRequest(proof);
        if (admitted.ok) {
          const request = deletionRequestSnapshot(admitted.value)!;
          for (const target of request.eventTargets) {
            const k = key(request.pubkey, target.eventId),
              previous = exact.get(k);
            if (!previous || request.id < deletionRequestSnapshot(previous)!.id)
              exact.set(k, admitted.value);
          }
          for (const target of request.addressTargets) {
            const k = key(request.pubkey, target.coordinate),
              previous = addresses.get(k),
              row = previous && deletionRequestSnapshot(previous);
            if (
              !row ||
              request.created_at > row.created_at ||
              (request.created_at === row.created_at && request.id < row.id)
            )
              addresses.set(k, admitted.value);
          }
        }
      }
      try {
        if (!install()) {
          reason = 'installation_unavailable';
          return closed ? 'closed' : 'rejected';
        }
      } catch {
        reason = 'installation_unavailable';
        return closed ? 'closed' : 'rejected';
      }
      return closed ? 'closed' : reason ? 'limit' : 'accepted';
    },
    snapshot: () => ({
      payloadBytes: bytes,
      events: proofs.size,
      stopped: reason !== undefined,
      closed,
      reason
    }),
    envelope: (id) => (closed ? undefined : proofs.get(id)),
    known(input) {
      const head = selectPublicHead(
        input,
        heads.get(publicHeadKey(input)) ?? input
      ).head;
      const event = verifiedEnvelopeSnapshot(publicHeadEnvelope(head))!;
      const identifier =
        event.kind === 30402 ? event.tags.find((t) => t[0] === 'd')?.[1] : '';
      const coordinate =
        identifier === undefined
          ? undefined
          : canonicalDeletionCoordinate(
              `${event.kind}:${event.pubkey}:${identifier}`
            );
      const e = exact.get(key(event.pubkey, event.id)),
        a =
          coordinate === undefined
            ? undefined
            : addresses.get(key(event.pubkey, coordinate));
      return {
        head,
        requests: e ? [e, ...(a && a !== e ? [a] : [])] : a ? [a] : []
      };
    },
    close() {
      closed = true;
      proofs.clear();
      heads.clear();
      exact.clear();
      addresses.clear();
      bytes = 0;
    }
  };
  const token = Object.freeze({}) as PublicRetention;
  owners.set(token, owner);
  return token;
}
export function retainPublicEnvelope(
  owner: PublicRetention,
  proof: VerifiedEnvelope,
  install: () => boolean = () => true
): Admission {
  return ownerOf(owner).admit(proof, install);
}
export function publicRetentionSnapshot(
  owner: PublicRetention
): RetentionSnapshot {
  return ownerOf(owner).snapshot();
}
export function publicRetentionEnvelope(
  owner: PublicRetention,
  id: string
): VerifiedEnvelope | undefined {
  return ownerOf(owner).envelope(id);
}
export function publicRetentionKnown(
  owner: PublicRetention,
  head: PublicHead
): KnownPublicEvidence {
  return ownerOf(owner).known(head);
}
export function closePublicRetention(owner: PublicRetention): void {
  ownerOf(owner).close();
}
