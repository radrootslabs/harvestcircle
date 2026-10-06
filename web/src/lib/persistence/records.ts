import {
  inspectLocalTemplate,
  type PublicSignedFields,
  type LocalUnsignedTemplate
} from '../nostr/local-template.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { canonicalDeletionCoordinate } from '../nostr/deletion-adapter.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot
} from '../catalog/heads.ts';
import {
  LOCAL_PERSISTENCE_BUDGETS,
  PUBLIC_INGRESS_BUDGETS,
  PUBLIC_PUBLICATION_BUDGETS,
  RELAY_BUDGETS
} from '../config/budgets.ts';
import { canonicalRelayOrigin } from '../config/relays.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import type {
  ConversationMappingRecord,
  PublicRecord
} from '../contracts/local-records.ts';

declare const recordBrand: unique symbol;
declare const mappingBrand: unique symbol;
export type PublicRecordHandle = Readonly<{ [recordBrand]: true }>;
export type ConversationMappingHandle = Readonly<{ [mappingBrand]: true }>;
type Saved = Readonly<{ owner: string; id: string; wire: string }>;
const records = new WeakMap<PublicRecordHandle, Saved>();
const mappings = new WeakMap<ConversationMappingHandle, Saved>();
export type RecordFailure =
  | 'invalid_scope'
  | 'oversized'
  | 'malformed'
  | 'unsupported_schema'
  | 'owner_mismatch'
  | 'id_mismatch';
