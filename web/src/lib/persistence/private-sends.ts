import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  reservedSendSnapshot,
  type ReservedSendIdentity
} from '../messaging/send-identity.ts';
import {
  verifiedOutboundSnapshot,
  type VerifiedOutboundEnvelope
} from '../nostr/verify-outbound-envelope.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle,
  type PrivateSendReservation
} from './private-records.ts';
import {
  loadPrivateRecord,
  commitPrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure,
  type PrivateStorageResult
} from './private-storage.ts';

declare const acknowledgementBrand: unique symbol;
export type SelfRecoveryAcknowledgement = Readonly<{
  [acknowledgementBrand]: true;
}>;
type Saved = Readonly<{
  record: PrivateSendReservation;
  revision: number;
  wire: string;
  self: Readonly<{ eventId: string; wire: string }>;
  current(): boolean;
}>;
const acknowledgements = new WeakMap<SelfRecoveryAcknowledgement, Saved>();
export type SelfRecoveryCommitResult =
  | Readonly<{
      status: 'saved' | 'existing' | 'reconciled';
      receipt: SelfRecoveryAcknowledgement;
    }>
  | Readonly<{ status: PrivateStorageFailure | 'invalid' | 'stopped' }>;

// A namespace read is not a crypto proof or acknowledgement. Defensive owner,
// command and immutable-rumor matching precedes any future SDK preparation.
export async function readSelfRecoveryBase(
  repository: PrivateStorageRepository,
  reserved: ReservedSendIdentity
): Promise<PrivateStorageResult<PrivateRecordHandle>> {
  const record = reservedSendSnapshot(reserved);
  if (!record) return { ok: false, reason: 'invalid_record' };
  const loaded = await loadPrivateRecord(
    repository,
    'private_sends',
    record.id
  );
  if (!loaded.ok)
    return {
      ok: false,
      reason: loaded.reason === 'invalid_scope' ? 'unavailable' : loaded.reason
    };
  const row = privateRecordSnapshot(loaded.value, record.owner, record.id);
  if (
    !row ||
    row.family === 'received_envelope' ||
    row.peer !== record.peer ||
    row.rumorHash !== record.rumorHash ||
    row.createdAt !== record.createdAt
  )
    return { ok: false, reason: 'invalid_record' };
  if (!reservedSendSnapshot(reserved))
    return { ok: false, reason: 'invalid_record' };
  return loaded;
}

