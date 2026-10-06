import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  exactLocalFields,
  type PublicDraftRecord,
  type PublicDraftForm
} from '../contracts/local-records.ts';
import { canonicalLocalId, newLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
import { decodePublicRecord, publicRecordSnapshot } from './records.ts';

declare const repositoryBrand: unique symbol;
export type PublicDraftRepository = Readonly<{ [repositoryBrand]: true }>;
type Scope = Readonly<{ database: BrowserDatabase; owner: string }>;
const repositories = new WeakMap<PublicDraftRepository, Scope>();
export type DraftFailure =
  | 'invalid_scope'
  | 'invalid_id'
  | 'invalid_form'
  | 'invalid_revision'
  | 'unavailable'
  | 'not_found'
  | 'conflict'
  | 'cap_reached'
  | 'corrupt_record'
  | 'aborted'
  | 'unknown_completion';
export type DraftResult<T> =
  | Readonly<{ ok: true; value: T }>
  | Readonly<{ ok: false; reason: DraftFailure }>;
function failed(reason: DraftFailure): DraftResult<never> {
  return { ok: false, reason };
}
function scopeOf(repository: PublicDraftRepository): Scope | undefined {
  const scope = repositories.get(repository);
  try {
    return scope && browserDatabaseState(scope.database).state === 'ready'
      ? scope
      : undefined;
  } catch {
    return undefined;
  }
}
// Explicit acquisition only. A canonical owner is a namespace, not proof of
// installed identity; account workflows retain that separate authority.
export function createPublicDraftRepository(
  database: BrowserDatabase,
  expectedOwner: unknown
): PublicDraftRepository | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  try {
    if (!owner || browserDatabaseState(database).state !== 'ready')
      return undefined;
    const handle = Object.freeze({}) as PublicDraftRepository;
    repositories.set(handle, { database, owner });
    return handle;
  } catch {
    return undefined;
  }
}
export function publicDraftRepositoryOwner(
  repository: PublicDraftRepository
): string | undefined {
  return scopeOf(repository)?.owner;
}

