import { beforeAll, describe, expect, it } from 'vitest';
import { planDecryptBatch } from '../../src/lib/messaging/inbox-state.ts';
import { PRIVATE_DECRYPTION_BUDGETS } from '../../src/lib/config/budgets.ts';
const ids = (n: number) =>
  Array.from({ length: n }, (_, i) => i.toString(16).padStart(64, '0'));
beforeAll(() => {
  expect(planDecryptBatch).toBeTypeOf('function');
});
describe('explicit envelope decryption work plan', () => {
  it('holds the21st for another explicit action', () => {
    const input = ids(21);
    expect(planDecryptBatch(input)).toEqual({
      selected: input.slice(0, 20),
      remaining: 1
    });
  });
  it('keeps the approved20-envelope maximum for every large queue', () => {
    expect(PRIVATE_DECRYPTION_BUDGETS.envelopesPerAction).toBe(20);
    expect(planDecryptBatch(ids(2000)).selected).toHaveLength(20);
    expect(planDecryptBatch(ids(2000)).remaining).toBe(1980);
  });
  it('preserves opaque stored-ID order instead of inner message or author sorting', () => {
    const input = ids(21).reverse();
    expect(planDecryptBatch(input).selected).toEqual(input.slice(0, 20));
  });
  it('does not schedule the same persisted outer ID twice', () => {
    const input = ids(21);
    expect(planDecryptBatch([input[0], ...input])).toEqual(
      planDecryptBatch(input)
    );
  });
  it('clones a frozen caller queue without mutation or aliases', () => {
    const input = Object.freeze(ids(21));
    const next = planDecryptBatch(input);
    next.selected[0] = 'a'.repeat(64);
    expect(input[0]).toBe('0'.repeat(64));
    expect(planDecryptBatch(input).remaining).toBe(1);
  });
  it('does not invent work for an empty queue', () => {
    expect(planDecryptBatch([])).toEqual({ selected: [], remaining: 0 });
  });
  it('refuses malformed and uppercase ciphertext identities before selecting work', () => {
    for (const value of ['wrong', 'A'.repeat(64), '0'.repeat(63)])
      expect(() => planDecryptBatch([value])).toThrow();
  });
  it('refuses an over-capacity queue without eviction or limit reset', () => {
    expect(() => planDecryptBatch(ids(2001))).toThrow();
  });
  it('does not truncate unsupported IDs after an otherwise complete batch', () => {
    expect(() => planDecryptBatch([...ids(20), 'wrong'])).toThrow();
  });
});
