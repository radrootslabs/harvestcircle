import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  exactLocalFields,
  type PublicDraftRecord
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
function proposed(
  owner: string,
  id: string,
  revision: number,
  form: unknown
): PublicDraftRecord | undefined {
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
    // Capture each caller field once before validating that captured value.
    // Even a changing Proxy cannot substitute a coercible object after a check.
    const captured = {
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
      captured.contactType !== '' &&
      captured.contactType !== 'email' &&
      captured.contactType !== 'phone' &&
      captured.contactType !== 'https'
    )
      return undefined;
    let composedBytes = 0;
    for (const text of [
      captured.title,
      captured.description,
      captured.location,
      captured.amount,
      captured.currency,
      captured.unit,
      captured.quantity,
      captured.contactType,
      captured.contactValue
    ]) {
      if (
        typeof text !== 'string' ||
        !boundedUtf8(text, LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes)
      )
        return undefined;
      composedBytes += new TextEncoder().encode(text).length;
      if (composedBytes > LOCAL_PERSISTENCE_BUDGETS.draftComposedBytes)
        return undefined;
    }
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
  const scope = scopeOf(repository);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  const id = newLocalId();
  if (!id) return Promise.resolve(failed('invalid_id'));
  const row = proposed(scope.owner, id, 0, form);
  if (!row) return Promise.resolve(failed('invalid_form'));
  return transaction(scope, 'readwrite', (store, decide, refuse) => {
    const collision = store.get([scope.owner, id]);
    collision.addEventListener('success', () => {
      if (collision.result !== undefined) {
        refuse('conflict');
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
          id,
          wire: JSON.stringify(row)
        });
        put.addEventListener('success', () => decide({ ok: true, value: row }));
      });
    });
  });
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
  const scope = scopeOf(repository),
    id = canonicalLocalId(expectedId);
  if (!scope) return Promise.resolve(failed('invalid_scope'));
  if (!id) return Promise.resolve(failed('invalid_id'));
  const revision = safeUnsignedInteger(expectedRevision);
  if (revision === undefined || revision === Number.MAX_SAFE_INTEGER)
    return Promise.resolve(failed('invalid_revision'));
  const row = proposed(scope.owner, id, revision + 1, form);
  if (!row) return Promise.resolve(failed('invalid_form'));
  return transaction(scope, 'readwrite', (store, decide, refuse) => {
    const request = store.get([scope.owner, id]);
    request.addEventListener('success', () => {
      if (request.result === undefined) {
        refuse('not_found');
        return;
      }
      const previous = stored(request.result, scope.owner, id);
      if (!previous) {
        refuse('corrupt_record');
        return;
      }
      if (previous.revision !== revision) {
        refuse('conflict');
        return;
      }
      const put = store.put({
        owner: scope.owner,
        id,
        wire: JSON.stringify(row)
      });
      put.addEventListener('success', () => decide({ ok: true, value: row }));
    });
  });
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
