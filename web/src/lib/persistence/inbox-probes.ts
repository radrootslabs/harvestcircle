import {
  inboxProbeArchiveAttemptEvidence,
  type InboxProbeArchiveAttemptResult,
  inboxProbeReadbackAttemptEvidence,
  type InboxProbeReadbackAttemptResult
} from '../nostr/private-pool.ts';
import { inboxProbePolicyMatches } from '../nostr/inbox-access.ts';
import { reconcileInboxProbeArchiveAdmission } from '../nostr/private-publisher.ts';
import {
  recoveredInboxProbeBinding,
  inboxProbeNonceVerificationEvidence,
  disposeInboxProbeNonceProof,
  disposeRecoveredInboxProbe,
  type RecoveredInboxProbe,
  type InboxProbeNonceProof,
  type RecoveredInboxProbeNonceProof
} from '../nostr/self-recovery-reader.ts';
import {
  inboxAccessConstructionBinding,
  type InboxAccessController
} from '../messaging/inbox-access-controller.ts';
import type { InboxOperatorQualification } from '../config/inbox-operators.ts';
import type { RelayPolicy } from '../config/relays.ts';
import {
  inboxProbeGiftwrapSnapshot,
  inboxProbeGiftwrapSeal,
  type InboxProbeGiftwrap
} from '../nostr/giftwrap-builder.ts';
import { inboxProbeSealIdentity } from '../nostr/seal-builder.ts';
import {
  inboxProbeSnapshot,
  inboxProbeRumorWire,
  inboxProbeReservationHandle,
  inboxProbeActionOwner,
  inboxProbeOperatorBinding,
  stopInboxProbeIdentity,
  type InboxProbeIdentity
} from '../nostr/inbox-access.ts';
import {
  privateSessionOwnership,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateStorageOwner,
  commitPrivateRecord,
  observePrivateRecord,
  privateProbeRemovalEvidence,
  type PrivateProbeRemovalProof,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordWire,
  privateRecordSnapshot,
  type PrivateAccessProbe,
  type PrivateAccessProbeReceipt,
  type PrivateRecordHandle
} from './private-records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';

declare const acknowledgementBrand: unique symbol;
export type InboxProbeAcknowledgement = Readonly<{
  [acknowledgementBrand]: true;
}>;
export type InboxProbeCommitResult =
  | Readonly<{
      status: 'prepared' | 'existing' | 'reconciled';
      acknowledgement: InboxProbeAcknowledgement;
    }>
  | Readonly<{
      status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped';
      disposition: 'original_protected';
    }>;
type Lineage = {
  repository: PrivateStorageRepository;
  session: PrivateSession;
  identity: InboxProbeIdentity | RecoveredInboxProbe;
  record(): PrivateAccessProbe;
  wire(): string;
  begin(): boolean;
  finish(): void;
  active(): boolean;
  retire(): void;
  admit(record: PrivateAccessProbe, wire: string): void;
};
function createLineage(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  identity: InboxProbeIdentity | RecoveredInboxProbe,
  originalRecord: PrivateAccessProbe,
  originalWire: string
): Lineage {
  let record = originalRecord,
    wire = originalWire,
    pending = false,
    retired = false;
  return {
    repository,
    session,
    identity,
    record: () => record,
    wire: () => wire,
    begin() {
      if (pending || retired) return false;
      pending = true;
      return true;
    },
    finish() {
      pending = false;
    },
    active: () => !retired,
    retire() {
      retired = true;
    },
    admit(next, nextWire) {
      record = next;
      wire = nextWire;
    }
  };
}
const lineages = new WeakMap<InboxProbeIdentity, Lineage>();
type Saved = Readonly<{
  repository: PrivateStorageRepository;
  session: PrivateSession;
  lineage: Lineage;
  current(): boolean;
  revoke(): void;
}> &
  (
    | Readonly<{ source: 'construction'; identity: InboxProbeIdentity }>
    | Readonly<{
        source: 'recovered';
        identity: RecoveredInboxProbe;
        access: InboxAccessController;
        policy: RelayPolicy;
        operator: InboxOperatorQualification;
      }>
  );
const acknowledgements = new WeakMap<InboxProbeAcknowledgement, Saved>();
const actions = new WeakMap<
  InboxProbeGiftwrap,
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    pending(): boolean;
  }>
>();
const refuse = (
  status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped'
): InboxProbeCommitResult => ({ status, disposition: 'original_protected' });

