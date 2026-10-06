import { canonicalPublicKey } from '../contracts/public-key.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  exactLocalFields,
  type PublicOperationRecord,
  type PreferenceOperationRecord,
  type PublicTargetReceipt
} from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from './records.ts';

declare const artifactBrand: unique symbol;
declare const transitionBrand: unique symbol;
export type CapturedArtifact = Readonly<{ [artifactBrand]: true }>;
export type PublicOperationTransition = Readonly<{ [transitionBrand]: true }>;
export type ArtifactFailure =
  'invalid_artifact' | 'invalid_record' | 'conflict' | 'revision_exhausted';
type Result<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; reason: ArtifactFailure }>;
type Artifact = Readonly<{
  author: string;
  kind: 30402 | 5 | 10050 | 1059;
  hash: string;
  wire: string;
}>;
type Operation = PublicOperationRecord | PreferenceOperationRecord;
type Transition = Readonly<{
  owner: string;
  id: string;
  family: Operation['family'];
  kind: 30402 | 5 | 10050;
  expectedRevision: number;
  revision: number;
  baseWire: string;
  nextWire: string;
}>;
const artifacts = new WeakMap<CapturedArtifact, Artifact>();
const transitions = new WeakMap<PublicOperationTransition, Transition>();
function failed(reason: ArtifactFailure): Result<never> {
  return { ok: false, reason };
}
// SDK-verified exact envelope binding only. A1059 token proves an outer
// signature/hash, not nested Message authenticity, routing or durable storage.
// Private typed repositories own those later checks. No plaintext14/13 token.
export function bindCapturedArtifact(
  rawWire: unknown,
  expectedAuthor: unknown,
  expectedKind: unknown,
  expectedHash: unknown
): Result<CapturedArtifact> {
  const author = canonicalPublicKey(expectedAuthor);
  if (
    !author ||
    (expectedKind !== 30402 &&
      expectedKind !== 5 &&
      expectedKind !== 10050 &&
      expectedKind !== 1059) ||
    typeof expectedHash !== 'string' ||
    !/^[0-9a-f]{64}$/.test(expectedHash) ||
    typeof rawWire !== 'string'
  )
    return failed('invalid_artifact');
  const proof = verifyEnvelope(rawWire);
  if (!proof.ok) return failed('invalid_artifact');
  const event = verifiedEnvelopeSnapshot(proof.value);
  if (
    !event ||
    !exactLocalFields(event, [
      'id',
      'pubkey',
      'kind',
      'created_at',
      'tags',
      'content',
      'sig'
    ]) ||
    event.pubkey !== author ||
    event.kind !== expectedKind ||
    event.id !== expectedHash
  )
    return failed('invalid_artifact');
  const token = Object.freeze({}) as CapturedArtifact;
  artifacts.set(token, {
    author,
    kind: expectedKind,
    hash: expectedHash,
    wire: rawWire
  });
  return { ok: true, value: token };
}
export function capturedArtifactSnapshot(
  handle: CapturedArtifact
): Artifact | undefined {
  const saved = artifacts.get(handle);
  return saved ? { ...saved } : undefined;
}
function baseOf(
  handle: PublicRecordHandle,
  owner: unknown,
  id: unknown
): Readonly<{ row: Operation; wire: string }> | undefined {
  const row = publicRecordSnapshot(handle, owner, id),
    wire = publicRecordWire(handle, owner, id);
  return row && row.family !== 'public_draft' && wire
    ? { row, wire }
    : undefined;
}
function retain(
  base: Readonly<{ row: Operation; wire: string }>,
  next: Operation
): Result<PublicOperationTransition> {
  const nextWire = JSON.stringify(next);
  const decoded = decodePublicRecord(nextWire, base.row.owner, base.row.id);
  if (!decoded.ok) return failed('invalid_record');
  const token = Object.freeze({}) as PublicOperationTransition;
  transitions.set(token, {
    owner: base.row.owner,
    id: base.row.id,
    family: base.row.family,
    kind: base.row.capture.kind,
    expectedRevision: base.row.revision,
    revision: next.revision,
    baseWire: base.wire,
    nextWire
  });
  return { ok: true, value: token };
}
export function preparePublicArtifactTransition(
  record: PublicRecordHandle,
  owner: unknown,
  id: unknown,
  artifact: CapturedArtifact
): Result<PublicOperationTransition> {
  const base = baseOf(record, owner, id),
    signed = artifacts.get(artifact);
  if (!base) return failed('invalid_record');
  if (
    !signed ||
    signed.author !== base.row.owner ||
    signed.kind !== base.row.capture.kind ||
    signed.hash !== base.row.capture.hash
  )
    return failed('invalid_artifact');
  if (base.row.artifact !== null)
    return base.row.artifact.wire === signed.wire
      ? retain(base, base.row)
      : failed('conflict');
  if (base.row.revision === Number.MAX_SAFE_INTEGER)
    return failed('revision_exhausted');
  return retain(base, {
    ...base.row,
    revision: base.row.revision + 1,
    artifact: { eventId: signed.hash, wire: signed.wire }
  });
}
function sameFact(a: PublicTargetReceipt, b: PublicTargetReceipt): boolean {
  return (
    a.actionId === b.actionId &&
    a.origin === b.origin &&
    a.role === b.role &&
    a.attempt === b.attempt &&
    a.eventId === b.eventId &&
    a.status === b.status &&
    a.observedAtMilliseconds === b.observedAtMilliseconds
  );
}
// Only canonical primitive wire is acquired; defensive record decoding validates
// every receipt field, target, role, event and exact readback signature. Distinct
// status/time observations remain distinct. No EventStore seen metadata is used.
export function preparePublicReceiptTransition(
  record: PublicRecordHandle,
  owner: unknown,
  id: unknown,
  receiptWire: unknown
): Result<PublicOperationTransition> {
  const base = baseOf(record, owner, id);
  if (
    !base ||
    !base.row.artifact ||
    typeof receiptWire !== 'string' ||
    !boundedUtf8(receiptWire, LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes)
  )
    return failed('invalid_record');
  try {
    const fact: unknown = JSON.parse(receiptWire);
    if (
      JSON.stringify(fact) !== receiptWire ||
      !exactLocalFields(fact, [
        'actionId',
        'origin',
        'role',
        'attempt',
        'eventId',
        'status',
        'observedAtMilliseconds',
        'readbackWire'
      ])
    )
      return failed('invalid_record');
    // Validate the proposed fact through the existing complete typed codec before
    // comparing keys; the parsed object contains only detached JSON data.
    const checked = decodePublicRecord(
      JSON.stringify({ ...base.row, receipts: [fact] }),
      base.row.owner,
      base.row.id
    );
    if (!checked.ok) return failed('invalid_record');
    const admitted = publicRecordSnapshot(
      checked.value,
      base.row.owner,
      base.row.id
    );
    if (!admitted || admitted.family === 'public_draft')
      return failed('invalid_record');
    const nextFact = admitted.receipts[0];
    const existing = base.row.receipts.find((row) => sameFact(row, nextFact));
    if (existing)
      return JSON.stringify(existing) === JSON.stringify(nextFact)
        ? retain(base, base.row)
        : failed('conflict');
    if (base.row.revision === Number.MAX_SAFE_INTEGER)
      return failed('revision_exhausted');
    return retain(base, {
      ...base.row,
      revision: base.row.revision + 1,
      receipts: [...base.row.receipts, nextFact]
    });
  } catch {
    return failed('invalid_record');
  }
}
// Internal repository input: a detached snapshot of a genuine frozen command,
// never a signing/network capability. Scope and full-wire CAS remain in IDB.
export function publicTransitionSnapshot(
  handle: PublicOperationTransition,
  owner: unknown,
  id: unknown
): Transition | undefined {
  const saved = transitions.get(handle);
  return saved &&
    saved.owner === canonicalPublicKey(owner) &&
    saved.id === canonicalLocalId(id)
    ? { ...saved }
    : undefined;
}
// Metadata-only scope admission avoids accepting caller-owned record objects.
export function publicTransitionIdentity(
  handle: PublicOperationTransition
): Readonly<{ owner: string; id: string }> | undefined {
  const saved = transitions.get(handle);
  return saved ? { owner: saved.owner, id: saved.id } : undefined;
}
