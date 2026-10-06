import { describe, expect, it } from 'vitest';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  closeBrowserDatabase,
  openBrowserDatabase
} from '../../src/lib/persistence/database.ts';
import { browserStores } from '../../src/lib/persistence/schema.ts';

describe('browser persistence bootstrap without browser capabilities', () => {
  it('imports without an implicit open and explicitly reports unavailable storage', async () => {
    expect(
      Object.getOwnPropertyDescriptor(globalThis, 'indexedDB')
    ).toHaveProperty('get');
    const result = await openBrowserDatabase();
    expect(result).toMatchObject({
      state: 'unavailable',
      reason: 'open_failed'
    });
    if (result.state !== 'unavailable') throw new Error('unexpected_ready');
    expect(result.message).toContain('still browse');
  });
  it('reports a genuinely absent capability without opening another store', async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'indexedDB');
    if (!original) throw new Error('missing_ssr_guard');
    Object.defineProperty(globalThis, 'indexedDB', {
      configurable: true,
      value: undefined
    });
    try {
      expect(await openBrowserDatabase()).toMatchObject({
        state: 'unavailable',
        reason: 'unavailable'
      });
    } finally {
      Object.defineProperty(globalThis, 'indexedDB', original);
    }
  });
  it('rejects already cancelled work before probing storage', async () => {
    const controller = new AbortController();
    controller.abort();
    expect(await openBrowserDatabase(controller.signal)).toMatchObject({
      state: 'unavailable',
      reason: 'cancelled'
    });
  });
  it('rejects fabricated owner capabilities', () => {
    const fake = Object.freeze({ kind: 'browser_database' as const });
    expect(() => closeBrowserDatabase(fake)).toThrow('invalid_database_owner');
    expect(() => browserDatabaseState(fake)).toThrow('invalid_database_owner');
    expect(() =>
      browserDatabaseTransaction(fake, ['public_drafts'], 'readonly')
    ).toThrow('invalid_database_owner');
  });
  it('keeps namespaces separated and bootstrap metadata immutable', () => {
    expect(browserStores).toHaveLength(7);
    expect(new Set(browserStores).size).toBe(7);
    expect(Object.isFrozen(browserStores)).toBe(true);
  });
});