// This owner accepts only the original genuine construction chain. Decoding a
// row, copying a snapshot or supplying the same public key grants no authority.
function input(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  wrap: InboxProbeGiftwrap
) {
  const artifact = inboxProbeGiftwrapSnapshot(wrap),
    seal = inboxProbeGiftwrapSeal(wrap),
    identity = seal && inboxProbeSealIdentity(seal),
    reservation = identity && inboxProbeSnapshot(identity),
    handle = identity && inboxProbeReservationHandle(identity);
  if (
    !artifact ||
    !identity ||
    !reservation ||
    !handle ||
    !inboxProbeRumorWire(identity, session)
  )
    return undefined;
  const ownership = privateSessionOwnership(session);
  if (
    !ownership?.current() ||
    privateStorageOwner(repository) !== ownership.owner ||
    reservation.owner !== ownership.owner ||
    artifact.owner !== reservation.owner ||
    artifact.destination !== reservation.owner ||
    artifact.command !== reservation.id ||
    artifact.rumorHash !== reservation.rumorHash ||
    artifact.createdAt !== reservation.createdAt
  )
    return undefined;
  const verified = verifyEnvelope(artifact.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value),
    reservationWire = privateRecordWire(
      handle,
      reservation.owner,
      reservation.id
    );
  if (
    !event ||
    !reservationWire ||
    event.kind !== 1059 ||
    JSON.stringify(event.tags) !== JSON.stringify([['p', reservation.owner]])
  )
    return undefined;
  const record: PrivateAccessProbe = {
      ...reservation,
      revision: 1,
      self: { eventId: event.id, wire: artifact.wire },
      receipts: []
    },
    decoded = decodePrivateRecord(
      JSON.stringify(record),
      reservation.owner,
      reservation.id
    );
  if (!decoded.ok) return undefined;
  const wire = privateRecordWire(
    decoded.value,
    reservation.owner,
    reservation.id
  );
  if (!wire) return undefined;
  const frozenReservation = JSON.stringify(reservation),
    frozenArtifact = JSON.stringify(artifact);
  function current() {
    return (
      ownership!.current() &&
      !!inboxProbeRumorWire(identity!, session) &&
      privateStorageOwner(repository) === reservation!.owner &&
      JSON.stringify(inboxProbeSnapshot(identity!)) === frozenReservation &&
      JSON.stringify(inboxProbeGiftwrapSnapshot(wrap)) === frozenArtifact
    );
  }
  return current()
    ? {
        identity,
        reservation,
        reservationWire,
        record,
        handle: decoded.value,
        wire,
        current
      }
    : undefined;
}

// Native completion and a separate physical-wire observation are necessary.
// This local acknowledgement permits neither EVENT nor readiness or cleanup.
export async function commitInboxProbe(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  wrap: InboxProbeGiftwrap,
  review: unknown
): Promise<InboxProbeCommitResult> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_probe_commit' ||
    typeof wrap !== 'object' ||
    wrap === null
  )
    return refuse('invalid');
  const prior = actions.get(wrap);
  if (prior?.pending()) return refuse('busy');
  if (prior && (prior.repository !== repository || prior.session !== session))
    return refuse('invalid_scope');
  let pending = true;
  let captured: ReturnType<typeof input>;
  try {
    actions.set(wrap, { repository, session, pending: () => pending });
    captured = input(repository, session, wrap);
    if (!captured) return refuse('invalid');
    const original = captured;
    if (!navigator.locks?.request) return refuse('unavailable');
    return await navigator.locks.request(
      'harvestcircle:owner:' + original.reservation.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return refuse('busy');
        if (!original.current()) return refuse('stopped');
        const idle = await withIdlePrivateLocalStorage(
          session,
          async (): Promise<InboxProbeCommitResult> => {
            if (!original.current()) return refuse('stopped');
            const { owner, id } = original.reservation,
              loaded = await observePrivateRecord(
                repository,
                'private_sends',
                id
              );
            if (!original.current()) return refuse('stopped');
            if (!loaded.ok) return refuse(loaded.reason);
            const previous = lineages.get(original.identity);
            if (
              previous &&
              (previous.repository !== repository ||
                previous.session !== session)
            )
              return refuse('invalid_scope');
            const expectedWire = previous?.wire() ?? original.wire;
            let status: 'prepared' | 'existing' | 'reconciled';
            if (loaded.value.wire === expectedWire) {
              status = 'existing';
            } else {
              if (previous || loaded.value.wire !== original.reservationWire)
                return refuse('conflict');
              const committed = await commitPrivateRecord(
                repository,
                original.handle,
                loaded.value.handle
              );
              if (!original.current()) return refuse('stopped');
              if (!committed.ok && committed.reason !== 'unknown_completion')
                return refuse(committed.reason);
              status = !committed.ok
                ? 'reconciled'
                : committed.value.state === 'existing'
                  ? 'existing'
                  : 'prepared';
            }
            const readback = await observePrivateRecord(
              repository,
              'private_sends',
              id
            );
            if (!original.current()) return refuse('stopped');
            if (!readback.ok) return refuse(readback.reason);
            if (readback.value.wire !== expectedWire) return refuse('conflict');
            // The handle and physical wire have both passed complete-owner
            // validation. Their equality is checked independently of metadata
            // normalization; equivalent raw JSON changes cannot be hidden.
            if (
              privateRecordWire(readback.value.handle, owner, id) !==
              expectedWire
            )
              return refuse('conflict');
            const lineage =
              previous ??
              createLineage(
                repository,
                session,
                original.identity,
                original.record,
                original.wire
              );
            lineages.set(original.identity, lineage);
            const acknowledgement = Object.freeze(
              {}
            ) as InboxProbeAcknowledgement;
            let revoked = false;
            acknowledgements.set(acknowledgement, {
              source: 'construction',
              repository,
              session,
              identity: original.identity,
              lineage,
              current: () => !revoked && lineage.active() && original.current(),
              revoke: () => {
                revoked = true;
              }
            });
            return { status, acknowledgement };
          }
        );
        return idle.ok
          ? idle.value
          : refuse(original.current() ? 'busy' : 'stopped');
      }
    );
  } catch {
    return refuse(
      captured ? (captured.current() ? 'unavailable' : 'stopped') : 'invalid'
    );
  } finally {
    pending = false;
    if (!captured && !prior) actions.delete(wrap);
  }
}

export function inboxProbeAcknowledgementSnapshot(
  acknowledgement: InboxProbeAcknowledgement
) {
  const saved = acknowledgements.get(acknowledgement);
  return saved?.current()
    ? {
        record: JSON.parse(
          JSON.stringify(saved.lineage.record())
        ) as PrivateAccessProbe,
        fact: 'acknowledged_local_probe_ciphertext_only' as const
      }
    : undefined;
}

