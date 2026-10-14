import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS, RELAY_BUDGETS } from '../config/budgets.ts';
import { privateRecordSettlement } from './local-inventory.ts';
import {
  inboxProbeCleanupAdmission,
  type InboxProbeNativeCleanup
} from './inbox-probe-cleanup.ts';
import type { InboxProbeCleanupSelection } from './inbox-probes.ts';
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
  handle: PrivateRecordHandle;
  wire: string;
  bytes: number;
}>;
const repositories = new WeakMap<PrivateStorageRepository, Scope>();
declare const probeRemovalBrand: unique symbol;
export type PrivateProbeRemovalProof = Readonly<{ [probeRemovalBrand]: true }>;
const probeRemovals = new WeakMap<
  PrivateProbeRemovalProof,
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    selection: InboxProbeCleanupSelection;
    owner: string;
    id: string;
    wire: string;
  }>
>();
export function privateProbeRemovalEvidence(
  proof: PrivateProbeRemovalProof,
  repository: PrivateStorageRepository,
  session: PrivateSession
) {
  const saved = probeRemovals.get(proof);
  return saved?.repository === repository && saved.session === session
    ? {
        selection: saved.selection,
        owner: saved.owner,
        id: saved.id,
        wire: saved.wire
      }
    : undefined;
}
type ValidatedWire = Readonly<{
  wire: string;
  handle: PrivateRecordHandle;
  bytes: number;
}>;
type ValidationState = Readonly<{
  entries: Map<string, ValidatedWire>;
  bytes: number;
}>;
// Ephemeral ciphertext proofs belong to the genuine repository scope. Every
// operation still visits every native owner row and checks all keys/headers and
// quotas. Neither persisted summaries nor cached count/absence is trusted.
const validations = new WeakMap<
  Scope,
  Readonly<Record<Store, ValidationState>>
>();
function remember(
  scope: Scope,
  store: Store,
  id: string,
  value: ValidatedWire
): void {
  const countCap =
      store === 'private_sends'
        ? LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
        : LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes,
    byteCap =
      store === 'private_sends'
        ? LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
        : LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes;
  if (value.bytes > byteCap) return;
  const stores = validations.get(scope) ?? {
      private_sends: { entries: new Map<string, ValidatedWire>(), bytes: 0 },
      received_envelopes: {
        entries: new Map<string, ValidatedWire>(),
        bytes: 0
      }
    },
    state = stores[store],
    entries = state.entries;
  let bytes = state.bytes;
  const old = entries.get(id);
  if (old) {
    entries.delete(id);
    bytes -= old.bytes;
  }
  while (entries.size >= countCap || bytes + value.bytes > byteCap) {
    const first = entries.keys().next().value;
    if (first === undefined) break;
    bytes -= entries.get(first)!.bytes;
    entries.delete(first);
  }
  entries.set(id, value);
  validations.set(scope, {
    ...stores,
    [store]: { entries, bytes: bytes + value.bytes }
  });
}

