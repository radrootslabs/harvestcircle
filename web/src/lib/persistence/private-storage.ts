import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
import {
  decodePrivateRecord,
  privateRecordIdentity,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle,
  type PrivateRecord
} from './private-records.ts';
declare const repositoryBrand: unique symbol;
export type PrivateStorageRepository = Readonly<{ [repositoryBrand]: true }>;
export type PrivateStorageFailure =
  | 'invalid_scope'
  | 'invalid_record'
  | 'conflict'
  | 'capacity'
  | 'corrupt_record'
  | 'unavailable'
  | 'aborted'
  | 'unknown_completion';
export type PrivateStorageResult<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; reason: PrivateStorageFailure }>;
type Store = 'private_sends' | 'received_envelopes';
type Scope = Readonly<{ database: BrowserDatabase; owner: string }>;
type Row = Readonly<{
  store: Store;
  record: PrivateRecord;
  wire: string;
  bytes: number;
}>;
const repositories = new WeakMap<PrivateStorageRepository, Scope>();
const failed = (
  reason: PrivateStorageFailure
): PrivateStorageResult<never> => ({ ok: false, reason });
function scopeOf(repository: PrivateStorageRepository): Scope | undefined {
  const scope = repositories.get(repository);
  try {
    return scope && browserDatabaseState(scope.database).state === 'ready'
      ? scope
      : undefined;
  } catch {
    return undefined;
  }
}
// Namespace only, not installed-account or cryptographic/effect permission.
export function createPrivateStorageRepository(
  database: BrowserDatabase,
  expectedOwner: unknown
): PrivateStorageRepository | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  try {
    if (!owner || browserDatabaseState(database).state !== 'ready')
      return undefined;
    const token = Object.freeze({}) as PrivateStorageRepository;
    repositories.set(token, { database, owner });
    return token;
  } catch {
    return undefined;
  }
}
function stored(
  value: unknown,
  store: Store,
  owner: string,
  id: unknown
): Row | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id ||
    typeof value.wire !== 'string'
  )
    return undefined;
  const decoded = decodePrivateRecord(value.wire, owner, id);
  if (!decoded.ok) return undefined;
  const record = privateRecordSnapshot(decoded.value, owner, id);
  if (
    !record ||
    (record.family === 'received_envelope') !== (store === 'received_envelopes')
  )
    return undefined;
  return {
    store,
    record,
    wire: value.wire,
    bytes: new TextEncoder().encode(JSON.stringify(value)).length
  };
}
// Each store has an independent owner-scoped scan/quota. A corrupt received
// namespace cannot reset or consume the outbox, and no uncertain row is evicted.
function transaction<T>(
  scope: Scope,
  store: Store,
  mode: 'readonly' | 'readwrite',
  finish: (
    rows: readonly Row[],
    tx: IDBTransaction,
    refuse: (reason: PrivateStorageFailure) => void
  ) => PrivateStorageResult<T> | undefined
): Promise<PrivateStorageResult<T>> {
  let tx: IDBTransaction;
  try {
    tx = browserDatabaseTransaction(scope.database, [store], mode);
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  const countCap =
    store === 'private_sends'
      ? LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
      : LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes;
  const byteCap =
    store === 'private_sends'
      ? LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
      : LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes;
  return new Promise((resolve) => {
    let result: PrivateStorageResult<T> | undefined,
      bytes = 0;
    let rows = Array.from<Row>([]);
    function refuse(reason: PrivateStorageFailure) {
      result = failed(reason);
      try {
        tx.abort();
      } catch {
        result = failed('unknown_completion');
      }
    }
    tx.addEventListener('complete', () =>
      resolve(result ?? failed('unknown_completion'))
    );
    tx.addEventListener('abort', () =>
      resolve(result && !result.ok ? result : failed('aborted'))
    );
    try {
      const cursorRequest = tx
        .objectStore(store)
        .index('by_owner')
        .openCursor(IDBKeyRange.only(scope.owner));
      cursorRequest.addEventListener('success', () => {
        try {
          const cursor = cursorRequest.result;
          if (!cursor) {
            const outcome = finish(rows, tx, refuse);
            if (outcome) result = outcome;
            return;
          }
          const key = cursor.primaryKey;
          const row =
            Array.isArray(key) && key.length === 2 && key[0] === scope.owner
              ? stored(cursor.value, store, scope.owner, key[1])
              : undefined;
          if (!row) {
            refuse('corrupt_record');
            return;
          }
          rows = rows.concat(row);
          bytes += row.bytes;
          if (rows.length > countCap || bytes > byteCap) {
            refuse('capacity');
            return;
          }
          cursor.continue();
        } catch {
          refuse('aborted');
        }
      });
    } catch {
      refuse('aborted');
    }
  });
}
function validTransition(
  base: PrivateRecord | undefined,
  next: PrivateRecord
): boolean {
  if (!base)
    return (
      next.family === 'received_envelope' &&
      next.revision === 0 &&
      next.read === null
    );
  if (
    base.owner !== next.owner ||
    base.id !== next.id ||
    next.revision !== base.revision + 1
  )
    return false;
  if (base.family === 'received_envelope') {
    return (
      next.family === base.family &&
      base.outer === next.outer &&
      base.observedAtMilliseconds === next.observedAtMilliseconds &&
      base.sources.every((source) => next.sources.includes(source))
    );
  }
  if (
    next.family !== 'private_send_operation' ||
    base.peer !== next.peer ||
    base.rumorHash !== next.rumorHash ||
    base.createdAt !== next.createdAt
  )
    return false;
  return (
    base.family === 'private_send_reservation' ||
    (JSON.stringify(base.self) === JSON.stringify(next.self) &&
      (base.deliveryPlan === undefined ||
        JSON.stringify(base.deliveryPlan) ===
          JSON.stringify(next.deliveryPlan) ||
        (!!next.deliveryPlan &&
          JSON.stringify(base.deliveryPlan.routes) !==
            JSON.stringify(next.deliveryPlan.routes) &&
          JSON.stringify(next.deliveryPlan.previousRoutes) ===
            JSON.stringify([
              ...(base.deliveryPlan.previousRoutes ?? []),
              base.deliveryPlan.routes
            ]))) &&
      (base.receipts ?? []).every(
        (fact, index) =>
          JSON.stringify(fact) === JSON.stringify(next.receipts?.[index])
      ) &&
      (base.peerArtifact === null ||
        JSON.stringify(base.peerArtifact) ===
          JSON.stringify(next.peerArtifact)))
  );
}
// Local ciphertext CAS only. Factory/identity/read authorization and genuine
// durable preparation receipts belong to their workflows. No network or signer
// await can occur inside this transaction, and request success grants no credit.
export function commitPrivateRecord(
  repository: PrivateStorageRepository,
  handle: PrivateRecordHandle,
  expectedBase: PrivateRecordHandle | null
): Promise<
  PrivateStorageResult<
    Readonly<{ id: string; revision: number; state: 'committed' | 'existing' }>
  >
> {
  const scope = scopeOf(repository),
    identity = privateRecordIdentity(handle);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  const next =
      identity && privateRecordSnapshot(handle, scope.owner, identity.id),
    wire = identity && privateRecordWire(handle, scope.owner, identity.id);
  const base =
    identity && expectedBase !== null
      ? privateRecordSnapshot(expectedBase, scope.owner, identity.id)
      : undefined;
  const baseWire =
    identity && expectedBase !== null
      ? privateRecordWire(expectedBase, scope.owner, identity.id)
      : undefined;
  if (
    !identity ||
    !next ||
    !wire ||
    (expectedBase !== null && (!base || !baseWire)) ||
    !validTransition(base, next)
  )
    return Promise.resolve(failed('invalid_record'));
  const store: Store =
    next.family === 'received_envelope'
      ? 'received_envelopes'
      : 'private_sends';
  const row = { owner: scope.owner, id: identity.id, wire },
    bytes = new TextEncoder().encode(JSON.stringify(row)).length;
  return transaction<
    Readonly<{ id: string; revision: number; state: 'committed' | 'existing' }>
  >(scope, store, 'readwrite', (rows, tx, refuse) => {
    const previous = rows.find((row) => row.record.id === identity.id);
    if (previous?.wire === wire)
      return {
        ok: true,
        value: { id: identity.id, revision: next.revision, state: 'existing' }
      };
    if ((previous?.wire ?? null) !== (baseWire ?? null)) {
      refuse('conflict');
      return;
    }
    const countCap =
      store === 'private_sends'
        ? LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
        : LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes;
    const byteCap =
      store === 'private_sends'
        ? LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
        : LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes;
    if (
      (!previous && rows.length >= countCap) ||
      rows.reduce((total, row) => total + row.bytes, bytes) -
        (previous?.bytes ?? 0) >
        byteCap
    ) {
      refuse('capacity');
      return;
    }
    tx.objectStore(store).put(row);
    return {
      ok: true,
      value: { id: identity.id, revision: next.revision, state: 'committed' }
    };
  });
}
export function loadPrivateRecord(
  repository: PrivateStorageRepository,
  store: Store,
  id: unknown
): Promise<PrivateStorageResult<PrivateRecordHandle>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (
    (store !== 'private_sends' && store !== 'received_envelopes') ||
    typeof id !== 'string'
  )
    return Promise.resolve(failed('invalid_record'));
  return transaction(scope, store, 'readonly', (rows) => {
    const row = rows.find((row) => row.record.id === id);
    if (!row) return failed('invalid_record');
    const decoded = decodePrivateRecord(row.wire, scope.owner, id);
    return decoded.ok ? decoded : failed('corrupt_record');
  });
}
export function inspectPrivateStorage(
  repository: PrivateStorageRepository,
  store: Store
): Promise<PrivateStorageResult<Readonly<{ count: number; bytes: number }>>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (store !== 'private_sends' && store !== 'received_envelopes')
    return Promise.resolve(failed('invalid_record'));
  return transaction(scope, store, 'readonly', (rows) => ({
    ok: true,
    value: {
      count: rows.length,
      bytes: rows.reduce((sum, row) => sum + row.bytes, 0)
    }
  }));
}

// Strict bounded original owner namespace scan; opaque ciphertext handles only.
// This does not mint identity, decrypt or network permission.
export function listPrivateReceivedRecords(
  repository: PrivateStorageRepository
): Promise<PrivateStorageResult<readonly PrivateRecordHandle[]>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  return transaction<readonly PrivateRecordHandle[]>(
    scope,
    'received_envelopes',
    'readonly',
    (rows) => {
      let handles = Array.from<PrivateRecordHandle>([]);
      for (const row of rows) {
        const decoded = decodePrivateRecord(
          row.wire,
          scope.owner,
          row.record.id
        );
        if (!decoded.ok) return failed('corrupt_record');
        handles = handles.concat(decoded.value);
      }
      return { ok: true, value: handles };
    }
  );
}