type Result<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; reason: RecordFailure }>;
function object(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
const exact = exactLocalFields;
function hash(value: unknown): value is string {
  return typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
}
function unsigned(value: unknown): value is number {
  return safeUnsignedInteger(value) !== undefined;
}
function signed(
  wire: unknown,
  owner: string,
  kinds: readonly number[]
): PublicSignedFields | undefined {
  if (typeof wire !== 'string') return undefined;
  const verified = verifyEnvelope(wire);
  if (!verified.ok) return undefined;
  const event = verifiedEnvelopeSnapshot(verified.value);
  return event &&
    exact(event, [
      'id',
      'pubkey',
      'created_at',
      'kind',
      'tags',
      'content',
      'sig'
    ]) &&
    event.pubkey === owner &&
    kinds.includes(event.kind)
    ? event
    : undefined;
}
function draft(form: unknown): boolean {
  const keys = [
    'title',
    'description',
    'location',
    'amount',
    'currency',
    'unit',
    'quantity',
    'contactType',
    'contactValue'
  ];
  if (
    !exact(form, keys) ||
    typeof form.contactType !== 'string' ||
    !['', 'email', 'phone', 'https'].includes(form.contactType)
  )
    return false;
  let bytes = 0;
  for (const key of keys) {
    const value = form[key];
    if (
      typeof value !== 'string' ||
      !boundedUtf8(value, LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes)
    )
      return false;
    bytes += new TextEncoder().encode(value).length;
    if (bytes > LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes) return false;
  }
  // Incomplete strings remain exact; publication validation is a separate step.
  return true;
}
function capture(
  value: unknown,
  owner: string,
  preference: boolean
): Record<string, unknown> | undefined {
  if (
    !exact(value, ['kind', 'wire', 'hash', 'targets', 'policyFingerprint']) ||
    !(preference
      ? value.kind === 10050
      : value.kind === 30402 || value.kind === 5) ||
    !hash(value.hash) ||
    !hash(value.policyFingerprint) ||
    typeof value.wire !== 'string' ||
    !boundedUtf8(value.wire, PUBLIC_INGRESS_BUDGETS.eventBytes) ||
    !Array.isArray(value.targets) ||
    value.targets.length === 0 ||
    value.targets.length > RELAY_BUDGETS.public ||
    value.targets.some(
      (target: unknown) => canonicalRelayOrigin(target) !== target
    ) ||
    new Set(value.targets).size !== value.targets.length
  )
    return undefined;
  const template = inspectLocalTemplate(value.wire, owner, value.kind);
  return template?.hash === value.hash ? value : undefined;
}
function linkedSource(
  raw: unknown,
  owner: string,
  template: LocalUnsignedTemplate
): boolean {
  if (typeof raw !== 'string') return false;
  const proof = verifyEnvelope(raw);
  if (!proof.ok) return false;
  const source = createPublicHeadCandidate(proof.value);
  if (!source) return false;
  const head = publicHeadSnapshot(source);
  if (head.kind !== 30402 || head.pubkey !== owner) return false;
  if (template.kind === 30402) {
    const identifier = template.tags.find((tag) => tag[0] === 'd')?.[1] ?? '';
    const event = verifiedEnvelopeSnapshot(proof.value);
    return (
      identifier === head.identifier &&
      template.created_at > head.created_at &&
      template.tags.find((tag) => tag[0] === 'published_at')?.[1] ===
        event?.tags.find((tag) => tag[0] === 'published_at')?.[1]
    );
  }
  if (template.kind !== 5 || template.created_at < head.created_at)
    return false;
  const coordinate = `30402:${owner}:${head.identifier}`;
  let targets = 0;
  for (const tag of template.tags) {
    if (tag[0] === 'e') {
      if (tag[1]?.toLowerCase() !== head.id) return false;
      targets++;
    } else if (tag[0] === 'a') {
      if (
        typeof tag[1] !== 'string' ||
        canonicalDeletionCoordinate(tag[1]) !== coordinate
      )
        return false;
      targets++;
    }
  }
  return targets > 0;
}
function operation(row: Record<string, unknown>): boolean {
  const preference = row.family === 'preference_operation';
  const keys = [
    'schema',
    'family',
    'owner',
    'id',
    'revision',
    'source',
    'capture',
    'artifact',
    'receipts'
  ];
  if (preference) keys.push('consent');
  if (
    !exact(row, keys) ||
    (preference && row.consent !== 'explicit_review') ||
    typeof row.owner !== 'string'
  )
    return false;
  const prepared = capture(row.capture, row.owner, preference);
  if (!prepared || !object(row.source)) return false;
  const source = row.source;
  const template = inspectLocalTemplate(
    prepared.wire,
    row.owner,
    prepared.kind
  );
  if (!template) return false;
  if (preference) {
    if (
      !exact(source, ['type', 'wire']) ||
      source.type !== 'inbox_head' ||
      (source.wire !== null && !signed(source.wire, row.owner, [10050]))
    )
      return false;
  } else if (source.type === 'draft') {
    if (
      prepared.kind !== 30402 ||
      !exact(source, ['type', 'id', 'revision']) ||
      !canonicalLocalId(source.id) ||
      !unsigned(source.revision)
    )
      return false;
  } else if (
    !exact(source, ['type', 'wire']) ||
    source.type !== 'public_head' ||
    !signed(source.wire, row.owner, [30402]) ||
    !linkedSource(source.wire, row.owner, template)
  )
    return false;
  let artifact: PublicSignedFields | undefined;
  if (row.artifact !== null) {
    if (
      !exact(row.artifact, ['eventId', 'wire']) ||
      row.artifact.eventId !== prepared.hash
    )
      return false;
    artifact = signed(row.artifact.wire, row.owner, [Number(prepared.kind)]);
    if (!artifact || artifact.id !== prepared.hash) return false;
  }
  if (!Array.isArray(row.receipts) || (!artifact && row.receipts.length !== 0))
    return false;
  for (const receipt of row.receipts as unknown[]) {
    if (
      !exact(receipt, [
        'actionId',
        'origin',
        'role',
        'attempt',
        'eventId',
        'status',
        'observedAtMilliseconds',
        'readbackWire'
      ]) ||
      !canonicalLocalId(receipt.actionId) ||
      receipt.role !== (preference ? 'preference' : 'publication') ||
      !(prepared.targets as unknown[]).includes(receipt.origin) ||
      receipt.eventId !== prepared.hash ||
      !unsigned(receipt.attempt) ||
      receipt.attempt < 1 ||
      receipt.attempt > PUBLIC_PUBLICATION_BUDGETS.attemptsPerTargetAction ||
      !unsigned(receipt.observedAtMilliseconds) ||
      typeof receipt.status !== 'string' ||
      !['accepted', 'refused', 'timed_out', 'unknown', 'stopped'].includes(
        receipt.status
      )
    )
      return false;
    if (receipt.readbackWire !== null) {
      const observed = signed(receipt.readbackWire, row.owner, [
        Number(prepared.kind)
      ]);
      if (
        !artifact ||
        !observed ||
        observed.id !== artifact.id ||
        observed.sig !== artifact.sig
      )
        return false;
    }
  }
  return true;
}
function read(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): Result<Saved> {
  const owner = canonicalPublicKey(expectedOwner),
    id = canonicalLocalId(expectedId);
  if (!owner || !id) return { ok: false, reason: 'invalid_scope' };
  if (typeof raw !== 'string') return { ok: false, reason: 'malformed' };
  if (!boundedUtf8(raw, LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes))
    return { ok: false, reason: 'oversized' };
  try {
    const row: unknown = JSON.parse(raw);
    if (!object(row) || JSON.stringify(row) !== raw)
      return { ok: false, reason: 'malformed' };
    if (row.schema !== 1) return { ok: false, reason: 'unsupported_schema' };
    if (row.owner !== owner) return { ok: false, reason: 'owner_mismatch' };
    if (row.id !== id) return { ok: false, reason: 'id_mismatch' };
    return { ok: true, value: { owner, id, wire: raw } };
  } catch {
    return { ok: false, reason: 'malformed' };
  }
}
export function decodePublicRecord(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): Result<PublicRecordHandle> {
  const parsed = read(raw, expectedOwner, expectedId);
  if (!parsed.ok) return parsed;
  try {
    const row: Record<string, unknown> = JSON.parse(
      parsed.value.wire
    ) as Record<string, unknown>;
    if (!unsigned(row.revision)) return { ok: false, reason: 'malformed' };
    const valid =
      row.family === 'public_draft'
        ? exact(row, [
            'schema',
            'family',
            'owner',
            'id',
            'revision',
            'savedAtMilliseconds',
            'form'
          ]) &&
          unsigned(row.savedAtMilliseconds) &&
          draft(row.form)
        : (row.family === 'public_operation' ||
            row.family === 'preference_operation') &&
          operation(row);
    if (!valid) return { ok: false, reason: 'malformed' };
    const token = Object.freeze({}) as PublicRecordHandle;
    records.set(token, parsed.value);
    return { ok: true, value: token };
  } catch {
    return { ok: false, reason: 'malformed' };
  }
}
export function publicRecordSnapshot(
  handle: PublicRecordHandle,
  owner: unknown,
  id: unknown
): PublicRecord | undefined {
  const saved = records.get(handle);
  return saved &&
    saved.owner === canonicalPublicKey(owner) &&
    saved.id === canonicalLocalId(id)
    ? (JSON.parse(saved.wire) as PublicRecord)
    : undefined;
}
export function publicRecordWire(
  handle: PublicRecordHandle,
  owner: unknown,
  id: unknown
): string | undefined {
  const saved = records.get(handle);
  return saved &&
    saved.owner === canonicalPublicKey(owner) &&
    saved.id === canonicalLocalId(id)
    ? saved.wire
    : undefined;
}
export function decodeConversationMapping(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): Result<ConversationMappingHandle> {
  const parsed = read(raw, expectedOwner, expectedId);
  if (!parsed.ok) return parsed;
  const row: unknown = JSON.parse(parsed.value.wire);
  if (
    !exact(row, ['schema', 'family', 'owner', 'id', 'peer']) ||
    row.family !== 'conversation_handle' ||
    !canonicalPublicKey(row.peer) ||
    row.peer === row.owner
  )
    return { ok: false, reason: 'malformed' };
  const token = Object.freeze({}) as ConversationMappingHandle;
  mappings.set(token, parsed.value);
  return { ok: true, value: token };
}
export function conversationMappingSnapshot(
  handle: ConversationMappingHandle,
  owner: unknown,
  id: unknown
): ConversationMappingRecord | undefined {
  const saved = mappings.get(handle);
  return saved &&
    saved.owner === canonicalPublicKey(owner) &&
    saved.id === canonicalLocalId(id)
    ? (JSON.parse(saved.wire) as ConversationMappingRecord)
    : undefined;
}
