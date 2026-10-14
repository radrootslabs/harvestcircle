import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  listPrivateReceivedRecords,
  privateStorageOwner,
  type PrivateStorageRepository
} from './private-storage.ts';
import {
  privateReceivedOuterMetadata,
  privateRecordIdentity,
  privateRecordSnapshot
} from './private-records.ts';
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
  if (privateStorageOwner(repository) !== ownership.owner)
    return { ok: false, reason: 'invalid_scope' };
  const loaded = await listPrivateReceivedRecords(repository);
  if (!ownership.current()) return { ok: false, reason: 'stopped' };
  if (!loaded.ok) return loaded;
  const rows = new Map<number, RetainedOuterMetadata>();
  for (const token of loaded.value) {
    const identity = privateRecordIdentity(token);
    if (identity?.owner !== ownership.owner)
      return { ok: false, reason: 'invalid_scope' };
    const record = privateRecordSnapshot(token, ownership.owner, identity.id);
    if (record?.family !== 'received_envelope')
      return { ok: false, reason: 'corrupt_record' };
    const event = privateReceivedOuterMetadata(
      token,
      ownership.owner,
      identity.id
    );
    if (!event || event.id !== record.id)
      return { ok: false, reason: 'corrupt_record' };
    for (const source of record.sources)
      rows.set(rows.size, {
        source,
        outerId: event.id,
        outerTime: event.createdAt
      });
  }
  return ownership.current()
    ? { ok: true, rows: Array.from(rows.values()) }
    : { ok: false, reason: 'stopped' };
}
