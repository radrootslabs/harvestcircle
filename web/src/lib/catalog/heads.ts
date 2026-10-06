import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';

declare const headBrand: unique symbol;
export type PublicHead = Readonly<{ readonly [headBrand]: true }>;
export type PublicHeadSnapshot = Readonly<{
  kind: 0 | 10050 | 30402;
  pubkey: string;
  identifier: string | null;
  id: string;
  created_at: number;
}>;
export type PublicHeadDecision =
  'applied' | 'duplicate' | 'older' | 'higher_id' | 'coordinate_mismatch';
export type PublicHeadSelection = Readonly<{
  decision: PublicHeadDecision;
  head: PublicHead;
}>;
interface HeadOwner extends PublicHeadSnapshot {
  readonly proof: VerifiedEnvelope;
}
const owners = new WeakMap<PublicHead, HeadOwner>();
function ownerOf(head: PublicHead): HeadOwner {
  const owner = owners.get(head);
  if (!owner) throw new Error('public_head_invalid');
  return owner;
}
// The frozen Lib189 raw NIP-01 selector precedes product/profile admission.
// Missing, empty, or malformed FIRST d selects the empty address. This is
// deliberately separate from the focused FoodIdentifier/naddr constraints.
export function createPublicHeadCandidate(
  proof: VerifiedEnvelope
): PublicHead | undefined {
  const event = verifiedEnvelopeSnapshot(proof);
  if (!event) return undefined;
  const kind = event.kind;
  if (kind !== 0 && kind !== 10050 && kind !== 30402) return undefined;
  const identifier =
    kind === 30402
      ? (event.tags.find((tag) => tag[0] === 'd')?.[1] ?? '')
      : null;
  const token = Object.freeze({}) as PublicHead;
  owners.set(token, {
    kind,
    pubkey: event.pubkey,
    identifier,
    id: event.id,
    created_at: event.created_at,
    proof
  });
  return token;
}
// Detached scalar metadata cannot alter the private proof or coordinate.
export function publicHeadSnapshot(head: PublicHead): PublicHeadSnapshot {
  const owner = ownerOf(head);
  return {
    kind: owner.kind,
    pubkey: owner.pubkey,
    identifier: owner.identifier,
    id: owner.id,
    created_at: owner.created_at
  };
}
export function publicHeadEnvelope(head: PublicHead): VerifiedEnvelope {
  return ownerOf(head).proof;
}
// Internal dictionary identity, never a normalized title or an on-wire a tag.
export function publicHeadKey(head: PublicHead): string {
  const owner = ownerOf(head);
  return JSON.stringify([owner.kind, owner.pubkey, owner.identifier]);
}
// Pure incremental ordering only. Display uncertainty, deletion, expiry,
// working-set retention and source resolution remain separate consumers.
export function selectPublicHead(
  current: PublicHead | undefined,
  candidate: PublicHead
): PublicHeadSelection {
  const next = ownerOf(candidate);
  if (current === undefined) return { decision: 'applied', head: candidate };
  const previous = ownerOf(current);
  if (
    next.kind !== previous.kind ||
    next.pubkey !== previous.pubkey ||
    next.identifier !== previous.identifier
  )
    return { decision: 'coordinate_mismatch', head: current };
  if (next.id === previous.id) return { decision: 'duplicate', head: current };
  if (next.created_at > previous.created_at)
    return { decision: 'applied', head: candidate };
  if (next.created_at < previous.created_at)
    return { decision: 'older', head: current };
  // Canonical lower-case hex order equals the pinned 32-byte event-ID order.
  return next.id < previous.id
    ? { decision: 'applied', head: candidate }
    : { decision: 'higher_id', head: current };
}
