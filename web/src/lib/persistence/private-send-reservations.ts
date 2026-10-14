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
import {
  inspectPrivateSendReservation,
  decodePrivateRecord,
  privateRecordSnapshot,
  type PrivateSendReservation
} from './private-records.ts';
export {
  inspectPrivateSendReservation,
  type PrivateSendReservation
} from './private-records.ts';
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
  | Readonly<{
      ok: false;
      reason: ReservationFailure;
      disposition: 'not_attempted' | 'not_created' | 'may_exist';
    }>;
type Scope = { database: BrowserDatabase; owner: string };
const repositories = new WeakMap<PrivateSendReservationRepository, Scope>();
const failed = (
  reason: ReservationFailure,
  disposition: 'not_attempted' | 'not_created' | 'may_exist' = 'not_attempted'
): ReservationResult => ({
  ok: false,
  reason,
  disposition
});
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
    let candidateAbsent = false;
    let count = 0,
      bytes = 0,
      collision = false;
    function refuse(reason: ReservationFailure) {
      outcome = failed(reason, 'may_exist');
      try {
        transaction.abort();
      } catch {
        outcome = failed('unknown_completion', 'may_exist');
      }
    }
    transaction.addEventListener('complete', () =>
      resolve(outcome ?? failed('unknown_completion', 'may_exist'))
    );
    transaction.addEventListener('abort', () => {
      const reason = outcome && !outcome.ok ? outcome.reason : 'aborted';
      resolve(failed(reason, candidateAbsent ? 'not_created' : 'may_exist'));
    });
    try {
      const store = transaction.objectStore('private_sends');
      // Same transaction and owner lock: absence plus actual abort proves no
      // original row survived. Request success alone never credits a write.
      const candidateRequest = store.get([scope.owner, candidate.id]);
      candidateRequest.addEventListener('success', () => {
        candidateAbsent = candidateRequest.result === undefined;
      });
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
            const decoded = decodePrivateRecord(row.wire, scope.owner, id),
              value =
                decoded.ok &&
                privateRecordSnapshot(decoded.value, scope.owner, id);
            if (!value || value.family === 'received_envelope') {
              refuse('corrupt_record');
              return;
            }
            count += 1;
            if (
              value.id === candidate.id &&
              value.family === 'private_access_probe'
            ) {
              refuse('conflict');
              return;
            }
            if (
              value.id === candidate.id &&
              value.family !== 'private_access_probe'
            )
              existing = {
                schema: 1,
                family: 'private_send_reservation',
                owner: value.owner,
                id: value.id,
                revision: 0,
                peer: value.peer,
                rumorHash: value.rumorHash,
                createdAt: value.createdAt
              };
            if (
              value.family !== 'private_access_probe' &&
              value.rumorHash === candidate.rumorHash
            )
              collision = true;
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
          if (candidate.createdAt > seconds || collision) {
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
