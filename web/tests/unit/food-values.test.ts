import { describe, expect, it, vi } from 'vitest';
import {
  canonicalFoodAmount,
  foodCurrency,
  foodPrice,
  foodQuantity,
  foodUnit,
  foodUnits
} from '../../src/lib/contracts/food-availability-v1/values';
import {
  boundedEnvelopeNumbers,
  foodPublishedAt,
  safeUnsignedInteger
} from '../../src/lib/nostr/envelope-bounds';
import corpus from '../../../contracts/interop/food_availability/corpus.v1.json';

describe('exact Food decimal values from pinned public Rust', () => {
  for (const vector of corpus.vectors.slice(0, 10))
    it(`preserves actual authored ${vector.id} values without numeric conversion`, () => {
      const details = vector.input.details!;
      expect(foodUnit(details.price.unit)).toBe(details.price.unit);
      expect(
        foodPrice(
          details.price.amount,
          details.price.currency,
          details.price.unit
        )
      ).toEqual(details.price);
      if (details.quantity)
        expect(
          foodQuantity(details.quantity.amount, details.quantity.unit)
        ).toEqual(details.quantity);
      else expect(details.quantity).toBeNull();
    });

  it('has exactly the ten pinned public units with no unknown fallback', () => {
    expect(foodUnits).toEqual([
      'g',
      'kg',
      'lb',
      'oz',
      'each',
      'dozen',
      'bunch',
      'punnet',
      'bag',
      'basket'
    ]);
    for (const value of ['KG', 'lbs', 'item', '', ' kg', undefined, 1])
      expect(foodUnit(value)).toBeUndefined();
  });

  for (const amount of [
    '0',
    '1',
    '0.01',
    '1234567890123456789012345678',
    '12345678901234.56789012345678',
    '0.000000000000000000000000001'
  ])
    it(`retains exact canonical decimal ${amount}`, () => {
      expect(canonicalFoodAmount(amount)).toBe(amount);
      expect(foodPrice(amount, 'CAD', 'lb')?.amount).toBe(amount);
    });

  for (const amount of [
    '',
    '01',
    '00',
    '.1',
    '1.',
    '1.0',
    '0.0',
    '1.20',
    '+1',
    '-1',
    '-0',
    '1e2',
    '1E-2',
    '1,000',
    '1_000',
    '1 000',
    ' 1',
    '1 ',
    '1\n',
    '１',
    '١',
    '0x10',
    'NaN',
    'Infinity',
    '12345678901234567890123456789',
    '0.0000000000000000000000000001'
  ])
    it(`rejects noncanonical or oversized decimal ${JSON.stringify(amount)}`, () => {
      expect(canonicalFoodAmount(amount)).toBeUndefined();
      expect(foodPrice(amount, 'CAD', 'lb')).toBeUndefined();
      expect(foodQuantity(amount, 'lb')).toBeUndefined();
    });

  it('keeps missing price distinct from a valid zero price and rejects zero quantity', () => {
    expect(canonicalFoodAmount(undefined)).toBeUndefined();
    expect(canonicalFoodAmount('0')).toBe('0');
    expect(foodPrice('0', 'CAD', 'lb')).toEqual({
      amount: '0',
      currency: 'CAD',
      unit: 'lb'
    });
    expect(foodQuantity('0', 'lb')).toBeUndefined();
    expect(foodQuantity(undefined, 'lb')).toBeUndefined();
    expect(foodQuantity('0.01', 'lb')).toEqual({ amount: '0.01', unit: 'lb' });
  });

  it('uses exactly three uppercase ASCII currency letters without inventing an ISO registry', () => {
    for (const value of ['CAD', 'USD', 'EUR', 'AAA'])
      expect(foodCurrency(value)).toBe(value);
    for (const value of [
      '',
      'CA',
      'CADD',
      'cad',
      ' Cad',
      'CAD\n',
      'C1D',
      'ＣAD',
      null
    ]) {
      expect(foodCurrency(value)).toBeUndefined();
      expect(foodPrice('1', value, 'lb')).toBeUndefined();
    }
    expect(foodPrice('1', 'CAD', 'unknown')).toBeUndefined();
    expect(foodQuantity('1', 'unknown')).toBeUndefined();
  });

  it('rejects huge strings and nonstrings before coercion or normalization', () => {
    const toString = vi.fn(() => '1');
    for (const value of [
      '1'.repeat(1024 * 1024),
      { toString },
      1,
      1n,
      null,
      Symbol('amount')
    ]) {
      expect(canonicalFoodAmount(value)).toBeUndefined();
      expect(foodCurrency(value)).toBeUndefined();
      expect(foodUnit(value)).toBeUndefined();
    }
    expect(toString).not.toHaveBeenCalled();
  });
});

describe('bounded browser numeric envelopes without an authenticity claim', () => {
  for (const value of [0, 1, 4294967296, Number.MAX_SAFE_INTEGER])
    it(`accepts exact nonnegative safe integer ${value}`, () => {
      expect(safeUnsignedInteger(value)).toBe(value);
      expect(boundedEnvelopeNumbers(30402, value)).toEqual({
        kind: 30402,
        created_at: value
      });
    });

  for (const value of [
    -0,
    -1,
    0.5,
    NaN,
    Infinity,
    -Infinity,
    Number.MAX_SAFE_INTEGER + 1,
    '1',
    1n,
    null,
    undefined
  ])
    it(`rejects unsupported numeric input ${String(value)}`, () => {
      expect(safeUnsignedInteger(value)).toBeUndefined();
      expect(boundedEnvelopeNumbers(30402, value)).toBeUndefined();
      expect(boundedEnvelopeNumbers(value, 1)).toBeUndefined();
    });

  it('bounds kind to the frozen NIP-01 unsigned 16-bit range', () => {
    expect(boundedEnvelopeNumbers(0, 0)).toEqual({ kind: 0, created_at: 0 });
    expect(boundedEnvelopeNumbers(65535, 1)).toEqual({
      kind: 65535,
      created_at: 1
    });
    expect(boundedEnvelopeNumbers(65536, 1)).toBeUndefined();
  });

  it('rejects a parsed maximum-u64 event timestamp instead of silently rounding', () => {
    const raw = JSON.parse(
      '{"kind":30402,"created_at":18446744073709551615}'
    ) as { kind: number; created_at: number };
    expect(Number.isSafeInteger(raw.created_at)).toBe(false);
    expect(boundedEnvelopeNumbers(raw.kind, raw.created_at)).toBeUndefined();
  });

  for (const value of ['1', '4294967296', '9007199254740991'])
    it(`accepts supported exact positive published_at ${value}`, () => {
      expect(foodPublishedAt(value)).toBe(Number(value));
      expect(String(foodPublishedAt(value))).toBe(value);
    });

  for (const value of [
    '0',
    '01',
    '+1',
    '-1',
    '1.0',
    '1e2',
    ' 1',
    '1 ',
    '1\n',
    '١',
    '9007199254740992',
    '9007199254740993',
    '18446744073709551615',
    '9'.repeat(1024 * 1024),
    1,
    null
  ])
    it(`rejects invalid or unsupported published_at ${typeof value === 'string' && value.length > 40 ? 'oversized string' : JSON.stringify(value)}`, () => {
      expect(foodPublishedAt(value)).toBeUndefined();
    });
});
