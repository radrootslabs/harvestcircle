import { expect, it } from 'vitest';
import { inspectPrivateSendReservation } from '../../src/lib/persistence/private-send-reservations.ts';
const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5',
  id = '12345678-1234-4234-8234-123456789abc';
const row = {
  schema: 1,
  family: 'private_send_reservation',
  owner,
  id,
  revision: 0,
  peer,
  rumorHash: 'f'.repeat(64),
  createdAt: 1700000100
};
const read = (value: unknown) =>
  inspectPrivateSendReservation(JSON.stringify(value), owner, id);
it('reservation contains stable command and rumor metadata without plaintext fields', () => {
  expect(read(row)).toEqual(row);
  expect(Object.keys(read(row)!).sort()).toEqual([
    'createdAt',
    'family',
    'id',
    'owner',
    'peer',
    'revision',
    'rumorHash',
    'schema'
  ]);
});
it('body, subject, private context, tags, wire and unknown fields are not admitted', () => {
  for (const key of [
    'body',
    'content',
    'subject',
    'product',
    'contact',
    'tags',
    'wire',
    'unknown'
  ])
    expect(read({ ...row, [key]: 'Private sentinel' })).toBeUndefined();
});
it('owner and UUID command namespace are exact and cannot be coerced', () => {
  expect(
    inspectPrivateSendReservation(JSON.stringify(row), peer, id)
  ).toBeUndefined();
  expect(
    inspectPrivateSendReservation(
      JSON.stringify(row),
      owner,
      '12345678-1234-4234-8234-123456789abd'
    )
  ).toBeUndefined();
  for (const value of [
    null,
    1,
    owner.toUpperCase(),
    '0'.repeat(64),
    { toString: () => owner }
  ])
    expect(
      inspectPrivateSendReservation(JSON.stringify(row), value, id)
    ).toBeUndefined();
  for (const value of [
    null,
    id.toUpperCase(),
    'a'.repeat(64),
    '12345678-1234-1234-8234-123456789abc'
  ])
    expect(read({ ...row, id: value })).toBeUndefined();
});
it('self, malformed/noncanonical peers and unsupported revisions/families are rejected', () => {
  for (const value of [owner, peer.toUpperCase(), '0'.repeat(64), null])
    expect(read({ ...row, peer: value })).toBeUndefined();
  for (const value of [1, -1, '0', null])
    expect(read({ ...row, revision: value })).toBeUndefined();
  expect(read({ ...row, schema: 2 })).toBeUndefined();
  expect(read({ ...row, family: 'private_send' })).toBeUndefined();
});
it('rumor ID and timestamp are bounded canonical data, never floats or coercions', () => {
  for (const value of ['F'.repeat(64), 'f'.repeat(63), null, 0])
    expect(read({ ...row, rumorHash: value })).toBeUndefined();
  for (const value of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, '100', null])
    expect(read({ ...row, createdAt: value })).toBeUndefined();
  expect(
    inspectPrivateSendReservation(
      JSON.stringify(row).replace('1700000100', '-0'),
      owner,
      id
    )
  ).toBeUndefined();
  expect(read({ ...row, createdAt: 0 })!.createdAt).toBe(0);
});
it('raw objects cannot invoke conversion and oversized/malformed input stays unsupported', () => {
  let converted = false;
  expect(
    inspectPrivateSendReservation(
      {
        toJSON() {
          converted = true;
          return row;
        }
      },
      owner,
      id
    )
  ).toBeUndefined();
  expect(converted).toBe(false);
  for (const raw of ['', '{', 'null', '[]', ' '.repeat(4097)])
    expect(inspectPrivateSendReservation(raw, owner, id)).toBeUndefined();
});
