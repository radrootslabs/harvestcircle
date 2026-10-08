import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { rumorPlanSnapshot, type RumorPlan } from './rumor-plan.ts';
import {
  recoveredSelfEnvelopeSnapshot,
  type RecoveredSelfEnvelope
} from '../nostr/self-recovery-reader.ts';
import {
  privateSendReservationOwner,
  reservePrivateSendReservation,
  type PrivateSendReservationRepository,
  type PrivateSendReservation,
  type ReservationFailure
} from '../persistence/private-send-reservations.ts';
declare const reservedBrand: unique symbol;
export type ReservedSendIdentity = Readonly<{ [reservedBrand]: true }>;
export type SendIdentityResult =
  | Readonly<{
      status: 'reserved' | 'existing';
      identity: ReservedSendIdentity;
    }>
  | Readonly<{ status: ReservationFailure | 'busy' }>;
type Controller = {
  record: PrivateSendReservation;
  plan: RumorPlan;
  current(): boolean;
};
const identities = new WeakMap<ReservedSendIdentity, Controller>();
// Explicit reviewed local reservation only. No plaintext persistence or SDK
// effect; later owners revalidate this identity before encryption/preparation.
export async function reserveSendIdentity(
  repository: PrivateSendReservationRepository,
  session: IdentitySession,
  commandId: unknown,
  plan: RumorPlan,
  review: unknown
): Promise<SendIdentityResult> {
  const id = canonicalLocalId(commandId),
    ownership = identityMessagingOwnership(session),
    snapshot = rumorPlanSnapshot(plan);
  if (review !== 'reviewed_private_intent' || !id) return { status: 'invalid' };
  if (!ownership || !ownership.current() || !snapshot)
    return { status: 'stopped' };
  if (
    snapshot.owner !== ownership.owner ||
    privateSendReservationOwner(repository) !== ownership.owner
  )
    return { status: 'invalid_scope' };
  const captured = {
    schema: 1 as const,
    family: 'private_send_reservation' as const,
    owner: ownership.owner,
    id,
    revision: 0 as const,
    peer: snapshot.peer,
    rumorHash: snapshot.id,
    createdAt: snapshot.createdAt
  };
  const current = () => {
    const observed = rumorPlanSnapshot(plan);
    return (
      ownership.current() &&
      observed?.owner === captured.owner &&
      observed.peer === captured.peer &&
      observed.id === captured.rumorHash
    );
  };
  try {
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return { status: 'unavailable' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + captured.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<SendIdentityResult> => {
        if (!lock) return { status: 'busy' };
        if (!current()) return { status: 'stopped' };
        const milliseconds = safeUnsignedInteger(Date.now());
        if (milliseconds === undefined) return { status: 'clock_conflict' };
        const result = await reservePrivateSendReservation(
          repository,
          JSON.stringify(captured),
          Math.floor(milliseconds / 1000),
          current
        );
        if (!result.ok) return { status: result.reason };
        if (!current()) return { status: 'stopped' };
        const identity = Object.freeze({}) as ReservedSendIdentity;
        identities.set(identity, { record: result.value, plan, current });
        return {
          status: result.state === 'created' ? 'reserved' : 'existing',
          identity
        };
      }
    );
  } catch {
    return { status: 'unavailable' };
  }
}
export function reservedSendSnapshot(
  identity: ReservedSendIdentity
): PrivateSendReservation | undefined {
  const value = identities.get(identity);
  return value?.current() ? { ...value.record } : undefined;
}
// Restore the original actual command from authenticated stored self evidence;
// never re-reserve it using the current clock or a detached caller hash.
export function restoreRecoveredSendIdentity(
  session: IdentitySession,
  recovered: RecoveredSelfEnvelope,
  plan: RumorPlan
): ReservedSendIdentity | undefined {
  const snapshot = recoveredSelfEnvelopeSnapshot(recovered),
    ownership = identityMessagingOwnership(session),
    rumor = rumorPlanSnapshot(plan);
  if (
    !snapshot ||
    !ownership?.current() ||
    !rumor ||
    ownership.owner !== snapshot.record.owner ||
    rumor.owner !== snapshot.record.owner ||
    rumor.peer !== snapshot.record.peer ||
    rumor.id !== snapshot.record.rumorHash ||
    rumor.createdAt !== snapshot.record.createdAt ||
    rumor.wire !== snapshot.rumorWire
  )
    return undefined;
  const record: PrivateSendReservation = {
    schema: 1,
    family: 'private_send_reservation',
    owner: snapshot.record.owner,
    id: snapshot.record.id,
    revision: 0,
    peer: snapshot.record.peer,
    rumorHash: snapshot.record.rumorHash,
    createdAt: snapshot.record.createdAt
  };
  const current = () => {
    const observed = rumorPlanSnapshot(plan),
      source = recoveredSelfEnvelopeSnapshot(recovered);
    return (
      ownership.current() &&
      !!source &&
      observed?.wire === source.rumorWire &&
      source.record.id === record.id &&
      source.record.owner === record.owner &&
      source.record.peer === record.peer &&
      source.record.rumorHash === record.rumorHash &&
      source.record.createdAt === record.createdAt
    );
  };
  const identity = Object.freeze({}) as ReservedSendIdentity;
  identities.set(identity, { record, plan, current });
  return current() ? identity : undefined;
}
// Internal memory-only input to future encryption. A detached wire/snapshot is
// neither reserved identity nor effect permission and cannot retarget a send.
export function reservedSendRumorWire(
  identity: ReservedSendIdentity
): string | undefined {
  const value = identities.get(identity);
  return value?.current() ? rumorPlanSnapshot(value.plan)?.wire : undefined;
}
