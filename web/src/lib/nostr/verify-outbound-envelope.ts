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
import {
  recoveredSelfEnvelopeSnapshot,
  type RecoveredSelfEnvelope
} from './self-recovery-reader.ts';
declare const outboundBrand: unique symbol;
export type VerifiedOutboundEnvelope = Readonly<{ [outboundBrand]: true }>;
type Saved = Readonly<{
  reserved: ReservedSendIdentity;
  wrap: PrivateGiftwrap;
}>;
const proofs = new WeakMap<VerifiedOutboundEnvelope, Saved>();
const recoveredProofs = new WeakMap<
  VerifiedOutboundEnvelope,
  { reserved: ReservedSendIdentity; source: RecoveredSelfEnvelope }
>();
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
  if (saved) return inspect(saved.reserved, saved.wrap);
  const recovery = recoveredProofs.get(token);
  return recovery && inspectRecovered(recovery.reserved, recovery.source);
}
function inspectRecovered(
  reserved: ReservedSendIdentity,
  source: RecoveredSelfEnvelope
) {
  const original = recoveredSelfEnvelopeSnapshot(source),
    record = reservedSendSnapshot(reserved),
    wire = reservedSendRumorWire(reserved);
  if (
    !original ||
    !record ||
    wire !== original.rumorWire ||
    record.owner !== original.record.owner ||
    record.id !== original.record.id ||
    record.peer !== original.record.peer ||
    record.rumorHash !== original.record.rumorHash ||
    record.createdAt !== original.record.createdAt ||
    !verifyOutboundLayerData(
      wire,
      wire,
      original.sealWire,
      original.record.self.wire,
      record.owner,
      record.peer,
      'self'
    )
  )
    return undefined;
  return {
    owner: record.owner,
    peer: record.peer,
    destination: record.owner,
    role: 'self' as const,
    command: record.id,
    rumorHash: record.rumorHash,
    wire: original.record.self.wire
  };
}
export function verifyRecoveredSelfEnvelope(
  reserved: ReservedSendIdentity,
  source: RecoveredSelfEnvelope,
  review: unknown
): VerifiedOutboundEnvelope | undefined {
  if (
    review !== 'reviewed_recovered_self' ||
    !inspectRecovered(reserved, source)
  )
    return undefined;
  const token = Object.freeze({}) as VerifiedOutboundEnvelope;
  recoveredProofs.set(token, { reserved, source });
  return token;
}
// Extra full-wire custody fence for actual recovery, absent for new factories.
export function verifiedOutboundRecoverySource(
  token: VerifiedOutboundEnvelope
): string | undefined {
  const recovery = recoveredProofs.get(token);
  return recovery && inspectRecovered(recovery.reserved, recovery.source)
    ? recoveredSelfEnvelopeSnapshot(recovery.source)?.storedWire
    : undefined;
}
