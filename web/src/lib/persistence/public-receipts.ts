import {
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from './records.ts';
import {
  preparePublicReceiptTransition,
  publicTransitionSnapshot
} from './artifact-records.ts';
import {
  publicPublicationOwner,
  publicPublicationQuotaRepository,
  readPublicListingIntent,
  type PublicPublicationRepository
} from './public-publications.ts';
import {
  commitPublicOperationTransition,
  observePublicOperationTransition
} from './quota.ts';
export type PublicListingReceiptResult =
  | Readonly<{ status: 'saved'; record: PublicRecordHandle }>
  | Readonly<{ status: 'invalid' | 'conflict' | 'unknown' | 'unavailable' }>;
// Exact full-wire CAS append. A request success or detached cached observation
// cannot acknowledge a receipt. Unknown native commits retain original payload.
export async function commitPublicListingReceipt(
  repository: PublicPublicationRepository,
  record: PublicRecordHandle,
  command: string,
  receiptWire: string
): Promise<PublicListingReceiptResult> {
  try {
    const owner = publicPublicationOwner(repository),
      quota = publicPublicationQuotaRepository(repository);
    const row = owner
      ? publicRecordSnapshot(record, owner, command)
      : undefined;
    if (
      !owner ||
      !quota ||
      row?.family !== 'public_operation' ||
      row.capture.kind !== 30402 ||
      !row.artifact
    )
      return { status: 'invalid' };
    const transition = preparePublicReceiptTransition(
      record,
      owner,
      command,
      receiptWire
    );
    if (!transition.ok) return { status: 'invalid' };
    const expected = publicTransitionSnapshot(transition.value, owner, command);
    if (!expected) return { status: 'invalid' };
    const saved = await commitPublicOperationTransition(
      quota,
      transition.value
    );
    if (!saved.ok) {
      if (saved.reason !== 'unknown_completion')
        return {
          status: saved.reason === 'conflict' ? 'conflict' : 'unavailable'
        };
      const observed = await observePublicOperationTransition(
        quota,
        transition.value
      );
      if (observed.state !== 'committed')
        return {
          status: observed.state === 'conflict' ? 'conflict' : 'unknown'
        };
    }
    const read = await readPublicListingIntent(repository, command);
    if (read.status !== 'found') return { status: 'unknown' };
    if (publicRecordWire(read.record, owner, command) !== expected.nextWire)
      return { status: 'conflict' };
    return { status: 'saved', record: read.record };
  } catch {
    return { status: 'unknown' };
  }
}
