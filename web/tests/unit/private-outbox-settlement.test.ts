import { describe, expect, it } from 'vitest';
import type {
  PrivateSendOperation,
  PrivateTargetReceipt
} from '../../src/lib/persistence/private-records.ts';
import {
  privateRecordSettlement,
  privateInventoryRow,
  PRIVATE_OUTBOX_CLEANUP_CONSEQUENCES
} from '../../src/lib/persistence/local-inventory.ts';
const owner = 'a'.repeat(64),
  peer = 'b'.repeat(64),
  id = '12345678-1234-4234-8234-123456789abc';
const route = (role: 'peer' | 'self_archive', origin: string) => ({
  role,
  author: role === 'peer' ? peer : owner,
  targets: [origin],
  knownBase: {
    author: role === 'peer' ? peer : owner,
    id: 'c'.repeat(64),
    createdAt: 100
  },
  sources: [{ source: 'wss://discovery.example.org', state: 'eose' as const }]
});
const routes = {
  peer: route('peer', 'wss://peer.example.org'),
  archive: route('self_archive', 'wss://archive.example.org')
};
function fact(
  role: 'peer' | 'self_archive',
  status: PrivateTargetReceipt['status'] = 'accepted',
  origin = role === 'peer' ? routes.peer.targets[0] : routes.archive.targets[0]
): PrivateTargetReceipt {
  return {
    actionId: id,
    origin,
    role,
    attempt: 1,
    eventId: role === 'peer' ? 'd'.repeat(64) : 'e'.repeat(64),
    status,
    observedAtMilliseconds: 1000,
    readbackWire: null
  };
}
// Pure structural bookkeeping inputs only. These objects cannot admit storage,
// mint genuine sender/relay receipts, authorize deletion or qualify delivery.
const operation: PrivateSendOperation = {
  schema: 1,
  family: 'private_send_operation',
  owner,
  peer,
  id,
  revision: 1,
  rumorHash: 'f'.repeat(64),
  createdAt: 100,
  self: { eventId: 'e'.repeat(64), wire: 'controlled' },
  peerArtifact: { eventId: 'd'.repeat(64), wire: 'controlled' },
  deliveryPlan: { state: 'prepared', routes },
  receipts: [fact('peer'), fact('self_archive')]
};
describe('conservative settled private-outbox eligibility', () => {
  it('requires both artifacts, an exact prepared route and every current named acceptance', () => {
    expect(privateRecordSettlement(operation)).toBe('settled');
    expect(privateRecordSettlement({ ...operation, peerArtifact: null })).toBe(
      'protected'
    );
    expect(
      privateRecordSettlement({ ...operation, deliveryPlan: undefined })
    ).toBe('protected');
    expect(privateRecordSettlement({ ...operation, receipts: [] })).toBe(
      'protected'
    );
    expect(
      privateRecordSettlement({ ...operation, receipts: [fact('peer')] })
    ).toBe('protected');
    expect(
      privateRecordSettlement({
        ...operation,
        family: 'private_send_reservation',
        revision: 0
      })
    ).toBe('protected');
  });
  for (const status of [
    'refused',
    'readback',
    'unknown',
    'timed_out',
    'stopped'
  ] as const)
    it('a current ' + status + ' target is not acceptance', () => {
      expect(
        privateRecordSettlement({
          ...operation,
          receipts: [fact('peer', status), fact('self_archive')]
        })
      ).toBe('protected');
    });
  it('a different event, role or origin cannot satisfy the current target', () => {
    for (const changed of [
      { eventId: '1'.repeat(64) },
      { role: 'self_archive' as const },
      { origin: 'wss://other.example.org' }
    ])
      expect(
        privateRecordSettlement({
          ...operation,
          receipts: [{ ...fact('peer'), ...changed }, fact('self_archive')]
        })
      ).toBe('protected');
  });
  for (const status of ['unknown', 'timed_out', 'stopped'] as const)
    it(
      'later exact acceptance resolves ' +
        status +
        ' while a later uncertainty stays protected',
      () => {
        expect(
          privateRecordSettlement({
            ...operation,
            receipts: [fact('peer', status), ...operation.receipts!]
          })
        ).toBe('settled');
        expect(
          privateRecordSettlement({
            ...operation,
            receipts: [...operation.receipts!, fact('peer', status)]
          })
        ).toBe('protected');
      }
    );
  it('changing routes or using readback does not resolve an uncertain old target', () => {
    const uncertain = fact('peer', 'unknown', 'wss://old.example.org');
    const changed = {
      ...operation,
      deliveryPlan: {
        ...operation.deliveryPlan!,
        previousRoutes: [{ ...routes, peer: route('peer', uncertain.origin) }]
      }
    };
    expect(
      privateRecordSettlement({
        ...changed,
        receipts: [uncertain, ...operation.receipts!]
      })
    ).toBe('protected');
    expect(
      privateRecordSettlement({
        ...changed,
        receipts: [
          uncertain,
          fact('peer', 'readback', uncertain.origin),
          ...operation.receipts!
        ]
      })
    ).toBe('protected');
    expect(
      privateRecordSettlement({
        ...changed,
        receipts: [
          uncertain,
          fact('peer', 'accepted', uncertain.origin),
          ...operation.receipts!
        ]
      })
    ).toBe('settled');
  });
  it('an earlier definitive refusal on an old route does not create an unresolved effect', () => {
    expect(
      privateRecordSettlement({
        ...operation,
        receipts: [
          fact('peer', 'refused', 'wss://old.example.org'),
          ...operation.receipts!
        ]
      })
    ).toBe('settled');
  });
  it('inventory exposes only local bounded status and discloses recovery/deduplication loss', () => {
    expect(privateInventoryRow(operation, 321)).toEqual({
      id,
      revision: 1,
      logicalBytes: 321,
      state: 'settled'
    });
    expect(PRIVATE_OUTBOX_CLEANUP_CONSEQUENCES).toContain(
      'duplicate-prevention'
    );
    expect(PRIVATE_OUTBOX_CLEANUP_CONSEQUENCES).toContain(
      'does not erase remote'
    );
  });
});
