import {
  browserDatabaseName,
  browserSchemaVersion,
  browserStores,
  initializeBrowserSchema,
  validateBrowserSchema,
  type BrowserStore
} from './schema.ts';

export type StorageFailure =
  | 'unavailable'
  | 'denied'
  | 'blocked'
  | 'cancelled'
  | 'incompatible_version'
  | 'migration_failed'
  | 'corrupt_schema'
  | 'open_failed';
export type BrowserDatabase = Readonly<{ kind: 'browser_database' }>;
type Owner = {
  database: IDBDatabase;
  close(reason: 'explicit' | 'version_change' | 'connection_closed'): void;
  snapshot(): Readonly<{
    state: 'ready' | 'closed';
    reason: 'explicit' | 'version_change' | 'connection_closed' | null;
  }>;
};
const owners = new WeakMap<BrowserDatabase, Owner>();
export type DatabaseOpenResult =
  | Readonly<{ state: 'ready'; owner: BrowserDatabase }>
  | Readonly<{
      state: 'unavailable';
      reason: StorageFailure;
      message: string;
    }>;

function failure(reason: StorageFailure): DatabaseOpenResult {
  return {
    state: 'unavailable',
    reason,
    message:
      'Browser storage is unavailable. You can still browse public listings. Existing local data has not been reset.'
  };
}
function classify(error: unknown): StorageFailure {
  const name = error instanceof DOMException ? error.name : '';
  if (name === 'SecurityError' || name === 'NotAllowedError') return 'denied';
  if (name === 'VersionError') return 'incompatible_version';
  return 'open_failed';
}
function ownerOf(owner: BrowserDatabase): Owner {
  const value = owners.get(owner);
  if (!value) throw new Error('invalid_database_owner');
  return value;
}
export function closeBrowserDatabase(owner: BrowserDatabase): void {
  ownerOf(owner).close('explicit');
}
export function browserDatabaseState(owner: BrowserDatabase) {
  return ownerOf(owner).snapshot();
}

// Explicit call only: importing this module never opens storage, including SSR.
export function openBrowserDatabase(
  signal?: AbortSignal
): Promise<DatabaseOpenResult> {
  if (signal?.aborted) return Promise.resolve(failure('cancelled'));
  let request: IDBOpenDBRequest;
  try {
    if (typeof indexedDB === 'undefined')
      return Promise.resolve(failure('unavailable'));
    request = indexedDB.open(browserDatabaseName, browserSchemaVersion);
  } catch (error) {
    return Promise.resolve(failure(classify(error)));
  }
  return new Promise((resolve) => {
    let settled = false;
    let migrationFailed = false;
    function finish(result: DatabaseOpenResult): void {
      if (settled) return;
      settled = true;
      signal?.removeEventListener('abort', cancel);
      resolve(result);
    }
    function abortUpgrade(): void {
      try {
        request.transaction?.abort();
      } catch {
        // Already completed/aborted; success still closes any late connection.
      }
    }
    function cancel(): void {
      finish(failure('cancelled'));
      abortUpgrade();
    }
    signal?.addEventListener('abort', cancel, { once: true });
    if (signal?.aborted) cancel();
    request.addEventListener('blocked', () => {
      finish(failure('blocked'));
      abortUpgrade();
    });
    request.addEventListener('upgradeneeded', (event) => {
      if (settled) {
        abortUpgrade();
        return;
      }
      try {
        initializeBrowserSchema(
          request.result,
          event.oldVersion,
          event.newVersion
        );
      } catch {
        migrationFailed = true;
        abortUpgrade();
      }
    });
    request.addEventListener('error', () => {
      finish(
        failure(migrationFailed ? 'migration_failed' : classify(request.error))
      );
    });
    request.addEventListener('success', () => {
      const database = request.result;
      if (settled) {
        database.close();
        return;
      }
      try {
        if (!validateBrowserSchema(database)) {
          database.close();
          finish(failure('corrupt_schema'));
          return;
        }
      } catch {
        database.close();
        finish(failure('corrupt_schema'));
        return;
      }
      const owner: BrowserDatabase = Object.freeze({
        kind: 'browser_database'
      });
      let state: 'ready' | 'closed' = 'ready';
      let reason: 'explicit' | 'version_change' | 'connection_closed' | null =
        null;
      const value: Owner = {
        database,
        close(selected) {
          if (state !== 'ready') return;
          state = 'closed';
          reason = selected;
          database.close();
        },
        snapshot() {
          return { state, reason };
        }
      };
      owners.set(owner, value);
      database.addEventListener('versionchange', () =>
        value.close('version_change')
      );
      database.addEventListener('close', () =>
        value.close('connection_closed')
      );
      finish({ state: 'ready', owner });
    });
  });
}

// Repositories own record validation and transaction completion/reconciliation.
// They must never hold this transaction across a signer or network await.
function validStores(value: unknown): value is readonly BrowserStore[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.length <= browserStores.length &&
    new Set(value).size === value.length &&
    value.every((name: unknown) =>
      browserStores.some((allowed) => allowed === name)
    )
  );
}
export function browserDatabaseTransaction(
  owner: BrowserDatabase,
  stores: readonly BrowserStore[],
  mode: 'readonly' | 'readwrite'
): IDBTransaction {
  const value = ownerOf(owner);
  if (value.snapshot().state !== 'ready') throw new Error('database_closed');
  if (!validStores(stores) || (mode !== 'readonly' && mode !== 'readwrite'))
    throw new Error('invalid_database_transaction');
  return value.database.transaction([...stores], mode);
}
