import {
  reservedSendSnapshot,
  reservedSendRumorWire,
  type ReservedSendIdentity
} from '../messaging/send-identity.ts';
import {
  privateGiftwrapSnapshot,
  privateGiftwrapSeal,
  type PrivateGiftwrap
} from './giftwrap-builder.ts';
import { privateSealSnapshot, privateSealReservation } from './seal-builder.ts';
import { verifyOutboundLayerData } from './outbound-envelope-layers.ts';
declare const outboundBrand: unique symbol;
export type VerifiedOutboundEnvelope = Readonly<{ [outboundBrand]: true }>;
type Saved = Readonly<{
  reserved: ReservedSendIdentity;
  wrap: PrivateGiftwrap;
}>;
const proofs = new WeakMap<VerifiedOutboundEnvelope, Saved>();
function inspect(reserved: ReservedSendIdentity, wrap: PrivateGiftwrap) {
  const record = reservedSendSnapshot(reserved),
    rumor = reservedSendRumorWire(reserved),
    outer = privateGiftwrapSnapshot(wrap),
    seal = privateGiftwrapSeal(wrap),
    inner = seal && privateSealSnapshot(seal);
  if (
    !record ||
    !rumor ||
    !outer ||
    !seal ||
    !inner ||
    privateSealReservation(seal) !== reserved ||
    outer.command !== record.id ||
    outer.owner !== record.owner ||
    outer.peer !== record.peer ||
    outer.rumorHash !== record.rumorHash ||
    inner.command !== outer.command ||
    inner.rumorHash !== outer.rumorHash ||
    inner.role !== outer.role ||
    inner.destination !== outer.destination ||
    !verifyOutboundLayerData(
      rumor,
      rumor,
      inner.wire,
      outer.wire,
      record.owner,
      record.peer,
      outer.role
    )
  )
    return undefined;
  const original = JSON.parse(rumor) as { id: string; created_at: number };
  if (
    original.id !== record.rumorHash ||
    original.created_at !== record.createdAt
  )
    return undefined;
  return outer;
}
// Only genuine current factory roundtrip associations enter this proof. Pure
// detached validation cannot mint it. No receipt, durability or Send follows.
export function verifyOutboundEnvelope(
  reserved: ReservedSendIdentity,
  wrap: PrivateGiftwrap,
  review: unknown
): VerifiedOutboundEnvelope | undefined {
  if (review !== 'reviewed_outbound_layers' || !inspect(reserved, wrap))
    return undefined;
  const token = Object.freeze({}) as VerifiedOutboundEnvelope;
  proofs.set(token, { reserved, wrap });
  return token;
}
export function verifiedOutboundSnapshot(token: VerifiedOutboundEnvelope) {
  const saved = proofs.get(token);
  return saved ? inspect(saved.reserved, saved.wrap) : undefined;
}
