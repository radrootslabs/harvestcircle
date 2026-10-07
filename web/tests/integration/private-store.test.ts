import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  finalizeEvent,
  EncryptedContentSymbol,
  verifiedSymbol
} from 'applesauce-core/helpers';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  recheckIdentityOwner
} from '../../src/lib/runtime/identity-session.ts';
import { createPrivateSession } from '../../src/lib/runtime/private-session.ts';
import {
  getPrivateStore,
  insertPrivateEnvelope,
  privateStoreEnvelope,
  privateStoreSnapshot
} from '../../src/lib/messaging/private-store.ts';
import {
  getPrivateCacheScope,
  retainPrivateCacheWire,
  privateCacheWire,
  privateCacheSnapshot,
  clearPrivateCacheScope,
  closePrivateCacheScope
} from '../../src/lib/nostr/private-cache-scope.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../../src/lib/nostr/verified-envelope.ts';
import {
  getPublicStore,
  insertPublicEnvelope,
  publicStoreEnvelope,
  closePublicStore
} from '../../src/lib/nostr/public-store.ts';

// HC_TEST_ONLY_PROVIDER: genuine SDK ownership; echo probe is not encryption qualification.
await test('disposable private store and synthetic ownership cache follow genuine SDK session generations', async (t) => {
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  const other =
    'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
  assert.equal(typeof window, 'undefined');
  assert.equal(getPrivateStore({} as never), undefined);
  let key = owner,
    prompts = 0;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey: () => {
          prompts++;
          return Promise.resolve(key);
        },
        signEvent: () =>
          Promise.reject(new Error('HC_TEST_ONLY_UNEXPECTED_SIGN')),
        nip44: {
          encrypt: (_peer: string, text: string) =>
            Promise.resolve('fixture:' + text),
          decrypt: (_peer: string, text: string) =>
            Promise.resolve(text.slice(8))
        }
      }
    }
  });
  const identity = createIdentitySession();
  function signed(
    kind: number,
    tags: string[][] = [],
    content = 'HC_TEST_ONLY_CIPHERTEXT'
  ) {
    const ephemeral = crypto.getRandomValues(new Uint8Array(32));
    try {
      return JSON.stringify(
        finalizeEvent(
          { kind, tags, content, created_at: 1700000060 },
          ephemeral
        )
      );
    } finally {
      ephemeral.fill(0);
    }
  }
  function proof(wire: string) {
    const r = verifyEnvelope(wire);
    assert.ok(r.ok);
    return r.value;
  }
  async function connect() {
    assert.equal((await connectIdentity(identity)).state, 'signing_only');
    assert.equal(
      (await probeIdentityMessaging(identity, 'reviewed_self_copy')).state,
      'messaging_capable'
    );
    const session = await createPrivateSession(
      identity,
      'reviewed_private_session'
    );
    assert.ok(session);
    return session;
  }
  let publicStore: ReturnType<typeof getPublicStore>;
  try {
    await t.test(
      'guest and forged browser sessions cannot acquire owned state',
      () => {
        assert.throws(
          () => getPrivateStore({} as never),
          /private_session_invalid/
        );
        assert.throws(
          () => getPrivateCacheScope({} as never, 'projection_ownership'),
          /private_session_invalid/
        );
        assert.equal(prompts, 0);
      }
    );
    const session = await connect();
    const store = getPrivateStore(session);
    assert.ok(store);
    const cache = getPrivateCacheScope(session, 'projection_ownership');
    assert.ok(cache);
    const outer = proof(signed(1059, [['p', owner]]));
    const id = verifiedEnvelopeSnapshot(outer)!.id;
    await t.test(
      'verified owner-addressed ciphertext is isolated, immutable and duplicate-safe',
      () => {
        assert.equal(getPrivateStore(session), store);
        assert.equal(insertPrivateEnvelope(store, outer), 'accepted');
        assert.equal(insertPrivateEnvelope(store, outer), 'duplicate');
        const view = verifiedEnvelopeSnapshot(privateStoreEnvelope(store, id)!);
        assert.ok(view);
        view.content = 'HC_TEST_ONLY_MUTATION';
        view.tags[0][1] = other;
        assert.equal(
          verifiedEnvelopeSnapshot(privateStoreEnvelope(store, id)!)!.content,
          'HC_TEST_ONLY_CIPHERTEXT'
        );
        assert.equal(privateStoreSnapshot(store).count, 1);
        publicStore = getPublicStore();
        assert.ok(publicStore);
        assert.equal(insertPublicEnvelope(publicStore, outer), 'not_public');
        assert.equal(publicStoreEnvelope(publicStore, id), undefined);
      }
    );
    await t.test(
      'private kind, exact routing and original outer metadata boundaries reject before retention',
      () => {
        for (const kind of [0, 13, 14, 30402, 22242])
          assert.equal(
            insertPrivateEnvelope(store, proof(signed(kind, [['p', owner]]))),
            'rejected'
          );
        for (const tags of [
          [],
          [['p', other]],
          [['p', owner, 'hint']],
          [
            ['p', owner],
            ['p', owner]
          ],
          [
            ['p', owner],
            ['subject', 'private']
          ],
          [
            ['p', owner],
            ['client', 'private']
          ],
          [
            ['p', owner],
            ['e', id]
          ]
        ])
          assert.equal(
            insertPrivateEnvelope(store, proof(signed(1059, tags))),
            'rejected'
          );
        assert.equal(insertPrivateEnvelope(store, {} as never), 'rejected');
        assert.equal(
          insertPrivateEnvelope(
            store,
            proof(signed(1059, [['p', owner]], 'x'.repeat(32768)))
          ),
          'rejected'
        );
        assert.equal(privateStoreSnapshot(store).count, 1);
      }
    );
    await t.test(
      'imported decrypted and verified symbols cannot gain trust or cross retained signed fields',
      () => {
        const dirty = JSON.parse(signed(1059, [['p', owner]])) as Record<
          string | symbol,
          unknown
        >;
        dirty.decrypted = 'HC_TEST_ONLY_IMPORTED_PREVIEW';
        dirty[EncryptedContentSymbol] = {
          body: 'HC_TEST_ONLY_IMPORTED_HELPER'
        };
        dirty[verifiedSymbol] = true;
        assert.equal(insertPrivateEnvelope(store, dirty as never), 'rejected');
        const token = proof(JSON.stringify(dirty));
        assert.equal(insertPrivateEnvelope(store, token), 'accepted');
        const clean = verifiedEnvelopeSnapshot(
          privateStoreEnvelope(store, verifiedEnvelopeSnapshot(token)!.id)!
        );
        assert.ok(clean);
        assert.equal('decrypted' in clean, false);
        assert.deepEqual(Object.getOwnPropertySymbols(clean), []);
      }
    );
    await t.test(
      'synthetic projection ownership is bounded JSON memory, never validated message admission',
      () => {
        const k = 'f'.repeat(64),
          wire = JSON.stringify({ body: 'HC_TEST_ONLY_SYNTHETIC_OWNERSHIP' });
        assert.equal(retainPrivateCacheWire(cache, k, wire), 'accepted');
        assert.equal(retainPrivateCacheWire(cache, k, wire), 'duplicate');
        assert.equal(retainPrivateCacheWire(cache, k, '{}'), 'conflict');
        assert.equal(privateCacheWire(cache, k), wire);
        assert.equal(
          retainPrivateCacheWire(cache, 'e'.repeat(64), {
            [EncryptedContentSymbol]: 'HC_TEST_ONLY_HELPER'
          }),
          'rejected'
        );
        assert.equal(
          retainPrivateCacheWire(cache, 'e'.repeat(64), 'x'.repeat(32769)),
          'rejected'
        );
        clearPrivateCacheScope(cache);
        assert.equal(privateCacheWire(cache, k), undefined);
        for (let i = 0; i < 2000; i++)
          assert.equal(
            retainPrivateCacheWire(
              cache,
              i.toString(16).padStart(64, '0'),
              '{}'
            ),
            'accepted'
          );
        assert.equal(
          retainPrivateCacheWire(cache, 'd'.repeat(64), '{}'),
          'limit'
        );
        assert.equal(privateCacheSnapshot(cache).count, 2000);
        clearPrivateCacheScope(cache);
        const payload = JSON.stringify('x'.repeat(32766));
        assert.equal(new TextEncoder().encode(payload).length, 32768);
        for (let i = 0; i < 1536; i++)
          assert.equal(
            retainPrivateCacheWire(
              cache,
              i.toString(16).padStart(64, '0'),
              payload
            ),
            'accepted'
          );
        assert.equal(privateCacheSnapshot(cache).bytes, 50331648);
        assert.equal(
          retainPrivateCacheWire(cache, 'd'.repeat(64), '0'),
          'limit'
        );
        assert.equal(privateCacheSnapshot(cache).bytes, 50331648);
        clearPrivateCacheScope(cache);
        assert.equal(
          retainPrivateCacheWire(cache, 'f'.repeat(64), wire),
          'accepted'
        );
      }
    );
    await t.test(
      'disconnect discards all owned raw and synthetic preview references and late writes',
      () => {
        const late = '{"body":"HC_TEST_ONLY_LATE_OWNERSHIP"}';
        const originalParse = JSON.parse.bind(JSON);
        const savedParse = JSON.parse;
        JSON.parse = (...args: Parameters<typeof JSON.parse>) => {
          const value: unknown = originalParse(...args);
          if (args[0] === late) disconnectIdentity(identity);
          return value;
        };
        try {
          assert.equal(
            retainPrivateCacheWire(cache, 'c'.repeat(64), late),
            'closed'
          );
        } finally {
          JSON.parse = savedParse;
        }
        assert.equal(privateStoreEnvelope(store, id), undefined);
        assert.deepEqual(privateStoreSnapshot(store), {
          closed: true,
          count: 0,
          bytes: 0
        });
        assert.equal(insertPrivateEnvelope(store, outer), 'closed');
        assert.equal(privateCacheWire(cache, 'f'.repeat(64)), undefined);
        assert.deepEqual(privateCacheSnapshot(cache), {
          closed: true,
          count: 0,
          bytes: 0
        });
        assert.equal(
          retainPrivateCacheWire(cache, 'a'.repeat(64), '{}'),
          'closed'
        );
        closePrivateCacheScope(cache);
      }
    );
    await t.test(
      'same-author reconnect cannot revive old store or preview; account replacement remains isolated',
      async () => {
        const again = await connect();
        assert.notEqual(again, session);
        const next = getPrivateStore(again);
        assert.ok(next);
        assert.notEqual(next, store);
        assert.equal(privateStoreEnvelope(next, id), undefined);
        assert.equal(
          privateCacheWire(
            getPrivateCacheScope(again, 'projection_ownership')!,
            'f'.repeat(64)
          ),
          undefined
        );
        key = other;
        assert.equal((await recheckIdentityOwner(identity)).state, 'guest');
        assert.equal(privateStoreSnapshot(next).closed, true);
        const changed = await connect();
        const foreign = getPrivateStore(changed);
        assert.ok(foreign);
        assert.equal(privateStoreEnvelope(foreign, id), undefined);
        assert.equal(insertPrivateEnvelope(foreign, outer), 'rejected');
        assert.equal(privateStoreEnvelope(store, id), undefined);
        assert.ok(publicStore);
        const publicToken = proof(signed(0, [], '{}'));
        const publicId = verifiedEnvelopeSnapshot(publicToken)!.id;
        assert.equal(
          insertPublicEnvelope(publicStore, publicToken),
          'accepted'
        );
        assert.ok(publicStoreEnvelope(publicStore, publicId));
      }
    );
  } finally {
    disconnectIdentity(identity);
    if (publicStore) closePublicStore(publicStore);
    Reflect.deleteProperty(globalThis, 'window');
  }
});
