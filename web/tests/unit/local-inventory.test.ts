import { describe, expect, it } from 'vitest';
import type { PublicOperationRecord } from '../../src/lib/contracts/local-records.ts';
import {
  publicRecordSettlement,
  publicInventoryRow,
  PUBLIC_CLEANUP_CONSEQUENCES
} from '../../src/lib/persistence/local-inventory.ts';
const id = '3d030bf5-901d-45e1-8251-41cbdf805e96';
const row: PublicOperationRecord = {
  schema: 1,
  family: 'public_operation',
  owner: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  id,
  revision: 0,
  source: { type: 'draft', id, revision: 0 },
  capture: {
    kind: 30402,
    wire: 'controlled policy input only',
    hash: 'a'.repeat(64),
    targets: ['wss://one.example.org', 'wss://two.example.org'],
    policyFingerprint: 'b'.repeat(64)
  },
  artifact: null,
  receipts: []
};
// Pure classification only; these deliberately controlled policy inputs grant
// no storage capability. Actual codecs/native IDB qualify browser admission.
function receipt(
  origin: string,
  status: 'accepted' | 'refused' | 'unknown' | 'timed_out' | 'stopped'
) {
  return {
    actionId: id,
    origin,
    role: 'publication' as const,
    attempt: 1,
    eventId: 'a'.repeat(64),
    status,
    observedAtMilliseconds: 1000,
    readbackWire: null
  };
}
describe('conservative public cleanup policy', () => {
  it('unsigned capture and signed intent with no receipts stay protected', () => {
    expect(publicRecordSettlement(row)).toBe('protected');
    expect(
      publicRecordSettlement({
        ...row,
        artifact: { eventId: 'a'.repeat(64), wire: 'controlled' }
      })
    ).toBe('protected');
  });
  it('missing target evidence stays protected', () => {
    expect(
      publicRecordSettlement({
        ...row,
        artifact: { eventId: 'a'.repeat(64), wire: 'controlled' },
        receipts: [receipt(row.capture.targets[0], 'accepted')]
      })
    ).toBe('protected');
  });
  for (const status of ['unknown', 'timed_out', 'stopped'] as const)
    it(
      'retains unresolved ' +
        status +
        ' even alongside later positive observations',
      () => {
        expect(
          publicRecordSettlement({
            ...row,
            artifact: { eventId: 'a'.repeat(64), wire: 'controlled' },
            receipts: [
              receipt(row.capture.targets[0], status),
              receipt(row.capture.targets[0], 'accepted'),
              receipt(row.capture.targets[1], 'accepted')
            ]
          })
        ).toBe('protected');
      }
    );
  it('all named terminal observations allow local settlement without claiming remote deletion', () => {
    const record = {
      ...row,
      artifact: { eventId: 'a'.repeat(64), wire: 'controlled' },
      receipts: [
        receipt(row.capture.targets[0], 'accepted'),
        receipt(row.capture.targets[1], 'refused')
      ]
    };
    expect(publicRecordSettlement(record)).toBe('settled');
    expect(publicInventoryRow(record, 123)).toEqual({
      key: 'public_operation:' + id,
      family: 'public_operation',
      id,
      revision: 0,
      logicalBytes: 123,
      state: 'settled'
    });
    expect(PUBLIC_CLEANUP_CONSEQUENCES).toContain(
      'does not delete remote copies'
    );
    expect(PUBLIC_CLEANUP_CONSEQUENCES).toContain('not a backup');
  });
});