export async function verifyInboxProbeAcknowledgement(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  acknowledgement: InboxProbeAcknowledgement
): Promise<boolean> {
  const saved = acknowledgements.get(acknowledgement);
  if (
    !saved ||
    saved.repository !== repository ||
    saved.session !== session ||
    !saved.current()
  )
    return false;
  const readback = await observePrivateRecord(
    repository,
    'private_sends',
    saved.lineage.record().id
  );
  const valid =
    saved.current() &&
    readback.ok &&
    readback.value.wire === saved.lineage.wire() &&
    privateRecordWire(
      readback.value.handle,
      saved.lineage.record().owner,
      saved.lineage.record().id
    ) === saved.lineage.wire();
  if (!valid) saved.revoke();
  return valid;
}

// Genuine lexical associations only. Detached row fields cannot mint a binding.
export function inboxProbePublicationBinding(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  acknowledgement: InboxProbeAcknowledgement,
  policy: RelayPolicy
) {
  const saved = acknowledgements.get(acknowledgement);
  if (
    !saved ||
    saved.source !== 'construction' ||
    saved.repository !== repository ||
    saved.session !== session ||
    !saved.current() ||
    !inboxProbePolicyMatches(saved.identity, session, policy)
  )
    return undefined;
  return {
    identity: saved.identity,
    record: JSON.parse(
      JSON.stringify(saved.lineage.record())
    ) as PrivateAccessProbe,
    wire: saved.lineage.wire(),
    current: () =>
      saved.current() &&
      inboxProbePolicyMatches(saved.identity, session, policy)
  };
}

export type RecoveredInboxProbeAdoptionResult =
  | Readonly<{
      status: 'adopted' | 'existing';
      acknowledgement: InboxProbeAcknowledgement;
    }>
  | Readonly<{
      status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped';
      disposition: 'original_protected';
    }>;
function createRecoveryAdoption() {
  let pending = false,
    result: RecoveredInboxProbeAdoptionResult | undefined;
  return {
    pending: () => pending,
    result: () => result,
    begin() {
      pending = true;
    },
    finish() {
      pending = false;
    },
    admit(value: RecoveredInboxProbeAdoptionResult) {
      result = value;
      return value;
    }
  };
}
type RecoveryAdoption = ReturnType<typeof createRecoveryAdoption>;
const recoveredAdoptions = new WeakMap<RecoveredInboxProbe, RecoveryAdoption>();

// A recovered proof is not a construction identity. Its acknowledgement adopts
// only complete current native custody; old receipt labels remain unverified.
export async function adoptRecoveredInboxProbe(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  access: InboxAccessController,
  policy: RelayPolicy,
  operator: InboxOperatorQualification,
  proof: RecoveredInboxProbe,
  review: unknown
): Promise<RecoveredInboxProbeAdoptionResult> {
  const refuse = (
    status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped'
  ): RecoveredInboxProbeAdoptionResult => ({
    status,
    disposition: 'original_protected'
  });
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_original_inbox_probe_retry'
  )
    return refuse('invalid');
  let action: RecoveryAdoption | undefined,
    issued: InboxProbeAcknowledgement | undefined;
  try {
    // Authenticate the proof and all lexical associations before any passive
    // private ownership or callback-capable native/SDK admission.
    const original = recoveredInboxProbeBinding(
      proof,
      repository,
      session,
      access,
      policy,
      operator
    );
    if (!original || privateStorageOwner(repository) !== original.owner)
      return refuse('invalid_scope');
    const previous = recoveredAdoptions.get(proof);
    if (previous?.pending()) return refuse('busy');
    const previousResult = previous?.result();
    if (previousResult && !('acknowledgement' in previousResult))
      return previousResult;
    action = previous ?? createRecoveryAdoption();
    action.begin();
    recoveredAdoptions.set(proof, action);
    const known = action.result();
    const prior =
      known && 'acknowledgement' in known ? known.acknowledgement : undefined;
    const saved = prior && acknowledgements.get(prior);
    function current() {
      const now = recoveredInboxProbeBinding(
        proof,
        repository,
        session,
        access,
        policy,
        operator
      );
      return (
        !!now &&
        now.storedWire === original!.storedWire &&
        now.command === original!.command &&
        now.eventId === original!.eventId &&
        (!saved || saved.current())
      );
    }
    if (!current()) return action.admit(refuse('stopped'));
    if (!navigator.locks?.request) return action.admit(refuse('unavailable'));
    const result = await navigator.locks.request(
      'harvestcircle:owner:' + original.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<RecoveredInboxProbeAdoptionResult> => {
        if (!lock) return refuse('busy');
        if (!current()) return refuse('stopped');
        const idle = await withIdlePrivateLocalStorage(
          session,
          async (): Promise<RecoveredInboxProbeAdoptionResult> => {
            if (!current()) return refuse('stopped');
            const observed = await observePrivateRecord(
              repository,
              'private_sends',
              original.command
            );
            if (!current()) return refuse('stopped');
            if (!observed.ok) return refuse(observed.reason);
            const expected = saved?.lineage.wire() ?? original.storedWire;
            if (
              observed.value.wire !== expected ||
              privateRecordWire(
                observed.value.handle,
                original.owner,
                original.command
              ) !== expected
            ) {
              saved?.revoke();
              return refuse('conflict');
            }
            if (prior) return { status: 'existing', acknowledgement: prior };
            const record = privateRecordSnapshot(
              observed.value.handle,
              original.owner,
              original.command
            );
            if (
              !record ||
              record.family !== 'private_access_probe' ||
              !record.self ||
              record.self.eventId !== original.eventId ||
              record.rumorHash !== original.rumorHash ||
              !current()
            )
              return refuse('invalid_record');
            const lineage = createLineage(
              repository,
              session,
              proof,
              record,
              expected
            );
            let revoked = false;
            const acknowledgement = Object.freeze(
              {}
            ) as InboxProbeAcknowledgement;
            issued = acknowledgement;
            acknowledgements.set(acknowledgement, {
              source: 'recovered',
              repository,
              session,
              identity: proof,
              access,
              policy,
              operator,
              lineage,
              current: () => !revoked && lineage.active() && current(),
              revoke: () => {
                revoked = true;
              }
            });
            return { status: 'adopted', acknowledgement };
          }
        );
        return idle.ok ? idle.value : refuse(current() ? 'busy' : 'stopped');
      }
    );
    // Busy before admission can retry this same original proof. No failed
    // custody or stopped acknowledgement can be recaptured as a new lineage.
    if (result.status !== 'busy') action.admit(result);
    if (!('acknowledgement' in result) && issued)
      acknowledgements.get(issued)?.revoke();
    return result;
  } catch {
    if (issued) acknowledgements.get(issued)?.revoke();
    const result = refuse('unavailable');
    action?.admit(result);
    return result;
  } finally {
    action?.finish();
  }
}

