import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  reservedSendSnapshot,
  type ReservedSendIdentity
} from './send-identity.ts';
import type { VerifiedOutboundEnvelope } from '../nostr/verify-outbound-envelope.ts';
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
  inboxRoutePlanSnapshot,
  recheckInboxRoutePlan,
  type InboxRoutePlan
} from './inbox-routing.ts';
import type { InboxResolver } from './resolve-inbox.ts';
import type { RelayPolicy } from '../config/relays.ts';
import {
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from '../persistence/private-storage.ts';
import {
  readSelfRecoveryBase,
  commitSelfRecovery,
  selfRecoveryAcknowledgementSnapshot,
  verifySelfRecoveryAcknowledgement,
  commitPairedDelivery,
  pairedDeliveryAcknowledgementSnapshot,
  verifyPairedDeliveryAcknowledgement,
  type SelfRecoveryAcknowledgement,
  type PairedDeliveryAcknowledgement
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
export type PairedDeliveryContext = Readonly<{
  plan: InboxRoutePlan;
  policy: RelayPolicy;
  own: InboxResolver;
  other: InboxResolver;
}>;
export type PairedDeliveryPreparationResult = Readonly<{
  status:
    | SelfRecoveryPreparationResult['status']
    | 'self_required'
    | 'review_required'
    | 'prepared';
}>;
type Phase =
  'unsaved' | 'preparing' | 'saved' | 'unknown_completion' | 'needs_action';
type PairPhase =
  | 'unprepared'
  | 'preparing'
  | 'prepared'
  | 'unknown_completion'
  | 'needs_action';
type Snapshot = Readonly<{
  state: Phase;
  busy: boolean;
  owner: string;
  command: string;
  copy: string;
  pairState: PairPhase;
  pairCopy: string;
}>;
type Controller = {
  run(review: unknown): Promise<SelfRecoveryPreparationResult>;
  runPair(
    context: PairedDeliveryContext,
    review: unknown
  ): Promise<PairedDeliveryPreparationResult>;
  pairReceipt(): PairedDeliveryAcknowledgement | undefined;
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
  review: unknown,
  recoveredSelf?: VerifiedOutboundEnvelope
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
    'reviewed_envelope_pair',
    recoveredSelf
  );
  if (!pairInput) return undefined;
  const record = recordInput,
    ownership = ownershipInput,
    pair = pairInput;
  let active = true,
    generation = 0,
    busy = false,
    phase: Phase = 'unsaved',
    pairPhase: PairPhase = 'unprepared',
    acknowledgement: SelfRecoveryAcknowledgement | undefined,
    pairedAcknowledgement: PairedDeliveryAcknowledgement | undefined,
    unsubscribe = () => {};
  function stop() {
    if (!active) return;
    active = false;
    generation++;
    acknowledgement = undefined;
    pairedAcknowledgement = undefined;
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
      pairState: pairPhase,
      pairCopy:
        pairPhase === 'prepared'
          ? 'Both encrypted artifacts and their delivery plan are saved locally. No recipient delivery is claimed.'
          : pairPhase === 'unknown_completion'
            ? 'Paired encrypted preparation could not be confirmed. Review the existing preparation before continuing.'
            : pairPhase === 'preparing'
              ? 'Preparing the missing peer envelope. No recipient delivery is claimed.'
              : pairPhase === 'needs_action'
                ? 'Paired delivery preparation needs review. The acknowledged self copy is a local recovery fact only.'
                : 'No paired delivery preparation is acknowledged.',
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
          // A genuine committed pair is the exact legitimate successor to our
          // self-only record. Reconcile through that receipt's full wire, never
          // treat arbitrary edits to a self record as acknowledged advancement.
          if (pairedAcknowledgement) {
            const paired = pairedDeliveryAcknowledgementSnapshot(
                pairedAcknowledgement
              ),
              originalSelf =
                selfRecoveryAcknowledgementSnapshot(acknowledgement);
            const matches =
              paired &&
              originalSelf &&
              paired.owner === record.owner &&
              paired.id === record.id &&
              paired.peer === record.peer &&
              paired.rumorHash === record.rumorHash &&
              paired.createdAt === record.createdAt &&
              paired.self.eventId === originalSelf.self.eventId &&
              paired.self.wire === originalSelf.self.wire;
            const valid =
              matches &&
              (await verifyPairedDeliveryAcknowledgement(
                repository,
                pairedAcknowledgement
              ));
            if (!admitted()) return { status: 'stopped' };
            if (valid) {
              phase = 'saved';
              return { status: 'saved' };
            }
            pairedAcknowledgement = undefined;
            acknowledgement = undefined;
            phase = 'needs_action';
            return { status: 'conflict' };
          }
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
  function pairReceipt() {
    return current() &&
      pairedAcknowledgement &&
      pairedDeliveryAcknowledgementSnapshot(pairedAcknowledgement)
      ? pairedAcknowledgement
      : undefined;
  }
  async function runPair(
    context: PairedDeliveryContext,
    reviewed: unknown
  ): Promise<PairedDeliveryPreparationResult> {
    if (reviewed !== 'reviewed_pair_preparation') return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    const self = receipt();
    if (!self) return { status: 'self_required' };
    busy = true;
    const attempt = generation;
    function finishPair(
      result: PairedDeliveryPreparationResult
    ): PairedDeliveryPreparationResult {
      if (current() && attempt === generation)
        pairPhase =
          result.status === 'unknown_completion'
            ? 'unknown_completion'
            : (result.status === 'prepared' ||
                  result.status === 'existing' ||
                  result.status === 'reconciled') &&
                pairedAcknowledgement &&
                pairedDeliveryAcknowledgementSnapshot(pairedAcknowledgement)
              ? 'prepared'
              : 'needs_action';
      return result;
    }
    try {
      const { plan, policy, own, other } = context,
        routes = inboxRoutePlanSnapshot(plan);
      if (
        !routes ||
        routes.peer.author !== record.peer ||
        routes.archive.author !== record.owner
      )
        return finishPair({ status: 'invalid' });
      const routeWire = JSON.stringify(routes);
      const routesCurrent = () =>
        recheckInboxRoutePlan(plan, policy, own, other) === 'unchanged' &&
        JSON.stringify(inboxRoutePlanSnapshot(plan)) === routeWire;
      const admitted = () => current() && attempt === generation;
      if (!routesCurrent()) return finishPair({ status: 'review_required' });
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (pairedAcknowledgement) {
        const valid = await verifyPairedDeliveryAcknowledgement(
          repository,
          pairedAcknowledgement
        );
        if (!admitted()) return finishPair({ status: 'stopped' });
        if (!routesCurrent()) return finishPair({ status: 'review_required' });
        const saved = pairedDeliveryAcknowledgementSnapshot(
          pairedAcknowledgement
        );
        return finishPair(
          valid &&
            saved &&
            JSON.stringify(saved.deliveryPlan.routes) === routeWire
            ? { status: 'prepared' }
            : { status: 'conflict' }
        );
      }
      const durableSelf = await verifySelfRecoveryAcknowledgement(
        repository,
        self
      );
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (!routesCurrent()) return finishPair({ status: 'review_required' });
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (!durableSelf) return finishPair({ status: 'conflict' });
      phase = 'preparing';
      pairPhase = 'preparing';
      // The existing lower crypto factory owns the owner lock during SDK work.
      const prepared = await prepareEnvelopeRole(
        pair,
        'peer',
        'reviewed_pair_role'
      );
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (!routesCurrent()) return finishPair({ status: 'review_required' });
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (prepared.status !== 'complete')
        return finishPair({ status: prepared.status });
      const proof = preparedEnvelopeProof(pair, 'peer');
      if (!proof) return finishPair({ status: 'mismatch' });
      const saved = await navigator.locks.request(
        'harvestcircle:owner:' + record.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock) => {
          if (!lock) return { status: 'busy' as const };
          if (!routesCurrent()) return { status: 'review_required' as const };
          if (!admitted()) return { status: 'stopped' as const };
          return await commitPairedDelivery(
            repository,
            session,
            reserved,
            self,
            proof,
            plan,
            'reviewed_pair_commit'
          );
        }
      );
      if (!admitted()) return finishPair({ status: 'stopped' });
      if (!routesCurrent()) return finishPair({ status: 'review_required' });
      if (!admitted()) return finishPair({ status: 'stopped' });
      if ('receipt' in saved) pairedAcknowledgement = saved.receipt;
      phase = 'saved';
      return finishPair({ status: saved.status });
    } catch {
      return finishPair({ status: current() ? 'unavailable' : 'stopped' });
    } finally {
      busy = false;
      if (current()) phase = 'saved';
    }
  }
  const token = Object.freeze({}) as SelfRecoveryPreparation;
  preparations.set(token, {
    run,
    runPair,
    pairReceipt,
    snapshot,
    receipt,
    peer,
    stop
  });
  unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current()) stop();
  });
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
export function preparePairedDelivery(
  token: SelfRecoveryPreparation,
  context: PairedDeliveryContext,
  review: unknown
): Promise<PairedDeliveryPreparationResult> {
  return (
    preparations.get(token)?.runPair(context, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function preparedPairedDelivery(token: SelfRecoveryPreparation) {
  return preparations.get(token)?.pairReceipt();
}
