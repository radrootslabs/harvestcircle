import { afterAll, expect, it } from 'vitest';
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
  verifiedSymbol,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import {
  sealCiphertextV2,
  captureSealTemplate,
  bindSealResponse
} from '../../src/lib/nostr/seal-template.ts';
import { isPublicEnvelopeKind } from '../../src/lib/nostr/public-kinds.ts';
const key = generateSecretKey();
const owner = getPublicKey(key);
afterAll(() => key.fill(0));
const cipher = btoa(String.fromCharCode(2) + 'x'.repeat(98));
function fixture() {
  const wire = captureSealTemplate(owner, cipher, 1700000000);
  if (!wire) throw Error('expected source seal template');
  return { wire, template: JSON.parse(wire) as UnsignedEvent };
}
it('captures only encrypted kind13 with exact sender/time and empty tags; public lane excludes it', () => {
  const { template } = fixture();
  expect(template).toEqual({
    pubkey: owner,
    kind: 13,
    created_at: 1700000000,
    tags: [],
    content: cipher
  });
  expect(isPublicEnvelopeKind(13)).toBe(false);
  expect(isPublicEnvelopeKind(14)).toBe(false);
});
it('requires bounded canonical v2 encoded payload and exact numeric/owner inputs', () => {
  expect(sealCiphertextV2(cipher)).toBe(true);
  for (const bad of [
    null,
    '',
    'plain text',
    '#version',
    cipher + '\n',
    btoa(String.fromCharCode(1) + 'x'.repeat(98)),
    'A'.repeat(16385)
  ])
    expect(sealCiphertextV2(bad)).toBe(false);
  for (const bad of [-0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, '100'])
    expect(captureSealTemplate(owner, cipher, bad)).toBeUndefined();
  expect(captureSealTemplate(owner.toUpperCase(), cipher, 1)).toBeUndefined();
});
it('accepts genuine signatures while reconstructing only seven allowed signed fields', () => {
  const { wire, template } = fixture();
  const signed = finalizeEvent(template, key);
  const result = bindSealResponse(wire, {
    ...signed,
    plaintext: 'private sentinel',
    toJSON: () => {
      throw Error('must not execute');
    }
  });
  expect(result).toBeDefined();
  expect(
    Object.keys(JSON.parse(result!) as Record<string, unknown>).sort()
  ).toEqual(['content', 'created_at', 'id', 'kind', 'pubkey', 'sig', 'tags']);
  expect(result).not.toContain('sentinel');
});
it('rejects valid signatures for substituted ciphertext, kind, tags, time or sender', () => {
  const { wire, template } = fixture();
  for (const changed of [
    { ...template, content: btoa(String.fromCharCode(2) + 'y'.repeat(98)) },
    { ...template, kind: 1059 },
    { ...template, tags: [['p', owner]] },
    { ...template, created_at: 1700000001 }
  ])
    expect(bindSealResponse(wire, finalizeEvent(changed, key))).toBeUndefined();
  const other = generateSecretKey();
  try {
    expect(
      bindSealResponse(wire, finalizeEvent(template, other))
    ).toBeUndefined();
  } finally {
    other.fill(0);
  }
});
it('freshly verifies hash and signature despite provider cache symbols', () => {
  const { wire, template } = fixture();
  const signed = finalizeEvent(template, key);
  expect(
    bindSealResponse(wire, {
      ...signed,
      id: '0'.repeat(64),
      [verifiedSymbol]: true
    })
  ).toBeUndefined();
  expect(
    bindSealResponse(wire, {
      ...signed,
      sig: '0'.repeat(128),
      [verifiedSymbol]: true
    })
  ).toBeUndefined();
});
it('own data fields only: getters/toJSON/iterators cannot supply accepted fields', () => {
  const { wire, template } = fixture();
  const signed = finalizeEvent(template, key);
  let reads = 0;
  const response = { ...signed };
  Object.defineProperty(response, 'content', {
    get() {
      reads++;
      return cipher;
    }
  });
  expect(bindSealResponse(wire, response)).toBeUndefined();
  expect(reads).toBe(0);
  const tags: string[] = [];
  Object.defineProperty(tags, Symbol.iterator, {
    value: () => {
      throw Error('must not enumerate');
    }
  });
  expect(bindSealResponse(wire, { ...signed, tags })).toBeDefined();
});
it('malformed templates/objects and nonempty provider tags never become private seals', () => {
  const { wire, template } = fixture();
  for (const bad of [null, [], {}, '{', ' '.repeat(16385)])
    expect(bindSealResponse(wire, bad)).toBeUndefined();
  expect(bindSealResponse('{}', finalizeEvent(template, key))).toBeUndefined();
  expect(
    bindSealResponse(wire, {
      ...finalizeEvent(template, key),
      tags: [['client', 'leak']]
    })
  ).toBeUndefined();
});