// Separate recovered binding: unchanged original/C1 factories cannot accept it.
// This is memory lineage only; every effect must reread exact native custody.
export function recoveredInboxProbePublicationBinding(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  acknowledgement: InboxProbeAcknowledgement,
  policy: RelayPolicy
) {
  const saved = acknowledgements.get(acknowledgement);
  if (
    !saved ||
    saved.source !== 'recovered' ||
    saved.repository !== repository ||
    saved.session !== session ||
    saved.policy !== policy ||
    !saved.current()
  )
    return undefined;
  return {
    identity: saved.identity,
    access: saved.access,
    record: JSON.parse(
      JSON.stringify(saved.lineage.record())
    ) as PrivateAccessProbe,
    wire: saved.lineage.wire(),
    current: () => saved.current()
  };
}
export type InboxProbeArchiveCommitResult = Readonly<{
  status:
    | 'saved'
    | 'existing'
    | 'reconciled'
    | PrivateStorageFailure
    | 'invalid'
    | 'busy';
  disposition: 'original_protected';
}>;
const appliedArchiveEvidence = new WeakMap<
  InboxProbeArchiveAttemptResult | InboxProbeReadbackAttemptResult,
  Lineage
>();
// This query authenticates the exact result's native-applied Lineage. Labels,
// cloned receipts and merely settled network attempts cannot release a charge.
export function appliedInboxProbeArchiveAdmission(
  result: InboxProbeArchiveAttemptResult
) {
  const effect = inboxProbeArchiveAttemptEvidence(result),
    lineage = appliedArchiveEvidence.get(result);
  if (!effect || !lineage || effect.repository !== lineage.repository)
    return undefined;
  const saved = acknowledgements.get(effect.acknowledgement);
  if (!saved || saved.lineage !== lineage) return undefined;
  const record = lineage.record(),
    fact = record.receipts.find(
      (receipt) => JSON.stringify(receipt) === effect.receiptWire
    );
  if (!fact || fact.role !== 'access_archive') return undefined;
  return {
    session: lineage.session,
    command: record.id,
    actionId: fact.actionId,
    origin: fact.origin,
    eventId: fact.eventId,
    attempt: fact.attempt
  };
}
// Only actual SDK attempt observations can advance memory-only provenance.
// This bookkeeping can settle after owner loss, but cannot authorize traffic.
async function commitProbeAttempt(
  repository: PrivateStorageRepository,
  result: InboxProbeArchiveAttemptResult | InboxProbeReadbackAttemptResult,
  role: 'access_archive' | 'access_readback'
): Promise<InboxProbeArchiveCommitResult> {
  const refuse = (
    status: InboxProbeArchiveCommitResult['status']
  ): InboxProbeArchiveCommitResult => ({
    status,
    disposition: 'original_protected'
  });
  const effect =
    role === 'access_archive'
      ? inboxProbeArchiveAttemptEvidence(
          result as InboxProbeArchiveAttemptResult
        )
      : inboxProbeReadbackAttemptEvidence(
          result as InboxProbeReadbackAttemptResult
        );
  if (!effect || effect.repository !== repository) return refuse('invalid');
  const saved = acknowledgements.get(effect.acknowledgement),
    lineage = saved?.lineage;
  if (
    !saved ||
    !lineage ||
    lineage.repository !== repository ||
    privateStorageOwner(repository) !== lineage.record().owner ||
    !lineage.record().self
  )
    return refuse('invalid');
  if (!lineage.begin()) return refuse('busy');
  try {
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return refuse('unavailable');
    return await navigator.locks.request(
      'harvestcircle:owner:' + lineage.record().owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return refuse('busy');
        const owner = lineage.record().owner,
          id = lineage.record().id,
          observed = await observePrivateRecord(
            repository,
            'private_sends',
            id
          );
        if (!observed.ok) return refuse(observed.reason);
        if (
          observed.value.wire !== lineage.wire() ||
          privateRecordWire(observed.value.handle, owner, id) !== lineage.wire()
        )
          return refuse('conflict');
        if (appliedArchiveEvidence.get(result) === lineage) {
          if (role === 'access_archive')
            reconcileInboxProbeArchiveAdmission(
              result as InboxProbeArchiveAttemptResult
            );
          return refuse('existing');
        }
        if (
          effect.predecessorWire !== lineage.wire() ||
          lineage.record().revision === Number.MAX_SAFE_INTEGER
        )
          return refuse('conflict');
        const fact: unknown = JSON.parse(effect.receiptWire);
        const candidate = decodePrivateRecord(
          JSON.stringify({
            ...lineage.record(),
            revision: lineage.record().revision + 1,
            receipts: [...lineage.record().receipts, fact]
          }),
          owner,
          id
        );
        if (!candidate.ok) return refuse('invalid_record');
        const nextWire = privateRecordWire(candidate.value, owner, id),
          next = privateRecordSnapshot(candidate.value, owner, id);
        if (!nextWire || !next || next.family !== 'private_access_probe')
          return refuse('invalid_record');
        const last = next.receipts[next.receipts.length - 1];
        if (
          last.role !== role ||
          last.eventId !== lineage.record().self!.eventId ||
          !(
            role === 'access_archive'
              ? lineage.record().binding.archiveTargets
              : lineage.record().binding.readTargets
          ).includes(last.origin)
        )
          return refuse('invalid_record');
        const committed = await commitPrivateRecord(
          repository,
          candidate.value,
          observed.value.handle
        );
        if (!committed.ok && committed.reason !== 'unknown_completion')
          return refuse(committed.reason);
        const readback = await observePrivateRecord(
          repository,
          'private_sends',
          id
        );
        if (!readback.ok) return refuse('unknown_completion');
        if (
          readback.value.wire !== nextWire ||
          privateRecordWire(readback.value.handle, owner, id) !== nextWire
        )
          return refuse('conflict');
        // Only this actual append/native completion/fresh physical observation can
        // refresh live ACKs. A decoded receipt or metadata label never can.
        lineage.admit(next, nextWire);
        appliedArchiveEvidence.set(result, lineage);
        if (role === 'access_archive')
          reconcileInboxProbeArchiveAdmission(
            result as InboxProbeArchiveAttemptResult
          );
        return refuse(
          !committed.ok
            ? 'reconciled'
            : committed.value.state === 'existing'
              ? 'existing'
              : 'saved'
        );
      }
    );
  } catch {
    return refuse('unknown_completion');
  } finally {
    lineage.finish();
  }
}

