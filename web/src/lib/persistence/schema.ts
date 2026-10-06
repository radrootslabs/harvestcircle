// Browser-only bootstrap metadata. Record validation, revisions and quotas
// belong to typed repositories; this schema never imports desktop state.
export const browserDatabaseName = 'harvestcircle_browser';
export const browserSchemaVersion = 1;
export const browserStores = Object.freeze([
  'public_drafts',
  'public_operations',
  'preference_operations',
  'private_sends',
  'received_envelopes',
  'conversations',
  'local_preferences'
] as const);
export type BrowserStore = (typeof browserStores)[number];

export function initializeBrowserSchema(
  database: IDBDatabase,
  oldVersion: number,
  newVersion: number | null
): void {
  // No released browser predecessor exists. Future migration owners must
  // explicitly qualify preservation of all uncertain records before upgrading.
  if (oldVersion !== 0 || newVersion !== browserSchemaVersion)
    throw new Error('unsupported_browser_migration');
  for (const name of browserStores) {
    const store = database.createObjectStore(name, {
      keyPath: ['owner', 'id']
    });
    store.createIndex('by_owner', 'owner', {
      unique: false,
      multiEntry: false
    });
  }
}

export function validateBrowserSchema(database: IDBDatabase): boolean {
  if (
    database.name !== browserDatabaseName ||
    database.version !== browserSchemaVersion ||
    database.objectStoreNames.length !== browserStores.length ||
    browserStores.some((name) => !database.objectStoreNames.contains(name))
  )
    return false;
  const transaction = database.transaction([...browserStores], 'readonly');
  for (const name of browserStores) {
    const store = transaction.objectStore(name);
    const key = store.keyPath;
    if (
      !Array.isArray(key) ||
      key.length !== 2 ||
      key[0] !== 'owner' ||
      key[1] !== 'id' ||
      store.autoIncrement ||
      store.indexNames.length !== 1 ||
      !store.indexNames.contains('by_owner')
    )
      return false;
    const index = store.index('by_owner');
    if (index.keyPath !== 'owner' || index.unique || index.multiEntry)
      return false;
  }
  return true;
}
