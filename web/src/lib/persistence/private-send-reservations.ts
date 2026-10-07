import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
export type PrivateSendReservation = Readonly<{
  schema: 1;
  family: 'private_send_reservation';
  owner: string;
  id: string;
  revision: 0;
  peer: string;
  rumorHash: string;
  createdAt: number;
}>;
declare const repositoryBrand: unique symbol;
export type PrivateSendReservationRepository = Readonly<{
  [repositoryBrand]: true;
}>;
export type ReservationFailure =
  | 'invalid'
  | 'invalid_scope'
  | 'unavailable'
  | 'aborted'
  | 'unknown_completion'
  | 'corrupt_record'
  | 'capacity'
  | 'conflict'
  | 'clock_conflict'
  | 'stopped';
export type ReservationResult =
  | Readonly<{
      ok: true;
      state: 'created' | 'existing';
      value: PrivateSendReservation;
    }>
  | Readonly<{ ok: false; reason: ReservationFailure }>;
type Scope = { database: BrowserDatabase; owner: string };
const repositories = new WeakMap<PrivateSendReservationRepository, Scope>();
const failed = (reason: ReservationFailure): ReservationResult => ({
  ok: false,
  reason
});
// Metadata only, never a plaintext/private-draft or encrypted-send decoder.
// Future paired-ciphertext/recovery families retain their own admission owner.
export function inspectPrivateSendReservation(
  raw: unknown,
  expectedOwner: unknown,
  expectedId: unknown
): PrivateSendReservation | undefined {
  const owner = canonicalPublicKey(expectedOwner),
    id = canonicalLocalId(expectedId);
  if (!owner || !id || typeof raw !== 'string' || !boundedUtf8(raw, 4096))
    return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    if (
      !exactLocalFields(value, [
        'schema',
        'family',
        'owner',
        'id',
        'revision',
        'peer',
        'rumorHash',
        'createdAt'
      ]) ||
      value.schema !== 1 ||
      value.family !== 'private_send_reservation' ||
      value.owner !== owner ||
      value.id !== id ||
      value.revision !== 0
    )
      return undefined;
    const peer = canonicalPublicKey(value.peer),
      createdAt = safeUnsignedInteger(value.createdAt);
    if (
      !peer ||
      peer === owner ||
      createdAt === undefined ||
      typeof value.rumorHash !== 'string' ||
      !/^[0-9a-f]{64}$/.test(value.rumorHash)
    )
      return undefined;
    return {
      schema: 1,
      family: 'private_send_reservation',
      owner,
      id,
      revision: 0,
      peer,
      rumorHash: value.rumorHash,
      createdAt
    };
  } catch {
    return undefined;
  }
}
export function createPrivateSendReservationRepository(
  database: BrowserDatabase,
  expectedOwner: unknown
): PrivateSendReservationRepository | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  try {
    if (!owner || browserDatabaseState(database).state !== 'ready')
      return undefined;
    const token = Object.freeze({}) as PrivateSendReservationRepository;
    repositories.set(token, { database, owner });
    return token;
  } catch {
    return undefined;
  }
}
export function privateSendReservationOwner(
  repository: PrivateSendReservationRepository
) {
  return repositories.get(repository)?.owner;
}
// Atomic local metadata CAS, not identity, signing, encryption, relay delivery
// or global uniqueness proof. Completion, never request success, grants credit.
export function reservePrivateSendReservation(
  repository: PrivateSendReservationRepository,
  rawReservation: unknown,
  observedSeconds: unknown,
  current: () => boolean
): Promise<ReservationResult> {
  const scope = repositories.get(repository);
  if (
    !scope ||
    typeof rawReservation !== 'string' ||
    typeof current !== 'function'
  )
    return Promise.resolve(failed('invalid'));
  let candidateId: unknown;
  try {
    if (!boundedUtf8(rawReservation, 4096))
      return Promise.resolve(failed('invalid'));
    const input: unknown = JSON.parse(rawReservation);
    if (typeof input !== 'object' || input === null || Array.isArray(input))
      return Promise.resolve(failed('invalid'));
    candidateId = (input as Record<string, unknown>).id;
  } catch {
    return Promise.resolve(failed('invalid'));
  }
  const candidate = inspectPrivateSendReservation(
      rawReservation,
      scope.owner,
      candidateId
    ),
    seconds = safeUnsignedInteger(observedSeconds);
  if (!candidate || seconds === undefined)
    return Promise.resolve(failed('invalid'));
  const wire = JSON.stringify(candidate),
    next = { owner: scope.owner, id: candidate.id, wire };
  let transaction: IDBTransaction;
  try {
    transaction = browserDatabaseTransaction(
      scope.database,
      ['private_sends'],
      'readwrite'
    );
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  return new Promise((resolve) => {
    let outcome: ReservationResult | undefined;
    let existing: PrivateSendReservation | undefined;
    let count = 0,
      bytes = 0,
      collision = false;
    function refuse(reason: ReservationFailure) {
      outcome = failed(reason);
      try {
        transaction.abort();
      } catch {
        outcome = failed('unknown_completion');
      }
    }
    transaction.addEventListener('complete', () =>
      resolve(outcome ?? failed('unknown_completion'))
    );
    transaction.addEventListener('abort', () =>
      resolve(outcome && !outcome.ok ? outcome : failed('aborted'))
    );
    try {
      const store = transaction.objectStore('private_sends');
      const request = store
        .index('by_owner')
        .openCursor(IDBKeyRange.only(scope.owner));
      request.addEventListener('success', () => {
        try {
          if (!current()) {
            refuse('stopped');
            return;
          }
          const cursor = request.result;
          if (cursor) {
            const key = cursor.primaryKey,
              id =
                Array.isArray(key) && key.length === 2 && key[0] === scope.owner
                  ? canonicalLocalId(key[1])
                  : undefined;
            const row: unknown = cursor.value;
            if (
              !id ||
              !exactLocalFields(row, ['owner', 'id', 'wire']) ||
              row.owner !== scope.owner ||
              row.id !== id ||
              typeof row.wire !== 'string'
            ) {
              refuse('corrupt_record');
              return;
            }
            const value = inspectPrivateSendReservation(
              row.wire,
              scope.owner,
              id
            );
            if (!value) {
              refuse('corrupt_record');
              return;
            }
            count += 1;
            if (value.id === candidate.id) existing = value;
            if (value.rumorHash === candidate.rumorHash) collision = true;
            bytes += new TextEncoder().encode(JSON.stringify(row)).length;
            if (
              count > LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends ||
              bytes > LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
            ) {
              refuse('capacity');
              return;
            }
            cursor.continue();
            return;
          }
          if (existing) {
            if (JSON.stringify(existing) !== wire) {
              refuse('conflict');
              return;
            }
            outcome = { ok: true, state: 'existing', value: existing };
            return;
          }
          if (candidate.createdAt !== seconds || collision) {
            refuse('clock_conflict');
            return;
          }
          if (
            count >= LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends ||
            bytes + new TextEncoder().encode(JSON.stringify(next)).length >
              LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
          ) {
            refuse('capacity');
            return;
          }
          if (!current()) {
            refuse('stopped');
            return;
          }
          // The owner cursor proved this key absent in the same readwrite
          // transaction; put cannot replace another concurrent reservation.
          store.put(next);
          outcome = { ok: true, state: 'created', value: candidate };
        } catch {
          refuse('aborted');
        }
      });
    } catch {
      refuse('aborted');
    }
  });
}