export function commitInboxProbeArchiveAttempt(
  repository: PrivateStorageRepository,
  result: InboxProbeArchiveAttemptResult
): Promise<InboxProbeArchiveCommitResult> {
  return commitProbeAttempt(repository, result, 'access_archive');
}
export function commitInboxProbeReadbackAttempt(
  repository: PrivateStorageRepository,
  result: InboxProbeReadbackAttemptResult
): Promise<InboxProbeArchiveCommitResult> {
  return commitProbeAttempt(repository, result, 'access_readback');
}

declare const nonceAcknowledgementBrand: unique symbol;
export type InboxProbeNonceAcknowledgement = Readonly<{
  [nonceAcknowledgementBrand]: true;
}>;
export type InboxProbeNonceCommitResult = Readonly<{
  status:
    | 'saved'
    | 'existing'
    | 'reconciled'
    | PrivateStorageFailure
    | 'invalid'
    | 'busy'
    | 'stopped';
  disposition: 'original_protected';
  acknowledgement?: InboxProbeNonceAcknowledgement;
  provenance?: 'current' | 'historical_only';
}>;
type NonceProof = InboxProbeNonceProof | RecoveredInboxProbeNonceProof;
type NonceOperationInput = Readonly<{
  repository: PrivateStorageRepository;
  session: PrivateSession;
  policy: RelayPolicy;
  proof: NonceProof;
  acknowledgement: InboxProbeAcknowledgement;
  observation: InboxProbeReadbackAttemptResult;
  lineage: Lineage;
  baseWire: string;
  nextWire: string;
  nextRecord: PrivateAccessProbe;
  nextHandle: PrivateRecordHandle;
}>;
type NonceOperation = NonceOperationInput &
  Readonly<{
    uncertain(): boolean;
    applied(): boolean;
    markAttempted(): void;
    markDefiniteFailure(): void;
    markApplied(): void;
    acknowledgementToken(): InboxProbeNonceAcknowledgement | undefined;
    issueAcknowledgement(): InboxProbeNonceAcknowledgement;
  }>;
function createNonceOperation(input: NonceOperationInput): NonceOperation {
  let uncertain = false,
    applied = false,
    acknowledgement: InboxProbeNonceAcknowledgement | undefined;
  return {
    ...input,
    uncertain: () => uncertain,
    applied: () => applied,
    markAttempted() {
      uncertain = true;
    },
    markDefiniteFailure() {
      uncertain = false;
    },
    markApplied() {
      applied = true;
    },
    acknowledgementToken: () => acknowledgement,
    issueAcknowledgement() {
      return (acknowledgement ??= Object.freeze(
        {}
      ) as InboxProbeNonceAcknowledgement);
    }
  };
}
const nonceOperations = new WeakMap<NonceProof, NonceOperation>();
const nonceAcknowledgements = new WeakMap<
  InboxProbeNonceAcknowledgement,
  NonceOperation
