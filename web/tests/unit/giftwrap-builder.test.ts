import { afterEach, expect, it, vi } from 'vitest';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import type { PrivateRecipientSeal } from '../../src/lib/nostr/seal-builder.ts';
const state = vi.hoisted(() => ({
  seals: new WeakMap<object, Record<string, unknown>>(),
  keys: [] as Uint8Array[],
  fixtureKeys: [] as Uint8Array[],
  conversations: [] as Uint8Array[],
  fail: false,
  response: 'normal'
}));
vi.mock('../../src/lib/nostr/seal-builder.ts', () => ({
  privateSealSnapshot: (token: object) => state.seals.get(token)
}));
vi.mock('applesauce-core/helpers', async (original) => {
  const sdk = await original<typeof import('applesauce-core/helpers')>();
  return {
    ...sdk,
    generateSecretKey: () => {
      const key = sdk.generateSecretKey();
      state.keys.push(key);
      return key;
    },
    finalizeEvent: (...args: Parameters<typeof sdk.finalizeEvent>) => {
      if (state.fail) throw Error('secret exception must not escape');
      const signed = sdk.finalizeEvent(...args);
      if (state.response === 'extra')
        return {
          ...signed,
          plaintext: 'private helper sentinel',
          toJSON: () => {
            throw Error('unexpected object conversion');
          }
        };
      if (state.response === 'wrong_target')
        return sdk.finalizeEvent(
          { ...args[0], tags: [['p', signed.pubkey]] },
          args[1]
        );
      if (state.response === 'cached_bad_id')
        return { ...signed, id: '0'.repeat(64), [sdk.verifiedSymbol]: true };
      return signed;
    }
  };
});
vi.mock('applesauce-core/helpers/encryption', async (original) => {
  const sdk =
    await original<typeof import('applesauce-core/helpers/encryption')>();
  return {
    ...sdk,
    nip44: {
      ...sdk.nip44,
      v2: {
        ...sdk.nip44.v2,
        utils: {
          ...sdk.nip44.v2.utils,
          getConversationKey: (
            ...args: Parameters<typeof sdk.nip44.v2.utils.getConversationKey>
          ) => {
            const key = sdk.nip44.v2.utils.getConversationKey(...args);
            state.conversations.push(key);
            return key;
          }
        }
      }
    }
  };
});
import { generateSecretKey, getPublicKey } from 'applesauce-core/helpers';
import {
  buildPrivateGiftwrap,
  privateGiftwrapSnapshot,
  type PrivateGiftwrap
} from '../../src/lib/nostr/giftwrap-builder.ts';
// Unit-only input port substitution checks crypto cleanup; real genuine token
// admission and nested decryption are separately exercised in Chromium.
function fixture(role: 'peer' | 'self' = 'peer') {
  const key = generateSecretKey(),
    owner = getPublicKey(key),
    peerKey = generateSecretKey(),
    peer = getPublicKey(peerKey);
  state.fixtureKeys.push(key, peerKey);
  const seal = Object.freeze({}) as PrivateRecipientSeal;
  const record = {
    owner,
    peer,
    destination: role === 'self' ? owner : peer,
    role,
    command: 'local intent',
    rumorHash: 'a'.repeat(64),
    wire: '{"encryptedSeal":"unit input port only"}'
  };
  state.seals.set(seal, record);
  state.keys.length = 0;
  return { seal, record, key };
}
afterEach(() => {
  for (const key of [
    ...state.keys,
    ...state.fixtureKeys,
    ...state.conversations
  ])
    key.fill(0);
  state.fixtureKeys.length = 0;
  state.conversations.length = 0;
  state.keys.length = 0;
  state.fail = false;
  state.response = 'normal';
  vi.unstubAllGlobals();
});
it('requires genuine seal ownership and literal review before generating a key', () => {
  vi.stubGlobal('window', {});
  expect(
    buildPrivateGiftwrap({} as PrivateRecipientSeal, 'reviewed_private_wrap')
  ).toBeUndefined();
  const f = fixture();
  expect(buildPrivateGiftwrap(f.seal, 'connect')).toBeUndefined();
  expect(state.keys).toHaveLength(0);
  f.key.fill(0);
  expect(privateGiftwrapSnapshot({} as PrivateGiftwrap)).toBeUndefined();
});
it('does not construct browser wrappers during SSR', () => {
  const f = fixture();
  expect(buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap')).toBeUndefined();
  expect(state.keys).toHaveLength(0);
  f.key.fill(0);
});
it('uses a fresh disposable author and exact one-target1059 with only signed fields', () => {
  vi.stubGlobal('window', {});
  const f = fixture('self'),
    token = buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap');
  expect(token).toBeDefined();
  const saved = privateGiftwrapSnapshot(token!),
    event = JSON.parse(saved!.wire) as {
      kind: number;
      tags: string[][];
      pubkey: string;
      created_at: number;
      content: string;
    };
  expect(event.kind).toBe(1059);
  expect(event.tags).toEqual([['p', f.record.owner]]);
  expect(event.pubkey).not.toBe(f.record.owner);
  expect(event.pubkey).not.toBe(f.record.peer);
  expect(Object.keys(event).sort()).toEqual([
    'content',
    'created_at',
    'id',
    'kind',
    'pubkey',
    'sig',
    'tags'
  ]);
  expect(verifyEnvelope(saved!.wire).ok).toBe(true);
  expect(event.created_at).toBeLessThanOrEqual(Math.floor(Date.now() / 1000));
  expect(event.created_at).toBeGreaterThanOrEqual(
    Math.floor(Date.now() / 1000) - 3600
  );
  expect(saved!.wire).not.toContain('encryptedSeal');
  expect(state.keys).toHaveLength(1);
  expect(state.keys[0].every((x) => x === 0)).toBe(true);
  expect(state.conversations).toHaveLength(1);
  expect(state.conversations[0].every((x) => x === 0)).toBe(true);
  f.key.fill(0);
});
it('clears the directly owned ephemeral key even when the SDK signer throws', () => {
  vi.stubGlobal('window', {});
  const f = fixture();
  state.fail = true;
  expect(buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap')).toBeUndefined();
  expect(state.keys).toHaveLength(1);
  expect(state.keys[0].every((x) => x === 0)).toBe(true);
  expect(state.conversations).toHaveLength(1);
  expect(state.conversations[0].every((x) => x === 0)).toBe(true);
  f.key.fill(0);
});
it('does not return a wrapper after original ownership changes inside SDK key generation', () => {
  vi.stubGlobal('window', {});
  const f = fixture();
  const random = crypto.getRandomValues.bind(crypto);
  let calls = 0;
  const spy = vi.spyOn(crypto, 'getRandomValues').mockImplementation(((
    v: Uint8Array<ArrayBuffer>
  ) => {
    calls++;
    if (calls === 2) state.seals.delete(f.seal);
    return random(v);
  }) as typeof crypto.getRandomValues);
  try {
    expect(
      buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap')
    ).toBeUndefined();
    expect(state.keys.every((k) => k.every((x) => x === 0))).toBe(true);
  } finally {
    spy.mockRestore();
    f.key.fill(0);
  }
});
it('detached wrapper metadata and casts cannot mint tokens; invalidation removes snapshot access', () => {
  vi.stubGlobal('window', {});
  const f = fixture(),
    token = buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap');
  expect(token).toBeDefined();
  const saved = privateGiftwrapSnapshot(token!);
  expect(
    privateGiftwrapSnapshot(saved as unknown as PrivateGiftwrap)
  ).toBeUndefined();
  state.seals.delete(f.seal);
  expect(privateGiftwrapSnapshot(token!)).toBeUndefined();
  f.key.fill(0);
});
it('fresh peer wrappers have distinct authors and ciphertext without exposing key material', () => {
  vi.stubGlobal('window', {});
  const f = fixture(),
    first = buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap'),
    second = buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap');
  const a = JSON.parse(privateGiftwrapSnapshot(first!)!.wire) as {
      pubkey: string;
      content: string;
    },
    b = JSON.parse(privateGiftwrapSnapshot(second!)!.wire) as {
      pubkey: string;
      content: string;
    };
  expect(a.pubkey).not.toBe(b.pubkey);
  expect(a.content).not.toBe(b.content);
  expect(state.keys).toHaveLength(2);
  expect(state.keys.every((k) => k.every((x) => x === 0))).toBe(true);
  // Both peer payloads use actual stock SDK NIP44; inner proof belongs077.
  expect(typeof nip44.v2.decrypt).toBe('function');
  f.key.fill(0);
});

it('discards SDK extra plaintext and hostile conversion rather than spreading the returned object', () => {
  vi.stubGlobal('window', {});
  const f = fixture();
  state.response = 'extra';
  const token = buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap');
  expect(token).toBeDefined();
  const wire = privateGiftwrapSnapshot(token!)!.wire;
  expect(wire).not.toContain('sentinel');
  expect(Object.keys(JSON.parse(wire) as Record<string, unknown>)).toHaveLength(
    7
  );
  f.key.fill(0);
});
for (const response of ['wrong_target', 'cached_bad_id'] as const)
  it(`rejects actual SDK ${response} response despite valid signature or cached verification`, () => {
    vi.stubGlobal('window', {});
    const f = fixture();
    state.response = response;
    expect(
      buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap')
    ).toBeUndefined();
    expect(state.keys).toHaveLength(1);
    expect(state.keys[0].every((x) => x === 0)).toBe(true);
    expect(state.conversations[0].every((x) => x === 0)).toBe(true);
    f.key.fill(0);
  });
for (const fault of ['mismatch', 'exception'] as const)
  it(`outer roundtrip ${fault} clears directly owned buffers and grants no wrapper`, () => {
    vi.stubGlobal('window', {});
    const f = fixture();
    const spy = vi.spyOn(nip44.v2, 'decrypt').mockImplementation(() => {
      if (fault === 'exception') throw Error('SDK roundtrip fault');
      return 'substitute signed seal';
    });
    try {
      expect(
        buildPrivateGiftwrap(f.seal, 'reviewed_private_wrap')
      ).toBeUndefined();
      expect(state.keys).toHaveLength(1);
      expect(state.keys[0].every((x) => x === 0)).toBe(true);
      expect(state.conversations[0].every((x) => x === 0)).toBe(true);
    } finally {
      spy.mockRestore();
      f.key.fill(0);
    }
  });
