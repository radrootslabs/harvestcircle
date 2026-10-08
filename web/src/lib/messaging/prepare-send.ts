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
  captureEnvelopePreparation,
  prepareEnvelopeRole,
  preparedEnvelopeProof,
  envelopePreparationSnapshot,
  closeEnvelopePreparation,
  type EnvelopePreparation
} from './envelope-preparation.ts';
import { privateRecordSnapshot } from '../persistence/private-records.ts';
import {
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from '../persistence/private-storage.ts';
import {
  readSelfRecoveryBase,
  commitSelfRecovery,
  selfRecoveryAcknowledgementSnapshot,
  verifySelfRecoveryAcknowledgement,
  type SelfRecoveryAcknowledgement
} from '../persistence/private-sends.ts';

declare const preparationBrand: unique symbol;
export type SelfRecoveryPreparation = Readonly<{ [preparationBrand]: true }>;
export type SelfRecoveryPreparationResult = Readonly<{
  status:
    | PrivateStorageFailure
    | 'invalid'
    | 'stopped'
    | 'busy'
    | 'saved'
    | 'existing'
    | 'reconciled'
    | 'recovery_required'
    | 'refused'
    | 'mismatch'
    | 'out_of_order';
}>;
type Phase =
  'unsaved' | 'preparing' | 'saved' | 'unknown_completion' | 'needs_action';
type Snapshot = Readonly<{
  state: Phase;
  busy: boolean;
  owner: string;
  command: string;
  copy: string;
}>;
type Controller = {
  run(review: unknown): Promise<SelfRecoveryPreparationResult>;
  snapshot(): Snapshot | undefined;
  receipt(): SelfRecoveryAcknowledgement | undefined;
  peer(
    receipt: SelfRecoveryAcknowledgement
  ): Promise<EnvelopePreparation | undefined>;
  stop(): void;
};
const preparations = new WeakMap<SelfRecoveryPreparation, Controller>();

// This workflow owns durable self-first admission. Lower crypto factories stay
// memory-only foundations; their output alone cannot advance this workflow.
export function captureSelfRecoveryPreparation(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  review: unknown
): SelfRecoveryPreparation | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_self_recovery')
    return undefined;
  const recordInput = reservedSendSnapshot(reserved),
    ownershipInput = identityMessagingOwnership(session);
  if (
    !recordInput ||
    !ownershipInput?.current() ||
    ownershipInput.owner !== recordInput.owner
  )
    return undefined;
  const pairInput = captureEnvelopePreparation(
    session,
    reserved,
    'reviewed_envelope_pair'
  );
  if (!pairInput) return undefined;
  const record = recordInput,
    ownership = ownershipInput,
    pair = pairInput;
  let active = true,
    generation = 0,
    busy = false,
    phase: Phase = 'unsaved',
    acknowledgement: SelfRecoveryAcknowledgement | undefined,
    unsubscribe = () => {};
  function stop() {
    if (!active) return;
    active = false;
    generation++;
    acknowledgement = undefined;
    closeEnvelopePreparation(pair);
    unsubscribe();
    unsubscribe = () => {};
  }
  function current() {
    const observed = reservedSendSnapshot(reserved);
    if (
      !active ||
      !ownership.current() ||
      !envelopePreparationSnapshot(pair) ||
      !observed ||
      observed.owner !== record.owner ||
      observed.id !== record.id ||
      observed.peer !== record.peer ||
      observed.rumorHash !== record.rumorHash ||
      observed.createdAt !== record.createdAt
    ) {
      stop();
      return false;
    }
    return true;
  }
  function receipt() {
    return current() &&
      acknowledgement &&
      selfRecoveryAcknowledgementSnapshot(acknowledgement)
      ? acknowledgement
      : undefined;
  }
  function snapshot(): Snapshot | undefined {
    if (!current()) return undefined;
    return {
      state: phase,
      busy,
      owner: record.owner,
      command: record.id,
      copy:
        phase === 'saved' && receipt()
          ? 'Saved encrypted in this browser'
          : phase === 'preparing'
            ? 'Preparing encryption'
            : phase === 'unknown_completion'
              ? 'Encrypted save could not be confirmed. Review before continuing.'
              : phase === 'needs_action'
                ? 'Private text has no acknowledged encrypted save in this browser.'
                : 'Private text is unsaved memory.'
    };
  }
  async function peer(expected: SelfRecoveryAcknowledgement) {
    if (!current() || busy || acknowledgement !== expected || !receipt())
      return undefined;
    const attempt = generation;
    const verified = await verifySelfRecoveryAcknowledgement(
      repository,
      expected
    );
    return verified &&
      current() &&
      generation === attempt &&
      acknowledgement === expected
      ? pair
      : undefined;
  }
  async function run(
    reviewed: unknown
  ): Promise<SelfRecoveryPreparationResult> {
    if (reviewed !== 'reviewed_self_recovery') return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return { status: 'unavailable' };
    busy = true;
    const attempt = generation,
      admitted = () => current() && attempt === generation;
    try {
      return await (async (): Promise<SelfRecoveryPreparationResult> => {
        if (!admitted()) return { status: 'stopped' };
        const base = await readSelfRecoveryBase(repository, reserved);
        if (!admitted()) return { status: 'stopped' };
        if (!base.ok) {
          phase = 'needs_action';
          return { status: base.reason };
        }
        if (acknowledgement) {
          const valid = await verifySelfRecoveryAcknowledgement(
            repository,
            acknowledgement
          );
          if (!admitted()) return { status: 'stopped' };
          if (valid) {
            phase = 'saved';
            return { status: 'saved' };
          }
          acknowledgement = undefined;
          phase = 'needs_action';
          return { status: 'conflict' };
        }
        const stored = privateRecordSnapshot(
          base.value,
          record.owner,
          record.id
        );
        if (!stored) return { status: 'invalid_record' };
        if (
          stored.family === 'private_send_operation' &&
          !preparedEnvelopeProof(pair, 'self')
        ) {
          phase = 'needs_action';
          return { status: 'recovery_required' };
        }
        phase = 'preparing';
        const prepared = await prepareEnvelopeRole(
          pair,
          'self',
          'reviewed_pair_role'
        );
        if (!admitted()) return { status: 'stopped' };
        if (prepared.status !== 'prepared' && prepared.status !== 'complete') {
          phase = 'needs_action';
          return { status: prepared.status };
        }
        const proof = preparedEnvelopeProof(pair, 'self');
        if (!proof) {
          phase = 'needs_action';
          return { status: 'mismatch' };
        }
        // The lower crypto owner acquires this lock itself. Acquire it again
        // only after crypto settles, then revalidate and CAS the stored base.
        const saved = await navigator.locks.request(
          'harvestcircle:owner:' + record.owner,
          { mode: 'exclusive', ifAvailable: true },
          async (lock) => {
            if (!lock) return { status: 'busy' as const };
            if (!admitted()) return { status: 'stopped' as const };
            return await commitSelfRecovery(
              repository,
              session,
              reserved,
              proof,
              'reviewed_self_commit'
            );
          }
        );
        if (!admitted()) return { status: 'stopped' };
        if ('receipt' in saved) {
          acknowledgement = saved.receipt;
          phase = 'saved';
        } else
          phase =
            saved.status === 'unknown_completion'
              ? 'unknown_completion'
              : 'needs_action';
        return { status: saved.status };
      })();
    } catch {
      phase = 'needs_action';
      return { status: admitted() ? 'unavailable' : 'stopped' };
    } finally {
      busy = false;
    }
  }
  const token = Object.freeze({}) as SelfRecoveryPreparation;
  preparations.set(token, { run, snapshot, receipt, peer, stop });
  unsubscribe = subscribeIdentityInvalidation(session, stop);
  return current() ? token : undefined;
}
export function prepareSelfRecovery(
  token: SelfRecoveryPreparation,
  review: unknown
): Promise<SelfRecoveryPreparationResult> {
  return (
    preparations.get(token)?.run(review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function selfRecoveryPreparationSnapshot(
  token: SelfRecoveryPreparation
) {
  return preparations.get(token)?.snapshot();
}
export function preparedSelfRecovery(token: SelfRecoveryPreparation) {
  return preparations.get(token)?.receipt();
}
export function selfRecoveryPeerPreparation(
  token: SelfRecoveryPreparation,
  receipt: SelfRecoveryAcknowledgement
): Promise<EnvelopePreparation | undefined> {
  return preparations.get(token)?.peer(receipt) ?? Promise.resolve(undefined);
}
export function stopSelfRecoveryPreparation(
  token: SelfRecoveryPreparation
): void {
  preparations.get(token)?.stop();
}
