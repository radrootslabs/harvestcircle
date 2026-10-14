import type { PublicRecord } from '../contracts/local-records.ts';
import type { PrivateRecord, PrivateTargetReceipt } from './private-records.ts';
export type PublicInventoryRow = Readonly<{
  key: string;
  family: PublicRecord['family'];
  id: string;
  revision: number;
  logicalBytes: number;
  state: 'settled' | 'protected';
}>;
// Conservative local journal classification, not independently proven relay
// evidence. Any unresolved historical receipt keeps the operation protected.
export function publicRecordSettlement(
  record: PublicRecord
): 'settled' | 'protected' {
  if (record.family === 'public_draft') return 'settled';
  if (
    !record.artifact ||
    record.receipts.length === 0 ||
    record.receipts.some(
      (receipt) => receipt.status !== 'accepted' && receipt.status !== 'refused'
    )
  )
    return 'protected';
  return record.capture.targets.every((origin) =>
    record.receipts.some((receipt) => receipt.origin === origin)
  )
    ? 'settled'
    : 'protected';
}
export function publicInventoryRow(
  record: PublicRecord,
  logicalBytes: number
): PublicInventoryRow {
  return {
    key: record.family + ':' + record.id,
    family: record.family,
    id: record.id,
    revision: record.revision,
    logicalBytes,
    state: publicRecordSettlement(record)
  };
}
export const PUBLIC_CLEANUP_CONSEQUENCES =
  'Removing selected records loses their local drafts or recovery receipts. It does not delete remote copies or reverse accepted effects. Pending or uncertain work is protected. Browser-local storage is not a backup.';

export type PrivateInventoryRow = Readonly<{
  id: string;
  revision: number;
  logicalBytes: number;
  state: 'settled' | 'protected';
}>;
// Strict decoded local bookkeeping only, never a new sender, remote receipt or
// replay capability. A later named acceptance resolves only its exact binding.
export function privateRecordSettlement(
  record: PrivateRecord
): 'settled' | 'protected' {
  if (
    record.family !== 'private_send_operation' ||
    !record.peerArtifact ||
    !record.deliveryPlan ||
    !record.receipts?.length
  )
    return 'protected';
  const facts = record.receipts;
  const sameTarget = (a: PrivateTargetReceipt, b: PrivateTargetReceipt) =>
    a.role === b.role && a.origin === b.origin && a.eventId === b.eventId;
  if (
    facts.some(
      (fact, index) =>
        ['unknown', 'timed_out', 'stopped'].includes(fact.status) &&
        !facts.some(
          (later, laterIndex) =>
            laterIndex > index &&
            later.status === 'accepted' &&
            sameTarget(fact, later)
        )
    )
  )
    return 'protected';
  return [
    record.deliveryPlan.routes.peer,
    record.deliveryPlan.routes.archive
  ].every((route) => {
    const eventId =
      route.role === 'peer'
        ? record.peerArtifact!.eventId
        : record.self.eventId;
    return route.targets.every((origin) =>
      facts.some(
        (fact) =>
          fact.status === 'accepted' &&
          fact.role === route.role &&
          fact.origin === origin &&
          fact.eventId === eventId
      )
    );
  })
    ? 'settled'
    : 'protected';
}
export function privateInventoryRow(
  record: PrivateRecord,
  logicalBytes: number
): PrivateInventoryRow {
  return {
    id: record.id,
    revision: record.revision,
    logicalBytes,
    state: privateRecordSettlement(record)
  };
}
export const PRIVATE_OUTBOX_CLEANUP_CONSEQUENCES =
  'Removing selected settled sends loses their local encrypted recovery, delivery status and duplicate-prevention evidence. Other local messages, participants and relays may retain copies. It does not erase remote messages or guarantee that a deliberately repeated message cannot be sent again. Browser-local storage is not a backup.';