>();
function currentNonceOperation(operation: NonceOperation): boolean {
  const evidence = inboxProbeNonceVerificationEvidence(
    operation.proof,
    operation.repository,
    operation.session,
    operation.policy
  );
  return (
    !!evidence &&
    evidence.acknowledgement === operation.acknowledgement &&
    evidence.observation === operation.observation &&
    evidence.current() &&
    acknowledgements.get(evidence.acknowledgement)?.lineage ===
      operation.lineage &&
    appliedArchiveEvidence.get(evidence.observation) === operation.lineage
  );
}
function nonceSuccess(
  operation: NonceOperation,
  status: 'saved' | 'existing' | 'reconciled'
): InboxProbeNonceCommitResult {
  if (!currentNonceOperation(operation))
    return {
      status,
      disposition: 'original_protected',
      provenance: 'historical_only'
    };
  let acknowledgement = operation.acknowledgementToken();
  if (!acknowledgement) {
    acknowledgement = operation.issueAcknowledgement();
    nonceAcknowledgements.set(acknowledgement, operation);
  }
  return {
    status,
    disposition: 'original_protected',
    provenance: 'current',
    acknowledgement
  };
}

// Persist a genuine local nonce observation, never a caller receipt/status.
// A source-admitted native operation may settle after loss as history only.
export async function commitInboxProbeNonceVerification(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  proof: NonceProof
): Promise<InboxProbeNonceCommitResult> {
  const refuse = (
    status: InboxProbeNonceCommitResult['status']
  ): InboxProbeNonceCommitResult => ({
    status,
    disposition: 'original_protected'
  });
  try {
    if (typeof window === 'undefined') return refuse('invalid');
    const evidence = inboxProbeNonceVerificationEvidence(
        proof,
        repository,
        session,
        policy
      ),
      saved = evidence && acknowledgements.get(evidence.acknowledgement),
      effect =
        evidence && inboxProbeReadbackAttemptEvidence(evidence.observation);
    // All strong nonce/read/ACK associations precede passive ownership or DB.
    if (
      !evidence ||
      !saved ||
      !effect ||
      effect.repository !== repository ||
      effect.acknowledgement !== evidence.acknowledgement ||
      saved.repository !== repository ||
      saved.session !== session ||
      appliedArchiveEvidence.get(evidence.observation) !== saved.lineage ||
      !evidence.current()
    )
      return refuse('invalid');
    const fact = JSON.parse(effect.receiptWire) as PrivateAccessProbeReceipt;
    const lineage = saved.lineage;
    if (
      fact.role !== 'access_readback' ||
      fact.status !== 'readback' ||
      fact.eventId !== evidence.eventId ||
      !lineage.record().self ||
      evidence.command !== lineage.record().id ||
      evidence.owner !== lineage.record().owner ||
      !Number.isSafeInteger(evidence.verifiedAtMilliseconds) ||
      evidence.verifiedAtMilliseconds < 0
    )
      return refuse('invalid');
    const original = lineage;
    if (!original.begin()) return refuse('busy');
    // Only the owner that actually acquired begin may finish this lineage.
    try {
      if (!navigator.locks?.request) return refuse('unavailable');
      return await navigator.locks.request(
        'harvestcircle:owner:' + evidence.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock): Promise<InboxProbeNonceCommitResult> => {
          if (!lock) return refuse('busy');
          if (!evidence.current()) return refuse('stopped');
          const job = await withIdlePrivateLocalStorage(
            session,
            async (): Promise<InboxProbeNonceCommitResult> => {
              if (!evidence.current()) return refuse('stopped');
              const observed = await observePrivateRecord(
                repository,
                'private_sends',
                evidence.command
              );
              if (!evidence.current()) return refuse('stopped');
              if (!observed.ok) return refuse(observed.reason);
              let operation = nonceOperations.get(proof);
              if (
                operation &&
                (operation.repository !== repository ||
                  operation.session !== session ||
                  operation.policy !== policy ||
                  operation.lineage !== original ||
                  operation.acknowledgement !== evidence.acknowledgement ||
                  operation.observation !== evidence.observation)
              )
                return refuse('invalid_scope');
              if (
                privateRecordWire(
                  observed.value.handle,
                  evidence.owner,
                  evidence.command
                ) !== observed.value.wire
              )
                return refuse('conflict');
              if (operation?.applied()) {
                if (observed.value.wire !== original.wire())
                  return refuse('conflict');
                return nonceSuccess(operation, 'existing');
              }
              if (
                operation?.uncertain() &&
                observed.value.wire === operation.nextWire
              ) {
                if (original.wire() !== operation.baseWire)
                  return refuse('conflict');
                original.admit(operation.nextRecord, operation.nextWire);
                operation.markApplied();
                return nonceSuccess(operation, 'reconciled');
              }
              if (
                observed.value.wire !== original.wire() ||
                (operation && operation.baseWire !== original.wire())
              )
                return refuse('conflict');
              if (!operation) {
                if (original.record().revision === Number.MAX_SAFE_INTEGER)
                  return refuse('invalid_record');
                const candidate = decodePrivateRecord(
                  JSON.stringify({
                    ...original.record(),
                    revision: original.record().revision + 1,
                    receipts: [
                      ...original.record().receipts,
                      {
                        ...fact,
                        status: 'nonce_verified',
                        observedAtMilliseconds: evidence.verifiedAtMilliseconds
                      }
                    ]
                  }),
                  evidence.owner,
                  evidence.command
                );
                if (!candidate.ok) return refuse('invalid_record');
                const nextWire = privateRecordWire(
                    candidate.value,
                    evidence.owner,
                    evidence.command
                  ),
                  nextRecord = privateRecordSnapshot(
                    candidate.value,
                    evidence.owner,
                    evidence.command
                  );
                if (
                  !nextWire ||
                  !nextRecord ||
                  nextRecord.family !== 'private_access_probe'
                )
                  return refuse('invalid_record');
                operation = createNonceOperation({
                  repository,
                  session,
                  policy,
                  proof,
                  acknowledgement: evidence.acknowledgement,
                  observation: evidence.observation,
                  lineage: original,
                  baseWire: original.wire(),
                  nextWire,
                  nextRecord,
                  nextHandle: candidate.value
                });
              }
              // This is the actual append-admission boundary. Before it, stale
              // proof has no write. After it, only bounded history may settle.
              if (!currentNonceOperation(operation)) return refuse('stopped');
              nonceOperations.set(proof, operation);
              const previousUncertainty = operation.uncertain();
              operation.markAttempted();
              const committed = await commitPrivateRecord(
                repository,
                operation.nextHandle,
                observed.value.handle
              );
              if (!committed.ok && committed.reason !== 'unknown_completion') {
                operation.markDefiniteFailure();
                return refuse(committed.reason);
              }
              if (
                committed.ok &&
                committed.value.state === 'existing' &&
                !previousUncertainty
              ) {
                // A matching handwritten successor is not our completed append.
                operation.markDefiniteFailure();
                return refuse('conflict');
              }
              const after = await observePrivateRecord(
                repository,
                'private_sends',
                evidence.command
              );
              if (!after.ok) return refuse('unknown_completion');
              if (
                after.value.wire !== operation.nextWire ||
                privateRecordWire(
                  after.value.handle,
                  evidence.owner,
                  evidence.command
                ) !== operation.nextWire
              )
                return refuse('conflict');
              original.admit(operation.nextRecord, operation.nextWire);
              operation.markApplied();
              return nonceSuccess(
                operation,
                !committed.ok
                  ? 'reconciled'
                  : committed.value.state === 'existing'
                    ? 'existing'
                    : 'saved'
              );
            }
          );
          if (!job.ok) return refuse(evidence.current() ? 'busy' : 'stopped');
          if (job.value.acknowledgement && !evidence.current())
            return {
              status: job.value.status,
              disposition: 'original_protected',
              provenance: 'historical_only'
            };
          return job.value;
        }
      );
    } finally {
      original.finish();
    }
  } catch {
    return refuse('unknown_completion');
  }
}

