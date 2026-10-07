import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  reservedSendSnapshot,
  type ReservedSendIdentity
} from './send-identity.ts';
import {
  capturePrivateSealOperation,
  buildPrivateSeal,
  stopPrivateSeal,
  expirePrivateSealWait,
  type PrivateSealOperation
} from '../nostr/seal-builder.ts';
import { buildPrivateGiftwrap } from '../nostr/giftwrap-builder.ts';
import {
  verifyOutboundEnvelope,
  verifiedOutboundSnapshot,
  type VerifiedOutboundEnvelope
} from '../nostr/verify-outbound-envelope.ts';
declare const pairBrand: unique symbol;
export type EnvelopePreparation = Readonly<{ [pairBrand]: true }>;
type Role = 'self' | 'peer';
type Completed = Readonly<{
  operation: PrivateSealOperation;
  proof: VerifiedOutboundEnvelope;
}>;
export type EnvelopePreparationResult = Readonly<{
  status:
    | 'prepared'
    | 'complete'
    | 'busy'
    | 'invalid'
    | 'out_of_order'
    | 'stopped'
    | 'refused'
    | 'mismatch'
    | 'unavailable';
}>;
type Controller = {
  run(role: unknown, review: unknown): Promise<EnvelopePreparationResult>;
  snapshot():
    | Readonly<{
        phase: Role | 'complete';
        busy: boolean;
        self: ReturnType<typeof verifiedOutboundSnapshot>;
        peer: ReturnType<typeof verifiedOutboundSnapshot>;
      }>
    | undefined;
  proof(role: unknown): VerifiedOutboundEnvelope | undefined;
  stop(expired: boolean): void;
  close(): void;
};
const pairs = new WeakMap<EnvelopePreparation, Controller>();
// Memory-only staged crypto preparation. Ciphertext snapshots/proofs are not
// durable acknowledgement, transport permission, inbox readiness or Send.
export function captureEnvelopePreparation(
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  review: unknown
): EnvelopePreparation | undefined {
  if (review !== 'reviewed_envelope_pair') return undefined;
  const inputRecord = reservedSendSnapshot(reserved),
    ownership = identityMessagingOwnership(session);
  if (
    !inputRecord ||
    !ownership ||
    inputRecord.owner !== ownership.owner ||
    !ownership.current()
  )
    return undefined;
  const record = inputRecord;
  let retained:
    | {
        session: IdentitySession;
        reserved: ReservedSendIdentity;
        ownership: typeof ownership;
      }
    | undefined = { session, reserved, ownership };
  let self: Completed | undefined,
    peer: Completed | undefined,
    active: PrivateSealOperation | undefined;
  let busy = false,
    generation = 0,
    unsubscribe = () => {};
  function stop(expired: boolean) {
    generation++;
    if (active) {
      if (expired) expirePrivateSealWait(active);
      else stopPrivateSeal(active);
    }
    // Completed self remains current when only an incomplete peer is stopped.
    // Admission stays occupied until the actual lower job settles.
  }
  function close() {
    if (!retained) return;
    retained = undefined;
    stop(false);
    if (self) stopPrivateSeal(self.operation);
    if (peer) stopPrivateSeal(peer.operation);
    self = undefined;
    peer = undefined;
    unsubscribe();
    unsubscribe = () => {};
  }
  function current() {
    const input = retained,
      observed = input && reservedSendSnapshot(input.reserved);
    if (
      !input ||
      !input.ownership.current() ||
      !observed ||
      observed.owner !== record.owner ||
      observed.peer !== record.peer ||
      observed.id !== record.id ||
      observed.rumorHash !== record.rumorHash ||
      observed.createdAt !== record.createdAt ||
      (self && !verifiedOutboundSnapshot(self.proof)) ||
      (peer && !verifiedOutboundSnapshot(peer.proof))
    ) {
      close();
      return false;
    }
    return true;
  }
  function snapshot() {
    if (!current()) return undefined;
    return {
      phase: peer
        ? ('complete' as const)
        : self
          ? ('peer' as const)
          : ('self' as const),
      busy,
      self: self && verifiedOutboundSnapshot(self.proof),
      peer: peer && verifiedOutboundSnapshot(peer.proof)
    };
  }
  function proof(role: unknown) {
    if (!current()) return undefined;
    return role === 'self'
      ? self?.proof
      : role === 'peer'
        ? peer?.proof
        : undefined;
  }
  async function run(
    role: unknown,
    reviewed: unknown
  ): Promise<EnvelopePreparationResult> {
    if (
      reviewed !== 'reviewed_pair_role' ||
      (role !== 'self' && role !== 'peer')
    )
      return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    if (role === 'peer' && !self) return { status: 'out_of_order' };
    if ((role === 'self' && self) || (role === 'peer' && peer))
      return { status: peer ? 'complete' : 'prepared' };
    // Set admission before the first yield; repeated calls cannot queue prompts.
    busy = true;
    const attempt = generation,
      input = retained;
    let operation: PrivateSealOperation | undefined;
    const admitted = () => current() && attempt === generation;
    try {
      if (!input || !admitted()) return { status: 'stopped' };
      operation = capturePrivateSealOperation(
        input.session,
        input.reserved,
        role,
        'reviewed_private_seal'
      );
      if (!operation || !admitted()) return { status: 'stopped' };
      active = operation;
      const result = await buildPrivateSeal(operation);
      if (!admitted()) return { status: 'stopped' };
      if (result.status !== 'sealed') return { status: result.status };
      const wrap = buildPrivateGiftwrap(result.seal, 'reviewed_private_wrap');
      if (!admitted()) return { status: 'stopped' };
      if (!wrap) return { status: 'mismatch' };
      const verified = verifyOutboundEnvelope(
        input.reserved,
        wrap,
        'reviewed_outbound_layers'
      );
      const observed = verified && verifiedOutboundSnapshot(verified);
      if (!admitted()) return { status: 'stopped' };
      if (
        !verified ||
        !observed ||
        observed.command !== record.id ||
        observed.rumorHash !== record.rumorHash ||
        observed.owner !== record.owner ||
        observed.peer !== record.peer ||
        observed.role !== role ||
        observed.destination !== (role === 'self' ? record.owner : record.peer)
      )
        return { status: 'mismatch' };
      const completed = { operation, proof: verified };
      if (role === 'self') self = completed;
      else peer = completed;
      active = undefined;
      return { status: peer ? 'complete' : 'prepared' };
    } catch {
      return { status: admitted() ? 'refused' : 'stopped' };
    } finally {
      if (operation && active === operation) {
        stopPrivateSeal(operation);
        active = undefined;
      }
      busy = false;
    }
  }
  const token = Object.freeze({}) as EnvelopePreparation;
  pairs.set(token, { run, snapshot, proof, stop, close });
  unsubscribe = subscribeIdentityInvalidation(session, close);
  return current() ? token : undefined;
}
export function prepareEnvelopeRole(
  pair: EnvelopePreparation,
  role: unknown,
  review: unknown
): Promise<EnvelopePreparationResult> {
  return (
    pairs.get(pair)?.run(role, review) ?? Promise.resolve({ status: 'invalid' })
  );
}
export function envelopePreparationSnapshot(pair: EnvelopePreparation) {
  return pairs.get(pair)?.snapshot();
}
export function preparedEnvelopeProof(
  pair: EnvelopePreparation,
  role: unknown
) {
  return pairs.get(pair)?.proof(role);
}
export function stopEnvelopePreparation(pair: EnvelopePreparation): void {
  pairs.get(pair)?.stop(false);
}
export function expireEnvelopePreparationWait(pair: EnvelopePreparation): void {
  pairs.get(pair)?.stop(true);
}
export function closeEnvelopePreparation(pair: EnvelopePreparation): void {
  pairs.get(pair)?.close();
}