function input(
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  proof: VerifiedOutboundEnvelope
) {
  const record = reservedSendSnapshot(reserved),
    ownership = identityMessagingOwnership(session),
    observed = verifiedOutboundSnapshot(proof);
  if (
    !record ||
    !ownership?.current() ||
    !observed ||
    ownership.owner !== record.owner ||
    observed.role !== 'self' ||
    observed.destination !== record.owner ||
    observed.command !== record.id ||
    observed.owner !== record.owner ||
    observed.peer !== record.peer ||
    observed.rumorHash !== record.rumorHash
  )
    return undefined;
  const verified = verifyEnvelope(observed.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
  if (!event || event.kind !== 1059) return undefined;
  const current = () => {
    const original = reservedSendSnapshot(reserved),
      outer = verifiedOutboundSnapshot(proof);
    return (
      ownership.current() &&
      original?.owner === record.owner &&
      original.id === record.id &&
      original.peer === record.peer &&
      original.rumorHash === record.rumorHash &&
      original.createdAt === record.createdAt &&
      outer?.role === 'self' &&
      outer.destination === record.owner &&
      outer.wire === observed.wire
    );
  };
  return current()
    ? {
        record,
        current,
        ownerCurrent: () => ownership.current(),
        self: { eventId: event.id, wire: observed.wire }
      }
    : undefined;
}

// No signer/network await occurs in an IDB transaction. Only genuine current
// factory association plus acknowledged exact ciphertext readback mints this
// local fact. It grants no relay readiness, pair plan or transmission permission.
export async function commitSelfRecovery(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  proof: VerifiedOutboundEnvelope,
  review: unknown
): Promise<SelfRecoveryCommitResult> {
  if (typeof window === 'undefined' || review !== 'reviewed_self_commit')
    return { status: 'invalid' };
  const captured = input(session, reserved, proof);
  if (!captured) return { status: 'invalid' };
  try {
    const loaded = await readSelfRecoveryBase(repository, reserved);
    if (!captured.current()) return { status: 'stopped' };
    if (!loaded.ok) return { status: loaded.reason };
    const base = privateRecordSnapshot(
      loaded.value,
      captured.record.owner,
      captured.record.id
    );
    if (!base || base.family === 'received_envelope')
      return { status: 'invalid_record' };
    let expected: PrivateRecordHandle,
      status: 'saved' | 'existing' | 'reconciled';
    if (base.family === 'private_send_operation') {
      if (
        base.self.eventId !== captured.self.eventId ||
        base.self.wire !== captured.self.wire
      )
        return { status: 'conflict' };
      expected = loaded.value;
      status = 'existing';
    } else {
      const raw = JSON.stringify({
        schema: 1,
        family: 'private_send_operation',
        owner: base.owner,
        id: base.id,
        revision: base.revision + 1,
        peer: base.peer,
        rumorHash: base.rumorHash,
        createdAt: base.createdAt,
        self: { eventId: captured.self.eventId, wire: captured.self.wire },
        peerArtifact: null
      });
      const decoded = decodePrivateRecord(raw, base.owner, base.id);
      if (!decoded.ok) return { status: 'invalid_record' };
      expected = decoded.value;
      if (!captured.current()) return { status: 'stopped' };
      const committed = await commitPrivateRecord(
        repository,
        expected,
        loaded.value
      );
      if (!captured.current()) return { status: 'stopped' };
      if (!committed.ok && committed.reason !== 'unknown_completion')
        return { status: committed.reason };
      status = !committed.ok
        ? 'reconciled'
        : committed.value.state === 'existing'
          ? 'existing'
          : 'saved';
    }
    const expectedWire = privateRecordWire(
      expected,
      captured.record.owner,
      captured.record.id
    );
    const readback = await loadPrivateRecord(
      repository,
      'private_sends',
      captured.record.id
    );
    if (!captured.current()) return { status: 'stopped' };
    if (!readback.ok) return { status: 'unknown_completion' };
    const wire = privateRecordWire(
        readback.value,
        captured.record.owner,
        captured.record.id
      ),
      record = privateRecordSnapshot(
        readback.value,
        captured.record.owner,
        captured.record.id
      );
    if (
      !wire ||
      wire !== expectedWire ||
      !record ||
      record.family !== 'private_send_operation' ||
      record.self.eventId !== captured.self.eventId ||
      record.self.wire !== captured.self.wire
    )
      return { status: 'conflict' };
    const receipt = Object.freeze({}) as SelfRecoveryAcknowledgement;
    acknowledgements.set(receipt, {
      record: captured.record,
      revision: record.revision,
      wire,
      self: captured.self,
      // The acknowledged local fact survives clearing the crypto preparation.
      // Effect permission remains separately gated by the live workflow.
      current: captured.ownerCurrent
    });
    return { status, receipt };
  } catch {
    return { status: captured.current() ? 'unavailable' : 'stopped' };
  }
}

export function selfRecoveryAcknowledgementSnapshot(
  receipt: SelfRecoveryAcknowledgement
) {
  const saved = acknowledgements.get(receipt);
  return saved?.current()
    ? {
        owner: saved.record.owner,
        id: saved.record.id,
        peer: saved.record.peer,
        rumorHash: saved.record.rumorHash,
        createdAt: saved.record.createdAt,
        revision: saved.revision,
        self: { eventId: saved.self.eventId, wire: saved.self.wire },
        copy: 'Saved encrypted in this browser' as const,
        fact: 'acknowledged_local_ciphertext_only' as const
      }
    : undefined;
}

// Recheck exact local evidence before a workflow advances. This does not
// promise retention after the read; clearing/eviction and later CAS remain real.
export async function verifySelfRecoveryAcknowledgement(
  repository: PrivateStorageRepository,
  receipt: SelfRecoveryAcknowledgement
): Promise<boolean> {
  const saved = acknowledgements.get(receipt);
  if (!saved?.current()) return false;
  const loaded = await loadPrivateRecord(
    repository,
    'private_sends',
    saved.record.id
  );
  return (
    saved.current() &&
    loaded.ok &&
    privateRecordWire(loaded.value, saved.record.owner, saved.record.id) ===
      saved.wire
  );
}
