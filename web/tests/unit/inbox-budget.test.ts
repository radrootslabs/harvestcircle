import { beforeAll, test, expect } from 'vitest';
import * as Budget from '../../src/lib/messaging/inbox-budget.ts';
import * as Cleanup from '../../src/lib/persistence/inbox-retention.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
const id = (n: number) => n.toString(16).padStart(64, '0');
beforeAll(() => {
  expect(Budget.inboxBudgetSnapshot).toBeTypeOf('function');
  expect(Cleanup.selectReceivedCleanupIds).toBeTypeOf('function');
  expect(Cleanup.captureReceivedCleanup).toBeTypeOf('function');
});
test('finite live retention and independent unresolved outbox caps remain exact', () =>
  expect(Budget.inboxBudgetSnapshot()).toEqual({
    finite: { deliveries: 500, bytes: 8388608, milliseconds: 15000 },
    live: { deliveries: 120, bytes: 2097152, milliseconds: 60000 },
    received: { records: 2000, bytes: 50331648 },
    outbox: { records: 100, bytes: 8388608 }
  }));
test('review selection deduplicates only exact outer IDs preserving order', () =>
  expect(Cleanup.selectReceivedCleanupIds([id(2), id(1), id(2)])).toEqual([
    id(2),
    id(1)
  ]));
test('reviewed ID selection and budget snapshots are detached copies', () => {
  const input = [id(1)];
  const selected = Cleanup.selectReceivedCleanupIds(input)!;
  input[0] = id(2);
  expect(selected).toEqual([id(1)]);
  const view: { received: { records: number } } = Budget.inboxBudgetSnapshot();
  view.received.records = 1;
  expect(Budget.inboxBudgetSnapshot().received.records).toBe(2000);
});
test('malformed tail never silently truncates reviewed cleanup selection', () =>
  expect(
    Cleanup.selectReceivedCleanupIds([
      ...Array.from({ length: 20 }, (_, n) => id(n)),
      'not-an-event-id'
    ])
  ).toBeUndefined());
test('empty oversized uppercase and nonarray review selections refuse without reset', () => {
  for (const value of [
    [],
    Array.from({ length: 2001 }, (_, n) => id(n)),
    ['A'.repeat(64)],
    {},
    null
  ])
    expect(Cleanup.selectReceivedCleanupIds(value)).toBeUndefined();
  expect(
    Cleanup.selectReceivedCleanupIds(
      Array.from({ length: 2000 }, (_, n) => id(n))
    )?.length
  ).toBe(2000);
});
test('ciphertext capacity exposes explicit local-copy review and never automatic eviction', () =>
  expect(Budget.inboxRecoveryChoices('capacity')).toEqual([
    'review_local_received_copies',
    'review_relay_recovery'
  ]));
test('transport exhaustion exposes reviewed recovery while unrelated states grant no action', () => {
  expect(Budget.inboxRecoveryChoices('budget')).toEqual([
    'wait_then_review_refresh',
    'review_history_scope',
    'review_local_received_copies'
  ]);
  expect(Budget.inboxRecoveryChoices(null)).toEqual([]);
  expect(Budget.inboxRecoveryChoices('complete')).toEqual([]);
});
test('SSR copied cleanup authority cannot admit or delete any local copy', async () => {
  expect(
    await Cleanup.captureReceivedCleanup(
      {} as PrivateStorageRepository,
      {} as PrivateSession,
      [id(1)],
      'reviewed_received_cleanup_selection'
    )
  ).toBeUndefined();
  expect(
    await Cleanup.deleteReviewedReceivedCopies(
      {} as Cleanup.ReceivedCleanup,
      'reviewed_delete_local_received_copies'
    )
  ).toEqual({ status: 'invalid' });
});

test('actual finite elapsed reason exposes reviewed transport recovery without deadline widening', () =>
  expect(Budget.inboxRecoveryChoices('elapsed')).toEqual([
    'wait_then_review_refresh',
    'review_history_scope',
    'review_local_received_copies'
  ]));