// Local source provenance only. Later cleanup/readiness must independently
// reobserve native custody and all required genuine current target effects.
export function inboxProbeNonceAcknowledgementBinding(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  acknowledgement: InboxProbeNonceAcknowledgement,
  policy: RelayPolicy
) {
  const saved = nonceAcknowledgements.get(acknowledgement);
  if (
    !saved ||
    saved.repository !== repository ||
    saved.session !== session ||
    saved.policy !== policy ||
    !saved.applied() ||
    !currentNonceOperation(saved)
  )
    return undefined;
  return {
    owner: saved.lineage.record().owner,
    command: saved.lineage.record().id,
    eventId: saved.lineage.record().self!.eventId,
    probeAcknowledgement: saved.acknowledgement,
    proof: saved.proof,
    observation: saved.observation,
    record: JSON.parse(
      JSON.stringify(saved.lineage.record())
    ) as PrivateAccessProbe,
    wire: saved.lineage.wire(),
    current: () => currentNonceOperation(saved)
  };
}

declare const cleanupSelectionBrand: unique symbol;
export type InboxProbeCleanupSelection = Readonly<{
  [cleanupSelectionBrand]: true;
}>;
type CleanupSelection = Readonly<{
  repository: PrivateStorageRepository;
  session: PrivateSession;
  policy: RelayPolicy;
  operation: NonceOperation;
  access: InboxAccessController;
  operator: InboxOperatorQualification;
  nonceReadReceipt: string;
  admissions(): readonly string[];
  record: PrivateAccessProbe;
  wire: string;
  current(): boolean;
}>;
const cleanupSelections = new WeakMap<
  InboxProbeCleanupSelection,
  CleanupSelection
>();