// This copy is form admission only: no ID/time capture, storage or effect.
export function publicDraftFormSnapshot(
  form: unknown
): PublicDraftForm | undefined {
  try {
    if (
      !exactLocalFields(form, [
        'title',
        'description',
        'location',
        'amount',
        'currency',
        'unit',
        'quantity',
        'contactType',
        'contactValue'
      ])
    )
      return undefined;
    const copy = {
      title: form.title,
      description: form.description,
      location: form.location,
      amount: form.amount,
      currency: form.currency,
      unit: form.unit,
      quantity: form.quantity,
      contactType: form.contactType,
      contactValue: form.contactValue
    };
    if (
      typeof copy.title !== 'string' ||
      typeof copy.description !== 'string' ||
      typeof copy.location !== 'string' ||
      typeof copy.amount !== 'string' ||
      typeof copy.currency !== 'string' ||
      typeof copy.unit !== 'string' ||
      typeof copy.quantity !== 'string' ||
      typeof copy.contactValue !== 'string' ||
      (copy.contactType !== '' &&
        copy.contactType !== 'email' &&
        copy.contactType !== 'phone' &&
        copy.contactType !== 'https')
    )
      return undefined;
    let bytes = 0;
    for (const text of [
      copy.title,
      copy.description,
      copy.location,
      copy.amount,
      copy.currency,
      copy.unit,
      copy.quantity,
      copy.contactType,
      copy.contactValue
    ]) {
      if (!boundedUtf8(text, LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes))
        return undefined;
      bytes += new TextEncoder().encode(text).length;
      if (bytes > LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes)
        return undefined;
    }
    return {
      title: copy.title,
      description: copy.description,
      location: copy.location,
      amount: copy.amount,
      currency: copy.currency,
      unit: copy.unit,
      quantity: copy.quantity,
      contactType: copy.contactType,
      contactValue: copy.contactValue
    };
  } catch {
    return undefined;
  }
}
function proposed(
  owner: string,
  id: string,
  revision: number,
  form: unknown
): PublicDraftRecord | undefined {
  try {
    const captured = publicDraftFormSnapshot(form);
    if (!captured) return undefined;
    const row = {
      schema: 1,
      family: 'public_draft',
      owner,
      id,
      revision,
      savedAtMilliseconds: Date.now(),
      form: captured
    };
    const admitted = decodePublicRecord(JSON.stringify(row), owner, id);
    if (!admitted.ok) return undefined;
    const snapshot = publicRecordSnapshot(admitted.value, owner, id);
    return snapshot?.family === 'public_draft' ? snapshot : undefined;
  } catch {
    return undefined;
  }
}
function stored(
  value: unknown,
  owner: string,
  id: string
): PublicDraftRecord | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id
  )
    return undefined;
  const admitted = decodePublicRecord(value.wire, owner, id);
  if (!admitted.ok) return undefined;
  const snapshot = publicRecordSnapshot(admitted.value, owner, id);
  return snapshot?.family === 'public_draft' ? snapshot : undefined;
}
function transaction<T>(
  scope: Scope,
  mode: 'readonly' | 'readwrite',
  run: (
    store: IDBObjectStore,
    decide: (result: DraftResult<T>) => void,
    refuse: (reason: DraftFailure) => void
  ) => void
): Promise<DraftResult<T>> {
  let active: IDBTransaction;
  try {
    active = browserDatabaseTransaction(
      scope.database,
      ['public_drafts'],
      mode
    );
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  return new Promise((resolve) => {
    let decision: DraftResult<T> | undefined;
    function refuse(reason: DraftFailure): void {
      decision = failed(reason);
      try {
        active.abort();
      } catch {
        decision = failed('unknown_completion');
      }
    }
    active.addEventListener('complete', () =>
      resolve(decision ?? failed('unknown_completion'))
    );
    active.addEventListener('abort', () =>
      resolve(decision && !decision.ok ? decision : failed('aborted'))
    );
    // A request success never acknowledges a save. Only the transaction's
    // complete event returns its captured payload; abort overrides any success.
    try {
      run(
        active.objectStore('public_drafts'),
        (result) => {
          decision = result;
        },
        refuse
      );
    } catch {
      refuse('aborted');
    }
  });
}
export function createPublicDraft(
  repository: PublicDraftRepository,
  form: unknown
): Promise<DraftResult<PublicDraftRecord>> {
  const captured = capturePublicDraftCreate(repository, form);
  return captured.ok
    ? commitPublicDraftWrite(repository, captured.value)
    : Promise.resolve(captured);
}
export function readPublicDraft(
  repository: PublicDraftRepository,
  expectedId: unknown
): Promise<DraftResult<PublicDraftRecord>> {
  const scope = scopeOf(repository),
    id = canonicalLocalId(expectedId);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (!id) return Promise.resolve(failed('invalid_id'));
  return transaction(scope, 'readonly', (store, decide, refuse) => {
    const request = store.get([scope.owner, id]);
    request.addEventListener('success', () => {
      if (request.result === undefined) {
        decide(failed('not_found'));
        return;
      }
      const row = stored(request.result, scope.owner, id);
      if (!row) {
        refuse('corrupt_record');
        return;
      }
      decide({ ok: true, value: row });
    });
  });
}
export function savePublicDraft(
  repository: PublicDraftRepository,
  expectedId: unknown,
  expectedRevision: unknown,
  form: unknown
): Promise<DraftResult<PublicDraftRecord>> {
  const captured = capturePublicDraftSave(
    repository,
    expectedId,
    expectedRevision,
    form
  );
  return captured.ok
    ? commitPublicDraftWrite(repository, captured.value)
    : Promise.resolve(captured);
}
export function listPublicDrafts(
  repository: PublicDraftRepository
): Promise<DraftResult<readonly PublicDraftRecord[]>> {
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  return transaction(scope, 'readonly', (store, decide, refuse) => {
    let rows = Array.from<PublicDraftRecord>([]);
    const request = store
      .index('by_owner')
      .openCursor(IDBKeyRange.only(scope.owner));
    request.addEventListener('success', () => {
      const cursor = request.result;
      if (!cursor) {
        decide({ ok: true, value: rows });
        return;
      }
      if (rows.length >= LOCAL_PERSISTENCE_BUDGETS.publicDrafts) {
        refuse('cap_reached');
        return;
      }
      const key = cursor.primaryKey;
      if (!Array.isArray(key) || key.length !== 2 || key[0] !== scope.owner) {
        refuse('corrupt_record');
        return;
      }
      const id = canonicalLocalId(key[1]);
      const row = id && stored(cursor.value, scope.owner, id);
      if (!row) {
        refuse('corrupt_record');
        return;
      }
      rows = rows.concat(row);
      cursor.continue();
    });
  });
}

// Immutable attempted local writes retain their original ID, revision, time and
// full form. They authorize only this repository's bounded local transaction.
declare const writeBrand: unique symbol;
export type PublicDraftWrite = Readonly<{ [writeBrand]: true }>;
type Write = Readonly<{
  repository: PublicDraftRepository;
  wire: string;
  expectedRevision: number | null;
}>;
const writes = new WeakMap<PublicDraftWrite, Write>();
function capture(
  repository: PublicDraftRepository,
  id: string,
  expectedRevision: number | null,
  form: unknown
): DraftResult<PublicDraftWrite> {
  const scope = scopeOf(repository);
  if (!scope) return failed('invalid_scope');
  const row = proposed(
    scope.owner,
    id,
    expectedRevision === null ? 0 : expectedRevision + 1,
    form
  );
  if (!row) return failed('invalid_form');
  const handle = Object.freeze({}) as PublicDraftWrite;
  writes.set(handle, {
    repository,
    wire: JSON.stringify(row),
    expectedRevision
  });
  return { ok: true, value: handle };
}
export function capturePublicDraftCreate(
  repository: PublicDraftRepository,
  form: unknown
): DraftResult<PublicDraftWrite> {
  if (!scopeOf(repository)) return failed('invalid_scope');
  const id = newLocalId();
  return id ? capture(repository, id, null, form) : failed('invalid_id');
}
export function capturePublicDraftSave(
  repository: PublicDraftRepository,
  expectedId: unknown,
  expectedRevision: unknown,
  form: unknown
): DraftResult<PublicDraftWrite> {
  if (!scopeOf(repository)) return failed('invalid_scope');
  const id = canonicalLocalId(expectedId),
    revision = safeUnsignedInteger(expectedRevision);
  if (!id) return failed('invalid_id');
  if (revision === undefined || revision === Number.MAX_SAFE_INTEGER)
    return failed('invalid_revision');
  return capture(repository, id, revision, form);
}
function writeOf(
  repository: PublicDraftRepository,
  handle: PublicDraftWrite
): Write | undefined {
  const write = writes.get(handle);
  return scopeOf(repository) && write?.repository === repository
    ? write
    : undefined;
}
export function publicDraftWriteSnapshot(
  repository: PublicDraftRepository,
  handle: PublicDraftWrite
): PublicDraftRecord | undefined {
  const write = writeOf(repository, handle);
  return write ? (JSON.parse(write.wire) as PublicDraftRecord) : undefined;
}
export function commitPublicDraftWrite(
  repository: PublicDraftRepository,
  handle: PublicDraftWrite
): Promise<DraftResult<PublicDraftRecord>> {
  const write = writeOf(repository, handle),
    scope = scopeOf(repository);
  if (!write || !scope) return Promise.resolve(failed('invalid_scope'));
  const row = JSON.parse(write.wire) as PublicDraftRecord;
  return transaction(scope, 'readwrite', (store, decide, refuse) => {
    const request = store.get([scope.owner, row.id]);
    request.addEventListener('success', () => {
      if (request.result !== undefined) {
        const previous = stored(request.result, scope.owner, row.id);
        if (!previous) {
          refuse('corrupt_record');
          return;
        }
        if (JSON.stringify(previous) === write.wire) {
          decide({ ok: true, value: row });
          return;
        }
        if (
          write.expectedRevision === null ||
          previous.revision !== write.expectedRevision
        ) {
          refuse('conflict');
          return;
        }
        const put = store.put({
          owner: scope.owner,
          id: row.id,
          wire: write.wire
        });
        put.addEventListener('success', () => decide({ ok: true, value: row }));
        return;
      }
      if (write.expectedRevision !== null) {
        refuse('not_found');
        return;
      }
      const count = store
        .index('by_owner')
        .count(IDBKeyRange.only(scope.owner));
      count.addEventListener('success', () => {
        if (count.result >= LOCAL_PERSISTENCE_BUDGETS.publicDrafts) {
          refuse('cap_reached');
          return;
        }
        const put = store.put({
          owner: scope.owner,
          id: row.id,
          wire: write.wire
        });
        put.addEventListener('success', () => decide({ ok: true, value: row }));
      });
    });
  });
}
export type DraftWriteObservation =
  | Readonly<{ state: 'committed'; value: PublicDraftRecord }>
  | Readonly<{ state: 'base_observed' }>
  | Readonly<{ state: 'conflict' }>
  | Readonly<{ state: 'unavailable'; reason: DraftFailure }>;
export async function observePublicDraftWrite(
  repository: PublicDraftRepository,
  handle: PublicDraftWrite
): Promise<DraftWriteObservation> {
  const write = writeOf(repository, handle);
  if (!write) return { state: 'unavailable', reason: 'invalid_scope' };
  const expected = JSON.parse(write.wire) as PublicDraftRecord;
  const read = await readPublicDraft(repository, expected.id);
  if (!read.ok)
    return read.reason === 'not_found' && write.expectedRevision === null
      ? { state: 'base_observed' }
      : { state: 'unavailable', reason: read.reason };
  if (JSON.stringify(read.value) === write.wire)
    return { state: 'committed', value: read.value };
  return read.value.revision === write.expectedRevision
    ? { state: 'base_observed' }
    : { state: 'conflict' };
}
