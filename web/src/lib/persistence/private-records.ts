import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import { canonicalRelayOrigin } from '../config/relays.ts';
import type { InboxRouteSnapshot } from '../messaging/inbox-routing.ts';
import {
  LOCAL_PERSISTENCE_BUDGETS,
  PRIVATE_TRANSPORT_BUDGETS,
  PRIVATE_PUBLICATION_BUDGETS,
  RELAY_BUDGETS
} from '../config/budgets.ts';
export type PrivateSendReservation = Readonly<{
  schema: 1;
  family: 'private_send_reservation';
  owner: string;
  id: string;
  revision: 0;
  peer: string;
  rumorHash: string;
  createdAt: number;
}>;
export type PrivateCiphertextArtifact = Readonly<{
  eventId: string;
  wire: string;
}>;
export type PrivateSendOperation = Readonly<{
  schema: 1;
  family: 'private_send_operation';
  owner: string;
  id: string;
  revision: number;
  peer: string;
  rumorHash: string;
  createdAt: number;
  self: PrivateCiphertextArtifact;
  peerArtifact: PrivateCiphertextArtifact | null;
  // Absent in original self-only records; never an implicit migration or an
  // effect capability. Both ciphertext roles are required for a paired plan.
  deliveryPlan?: PrivateDeliveryPlan;
  // Optional compatibility metadata; local observations are never a sender
  // factory proof, effect permission, person delivery or remote read receipt.
  receipts?: readonly PrivateTargetReceipt[];
}>;
export type PrivateTargetReceipt = Readonly<{
  actionId: string;
  origin: string;
  role: 'peer' | 'self_archive';
  attempt: number;
  eventId: string;
  status:
    'accepted' | 'refused' | 'timed_out' | 'unknown' | 'stopped' | 'readback';
  observedAtMilliseconds: number;
  readbackWire: string | null;
}>;
export type PrivateDeliveryPlan = Readonly<{
  state: 'prepared';
  routes: InboxRouteSnapshot;
}>;
export type ReceivedEnvelopeRecord = Readonly<{
  schema: 1;
  family: 'received_envelope';
  owner: string;
  id: string;
  revision: number;
  outer: string;
  observedAtMilliseconds: number;
  sources: readonly string[];
  read: Readonly<{ rumorHash: string; atMilliseconds: number }> | null;
}>;
export type PrivateRecord =
  PrivateSendReservation | PrivateSendOperation | ReceivedEnvelopeRecord;
declare const recordBrand: unique symbol;
export type PrivateRecordHandle = Readonly<{ [recordBrand]: true }>;
const records = new WeakMap<
  PrivateRecordHandle,
  Readonly<{ owner: string; id: string; wire: string }>
>();
export type PrivateRecordFailure =
  | 'invalid_scope'
  | 'oversized'
  | 'malformed'
  | 'unsupported_schema'
  | 'owner_mismatch'
  | 'id_mismatch';
type Result =
  | Readonly<{ ok: true; value: PrivateRecordHandle }>
  | Readonly<{ ok: false; reason: PrivateRecordFailure }>;
const failed = (reason: PrivateRecordFailure): Result => ({
  ok: false,
  reason
});
const hash = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
const unsigned = (value: unknown): value is number =>
  safeUnsignedInteger(value) !== undefined;
