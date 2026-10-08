import { canonicalLocalId } from '../private-handles.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  loadPrivateRecord,
  type PrivateStorageRepository
} from '../persistence/private-storage.ts';
import { privateRecordWire } from '../persistence/private-records.ts';
import {
  captureSelfRecoveryReader,
  readSelfRecoveryEnvelope,
  recoveredSelfEnvelopeSnapshot,
  stopSelfRecoveryReader,
  expireSelfRecoveryReaderWait,
  type SelfRecoveryReader,
  type SelfRecoveryReadResult
} from '../nostr/self-recovery-reader.ts';
import {
  restoreRecoveredRumor,
  rumorPlanSnapshot,
  stopRumorPlan,
  type RumorPlan
} from './rumor-plan.ts';
import { restoreRecoveredSendIdentity } from './send-identity.ts';
import { verifyRecoveredSelfEnvelope } from '../nostr/verify-outbound-envelope.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  preparePairedDelivery,
  stopSelfRecoveryPreparation,
  type SelfRecoveryPreparation,
  type PairedDeliveryContext,
  type PairedDeliveryPreparationResult
} from './prepare-send.ts';
declare const resumeBrand: unique symbol;
export type ResumePreparation = Readonly<{ [resumeBrand]: true }>;
export type ResumePreparationResult = Readonly<{
  status:
    | SelfRecoveryReadResult['status']
    | PairedDeliveryPreparationResult['status']
    | 'already_prepared';
}>;
type Snapshot = Readonly<{
  owner: string;
  command: string;
  state:
    | 'unsaved'
    | 'recovering'
    | 'recovered'
    | 'prepared'
    | 'lost'
    | 'needs_action';
  busy: boolean;
  copy: string;
  peer?: string;
  rumorHash?: string;
  createdAt?: number;
}>;
type Controller = {
  run(
    context: PairedDeliveryContext | undefined,
    review: unknown
  ): Promise<ResumePreparationResult>;
  snapshot(): Snapshot | undefined;
  rumor(): RumorPlan | undefined;
  stop(expired: boolean): void;
};
const resumes = new WeakMap<ResumePreparation, Controller>();
// Explicit memory-only recovery workflow. Capture/render performs no IO or
// extension job; actual Resume cannot recreate lost ciphertext or send a relay
// event. Original authenticated command/hash/time survive memory cache loss.
export function captureResumePreparation(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  localId: unknown,
  review: unknown
): ResumePreparation | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_private_resume')
    return undefined;
  const inputId = canonicalLocalId(localId),
    inputOwnership = identityMessagingOwnership(session);
  if (!inputId || !inputOwnership?.current()) return undefined;
  const id = inputId,
    ownership = inputOwnership;
  let active = true,
    busy = false,
    generation = 0,
    state: Snapshot['state'] = 'unsaved',
    reader: SelfRecoveryReader | undefined,
    plan: RumorPlan | undefined,
    preparation: SelfRecoveryPreparation | undefined,
    alreadyPaired = false,
    peerOnly = false,
    originalWire: string | undefined,
    unsubscribe = () => {};
  function stop(expired: boolean) {
    if (!active) return;
    active = false;
    generation++;
    if (reader) {
      if (expired) expireSelfRecoveryReaderWait(reader);
      else stopSelfRecoveryReader(reader);
    }
    if (preparation) stopSelfRecoveryPreparation(preparation);
    if (plan) stopRumorPlan(plan);
    reader = undefined;
    preparation = undefined;
    plan = undefined;
    originalWire = undefined;
    unsubscribe();
    unsubscribe = () => {};
  }
  function current() {
    if (!active || !ownership.current() || (plan && !rumorPlanSnapshot(plan))) {
      stop(false);
      return false;
    }
    return true;
  }
  function snapshot(): Snapshot | undefined {
    if (!current()) return undefined;
    const rumor = plan && rumorPlanSnapshot(plan);
    return {
      owner: ownership.owner,
      command: id,
      state,
      busy,
      copy:
        state === 'lost'
          ? 'Encrypted recovery evidence was lost. This message cannot be silently recreated.'
          : state === 'needs_action'
            ? 'Encrypted recovery needs your review; no message was sent.'
            : rumor
              ? 'Original encrypted message recovered in this browser; no delivery is claimed.'
              : 'Resume is required to decrypt stored recovery evidence.',
      ...(rumor
        ? { peer: rumor.peer, rumorHash: rumor.id, createdAt: rumor.createdAt }
        : {})
    };
  }
  async function run(
    context: PairedDeliveryContext | undefined,
    reviewed: unknown
  ): Promise<ResumePreparationResult> {
    if (reviewed !== 'reviewed_private_resume') return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    busy = true;
    const attempt = generation,
      admitted = () => current() && generation === attempt;
    try {
      if (!plan) {
        state = 'recovering';
        reader = captureSelfRecoveryReader(
          repository,
          session,
          id,
          'reviewed_self_decryption'
        );
        if (!reader) {
          state = 'needs_action';
          return { status: 'stopped' };
        }
        const recovered = await readSelfRecoveryEnvelope(reader);
        if (!admitted()) return { status: 'stopped' };
        if (recovered.status !== 'recovered') {
          state =
            recovered.status === 'lost_evidence' ? 'lost' : 'needs_action';
          stopSelfRecoveryReader(reader);
          reader = undefined;
          return { status: recovered.status };
        }
        const source = recoveredSelfEnvelopeSnapshot(recovered.envelope);
        plan = restoreRecoveredRumor(session, recovered.envelope);
        const reserved =
            plan &&
            restoreRecoveredSendIdentity(session, recovered.envelope, plan),
          proof =
            reserved &&
            verifyRecoveredSelfEnvelope(
              reserved,
              recovered.envelope,
              'reviewed_recovered_self'
            );
        if (!source || !plan || !reserved || !proof || !admitted()) {
          stop(false);
          return { status: 'stopped' };
        }
        originalWire = source.storedWire;
        alreadyPaired =
          !!source.record.peerArtifact && !!source.record.deliveryPlan;
        peerOnly = !!source.record.peerArtifact && !source.record.deliveryPlan;
        preparation = captureSelfRecoveryPreparation(
          repository,
          session,
          reserved,
          'reviewed_self_recovery',
          proof
        );
        if (!preparation || !admitted()) {
          stop(false);
          return { status: 'stopped' };
        }
      }
      if (!preparation || !admitted()) return { status: 'stopped' };
      if (alreadyPaired || peerOnly) {
        const stored = await loadPrivateRecord(repository, 'private_sends', id);
        if (!admitted()) return { status: 'stopped' };
        if (
          !stored.ok ||
          privateRecordWire(stored.value, ownership.owner, id) !== originalWire
        ) {
          state = stored.ok
            ? 'needs_action'
            : stored.reason === 'invalid_record'
              ? 'lost'
              : 'needs_action';
          if (plan) stopRumorPlan(plan);
          plan = undefined;
          stopSelfRecoveryPreparation(preparation);
          preparation = undefined;
          if (reader) stopSelfRecoveryReader(reader);
          reader = undefined;
          return {
            status: stored.ok
              ? 'conflict'
              : stored.reason === 'invalid_record'
                ? 'lost_evidence'
                : stored.reason
          };
        }
        state = 'recovered';
        return {
          status: alreadyPaired ? 'already_prepared' : 'review_required'
        };
      }
      const saved = await prepareSelfRecovery(
        preparation,
        'reviewed_self_recovery'
      );
      if (!admitted()) return { status: 'stopped' };
      if (!['saved', 'existing', 'reconciled'].includes(saved.status)) {
        state = 'needs_action';
        return { status: saved.status };
      }
      state = 'recovered';
      if (!context) return { status: 'recovered' };
      const paired = await preparePairedDelivery(
        preparation,
        context,
        'reviewed_pair_preparation'
      );
      if (!admitted()) return { status: 'stopped' };
      state = paired.status === 'prepared' ? 'prepared' : 'needs_action';
      return { status: paired.status };
    } catch {
      state = 'needs_action';
      return { status: admitted() ? 'unavailable' : 'stopped' };
    } finally {
      busy = false;
    }
  }
  const token = Object.freeze({}) as ResumePreparation;
  resumes.set(token, {
    run,
    snapshot,
    rumor: () => (current() ? plan : undefined),
    stop
  });
  unsubscribe = subscribeIdentityInvalidation(session, () => stop(false));
  return current() ? token : undefined;
}
export function resumeEncryptedPreparation(
  token: ResumePreparation,
  context: PairedDeliveryContext | undefined,
  review: unknown
): Promise<ResumePreparationResult> {
  return (
    resumes.get(token)?.run(context, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function resumePreparationSnapshot(token: ResumePreparation) {
  return resumes.get(token)?.snapshot();
}
export function resumeRecoveredRumor(token: ResumePreparation) {
  return resumes.get(token)?.rumor();
}
export function stopResumePreparation(token: ResumePreparation): void {
  resumes.get(token)?.stop(false);
}
export function expireResumePreparationWait(token: ResumePreparation): void {
  resumes.get(token)?.stop(true);
}
