import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle,
  type PrivateTargetReceipt
} from './private-records.ts';
import {
  commitPrivateRecord,
  loadPrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
declare const transitionBrand: unique symbol;
export type PrivateReceiptTransition = Readonly<{ [transitionBrand]: true }>;
type Transition = Readonly<{
  owner: string;
  id: string;
  base: PrivateRecordHandle;
  next: PrivateRecordHandle;
  baseWire: string;
  nextWire: string;
}>;
const transitions = new WeakMap<PrivateReceiptTransition, Transition>();
type Result =
  | Readonly<{ ok: true; value: PrivateReceiptTransition }>
  | Readonly<{
      ok: false;
      reason: 'invalid_record' | 'conflict' | 'revision_exhausted';
    }>;
function sameFact(a: PrivateTargetReceipt, b: PrivateTargetReceipt) {
  return (
    a.actionId === b.actionId &&
    a.origin === b.origin &&
    a.role === b.role &&
    a.attempt === b.attempt &&
    a.eventId === b.eventId &&
    a.status === b.status
  );
}
// Primitive canonical JSON only. Strict complete private codec binds the named
// role/destination to the persisted outer ID and verifies exact readback bytes.
// This is observed local bookkeeping, not network provenance/effect authority.
export function preparePrivateReceiptTransition(
  record: PrivateRecordHandle,
  owner: unknown,
  id: unknown,
  receiptWire: unknown
): Result {
  const row = privateRecordSnapshot(record, owner, id),
    baseWire = privateRecordWire(record, owner, id);
  if (
    !row ||
    row.family !== 'private_send_operation' ||
    !row.peerArtifact ||
    !row.deliveryPlan ||
    !baseWire ||
    typeof receiptWire !== 'string' ||
    !boundedUtf8(receiptWire, LOCAL_PERSISTENCE_BUDGETS.privateSendBytes)
  )
    return { ok: false, reason: 'invalid_record' };
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
      return { ok: false, reason: 'invalid_record' };
    const checked = decodePrivateRecord(
      JSON.stringify({ ...row, receipts: [fact] }),
      row.owner,
      row.id
    );
    if (!checked.ok) return { ok: false, reason: 'invalid_record' };
    const admitted = privateRecordSnapshot(checked.value, row.owner, row.id);
    if (
      !admitted ||
      admitted.family !== 'private_send_operation' ||
      !admitted.receipts
    )
      return { ok: false, reason: 'invalid_record' };
    const nextFact = admitted.receipts[0],
      existing = row.receipts?.find((r) => sameFact(r, nextFact));
    let next = record,
      nextWire = baseWire;
    if (existing) {
      if (existing.readbackWire !== nextFact.readbackWire)
        return { ok: false, reason: 'conflict' };
    } else {
      if (row.revision === Number.MAX_SAFE_INTEGER)
        return { ok: false, reason: 'revision_exhausted' };
      const decoded = decodePrivateRecord(
        JSON.stringify({
          ...row,
          revision: row.revision + 1,
          receipts: [...(row.receipts ?? []), nextFact]
        }),
        row.owner,
        row.id
      );
      if (!decoded.ok) return { ok: false, reason: 'invalid_record' };
      next = decoded.value;
      const wire = privateRecordWire(next, row.owner, row.id);
      if (!wire) return { ok: false, reason: 'invalid_record' };
      nextWire = wire;
    }
    const token = Object.freeze({}) as PrivateReceiptTransition;
    transitions.set(token, {
      owner: row.owner,
      id: row.id,
      base: record,
      next,
      baseWire,
      nextWire
    });
    return { ok: true, value: token };
  } catch {
    return { ok: false, reason: 'invalid_record' };
  }
}
export function privateReceiptTransitionSnapshot(
  token: PrivateReceiptTransition,
  owner: unknown,
  id: unknown
) {
  const saved = transitions.get(token);
  return saved && saved.owner === owner && saved.id === id
    ? { ...saved }
    : undefined;
}
export type PrivateReceiptCommitResult =
  | Readonly<{ status: 'saved' | 'existing'; record: PrivateRecordHandle }>
  | Readonly<{ status: PrivateStorageFailure | 'invalid' }>;
// No SDK work in IDB. Request success is insufficient: actual exact full-wire
// readback must acknowledge the CAS append. Unknown commits stay uncertain.
export async function commitPrivateReceiptTransition(
  repository: PrivateStorageRepository,
  token: PrivateReceiptTransition
): Promise<PrivateReceiptCommitResult> {
  const saved = transitions.get(token);
  if (!saved) return { status: 'invalid' };
  try {
    let status: 'saved' | 'existing' = 'existing';
    if (saved.baseWire !== saved.nextWire) {
      const committed = await commitPrivateRecord(
        repository,
        saved.next,
        saved.base
      );
      if (!committed.ok) return { status: committed.reason };
      status = committed.value.state === 'existing' ? 'existing' : 'saved';
    }
    const readback = await loadPrivateRecord(
      repository,
      'private_sends',
      saved.id
    );
    if (!readback.ok) return { status: 'unknown_completion' };
    if (
      privateRecordWire(readback.value, saved.owner, saved.id) !==
      saved.nextWire
    )
      return { status: 'conflict' };
    return { status, record: readback.value };
  } catch {
    return { status: 'unknown_completion' };
  }
}