// A positive label is not an effect. Only exact source results already applied
// to this very Lineage can resolve a historical archive attempt or qualify a
// required target. Array membership does not confer provenance.
export function captureInboxProbeCleanupSelection(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  nonce: InboxProbeNonceAcknowledgement,
  observations: unknown,
  review: unknown
): InboxProbeCleanupSelection | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_probe_cleanup_selection'
  )
    return undefined;
  try {
    const operation = nonceAcknowledgements.get(nonce);
    if (
      !operation ||
      operation.repository !== repository ||
      operation.session !== session ||
      operation.policy !== policy ||
      !operation.applied() ||
      !currentNonceOperation(operation)
    )
      return undefined;
    const saved = acknowledgements.get(operation.acknowledgement),
      record = operation.lineage.record(),
      wire = operation.lineage.wire();
    if (!saved || !record.self || !Array.isArray(observations))
      return undefined;
    // Array length does not bound a custom iterator. Inspect only a fixed
    // number of own data elements; descriptor traps may invalidate authority,
    // so the exact Lineage/nonce/C7 fence below still follows every inspection.
    function ownObservation(value: object, name: string): unknown {
      const field = Object.getOwnPropertyDescriptor(value, name);
      return field && 'value' in field ? field.value : undefined;
    }
    const length = ownObservation(observations, 'length');
    if (
      typeof length !== 'number' ||
      !Number.isSafeInteger(length) ||
      length < 1 ||
      length > record.receipts.length
    )
      return undefined;
    const positives = new Map<string, true>();
    for (let index = 0; index < length; index++) {
      const value = ownObservation(observations, String(index));
      if (
        !value ||
        typeof value !== 'object' ||
        appliedArchiveEvidence.get(value as InboxProbeArchiveAttemptResult) !==
          operation.lineage
      )
        return undefined;
      const effect =
        inboxProbeArchiveAttemptEvidence(
          value as InboxProbeArchiveAttemptResult
        ) ??
        inboxProbeReadbackAttemptEvidence(
          value as InboxProbeReadbackAttemptResult
        );
      if (
        !effect ||
        effect.repository !== repository ||
        acknowledgements.get(effect.acknowledgement)?.lineage !==
          operation.lineage
      )
        return undefined;
      positives.set(effect.receiptWire, true);
    }
    if (appliedArchiveEvidence.get(operation.observation) !== operation.lineage)
      return undefined;
    const nonceFact = operation.nextRecord.receipts.at(-1);
    if (
      !nonceFact ||
      nonceFact.status !== 'nonce_verified' ||
      !record.receipts.some(
        (fact) => JSON.stringify(fact) === JSON.stringify(nonceFact)
      )
    )
      return undefined;
    const accepted = new Map<string, number>(),
      unresolved = new Map<string, number>();
    let read = false;
    for (const [index, fact] of record.receipts.entries()) {
      const key = JSON.stringify([fact.role, fact.origin, fact.eventId]);
      if (fact.role === 'access_archive') {
        if (fact.status === 'accepted') {
          if (positives.has(JSON.stringify(fact))) accepted.set(key, index);
        } else unresolved.set(key, index);
      } else if (
        fact.status === 'readback' &&
        positives.has(JSON.stringify(fact)) &&
        inboxProbeReadbackAttemptEvidence(operation.observation)
          ?.receiptWire === JSON.stringify(fact)
      ) {
        read = true;
      }
    }
    if (
      !read ||
      record.binding.archiveTargets.some(
        (origin) =>
          !accepted.has(
            JSON.stringify(['access_archive', origin, record.self!.eventId])
          )
      )
    )
      return undefined;
    for (const [key, index] of unresolved)
      if ((accepted.get(key) ?? -1) <= index) return undefined;
    const access =
        saved.source === 'construction'
          ? inboxProbeActionOwner(saved.identity, session, policy)
          : saved.access,
      scope = access && inboxAccessConstructionBinding(access, session, policy),
      operator =
        saved.source === 'construction'
          ? inboxProbeOperatorBinding(
              saved.identity,
              repository,
              session,
              policy
            )
          : saved.operator;
    if (!access || !scope || !operator) return undefined;
    function current() {
      return (
        operation!.lineage.active() &&
        operation!.lineage.wire() === wire &&
        currentNonceOperation(operation!) &&
        scope!.current()
      );
    }
    if (!current()) return undefined;
    const token = Object.freeze({}) as InboxProbeCleanupSelection;
    cleanupSelections.set(token, {
      repository,
      session,
      policy,
      operation,
      access,
      operator,
      nonceReadReceipt: inboxProbeReadbackAttemptEvidence(
        operation.observation
      )!.receiptWire,
      admissions: () => Array.from(positives.keys()),
      record: JSON.parse(JSON.stringify(record)) as PrivateAccessProbe,
      wire,
      current
    });
    return token;
  } catch {
    return undefined;
  }
}

export function inboxProbeCleanupSelectionBinding(
  token: InboxProbeCleanupSelection,
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy
) {
  const saved = cleanupSelections.get(token);
  if (
    !saved ||
    saved.repository !== repository ||
    saved.session !== session ||
    saved.policy !== policy ||
    !saved.current()
  )
    return undefined;
  return {
    identity: saved.operation.lineage.identity,
    source: acknowledgements.get(saved.operation.acknowledgement)!.source,
    access: saved.access,
    operator: saved.operator,
    nonceReadReceipt: saved.nonceReadReceipt,
    admissions: saved.admissions,
    nonce: saved.operation.acknowledgementToken()!,
    record: JSON.parse(JSON.stringify(saved.record)) as PrivateAccessProbe,
    wire: saved.wire,
    current: saved.current,
    begin: () => saved.current() && saved.operation.lineage.begin(),
    finish: () => saved.operation.lineage.finish()
  };
}

// Native proof, not an absent ID or decoded row, permits disposal of this exact
// genuine source lineage. No controller is closed and no readiness is granted.
export function retireInboxProbeCleanupSelection(
  token: InboxProbeCleanupSelection,
  proof: PrivateProbeRemovalProof
): boolean {
  const saved = cleanupSelections.get(token);
  if (!saved || !saved.current()) return false;
  const removed = privateProbeRemovalEvidence(
    proof,
    saved.repository,
    saved.session
  );
  if (!removed || removed.selection !== token || removed.wire !== saved.wire)
    return false;
  disposeInboxProbeNonceProof(
    saved.operation.proof,
    saved.repository,
    saved.session,
    saved.policy
  );
  const origin = acknowledgements.get(saved.operation.acknowledgement)!;
  if (origin.source === 'construction') stopInboxProbeIdentity(origin.identity);
  else
    disposeRecoveredInboxProbe(
      origin.identity,
      saved.repository,
      saved.session,
      saved.policy
    );
  saved.operation.lineage.retire();
  origin.revoke();
  cleanupSelections.delete(token);
  return true;
}
