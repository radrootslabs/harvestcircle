import type { PublicRecord } from '../contracts/local-records.ts';
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
