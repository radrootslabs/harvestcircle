import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  exactLocalFields,
  type PublicRecord
} from '../contracts/local-records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from './records.ts';
import {
  publicInventoryRow,
  PUBLIC_CLEANUP_CONSEQUENCES,
  type PublicInventoryRow
} from './local-inventory.ts';

import {
  publicTransitionIdentity,
  publicTransitionSnapshot,
  type PublicOperationTransition
} from './artifact-records.ts';
import { decideFrozenTransition } from './operation-transactions.ts';

declare const repositoryBrand: unique symbol;
declare const inventoryBrand: unique symbol;
declare const cleanupBrand: unique symbol;
export type PublicQuotaRepository = Readonly<{ [repositoryBrand]: true }>;
export type PublicInventory = Readonly<{ [inventoryBrand]: true }>;
export type PublicCleanup = Readonly<{ [cleanupBrand]: true }>;
export type PublicQuotaFailure =
  | 'invalid_scope'
  | 'invalid_record'
  | 'invalid_selection'
  | 'protected'
  | 'conflict'
  | 'capacity'
  | 'corrupt_record'
  | 'unavailable'
  | 'aborted'
  | 'unknown_completion';
export type PublicQuotaResult<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; reason: PublicQuotaFailure }>;
type Scope = Readonly<{ database: BrowserDatabase; owner: string }>;
type Store = 'public_drafts' | 'public_operations' | 'preference_operations';
type Row = Readonly<{
  store: Store;
  wire: string;
  record: PublicRecord;
  bytes: number;
}>;
type Inspection = Readonly<{
  repository: PublicQuotaRepository;
  rows: readonly Row[];
}>;
type Cleanup = Readonly<{ inventory: Inspection; selected: readonly string[] }>;
const repositories = new WeakMap<PublicQuotaRepository, Scope>();
const inventories = new WeakMap<PublicInventory, Inspection>();
const cleanups = new WeakMap<PublicCleanup, Cleanup>();
const stores: readonly Store[] = [
  'public_drafts',
  'public_operations',
  'preference_operations'
];
function failed(reason: PublicQuotaFailure): PublicQuotaResult<never> {
  return { ok: false, reason };
}
function scopeOf(repository: PublicQuotaRepository): Scope | undefined {
  const scope = repositories.get(repository);
  try {
    return scope && browserDatabaseState(scope.database).state === 'ready'
      ? scope
      : undefined;
  } catch {
    return undefined;
  }
}
// Metadata only; this does not grant installed-account or effect authority.
export function publicQuotaOwner(
  repository: PublicQuotaRepository
): string | undefined {
  return scopeOf(repository)?.owner;
}
// A namespace capability only; installed-account authorization belongs to the
// account workflow. This repository never acquires any private store.
export function createPublicQuotaRepository(
  database: BrowserDatabase,
  expectedOwner: unknown
): PublicQuotaRepository | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  try {
    if (!owner || browserDatabaseState(database).state !== 'ready')
      return undefined;
    const handle = Object.freeze({}) as PublicQuotaRepository;
    repositories.set(handle, { database, owner });
    return handle;
  } catch {
    return undefined;
  }
}
function stored(
  value: unknown,
  store: Store,
  owner: string,
  id: string
): Row | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id ||
    typeof value.wire !== 'string'
  )
    return undefined;
  const admitted = decodePublicRecord(value.wire, owner, id);
  if (!admitted.ok) return undefined;
  const record = publicRecordSnapshot(admitted.value, owner, id);
  const family =
    store === 'public_drafts'
      ? 'public_draft'
      : store === 'public_operations'
        ? 'public_operation'
        : 'preference_operation';
  return record?.family === family
    ? {
        store,
        record,
        wire: value.wire,
        bytes: new TextEncoder().encode(value.wire).length
      }
    : undefined;
}
function transaction<T>(
  scope: Scope,
  mode: 'readonly' | 'readwrite',
  finish: (
    rows: readonly Row[],
    transaction: IDBTransaction,
    refuse: (reason: PublicQuotaFailure) => void
  ) => PublicQuotaResult<T> | undefined
): Promise<PublicQuotaResult<T>> {
  let active: IDBTransaction;
  try {
    active = browserDatabaseTransaction(scope.database, stores, mode);
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  return new Promise((resolve) => {
    let result: PublicQuotaResult<T> | undefined;
    let rows = Array.from<Row>([]),
      draftCount = 0,
      operationCount = 0,
      operationBytes = 0;
    function refuse(reason: PublicQuotaFailure) {
      result = failed(reason);
      try {
        active.abort();
      } catch {
        result = failed('unknown_completion');
      }
    }
    active.addEventListener('complete', () =>
      resolve(result ?? failed('unknown_completion'))
    );
    active.addEventListener('abort', () =>
      resolve(result && !result.ok ? result : failed('aborted'))
    );
    function scan(index: number): void {
      try {
        const store = stores[index];
        if (store === undefined) {
          const finished = finish(rows, active, refuse);
          if (finished !== undefined) result = finished;
          return;
        }
        const request = active
          .objectStore(store)
          .index('by_owner')
          .openCursor(IDBKeyRange.only(scope.owner));
        request.addEventListener('success', () => {
          try {
            const cursor = request.result;
            if (!cursor) {
              scan(index + 1);
              return;
            }
            const key = cursor.primaryKey;
            const id =
              Array.isArray(key) && key.length === 2 && key[0] === scope.owner
                ? canonicalLocalId(key[1])
                : undefined;
            const row = id && stored(cursor.value, store, scope.owner, id);
            if (!row) {
              refuse('corrupt_record');
              return;
            }
            if (row.record.family === 'public_draft') draftCount++;
            else {
              operationCount++;
              operationBytes += row.bytes;
            }
            if (
              draftCount > LOCAL_PERSISTENCE_BUDGETS.publicDrafts ||
              operationCount > LOCAL_PERSISTENCE_BUDGETS.publicOperations ||
              operationBytes > LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes
            ) {
              refuse('capacity');
              return;
            }
            rows = rows.concat(row);
            cursor.continue();
          } catch {
            refuse('aborted');
          }
        });
      } catch {
        refuse('aborted');
      }
    }
    scan(0);
  });
}
// Explicit new-row admission only. Artifact/CAS transitions retain HCP050.
// Logical bytes include the exact stored record wire, not browser disk/heap use.
export function admitPublicOperation(
  repository: PublicQuotaRepository,
  expectedId: unknown,
  handle: PublicRecordHandle
): Promise<PublicQuotaResult<Readonly<{ id: string; revision: number }>>> {
  const scope = scopeOf(repository),
    id = canonicalLocalId(expectedId);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (!id) return Promise.resolve(failed('invalid_record'));
  const record = publicRecordSnapshot(handle, scope.owner, id),
    wire = publicRecordWire(handle, scope.owner, id);
  if (
    !record ||
    !wire ||
    record.family === 'public_draft' ||
    record.revision !== 0
  )
    return Promise.resolve(failed('invalid_record'));
  const store: Store =
    record.family === 'public_operation'
      ? 'public_operations'
      : 'preference_operations';
  const bytes = new TextEncoder().encode(wire).length;
  return transaction(scope, 'readwrite', (rows, active, refuse) => {
    const previous = rows.find(
      (row) => row.store === store && row.record.id === id
    );
    if (previous)
      return previous.wire === wire
        ? { ok: true, value: { id, revision: 0 } }
        : (refuse('conflict'), undefined);
    const operations = rows.filter(
      (row) => row.record.family !== 'public_draft'
    );
    if (
      operations.length >= LOCAL_PERSISTENCE_BUDGETS.publicOperations ||
      operations.reduce((sum, row) => sum + row.bytes, bytes) >
        LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes
    ) {
      refuse('capacity');
      return;
    }
    active.objectStore(store).put({ owner: scope.owner, id, wire });
    return { ok: true, value: { id, revision: 0 } };
  });
}
// Claim under the caller's origin/author Web Lock. A previous capture is never
// overwritten or automatically signed again, even after a page lost its lock.
// No extension/network await occurs inside the short transaction.
export function claimPublicOperation(
  repository: PublicQuotaRepository,
  expectedId: unknown,
  handle: PublicRecordHandle
): Promise<
  PublicQuotaResult<
    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>
  >
> {
  const scope = scopeOf(repository),
    id = canonicalLocalId(expectedId);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (!id) return Promise.resolve(failed('invalid_record'));
  const record = publicRecordSnapshot(handle, scope.owner, id),
    wire = publicRecordWire(handle, scope.owner, id);
  if (
    !record ||
    !wire ||
    record.family === 'public_draft' ||
    record.revision !== 0 ||
    record.artifact !== null
  )
    return Promise.resolve(failed('invalid_record'));
  const store: Store =
    record.family === 'public_operation'
      ? 'public_operations'
      : 'preference_operations';
  const bytes = new TextEncoder().encode(wire).length;
  return transaction<
    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>
  >(scope, 'readwrite', (rows, active, refuse) => {
    const previous = rows.find(
      (row) => row.record.family !== 'public_draft' && row.record.id === id
    );
    if (previous) {
      const original = previous.record;
      if (
        original.family === 'public_draft' ||
        original.family !== record.family ||
        JSON.stringify(original.source) !== JSON.stringify(record.source) ||
        JSON.stringify(original.capture) !== JSON.stringify(record.capture) ||
        ('consent' in original ? original.consent : null) !==
          ('consent' in record ? record.consent : null)
      ) {
        refuse('conflict');
        return;
      }
      const decoded = decodePublicRecord(previous.wire, scope.owner, id);
      if (!decoded.ok) {
        refuse('corrupt_record');
        return;
      }
      return { ok: true, value: { state: 'existing', record: decoded.value } };
    }
    const operations = rows.filter(
      (row) => row.record.family !== 'public_draft'
    );
    if (
      operations.length >= LOCAL_PERSISTENCE_BUDGETS.publicOperations ||
      operations.reduce((sum, row) => sum + row.bytes, bytes) >
        LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes
    ) {
      refuse('capacity');
      return;
    }
    active.objectStore(store).put({ owner: scope.owner, id, wire });
    return { ok: true, value: { state: 'created', record: handle } };
  });
}
export function inspectPublicStorage(
  repository: PublicQuotaRepository
): Promise<PublicQuotaResult<PublicInventory>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  return transaction(scope, 'readonly', (rows) => {
    const handle = Object.freeze({}) as PublicInventory;
    inventories.set(handle, { repository, rows });
    return { ok: true, value: handle };
  });
}
export function publicInventorySnapshot(
  repository: PublicQuotaRepository,
  handle: PublicInventory
):
  | Readonly<{ rows: readonly PublicInventoryRow[]; consequences: string }>
  | undefined {
  const inventory = inventories.get(handle);
  return scopeOf(repository) && inventory?.repository === repository
    ? {
        rows: inventory.rows.map((row) =>
          publicInventoryRow(row.record, row.bytes)
        ),
        consequences: PUBLIC_CLEANUP_CONSEQUENCES
      }
    : undefined;
}
export function reviewPublicCleanup(
  repository: PublicQuotaRepository,
  handle: PublicInventory,
  selection: unknown
): PublicQuotaResult<PublicCleanup> {
  const inventory = inventories.get(handle);
  if (!scopeOf(repository) || inventory?.repository !== repository)
    return failed('invalid_scope');
  try {
    if (!Array.isArray(selection)) return failed('invalid_selection');
    const length = selection.length;
    if (
      !Number.isSafeInteger(length) ||
      length <= 0 ||
      length >
        LOCAL_PERSISTENCE_BUDGETS.publicDrafts +
          LOCAL_PERSISTENCE_BUDGETS.publicOperations
    )
      return failed('invalid_selection');
    let selected = Array.from<string>([]);
    for (let index = 0; index < length; index++) {
      const key: unknown = selection[index];
      if (typeof key !== 'string' || selected.includes(key))
        return failed('invalid_selection');
      const row = inventory.rows.find(
        (row) => publicInventoryRow(row.record, row.bytes).key === key
      );
      if (!row) return failed('invalid_selection');
      if (publicInventoryRow(row.record, row.bytes).state !== 'settled')
        return failed('protected');
      selected = selected.concat(key);
    }
    if (!scopeOf(repository)) return failed('invalid_scope');
    const token = Object.freeze({}) as PublicCleanup;
    cleanups.set(token, { inventory, selected });
    return { ok: true, value: token };
  } catch {
    return failed('invalid_selection');
  }
}
export function publicCleanupSnapshot(
  repository: PublicQuotaRepository,
  handle: PublicCleanup
):
  | Readonly<{
      selected: readonly PublicInventoryRow[];
      logicalBytesRemoved: number;
      consequences: string;
    }>
  | undefined {
  const cleanup = cleanups.get(handle);
  if (!scopeOf(repository) || cleanup?.inventory.repository !== repository)
    return undefined;
  const selected = cleanup.inventory.rows
    .map((row) => publicInventoryRow(row.record, row.bytes))
    .filter((row) => cleanup.selected.includes(row.key));
  return {
    selected,
    logicalBytesRemoved: selected.reduce(
      (sum, row) => sum + row.logicalBytes,
      0
    ),
    consequences: PUBLIC_CLEANUP_CONSEQUENCES
  };
}
export function commitPublicCleanup(
  repository: PublicQuotaRepository,
  handle: PublicCleanup
): Promise<PublicQuotaResult<readonly string[]>> {
  const cleanup = cleanups.get(handle),
    scope = scopeOf(repository);
  if (!scope || cleanup?.inventory.repository !== repository)
    return Promise.resolve(failed('invalid_scope'));
  return transaction(scope, 'readwrite', (rows, active, refuse) => {
    const before = cleanup.inventory.rows;
    if (
      rows.length !== before.length ||
      rows.some(
        (row, i) =>
          row.store !== before[i].store ||
          row.record.id !== before[i].record.id ||
          row.wire !== before[i].wire
      )
    ) {
      refuse('conflict');
      return;
    }
    const selected = rows.filter((row) =>
      cleanup.selected.includes(publicInventoryRow(row.record, row.bytes).key)
    );
    if (
      selected.length !== cleanup.selected.length ||
      selected.some(
        (row) => publicInventoryRow(row.record, row.bytes).state !== 'settled'
      )
    ) {
      refuse('protected');
      return;
    }
    for (const row of selected)
      active.objectStore(row.store).delete([scope.owner, row.record.id]);
    return { ok: true, value: Array.from(cleanup.selected) };
  });
}