const failed = (
  reason: PrivateStorageFailure
): PrivateStorageResult<never> => ({ ok: false, reason });
function scopeOf(repository: PrivateStorageRepository): Scope | undefined {
  const scope = repositories.get(repository);
  try {
    if (scope && browserDatabaseState(scope.database).state === 'ready')
      return scope;
    if (scope) validations.delete(scope);
    return undefined;
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
export function privateStorageOwner(
  repository: PrivateStorageRepository
): string | undefined {
  return scopeOf(repository)?.owner;
}
function stored(
  scope: Scope,
  value: unknown,
  store: Store,
  owner: string,
  id: unknown
): Row | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id ||
    typeof value.wire !== 'string' ||
    typeof id !== 'string'
  )
    return undefined;
  const cached = validations.get(scope)?.[store].entries.get(id);
  const decoded =
    cached?.wire === value.wire
      ? { ok: true as const, value: cached.handle }
      : decodePrivateRecord(value.wire, owner, id);
  if (!decoded.ok) return undefined;
  const record = privateRecordSnapshot(decoded.value, owner, id);
  if (
    !record ||
    (record.family === 'received_envelope') !== (store === 'received_envelopes')
  )
    return undefined;
  // Equal original wire plus the three exact validated header fields fixes the
  // encoded row length; property ordering does not change that length.
  const bytes =
    cached?.wire === value.wire
      ? cached.bytes
      : new TextEncoder().encode(JSON.stringify(value)).length;
  if (cached?.wire !== value.wire)
    remember(scope, store, id, {
      wire: value.wire,
      handle: decoded.value,
      bytes
    });
  return { store, record, handle: decoded.value, wire: value.wire, bytes };
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
  ) => PrivateStorageResult<T> | undefined,
  watch?: (refuse: (reason: PrivateStorageFailure) => void) => () => void
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
    const rows = new Map<number, Row>();
    function refuse(reason: PrivateStorageFailure) {
      if (result && !result.ok) return;
      result = failed(reason);
      try {
        tx.abort();
      } catch {
        result = failed('unknown_completion');
      }
    }
    let off = () => {};
    tx.addEventListener('complete', () => {
      off();
      resolve(result ?? failed('unknown_completion'));
    });
    tx.addEventListener('abort', () => {
      off();
      // Actual native abort confirms rollback even if another abort call raced.
      resolve(
        result && !result.ok && result.reason !== 'unknown_completion'
          ? result
          : failed('aborted')
      );
    });
    if (watch) {
      try {
        off = watch(refuse);
      } catch {
        refuse('unavailable');
        return;
      }
    }
    try {
      const cursorRequest = tx
        .objectStore(store)
        .index('by_owner')
        .openCursor(IDBKeyRange.only(scope.owner));
      cursorRequest.addEventListener('success', () => {
        try {
          const cursor = cursorRequest.result;
          if (!cursor) {
            const outcome = finish(Array.from(rows.values()), tx, refuse);
            if (outcome) result = outcome;
            return;
          }
          const key = cursor.primaryKey;
          const row =
            Array.isArray(key) && key.length === 2 && key[0] === scope.owner
              ? stored(scope, cursor.value, store, scope.owner, key[1])
              : undefined;
          if (!row) {
            refuse('corrupt_record');
            return;
          }
          rows.set(rows.size, row);
          bytes += row.bytes;
          if (rows.size > countCap || bytes > byteCap) {
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
      next.revision === 0 &&
      ((next.family === 'received_envelope' && next.read === null) ||
        (next.family === 'private_access_probe' &&
          next.self === null &&
          next.receipts.length === 0))
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
  if (base.family === 'private_access_probe')
    return (
      next.family === base.family &&
      next.rumorHash === base.rumorHash &&
      next.createdAt === base.createdAt &&
      JSON.stringify(next.binding) === JSON.stringify(base.binding) &&
      (base.self === null ||
        JSON.stringify(base.self) === JSON.stringify(next.self)) &&
      base.receipts.every(
        (fact, index) =>
          JSON.stringify(fact) === JSON.stringify(next.receipts[index])
      )
    );
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
// Combine received lookup/merge/write only. The caller holds the actual owner
// lock. Complete namespace validation and native completion remain mandatory;
// changed writes require the caller's later exact readonly verification.
export function retainPrivateReceivedRecord(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  incoming: PrivateRecordHandle,
  cancellation?: AbortSignal
): Promise<
  PrivateStorageResult<
    Readonly<{
      record: PrivateRecordHandle;
      status: 'retained' | 'duplicate';
      changed: boolean;
    }>
  >
> {
  const scope = scopeOf(repository),
    ownership = privateSessionOwnership(session),
    identity = privateRecordIdentity(incoming),
    candidate =
      scope &&
      identity &&
      privateRecordSnapshot(incoming, scope.owner, identity.id);
  // Pre-admission cancellation is not evidence of a native rollback.
  if (cancellation?.aborted) return Promise.resolve(failed('invalid_scope'));
  if (!scope || !ownership?.current() || ownership.owner !== scope.owner)
    return Promise.resolve(failed('invalid_scope'));
  if (
    !identity ||
    candidate?.family !== 'received_envelope' ||
    candidate.revision !== 0 ||
    candidate.read !== null ||
    candidate.sources.length !== 1
  )
    return Promise.resolve(failed('invalid_record'));
  return transaction<
    Readonly<{
      record: PrivateRecordHandle;
      status: 'retained' | 'duplicate';
      changed: boolean;
    }>
  >(
    scope,
    'received_envelopes',
    'readwrite',
    (rows, tx, refuse) => {
      if (cancellation?.aborted) {
        refuse('aborted');
        return;
      }
      if (!ownership.current()) {
        refuse('invalid_scope');
        return;
      }
      const previous = rows.find((row) => row.record.id === identity.id);
      let record = incoming,
        status: 'retained' | 'duplicate' = 'retained';
      if (previous) {
        if (previous.record.family !== 'received_envelope') {
          refuse('conflict');
          return;
        }
        status = 'duplicate';
        if (previous.record.sources.includes(candidate.sources[0]))
          return {
            ok: true,
            value: { record: previous.handle, status, changed: false }
          };
        if (
          previous.record.sources.length >= RELAY_BUDGETS.inbox ||
          previous.record.revision === Number.MAX_SAFE_INTEGER
        ) {
          refuse('capacity');
          return;
        }
        const decoded = decodePrivateRecord(
          JSON.stringify({
            ...previous.record,
            revision: previous.record.revision + 1,
            sources: [...previous.record.sources, candidate.sources[0]]
          }),
          scope.owner,
          identity.id
        );
        if (!decoded.ok) {
          refuse('invalid_record');
          return;
        }
        record = decoded.value;
      }
      const wire = privateRecordWire(record, scope.owner, identity.id);
      if (!wire) {
        refuse('invalid_record');
        return;
      }
      const row = { owner: scope.owner, id: identity.id, wire },
        bytes = new TextEncoder().encode(JSON.stringify(row)).length;
      if (
        (!previous &&
          rows.length >= LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes) ||
        rows.reduce((total, item) => total + item.bytes, bytes) -
          (previous?.bytes ?? 0) >
          LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes
      ) {
        refuse('capacity');
        return;
      }
      if (cancellation?.aborted) {
        refuse('aborted');
        return;
      }
      if (!ownership.current()) {
        refuse('invalid_scope');
        return;
      }
      tx.objectStore('received_envelopes').put(row);
      return { ok: true, value: { record, status, changed: true } };
    },
    (refuse) => {
      const abort = () => refuse('aborted'),
        off = subscribePrivateSessionClose(session, () =>
          refuse('invalid_scope')
        );
      cancellation?.addEventListener('abort', abort, { once: true });
      if (cancellation?.aborted) abort();
      return () => {
        off();
        cancellation?.removeEventListener('abort', abort);
      };
    }
  );
}

// Observe only the selected validated row after actual native completion. The
// handle normalizes metadata; wire is the original physical stored string.
// Whole-wire custody owners must compare wire, not the normalized handle alone.
export function observePrivateRecord(
  repository: PrivateStorageRepository,
  store: Store,
  id: unknown
): Promise<
  PrivateStorageResult<Readonly<{ handle: PrivateRecordHandle; wire: string }>>
> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (
    (store !== 'private_sends' && store !== 'received_envelopes') ||
    typeof id !== 'string'
  )
    return Promise.resolve(failed('invalid_record'));
  return transaction<Readonly<{ handle: PrivateRecordHandle; wire: string }>>(
    scope,
    store,
    'readonly',
    (rows) => {
      const row = rows.find((row) => row.record.id === id);
      return row
        ? { ok: true, value: { handle: row.handle, wire: row.wire } }
        : failed('invalid_record');
    }
  );
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
  return transaction<PrivateRecordHandle>(scope, store, 'readonly', (rows) => {
    const row = rows.find((row) => row.record.id === id);
    if (!row) return failed('invalid_record');
    return { ok: true, value: row.handle };
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
      return { ok: true, value: rows.map((row) => row.handle) };
    }
  );
}

// Strict bounded readonly outbox observations; no publication authority.
export function listPrivateSendRecords(
  repository: PrivateStorageRepository
): Promise<PrivateStorageResult<readonly PrivateRecordHandle[]>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  return transaction<readonly PrivateRecordHandle[]>(
    scope,
    'private_sends',
    'readonly',
    (rows) => {
      return { ok: true, value: rows.map((row) => row.handle) };
    }
  );
}

// Explicit original-owner local received-copy loss only. Immutable selected
// whole wires CAS before any delete; independent outbox is never opened.
// Actual close aborts an admitted transaction where still possible. Native
// complete is necessary, and caller must separately verify readonly absence.
export function removePrivateReceivedRecords(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  handles: readonly PrivateRecordHandle[],
  review: unknown,
  cancellation?: AbortSignal
): Promise<PrivateStorageResult<Readonly<{ removed: number }>>> {
  const scope = scopeOf(repository),
    ownership = privateSessionOwnership(session);
  if (
    review !== 'reviewed_delete_local_received_copies' ||
    !scope ||
    cancellation?.aborted ||
    !ownership?.current() ||
    scope.owner !== ownership.owner
  )
    return Promise.resolve(failed('invalid_scope'));
  if (
    !Array.isArray(handles) ||
    !handles.length ||
    handles.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes
  )
    return Promise.resolve(failed('invalid_record'));
  const selected = handles.map((handle: PrivateRecordHandle) => {
    const identity = privateRecordIdentity(handle);
    const record =
        identity && privateRecordSnapshot(handle, scope.owner, identity.id),
      wire = identity && privateRecordWire(handle, scope.owner, identity.id);
    return identity && record?.family === 'received_envelope' && wire
      ? { id: identity.id, wire }
      : undefined;
  });
  if (
    selected.some((x) => !x) ||
    new Set(selected.map((x) => x?.id)).size !== selected.length
  )
    return Promise.resolve(failed('invalid_record'));
  return transaction<Readonly<{ removed: number }>>(
    scope,
    'received_envelopes',
    'readwrite',
    (rows, tx, refuse) => {
      if (!ownership.current() || cancellation?.aborted) {
        refuse('invalid_scope');
        return;
      }
      for (const item of selected) {
        if (
          !item ||
          rows.find((row) => row.record.id === item.id)?.wire !== item.wire
        ) {
          refuse('conflict');
          return;
        }
      }
      if (!ownership.current() || cancellation?.aborted) {
        refuse('invalid_scope');
        return;
      }
      for (const item of selected)
        if (item)
          tx.objectStore('received_envelopes').delete([scope.owner, item.id]);
      return { ok: true, value: { removed: selected.length } };
    },
    (refuse) => {
      const abort = () => refuse('invalid_scope');
      const off = subscribePrivateSessionClose(session, abort);
      cancellation?.addEventListener('abort', abort, { once: true });
      if (cancellation?.aborted) abort();
      return () => {
        off();
        cancellation?.removeEventListener('abort', abort);
      };
    }
  );
}

// Reviewed settled-send loss only; the cleanup owner holds the owner WebLock
// and idle SDK slot. Revalidate the full namespace and immutable selection in
// the same short transaction; native complete requires separate absence proof.
export function removePrivateSettledSendRecords(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  handles: readonly PrivateRecordHandle[],
  review: unknown,
  cancellation?: AbortSignal
): Promise<PrivateStorageResult<Readonly<{ removed: number }>>> {
  const scope = scopeOf(repository),
    ownership = privateSessionOwnership(session);
  if (
    review !== 'reviewed_delete_local_settled_sends' ||
    !scope ||
    cancellation?.aborted ||
    !ownership?.current() ||
    scope.owner !== ownership.owner
  )
    return Promise.resolve(failed('invalid_scope'));
  if (
    !Array.isArray(handles) ||
    !handles.length ||
    handles.length > LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends
  )
    return Promise.resolve(failed('invalid_record'));
  const selected = handles.map((handle: PrivateRecordHandle) => {
    const identity = privateRecordIdentity(handle),
      record =
        identity && privateRecordSnapshot(handle, scope.owner, identity.id),
      wire = identity && privateRecordWire(handle, scope.owner, identity.id);
    return identity &&
      record &&
      privateRecordSettlement(record) === 'settled' &&
      wire
      ? { id: identity.id, wire }
      : undefined;
  });
  if (
    selected.some((x) => !x) ||
    new Set(selected.map((x) => x?.id)).size !== selected.length
  )
    return Promise.resolve(failed('invalid_record'));
  return transaction<Readonly<{ removed: number }>>(
    scope,
    'private_sends',
    'readwrite',
    (rows, tx, refuse) => {
      if (!ownership.current() || cancellation?.aborted) {
        refuse('invalid_scope');
        return;
      }
      for (const item of selected) {
        const actual = item && rows.find((row) => row.record.id === item.id);
        if (
          !item ||
          actual?.wire !== item.wire ||
          privateRecordSettlement(actual.record) !== 'settled'
        ) {
          refuse('conflict');
          return;
        }
      }
      if (!ownership.current() || cancellation?.aborted) {
        refuse('invalid_scope');
        return;
      }
      for (const item of selected)
        if (item)
          tx.objectStore('private_sends').delete([scope.owner, item.id]);
      return { ok: true, value: { removed: selected.length } };
    },
    (refuse) => {
      const abort = () => refuse('invalid_scope'),
        off = subscribePrivateSessionClose(session, abort);
      cancellation?.addEventListener('abort', abort, { once: true });
      if (cancellation?.aborted) abort();
      return () => {
        off();
        cancellation?.removeEventListener('abort', abort);
      };
    }
  );
}

// The only admission is a genuine ticket minted inside the cleanup owner's
// actual WebLock/idle SDK job. Ordinary settlement still protects all probes.
// No asynchronous/provider work occurs inside the native readwrite transaction.
export async function removePrivateInboxProbe(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  ticket: InboxProbeNativeCleanup
): Promise<
  | Readonly<{ ok: true; value: PrivateProbeRemovalProof }>
  | Readonly<{
      ok: false;
      reason: PrivateStorageFailure;
      disposition: 'not_removed' | 'may_have_removed';
    }>
> {
  const denied = (reason: PrivateStorageFailure, uncertain = false) => ({
    ok: false as const,
    reason,
    disposition: uncertain
      ? ('may_have_removed' as const)
      : ('not_removed' as const)
  });
  const admitted = inboxProbeCleanupAdmission(ticket, repository, session);
  if (!admitted) return denied('invalid_scope');
  const scope = scopeOf(repository),
    ownership = privateSessionOwnership(session);
  if (
    !scope ||
    !ownership?.current() ||
    scope.owner !== ownership.owner ||
    admitted.owner !== scope.owner ||
    !admitted.current() ||
    !admitted.take()
  )
    return denied('invalid_scope');
  const committed = await transaction<Readonly<{ removed: number }>>(
    scope,
    'private_sends',
    'readwrite',
    (rows, tx, refuse) => {
      if (
        !admitted.current() ||
        !ownership.current() ||
        admitted.signal.aborted
      ) {
        refuse('invalid_scope');
        return;
      }
      const selected = rows.find((row) => row.record.id === admitted.id);
      if (
        !selected ||
        selected.record.family !== 'private_access_probe' ||
        selected.wire !== admitted.wire
      ) {
        refuse('conflict');
        return;
      }
      const store = tx.objectStore('private_sends'),
        remove = store.delete.bind(store);
      if (
        !admitted.current() ||
        !ownership.current() ||
        admitted.signal.aborted
      ) {
        refuse('invalid_scope');
        return;
      }
      remove([scope.owner, admitted.id]);
      return { ok: true, value: { removed: 1 } };
    },
    (refuse) => {
      const abort = () => refuse('invalid_scope'),
        off = subscribePrivateSessionClose(session, abort);
      admitted.signal.addEventListener('abort', abort, { once: true });
      if (admitted.signal.aborted) abort();
      return () => {
        off();
        admitted.signal.removeEventListener('abort', abort);
      };
    }
  );
  if (!committed.ok)
    return denied(committed.reason, committed.reason === 'unknown_completion');
  // Actual complete is necessary; separate complete-owner readonly absence is
  // also necessary. Preexisting/manual absence can never reach proof issuance.
  const readback = await listPrivateSendRecords(repository);
  if (!readback.ok) return denied('unknown_completion', true);
  if (
    readback.value.some(
      (handle) => privateRecordIdentity(handle)?.id === admitted.id
    )
  )
    return denied('conflict', true);
  const proof = Object.freeze({}) as PrivateProbeRemovalProof;
  probeRemovals.set(proof, {
    repository,
    session,
    selection: admitted.selection,
    owner: admitted.owner,
    id: admitted.id,
    wire: admitted.wire
  });
  return { ok: true, value: proof };
}
