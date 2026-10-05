import { describe, expect, it, vi } from 'vitest';
import * as pointers from 'applesauce-core/helpers/pointers';
import { bech32 } from '@scure/base';
import {
  decodeProductReference,
  encodeProductReference
} from '../../src/lib/nostr/references';
import { productHref, productEditHref } from '../../src/lib/routes';
import { match } from '../../src/params/naddr';
import oracle from './reference-oracle.json';

// Instrument the actual export; its implementation and decoded results remain
// real. Native ESM namespaces themselves are immutable and cannot be spied on.
vi.mock('applesauce-core/helpers/pointers', { spy: true });

const pubkey = oracle.key_cases[0].pubkey;
const encode = (identifier: string, kind = 30402) =>
  pointers.naddrEncode({
    kind,
    pubkey,
    identifier,
    relays: ['wss://example.invalid']
  });
const raw = (input: string) => {
  const decoded = bech32.decode(input, 2048);
  return Uint8Array.from(bech32.fromWords(decoded.words));
};
const wrap = (data: Uint8Array) =>
  bech32.encode('naddr', bech32.toWords(data), 2048);

describe('bounded exact product references', () => {
  for (const row of oracle.coordinate_cases)
    it(`Rust raw identifier ${JSON.stringify(row.identifier)}`, () => {
      const input = encode(row.identifier);
      const result = decodeProductReference(input);
      expect(result).toEqual({
        kind: 30402,
        pubkey,
        identifier: row.identifier,
        naddr: pointers.naddrEncode({
          kind: 30402,
          pubkey,
          identifier: row.identifier
        })
      });
      expect(
        encodeProductReference({
          kind: 30402,
          pubkey,
          identifier: row.identifier
        })
      ).toBe(result?.naddr);
      expect(decodeProductReference(result?.naddr)?.identifier).toBe(
        row.identifier
      );
      expect(productHref(input)).toBe(`/products/${result?.naddr}`);
      expect(productEditHref(input)).toBe(`/products/${result?.naddr}/edit`);
      expect(match(input)).toBe(true);
      expect(decodeProductReference(input.toUpperCase())).toEqual(result);
    });
  it('ignores unknown framed TLVs and uses the selected decoder first identifier', () => {
    const bytes = raw(encode('Carrots:菜'));
    const changed = wrap(Uint8Array.from([99, 3, 1, 2, 3, ...bytes, 0, 1, 65]));
    expect(decodeProductReference(changed)?.identifier).toBe('Carrots:菜');
    expect(decodeProductReference(changed)?.naddr).toBe(
      encodeProductReference({ kind: 30402, pubkey, identifier: 'Carrots:菜' })
    );
  });
  it('rejects malformed UTF8 instead of replacement, but accepts actual U+FFFD', () => {
    const bytes = raw(encode('a'));
    for (const invalid of [
      [0xff],
      [0xc0, 0xaf],
      [0xed, 0xa0, 0x80],
      [0xf4, 0x90, 0x80, 0x80],
      [0xe2, 0x82]
    ]) {
      const input = wrap(
        Uint8Array.from([0, invalid.length, ...invalid, ...bytes.slice(3)])
      );
      expect(decodeProductReference(input)).toBeUndefined();
    }
    expect(decodeProductReference(encode('\ufffd'))?.identifier).toBe('\ufffd');
  });
  it('rejects truncated TLV framing including malformed unknown TLVs', () => {
    const bytes = raw(encode('a'));
    for (const suffix of [[99], [99, 2, 1]])
      expect(
        decodeProductReference(wrap(Uint8Array.from([...bytes, ...suffix])))
      ).toBeUndefined();
  });
  it('admits exactly 2048 bytes and rejects overflow before decoding', () => {
    const bytes = [...raw(encode('Carrots:菜'))];
    // 1272 payload bytes encode to 2036 words plus the prefix/checksum:2048.
    while (bytes.length < 1272) {
      const length = Math.min(255, 1272 - bytes.length - 2);
      bytes.push(99, length, ...Array.from({ length }, () => 0));
    }
    const input = wrap(Uint8Array.from(bytes));
    expect(new TextEncoder().encode(input).length).toBe(2048);
    expect(decodeProductReference(input)?.identifier).toBe('Carrots:菜');
    const spy = vi.spyOn(pointers, 'decodePointer');
    spy.mockClear();
    try {
      expect(decodeProductReference(input + 'q')).toBeUndefined();
      expect(spy).not.toHaveBeenCalled();
    } finally {
      spy.mockRestore();
    }
  });
  it('bounds before Applesauce decoding', () => {
    const spy = vi.spyOn(pointers, 'decodePointer');
    try {
      for (const input of [
        'naddr1' + 'q'.repeat(2043),
        '菜'.repeat(683),
        'naddr1' + '\ud800',
        null,
        {},
        ''
      ])
        expect(decodeProductReference(input)).toBeUndefined();
      expect(spy).not.toHaveBeenCalled();
    } finally {
      spy.mockRestore();
    }
  });
  it('rejects unsupported/mixed-case/bad keys and never fetches', () => {
    const value = encode('a');
    const bad = [
      value.slice(0, 7).toUpperCase() + value.slice(7),
      value + 'q',
      encode('a', 1),
      pointers.naddrEncode({
        kind: 30402,
        pubkey: '0'.repeat(64),
        identifier: 'a'
      }),
      pointers.naddrEncode({
        kind: 30402,
        pubkey: 'f'.repeat(64),
        identifier: 'a'
      }),
      'https://example.invalid/',
      `nostr:${value}`
    ];
    const fetchSpy = vi.spyOn(globalThis, 'fetch');
    try {
      for (const input of bad) {
        expect(decodeProductReference(input)).toBeUndefined();
        expect(productHref(input)).toBeUndefined();
        expect(productEditHref(input)).toBeUndefined();
        expect(match(input)).toBe(false);
      }
      expect(fetchSpy).not.toHaveBeenCalled();
    } finally {
      fetchSpy.mockRestore();
    }
  });
  it('does not truncate unencodable identifier bytes or repair invalid input', () => {
    for (const identifier of ['x'.repeat(256), '菜'.repeat(86), '\ud800'])
      expect(
        encodeProductReference({ kind: 30402, pubkey, identifier })
      ).toBeUndefined();
    expect(
      encodeProductReference({ kind: 1, pubkey, identifier: 'a' })
    ).toBeUndefined();
    expect(
      encodeProductReference({
        kind: 30402,
        pubkey: pubkey.toUpperCase(),
        identifier: 'a'
      })
    ).toBeUndefined();
    expect(
      encodeProductReference({
        kind: 30402,
        pubkey,
        identifier: 'x'.repeat(255)
      })
    ).toBeDefined();
  });
});
