import type { PublicDraftRecord } from '../contracts/local-records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  createPublicDraftRepository,
  publicDraftRepositoryOwner,
  type PublicDraftRepository
} from './drafts.ts';
import {
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
import {
  createPublicQuotaRepository,
  publicQuotaOwner,
  claimPublicDraftOperation,
  type PublicQuotaRepository,
  type PublicQuotaFailure
} from './quota.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  type PublicRecordHandle
} from './records.ts';
import {
  publicDraftControllerOwnership,
  type PublicDraftController
} from '../publishing/draft-controller.ts';
import { exactLocalFields } from '../contracts/local-records.ts';

declare const repositoryBrand: unique symbol;
export type PublicPublicationRepository = Readonly<{ [repositoryBrand]: true }>;
type Scope = Readonly<{
  database: BrowserDatabase;
  drafts: PublicDraftRepository;
  quota: PublicQuotaRepository;
  owner: string;
}>;
const repositories = new WeakMap<PublicPublicationRepository, Scope>();
function scopeOf(repository: PublicPublicationRepository): Scope | undefined {
  const scope = repositories.get(repository);
  return scope &&
    publicDraftRepositoryOwner(scope.drafts) === scope.owner &&
    publicQuotaOwner(scope.quota) === scope.owner
    ? scope
    : undefined;
}
export function createPublicPublicationRepository(
  database: BrowserDatabase,
  owner: unknown
): PublicPublicationRepository | undefined {
  const drafts = createPublicDraftRepository(database, owner),
    quota = createPublicQuotaRepository(database, owner);
  const admittedOwner = drafts && publicDraftRepositoryOwner(drafts);
  if (
    !drafts ||
    !quota ||
    !admittedOwner ||
    publicQuotaOwner(quota) !== admittedOwner
  )
    return undefined;
  const handle = Object.freeze({}) as PublicPublicationRepository;
  repositories.set(handle, { database, drafts, quota, owner: admittedOwner });
  return handle;
}
export function publicPublicationDraftRepository(
  repository: PublicPublicationRepository
): PublicDraftRepository | undefined {
  return scopeOf(repository)?.drafts;
}
export function publicPublicationOwner(
  repository: PublicPublicationRepository
): string | undefined {
  return scopeOf(repository)?.owner;
}
// Original same-database quota custody only; it does not mint an identity,
// extension approval, lock lease or public network capability.
export function publicPublicationQuotaRepository(
  repository: PublicPublicationRepository
): PublicQuotaRepository | undefined {
  return scopeOf(repository)?.quota;
}
export type PublicListingSave =
  | Readonly<{ status: 'saved' | 'existing'; record: PublicRecordHandle }>
  | Readonly<{ status: PublicQuotaFailure | 'busy' }>;
export type PublicListingRead =
  | Readonly<{ status: 'found'; record: PublicRecordHandle }>
  | Readonly<{ status: 'missing' | 'unavailable' | 'corrupt_record' }>;
// Readback is exact local observation, never global absence or relay receipt.
export function readPublicListingIntent(
  repository: PublicPublicationRepository,
  command: unknown
): Promise<PublicListingRead> {
  const scope = scopeOf(repository),
    id = canonicalLocalId(command);
  if (!scope || !id) return Promise.resolve({ status: 'unavailable' });
  let transaction: IDBTransaction;
  try {
    transaction = browserDatabaseTransaction(
      scope.database,
      ['public_operations'],
      'readonly'
    );
  } catch {
    return Promise.resolve({ status: 'unavailable' });
  }
  return new Promise((resolve) => {
    let decision: PublicListingRead | undefined;
    transaction.addEventListener('complete', () =>
      resolve(
        scopeOf(repository)
          ? (decision ?? { status: 'unavailable' })
          : { status: 'unavailable' }
      )
    );
    transaction.addEventListener('abort', () =>
      resolve({ status: 'unavailable' })
    );
    try {
      const request = transaction
        .objectStore('public_operations')
        .get([scope.owner, id]);
      request.addEventListener('success', () => {
        const value: unknown = request.result;
        if (value === undefined) {
          decision = { status: 'missing' };
          return;
        }
        if (
          !exactLocalFields(value, ['owner', 'id', 'wire']) ||
          value.owner !== scope.owner ||
          value.id !== id
        ) {
          decision = { status: 'corrupt_record' };
          return;
        }
        const decoded = decodePublicRecord(value.wire, scope.owner, id);
        const row =
          decoded.ok && publicRecordSnapshot(decoded.value, scope.owner, id);
        decision =
          decoded.ok && row && row.family === 'public_operation'
            ? { status: 'found', record: decoded.value }
            : { status: 'corrupt_record' };
      });
    } catch {
      decision = { status: 'unavailable' };
      try {
        transaction.abort();
      } catch {
        resolve({ status: 'unavailable' });
      }
    }
  });
}
// Only a short local transaction is held. Extension/relay effects are later
// checkpoint owners and are absent from this repository.
export async function savePublicListingIntent(
  repository: PublicPublicationRepository,
  controller: PublicDraftController,
  record: PublicRecordHandle,
  expected: PublicDraftRecord,
  editVersion: number,
  command: string,
  current: (this: void) => boolean
): Promise<PublicListingSave> {
  try {
    const scope = scopeOf(repository),
      custody = publicDraftControllerOwnership(controller);
    const row = scope && publicRecordSnapshot(record, scope.owner, command);
    if (
      !scope ||
      !custody ||
      custody.repository !== scope.drafts ||
      !row ||
      row.family !== 'public_operation' ||
      row.source.type !== 'draft'
    )
      return { status: 'invalid_scope' };
    if (
      !current() ||
      typeof window === 'undefined' ||
      !navigator.locks?.request
    )
      return { status: 'unavailable' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + scope.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<PublicListingSave> => {
        if (!lock) return { status: 'busy' };
        const fresh = (): boolean => scopeOf(repository) === scope && current();
        if (!fresh()) return { status: 'unavailable' };
        const claimed = await claimPublicDraftOperation(
          scope.quota,
          row.id,
          record,
          {
            expected,
            current: fresh,
            draftCurrent: () =>
              custody.current(JSON.stringify(expected), editVersion)
          }
        );
        if (!fresh()) return { status: 'unavailable' };
        if (claimed.ok)
          return {
            status: claimed.value.state === 'created' ? 'saved' : 'existing',
            record: claimed.value.record
          };
        if (claimed.reason === 'unknown_completion') {
          const observed = await readPublicListingIntent(repository, row.id);
          const original =
            observed.status === 'found' &&
            publicRecordSnapshot(observed.record, scope.owner, row.id);
          if (!fresh()) return { status: 'unavailable' };
          if (
            observed.status === 'found' &&
            original &&
            original.family === 'public_operation' &&
            JSON.stringify(original.source) === JSON.stringify(row.source) &&
            JSON.stringify(original.capture) === JSON.stringify(row.capture)
          )
            return { status: 'existing', record: observed.record };
          if (original) return { status: 'conflict' };
        }
        return { status: claimed.reason };
      }
    );
  } catch {
    return { status: 'unavailable' };
  }
}