// No signer/network await exists inside this transaction. It consumes only a
// genuine immutable typed transition captured before storage acquisition.
// Exact owner-scoped actual stored preference, not a caller's structural row.
// Read is bounded by the same complete quota/codec scan and transaction ack.
export function loadPreferenceOperation(
  repository: PublicQuotaRepository,
  expectedId: unknown
): Promise<PublicQuotaResult<PublicRecordHandle>> {
  const scope = scopeOf(repository),
    id = canonicalLocalId(expectedId);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (!id) return Promise.resolve(failed('invalid_record'));
  return transaction(scope, 'readonly', (rows) => {
    const row = rows.find(
      (row) => row.store === 'preference_operations' && row.record.id === id
    );
    if (!row) return failed('invalid_record');
    const decoded = decodePublicRecord(row.wire, scope.owner, id);
    return decoded.ok ? decoded : failed('corrupt_record');
  });
}
export function commitPublicOperationTransition(
  repository: PublicQuotaRepository,
  handle: PublicOperationTransition
): Promise<PublicQuotaResult<Readonly<{ id: string; revision: number }>>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  const identity = publicTransitionIdentity(handle);
  const command =
    identity && publicTransitionSnapshot(handle, scope.owner, identity.id);
  if (!command) return Promise.resolve(failed('invalid_record'));
  const store: Store =
    command.family === 'public_operation'
      ? 'public_operations'
      : 'preference_operations';
  return transaction(scope, 'readwrite', (rows, active, refuse) => {
    const previous = rows.find(
      (row) => row.store === store && row.record.id === command.id
    );
    const decision = decideFrozenTransition(
      previous?.wire,
      command.baseWire,
      command.nextWire
    );
    if (decision === 'conflict') {
      refuse('conflict');
      return;
    }
    if (decision === 'committed')
      return {
        ok: true,
        value: { id: command.id, revision: command.revision }
      };
    const operations = rows.filter(
      (row) => row.record.family !== 'public_draft'
    );
    const bytes =
      operations.reduce((sum, row) => sum + row.bytes, 0) -
      (previous?.bytes ?? 0) +
      new TextEncoder().encode(command.nextWire).length;
    if (bytes > LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes) {
      refuse('capacity');
      return;
    }
    active
      .objectStore(store)
      .put({ owner: scope.owner, id: command.id, wire: command.nextWire });
    return { ok: true, value: { id: command.id, revision: command.revision } };
  });
}
export async function observePublicOperationTransition(
  repository: PublicQuotaRepository,
  handle: PublicOperationTransition
): Promise<
  | Readonly<{ state: 'committed'; id: string; revision: number }>
  | Readonly<{ state: 'base_observed' | 'conflict' | 'unavailable' }>
> {
  const scope = scopeOf(repository),
    identity = publicTransitionIdentity(handle);
  const command =
    scope &&
    identity &&
    publicTransitionSnapshot(handle, scope.owner, identity.id);
  if (!scope || !command) return { state: 'unavailable' };
  const store: Store =
    command.family === 'public_operation'
      ? 'public_operations'
      : 'preference_operations';
  const read = await transaction(scope, 'readonly', (rows) => ({
    ok: true,
    value: decideFrozenTransition(
      rows.find((row) => row.store === store && row.record.id === command.id)
        ?.wire,
      command.baseWire,
      command.nextWire
    )
  }));
  if (!read.ok || !scopeOf(repository)) return { state: 'unavailable' };
  return read.value === 'committed'
    ? { state: 'committed', id: command.id, revision: command.revision }
    : { state: read.value };
}
