import { expect, it } from 'vitest';
import { createIdentitySession } from '../../src/lib/runtime/identity-session.ts';
import { createPrivateSession } from '../../src/lib/runtime/private-session.ts';
import {
  getPrivateStore,
  privateStoreEnvelope,
  closePrivateStore
} from '../../src/lib/messaging/private-store.ts';
import {
  getPrivateCacheScope,
  privateCacheWire,
  privateCacheSnapshot,
  closePrivateCacheScope
} from '../../src/lib/nostr/private-cache-scope.ts';

it('SSR private store and cache acquisition has no browser or extension effect', async () => {
  expect(getPrivateStore({} as never)).toBeUndefined();
  expect(
    getPrivateCacheScope({} as never, 'projection_ownership')
  ).toBeUndefined();
  expect(
    await createPrivateSession(
      createIdentitySession(),
      'reviewed_private_session'
    )
  ).toBeUndefined();
});
it('forged private store and generic cache handles provide no read or disposal authority', () => {
  expect(() => privateStoreEnvelope({} as never, '0'.repeat(64))).toThrow(
    'private_store_invalid'
  );
  expect(() => privateCacheWire({} as never, '0'.repeat(64))).toThrow(
    'private_cache_invalid'
  );
  expect(() => privateCacheSnapshot({} as never)).toThrow(
    'private_cache_invalid'
  );
  expect(() => closePrivateStore({} as never)).toThrow('private_store_invalid');
  expect(() => closePrivateCacheScope({} as never)).toThrow(
    'private_cache_invalid'
  );
});
