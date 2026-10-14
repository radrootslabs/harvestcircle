import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  bindFirstRumorReservation,
  rumorReservationCommand,
  rumorPlanSnapshot,
  type RumorPlan
} from './rumor-plan.ts';
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
      disposition: 'stored_original';
    }>
  | Readonly<{
      status: ReservationFailure | 'busy';
      disposition: 'not_created' | 'may_exist';
    }>;
type Controller = {
  record: PrivateSendReservation;
  plan: RumorPlan;
  current(): boolean;
};
const identities = new WeakMap<ReservedSendIdentity, Controller>();
// Metadata only, shared by every call using the same genuine plan. Retain the
// last actual storage disposition even when its plaintext capability is stopped.
const possibleOriginals = new WeakMap<RumorPlan, boolean>();
function originalMayExist(plan: RumorPlan): boolean {
  return (
    possibleOriginals.get(plan) ??
    (!rumorPlanSnapshot(plan) || rumorReservationCommand(plan) !== undefined)
  );
}
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
  if (
    snapshot &&
    rumorReservationCommand(plan) === undefined &&
    !possibleOriginals.has(plan)
  )
    possibleOriginals.set(plan, false);
  // A previous owner-locked binding may already have reached storage. Early
  // failure during its explicit reconciliation cannot certify a new draft.
  const failure = (
    status: ReservationFailure | 'busy'
  ): SendIdentityResult => ({
    status,
    disposition: originalMayExist(plan) ? 'may_exist' : 'not_created'
  });
  if (review !== 'reviewed_private_intent' || !id) return failure('invalid');
  if (!ownership?.current() || !snapshot) return failure('stopped');
  if (
    snapshot.owner !== ownership.owner ||
    privateSendReservationOwner(repository) !== ownership.owner
  )
    return failure('invalid_scope');
  const actor = ownership;
  try {
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return failure('unavailable');
    return await navigator.locks.request(
      'harvestcircle:owner:' + snapshot.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<SendIdentityResult> => {
        if (!lock) return failure('busy');
        if (!actor.current()) return failure('stopped');
        if (!rumorPlanSnapshot(plan)) return failure('stopped');
        const milliseconds = safeUnsignedInteger(Date.now());
        if (milliseconds === undefined) return failure('clock_conflict');
        const seconds = Math.floor(milliseconds / 1000);
        const previouslyPossible = originalMayExist(plan);
        const bound = bindFirstRumorReservation(plan, id, seconds);
        if (!bound)
          return failure(previouslyPossible ? 'conflict' : 'clock_conflict');
        const captured: PrivateSendReservation = {
          schema: 1,
          family: 'private_send_reservation',
          owner: bound.owner,
          id,
          revision: 0,
          peer: bound.peer,
          rumorHash: bound.id,
          createdAt: bound.createdAt
        };
        const current = () => {
          const observed = rumorPlanSnapshot(plan);
          return (
            actor.current() &&
            observed?.owner === captured.owner &&
            observed.peer === captured.peer &&
            observed.id === captured.rumorHash &&
            observed.createdAt === captured.createdAt &&
            observed.wire === bound.wire &&
            rumorReservationCommand(plan) === id
          );
        };
        if (!current()) return failure('stopped');
        possibleOriginals.set(plan, true);
        const result = await reservePrivateSendReservation(
          repository,
          JSON.stringify(captured),
          seconds,
          current
        );
        if (!result.ok) {
          possibleOriginals.set(
            plan,
            result.disposition === 'not_attempted'
              ? previouslyPossible
              : result.disposition !== 'not_created'
          );
          return failure(result.reason);
        }
        if (!current()) return failure('stopped');
        const identity = Object.freeze({}) as ReservedSendIdentity;
        identities.set(identity, { record: result.value, plan, current });
        return {
          status: result.state === 'created' ? 'reserved' : 'existing',
          disposition: 'stored_original',
          identity
        };
      }
    );
  } catch {
    return failure('unavailable');
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
