import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  listPrivateReceivedRecords,
  type PrivateStorageRepository
} from './private-storage.ts';
import {
  privateRecordIdentity,
  privateRecordSnapshot
} from './private-records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
export type RetainedOuterMetadata = Readonly<{
  source: string;
  outerId: string;
  outerTime: number;
}>;
export async function readRetainedOuterMetadata(
  repository: PrivateStorageRepository,
  session: PrivateSession
): Promise<
  | Readonly<{ ok: true; rows: readonly RetainedOuterMetadata[] }>
  | Readonly<{ ok: false; reason: string }>
> {
  const ownership = privateSessionOwnership(session);
  if (!ownership?.current()) return { ok: false, reason: 'stopped' };
  const loaded = await listPrivateReceivedRecords(repository);
  if (!ownership.current()) return { ok: false, reason: 'stopped' };
  if (!loaded.ok) return loaded;
  let rows = Array.from<RetainedOuterMetadata>([]);
  for (const token of loaded.value) {
    const identity = privateRecordIdentity(token);
    if (identity?.owner !== ownership.owner)
      return { ok: false, reason: 'invalid_scope' };
    const record = privateRecordSnapshot(token, ownership.owner, identity.id);
    if (record?.family !== 'received_envelope')
      return { ok: false, reason: 'corrupt_record' };
    const verified = verifyEnvelope(record.outer),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (!event || event.kind !== 1059 || event.id !== record.id)
      return { ok: false, reason: 'corrupt_record' };
    for (const source of record.sources)
      rows = rows.concat({
        source,
        outerId: event.id,
        outerTime: event.created_at
      });
  }
  return ownership.current()
    ? { ok: true, rows }
    : { ok: false, reason: 'stopped' };
}