const reservationFields = [
  'schema',
  'family',
  'owner',
  'id',
  'revision',
  'peer',
  'rumorHash',
  'createdAt'
];
function routeHead(value: unknown): boolean {
  return (
    exactLocalFields(value, ['id', 'createdAt']) &&
    hash(value.id) &&
    unsigned(value.createdAt)
  );
}
function persistedRoute(
  value: unknown,
  author: string,
  role: 'peer' | 'self_archive'
): boolean {
  if (
    !exactLocalFields(value, [
      'role',
      'author',
      'targets',
      'knownBase',
      'sources'
    ]) ||
    value.role !== role ||
    value.author !== author ||
    !Array.isArray(value.targets) ||
    value.targets.length < 1 ||
    value.targets.length > RELAY_BUDGETS.inbox ||
    new Set(value.targets).size !== value.targets.length ||
    !value.targets.every(
      (origin) =>
        typeof origin === 'string' && canonicalRelayOrigin(origin) === origin
    ) ||
    !exactLocalFields(value.knownBase, ['author', 'id', 'createdAt']) ||
    value.knownBase.author !== author ||
    !hash(value.knownBase.id) ||
    !unsigned(value.knownBase.createdAt) ||
    !Array.isArray(value.sources) ||
    value.sources.length < 1 ||
    value.sources.length > RELAY_BUDGETS.public
  )
    return false;
  const origins = new Set<string>();
  function acceptOrigin(origin: string): boolean {
    if (origins.has(origin)) return false;
    origins.add(origin);
    return true;
  }
  for (const source of value.sources) {
    if (typeof source !== 'object' || source === null || Array.isArray(source))
      return false;
    const row = source as Record<string, unknown>;
    if (
      !exactLocalFields(
        row,
        row.head === undefined
          ? ['source', 'state']
          : ['source', 'state', 'head']
      ) ||
      row.state !== 'eose' ||
      typeof row.source !== 'string' ||
      canonicalRelayOrigin(row.source) !== row.source ||
      !acceptOrigin(row.source) ||
      (row.head !== undefined && !routeHead(row.head))
    )
      return false;
  }
  return true;
}
function deliveryPlan(
  value: unknown,
  owner: string,
  peer: string
): value is PrivateDeliveryPlan {
  return (
    exactLocalFields(value, ['state', 'routes']) &&
    value.state === 'prepared' &&
    exactLocalFields(value.routes, ['peer', 'archive']) &&
    persistedRoute(value.routes.peer, peer, 'peer') &&
    persistedRoute(value.routes.archive, owner, 'self_archive')
  );
}
// JSON.parse keeps only the last duplicate key. Retaining the original outer
// bytes therefore requires a separate syntax-only duplicate check, including
// escaped key aliases and nested local metadata. JSON validity/semantics remain
// with the ordinary parser/verifier; this scanner never canonicalizes a wire.
function uniqueJSONFields(raw: string): boolean {
  type Frame = {
    array: boolean;
    key: boolean;
    parent: Frame | undefined;
    accept: (name: string) => boolean;
  };
  function frame(parent: Frame | undefined, array: boolean): Frame {
    const names = new Set<string>();
    return {
      parent,
      array,
      key: !array,
      accept: (name: string) => {
        if (names.has(name)) return false;
        names.add(name);
        return true;
      }
    };
  }
  let current: Frame | undefined;
  try {
    for (let index = 0; index < raw.length; index++) {
      const character = raw[index];
      if (character === '"') {
        let end = index + 1;
        while (end < raw.length) {
          if (raw[end] === '\\') {
            end += 2;
            continue;
          }
          if (raw[end] === '"') break;
          end++;
        }
        if (end >= raw.length) return false;
        if (current && !current.array && current.key) {
          const name: unknown = JSON.parse(raw.slice(index, end + 1));
          if (typeof name !== 'string' || !current.accept(name)) return false;
          current = {
            parent: current.parent,
            array: current.array,
            accept: current.accept,
            key: false
          };
        }
        index = end;
      } else if (character === '{' || character === '[') {
        current = frame(current, character === '[');
      } else if (character === '}' || character === ']') {
        if (!current) return false;
        current = current.parent;
      } else if (character === ',' && current && !current.array)
        current = {
          parent: current.parent,
          array: current.array,
          accept: current.accept,
          key: true
        };
    }
    return current === undefined;
  } catch {
    return false;
  }
}
function sendMetadata(
  value: Record<string, unknown>,
  owner: string,
  id: string
): boolean {
  const peer = canonicalPublicKey(value.peer);
  return (
    value.schema === 1 &&
    value.owner === owner &&
    value.id === id &&
    !!peer &&
    peer !== owner &&
    hash(value.rumorHash) &&
    unsigned(value.createdAt)
  );
}
// Preserved metadata-only reservation admission; never plaintext or crypto proof.
export function inspectPrivateSendReservation(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): PrivateSendReservation | undefined {
  const owner = canonicalPublicKey(expectedOwner),
    id = canonicalLocalId(expectedId);
  if (!owner || !id || typeof raw !== 'string' || !boundedUtf8(raw, 4096))
    return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    if (
      !exactLocalFields(value, reservationFields) ||
      !uniqueJSONFields(raw) ||
      value.family !== 'private_send_reservation' ||
      value.revision !== 0 ||
      !sendMetadata(value, owner, id)
    )
      return undefined;
    return {
      schema: 1,
      family: 'private_send_reservation',
      owner,
      id,
      revision: 0,
      peer: value.peer as string,
      rumorHash: value.rumorHash as string,
      createdAt: value.createdAt as number
    };
  } catch {
    return undefined;
  }
}
function outer(raw: unknown, destination: string): string | undefined {
  if (
    typeof raw !== 'string' ||
    !boundedUtf8(raw, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
  )
    return undefined;
  const proof = verifyEnvelope(raw);
  if (!proof.ok) return undefined;
  if (!uniqueJSONFields(raw)) return undefined;
  const event = verifiedEnvelopeSnapshot(proof.value);
  if (
    !event ||
    !exactLocalFields(event, [
      'id',
      'pubkey',
      'created_at',
      'kind',
      'tags',
      'content',
      'sig'
    ]) ||
    event.kind !== 1059 ||
    JSON.stringify(event.tags) !== JSON.stringify([['p', destination]])
  )
    return undefined;
  // Structural standard-v2 preflight only. No SDK decrypt or nested admission;
  // valid encoding/signature cannot prove this ciphertext's inner contents.
  if (
    event.content.length < 132 ||
    !/^[A-Za-z0-9+/]+={0,2}$/.test(event.content)
  )
    return undefined;
  try {
    const bytes = atob(event.content);
    if (
      bytes.length < 99 ||
      bytes.charCodeAt(0) !== 2 ||
      btoa(bytes) !== event.content
    )
      return undefined;
  } catch {
    return undefined;
  }
  return event.id;
}
function artifact(
  value: unknown,
  destination: string
): value is PrivateCiphertextArtifact {
  return (
    exactLocalFields(value, ['eventId', 'wire']) &&
    hash(value.eventId) &&
    outer(value.wire, destination) === value.eventId
  );
}
function receipt(
  value: unknown,
  self: PrivateCiphertextArtifact,
  peer: PrivateCiphertextArtifact,
  plan: PrivateDeliveryPlan
): boolean {
  if (
    !exactLocalFields(value, [
      'actionId',
      'origin',
      'role',
      'attempt',
      'eventId',
      'status',
      'observedAtMilliseconds',
      'readbackWire'
    ]) ||
    !canonicalLocalId(value.actionId) ||
    (value.role !== 'peer' && value.role !== 'self_archive') ||
    typeof value.origin !== 'string' ||
    canonicalRelayOrigin(value.origin) !== value.origin ||
    !unsigned(value.attempt) ||
    value.attempt < 1 ||
    value.attempt > PRIVATE_PUBLICATION_BUDGETS.attemptsPerTargetAction ||
    !unsigned(value.observedAtMilliseconds) ||
    typeof value.status !== 'string' ||
    ![
      'accepted',
      'refused',
      'timed_out',
      'unknown',
      'stopped',
      'readback'
    ].includes(value.status)
  )
    return false;
  const route = value.role === 'peer' ? plan.routes.peer : plan.routes.archive,
    outer = value.role === 'peer' ? peer : self;
  return (
    route.targets.includes(value.origin) &&
    value.eventId === outer.eventId &&
    (value.status === 'readback'
      ? value.readbackWire === outer.wire
      : value.readbackWire === null)
  );
}
// Strict local storage codec only, never installed-account/effect authorization,
// genuine sender factory proof, nested authenticity, read receipt or Send.
export function decodePrivateRecord(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): Result {
  const owner = canonicalPublicKey(expectedOwner);
  if (!owner || typeof expectedId !== 'string') return failed('invalid_scope');
  const id =
    canonicalLocalId(expectedId) ?? (hash(expectedId) ? expectedId : undefined);
  if (!id) return failed('invalid_scope');
  if (typeof raw !== 'string') return failed('malformed');
  if (!boundedUtf8(raw, LOCAL_PERSISTENCE_BUDGETS.privateSendBytes))
    return failed('oversized');
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null || Array.isArray(value))
      return failed('malformed');
    const fields = value as Record<string, unknown>;
    if (!uniqueJSONFields(raw)) return failed('malformed');
    if (fields.schema !== 1) return failed('unsupported_schema');
    if (fields.owner !== owner) return failed('owner_mismatch');
    if (fields.id !== id) return failed('id_mismatch');
    let record: PrivateRecord;
    if (fields.family === 'private_send_reservation') {
      const reservation = inspectPrivateSendReservation(raw, owner, id);
      if (!reservation) return failed('malformed');
      record = reservation;
    } else if (fields.family === 'private_send_operation') {
      if (
        !canonicalLocalId(id) ||
        !exactLocalFields(fields, [
          ...reservationFields,
          'self',
          'peerArtifact',
          ...(fields.deliveryPlan === undefined ? [] : ['deliveryPlan']),
          ...(fields.receipts === undefined ? [] : ['receipts'])
        ]) ||
        !sendMetadata(fields, owner, id) ||
        !unsigned(fields.revision) ||
        fields.revision < 1 ||
        !artifact(fields.self, owner) ||
        (fields.peerArtifact !== null &&
          !artifact(fields.peerArtifact, fields.peer as string)) ||
        (fields.deliveryPlan !== undefined &&
          (fields.peerArtifact === null ||
            !deliveryPlan(
              fields.deliveryPlan,
              owner,
              fields.peer as string
            ))) ||
        (fields.receipts !== undefined &&
          (!Array.isArray(fields.receipts) ||
            fields.peerArtifact === null ||
            fields.deliveryPlan === undefined ||
            !deliveryPlan(fields.deliveryPlan, owner, fields.peer as string) ||
            !fields.receipts.every((value) =>
              receipt(
                value,
                fields.self as PrivateCiphertextArtifact,
                fields.peerArtifact as PrivateCiphertextArtifact,
                fields.deliveryPlan as PrivateDeliveryPlan
              )
            )))
      )
        return failed('malformed');
      record = {
        schema: 1,
        family: 'private_send_operation',
        owner,
        id,
        revision: fields.revision,
        peer: fields.peer as string,
        rumorHash: fields.rumorHash as string,
        createdAt: fields.createdAt as number,
        self: { eventId: fields.self.eventId, wire: fields.self.wire },
        peerArtifact:
          fields.peerArtifact === null
            ? null
            : {
                eventId: fields.peerArtifact.eventId,
                wire: fields.peerArtifact.wire
              },
        ...(fields.deliveryPlan === undefined
          ? {}
          : { deliveryPlan: fields.deliveryPlan }),
        ...(fields.receipts === undefined
          ? {}
          : { receipts: fields.receipts as PrivateTargetReceipt[] })
      };
    } else if (fields.family === 'received_envelope') {
      if (
        !hash(id) ||
        !exactLocalFields(fields, [
          'schema',
          'family',
          'owner',
          'id',
          'revision',
          'outer',
          'observedAtMilliseconds',
          'sources',
          'read'
        ]) ||
        !unsigned(fields.revision) ||
        outer(fields.outer, owner) !== id ||
        !unsigned(fields.observedAtMilliseconds) ||
        !Array.isArray(fields.sources) ||
        fields.sources.length === 0 ||
        fields.sources.length > RELAY_BUDGETS.inbox ||
        fields.sources.some(
          (source: unknown) => canonicalRelayOrigin(source) !== source
        ) ||
        new Set(fields.sources).size !== fields.sources.length ||
        (fields.read !== null &&
          (!exactLocalFields(fields.read, ['rumorHash', 'atMilliseconds']) ||
            !hash(fields.read.rumorHash) ||
            !unsigned(fields.read.atMilliseconds)))
      )
        return failed('malformed');
      record = {
        schema: 1,
        family: 'received_envelope',
        owner,
        id,
        revision: fields.revision,
        outer: fields.outer as string,
        observedAtMilliseconds: fields.observedAtMilliseconds,
        sources: (fields.sources as unknown[]).map(
          (source) => source as string
        ),
        read:
          fields.read === null
            ? null
            : {
                rumorHash: fields.read.rumorHash as string,
                atMilliseconds: fields.read.atMilliseconds as number
              }
      };
    } else return failed('malformed');
    const token = Object.freeze({}) as PrivateRecordHandle;
    records.set(token, { owner, id, wire: JSON.stringify(record) });
    return { ok: true, value: token };
  } catch {
    return failed('malformed');
  }
}
export function privateRecordWire(
  token: PrivateRecordHandle,
  owner: unknown,
  id: unknown
): string | undefined {
  const saved = records.get(token);
  return saved && saved.owner === owner && saved.id === id
    ? saved.wire
    : undefined;
}
export function privateRecordSnapshot(
  token: PrivateRecordHandle,
  owner: unknown,
  id: unknown
): PrivateRecord | undefined {
  const wire = privateRecordWire(token, owner, id);
  return wire === undefined ? undefined : (JSON.parse(wire) as PrivateRecord);
}
// Handle identity contains no private body; callers must still match namespace.
export function privateRecordIdentity(
  token: PrivateRecordHandle
): Readonly<{ owner: string; id: string }> | undefined {
  const saved = records.get(token);
  return saved ? { owner: saved.owner, id: saved.id } : undefined;
}
export function inspectPrivateSendRecord(
  raw: unknown,
  owner: unknown,
  id: unknown
): PrivateSendReservation | PrivateSendOperation | undefined {
  const decoded = decodePrivateRecord(raw, owner, id);
  if (!decoded.ok) return undefined;
  const value = privateRecordSnapshot(decoded.value, owner, id);
  return value?.family !== 'received_envelope' ? value : undefined;
}
