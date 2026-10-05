import { describe, expect, it } from 'vitest';
import { canonicalPublicKey } from '../../src/lib/contracts/public-key';
import oracle from './reference-oracle.json';

describe('exact public Rust key admission', () => {
  for (const row of oracle.key_cases)
    it(row.pubkey, () => {
      expect(canonicalPublicKey(row.pubkey)).toBe(
        row.accepted ? row.pubkey : undefined
      );
    });
  it('rejects noncanonical and unbounded input', () => {
    const key = oracle.key_cases[0].pubkey;
    for (const value of [
      null,
      {},
      key.toUpperCase(),
      ` ${key}`,
      key + '0',
      'a'.repeat(10000)
    ])
      expect(canonicalPublicKey(value)).toBeUndefined();
  });
});
